use crate::{
    grading,
    model::*,
    now,
    practice::GenerationPlan,
    sessions,
    store::{ApiError, ApiResult},
    validate_question, variants, AppState,
};
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;

const MODEL_CALL_TIMEOUT: Duration = Duration::from_secs(45);
const JOB_TIMEOUT: Duration = Duration::from_secs(360);
pub const STALE_SESSION_MILLIS: i64 = 420_000;
async fn completion(state: &AppState, model: &str, system: &str, prompt: &str) -> ApiResult<Value> {
    let key = state
        .openrouter_key
        .as_deref()
        .ok_or_else(|| ApiError::unavailable("AI generation is not configured on this server"))?;
    let response = state
        .http
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(key)
        .timeout(MODEL_CALL_TIMEOUT)
        .json(&json!({"model":model,"max_tokens":5500,"response_format":{"type":"json_object"},"messages":[{"role":"system","content":system},{"role":"user","content":prompt}]}))
        .send()
        .await
        .map_err(|_| ApiError::unavailable("AI provider request timed out or failed"))?;
    if !response.status().is_success() {
        return Err(ApiError::unavailable(format!(
            "AI provider rejected the request (HTTP {})",
            response.status().as_u16()
        )));
    }
    let mut response = response;
    let mut body = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::unavailable("AI response interrupted"))?
    {
        if body.len() + chunk.len() > 96_000 {
            return Err(ApiError::bad("AI response exceeds permitted size"));
        }
        body.extend_from_slice(&chunk);
    }
    let envelope: Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::bad("AI provider returned invalid JSON"))?;
    let content = envelope
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("AI provider returned no answer"))?;
    serde_json::from_str(content)
        .map_err(|_| ApiError::bad("AI model returned invalid structured content"))
}
pub async fn dispatch(state: AppState, event: WorkerEvent) -> ApiResult<()> {
    if std::env::var_os("AWS_LAMBDA_RUNTIME_API").is_some() {
        let name = std::env::var("AI_WORKER_FUNCTION")
            .map_err(|_| ApiError::unavailable("AI worker is not configured"))?;
        state
            .lambda
            .invoke()
            .function_name(name)
            .invocation_type(aws_sdk_lambda::types::InvocationType::Event)
            .payload(aws_sdk_lambda::primitives::Blob::new(
                serde_json::to_vec(&event).map_err(|_| ApiError::bad("Invalid worker event"))?,
            ))
            .send()
            .await
            .map_err(|_| ApiError::unavailable("AI worker could not be invoked"))?;
    } else {
        tokio::spawn(async move {
            if let Err(error) = run_session(&state, event).await {
                tracing::error!(error=%error.1, "Session worker failed");
            }
        });
    }
    Ok(())
}

pub async fn run_session(state: &AppState, event: WorkerEvent) -> ApiResult<()> {
    if !crate::valid_user(&event.user_id) || uuid::Uuid::parse_str(&event.session_id).is_err() {
        return Err(ApiError::bad("Invalid worker event"));
    }
    let Some(record) = sessions::claim(state, &event.user_id, &event.session_id).await? else {
        return Ok(());
    };
    let pipelines: Vec<_> = record
        .plans
        .into_iter()
        .map(|plan| {
            let state = state.clone();
            async move { generate_question(&state, &plan).await }
        })
        .collect();
    let work = async {
        let questions = prepare_parallel(pipelines, |generated| {
            sessions::progress(state, &event.user_id, &event.session_id, generated)
        })
        .await?;
        sessions::publish(state, &event.user_id, &event.session_id, questions).await
    };
    let result = tokio::time::timeout(JOB_TIMEOUT, work).await;
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => sessions::fail(state, &event.user_id, &event.session_id, error.1).await,
        Err(_) => {
            sessions::fail(
                state,
                &event.user_id,
                &event.session_id,
                "Session preparation exceeded the worker deadline; no partial queue was published"
                    .into(),
            )
            .await
        }
    }
}

/// Spawn every complete author -> blind solver pipeline before awaiting any
/// result. A single collector owns progress; completion order never determines
/// queue order. Dropping the collector aborts its JoinSet on the job deadline.
pub async fn prepare_parallel<F, P, PF>(
    pipelines: Vec<F>,
    mut progress: P,
) -> ApiResult<Vec<Question>>
where
    F: Future<Output = ApiResult<Question>> + Send + 'static,
    P: FnMut(u32) -> PF,
    PF: Future<Output = ApiResult<()>>,
{
    let length = pipelines.len();
    let mut tasks = tokio::task::JoinSet::new();
    for (slot, pipeline) in pipelines.into_iter().enumerate() {
        tasks.spawn(async move { (slot, pipeline.await) });
    }
    let mut ordered: Vec<Option<Question>> = (0..length).map(|_| None).collect();
    let mut generated = 0;
    let mut failure = None;
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((slot, Ok(question))) => {
                ordered[slot] = Some(question);
                generated += 1;
                progress(generated).await?;
            }
            Ok((_, Err(error))) => {
                failure.get_or_insert(error);
            }
            Err(_) => {
                failure.get_or_insert_with(|| {
                    ApiError::unavailable("A question preparation pipeline was interrupted")
                });
            }
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }
    ordered
        .into_iter()
        .map(|q| q.ok_or_else(|| ApiError::bad("A prepared question slot is missing")))
        .collect()
}

async fn generate_question(state: &AppState, plan: &GenerationPlan) -> ApiResult<Question> {
    let unit = state
        .catalog
        .units
        .iter()
        .find(|u| u.topics.iter().any(|t| t.id == plan.topic_id))
        .ok_or_else(|| ApiError::bad("Unknown topic"))?;
    let topic = unit
        .topics
        .iter()
        .find(|t| t.id == plan.topic_id)
        .expect("topic found");
    let reference = state
        .bank
        .iter()
        .filter(|q| q.topic_id == plan.topic_id)
        .max_by_key(|q| (q.kind == plan.kind, q.template.is_some()))
        .ok_or_else(|| ApiError::bad("Topic has no curated source reference"))?;
    let specification = json!({
        "count": 1, "slot": plan.slot + 1,
        "variationGuidance": format!("Independent session slot {}: choose a distinctive example, assumptions, values and formulation; do not copy the reference prompt.", plan.slot + 1),
        "module": unit.module, "unit": unit.title, "topic": topic.title,
        "topicId": plan.topic_id, "kind": plan.kind, "difficulty": plan.difficulty,
        "formulaOnly": plan.formula_only, "source": reference.source,
        "allowedAnswerFormats": ["text", "choice", "self"],
        "templateParameterLimits": {
            "linear-evaluation": {"a": "nonzero integer -9..9", "b": "integer -9..9", "x": "integer -9..9"},
            "linear-equation": {"a": "nonzero integer -9..9", "b": "integer -9..9", "x": "integer -9..9; solution value"},
            "arithmetic-sequence": {"a": "integer -9..9", "d": "integer -9..9", "n": "integer 1..8"},
            "power": {"base": "integer 1..4", "exponent": "integer -3..3"},
            "derivative-at": {"a": "integer -6..6", "n": "integer 1..3", "x": "integer -3..3"},
            "integral-power": {"a": "integer -6..6", "n": "integer 0..2", "lower": "integer 0..3", "upper": "integer 0..3"}
        }
    });
    let prompt = format!(
        "Create exactly ONE original mathematics question matching these exact JSON constraints: {specification}.
Return {{\"questions\":[...]}}. Copy the exact topicId, lowercase kind, lowercase difficulty, and source object from the constraints; do not invent alternative IDs or enum spellings.
Use exactly the Question object shape shown by this example: {}.
Do not reproduce its prompt. Include all required fields. acceptedAnswers, choices, hints, and verification.models are arrays. answerFormat must be one of text, choice, self, never algebraic.
For numeric text tasks, choose a mathematically relevant supported template family from templateParameterLimits and use template {{\"type\":family,\"parameters\":numericParameterObject}}. The values must satisfy all listed limits. The answer field must contain only a number or arithmetic expression such as 3/4, with no variable assignment, units, or LaTeX delimiters. Set acceptedAnswers to [] for numeric tasks: the server already accepts numerically equivalent fractions and expressions, so do not add aliases such as x=4.
If the requested topic does NOT match a supported numerical template, use an exact finite choice set or self-assessed reasoning on the actual requested topic. NEVER substitute irrelevant algebra, elementary arithmetic or a supported template from another topic merely to enable numeric grading.
For expressions with variables, explanations, proofs, or counterexamples, use self assessment or a finite choice set: never claim general symbolic automatic grading. Reasoning tasks and flashcards use answerFormat self and template null.
If formulaOnly is true, create a FLASHCARD recalling an actual formula, identity or mathematical rule, including necessary assumptions and domains. A plain definition, descriptive fact, worked calculation or 'compute this value' task is NOT a formula recall card and will be rejected. Ask to state or complete the general formula/rule, not apply it to specific numbers.
Use only the requested topic and studied prerequisites. Every question must be mentally tractable, with two staged hints and a short worked explanation. No graph needed. Verification is assigned by the server: use method self-assessed, models [], checkedAt null. flagged is false.",
        serde_json::to_string(reference).map_err(|_| ApiError::bad("Invalid reference"))?
    );
    let generated = completion(state, AUTHOR_MODEL, "You author original, accurate UK undergraduate mathematics practice. Output only requested JSON. Source provenance must be exactly the supplied private catalog reference; never claim to have accessed its PDF.", &prompt).await?;
    let values = generated
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::bad("Generator returned no question array"))?;
    if values.len() != 1 {
        return Err(ApiError::bad(
            "Each independent pipeline must return exactly one question",
        ));
    }
    let question = serde_json::from_value::<Question>(values[0].clone()).map_err(|error| {
        ApiError::bad(format!("Generated question has an invalid schema: {error}"))
    })?;
    let mut question = vet(state, plan, question, reference).await?;
    question.id = format!("ai-{}", uuid::Uuid::new_v4());
    Ok(question)
}
async fn vet(
    state: &AppState,
    plan: &GenerationPlan,
    mut q: Question,
    reference: &Question,
) -> ApiResult<Question> {
    validate_question(&q, &state.catalog)?;
    if q.topic_id != plan.topic_id
        || q.kind != plan.kind
        || q.difficulty != plan.difficulty
        || q.source.handbook != reference.source.handbook
        || q.source.page != reference.source.page
        || q.source.section != reference.source.section
        || q.flagged
    {
        return Err(ApiError::bad(
            "Generated question topic, kind, difficulty or source does not match request",
        ));
    }
    if q.kind == Kind::Flashcard && q.answer_format != AnswerFormat::SelfAssessed {
        return Err(ApiError::bad("Flashcards require self assessment"));
    }
    if plan.formula_only && q.kind != Kind::Flashcard {
        return Err(ApiError::bad("Formula mode requires actual flashcards"));
    }
    if q.answer_format == AnswerFormat::Text
        && (grading::numeric(&q.answer).is_none() || !q.accepted_answers.is_empty())
    {
        return Err(ApiError::bad("Text tasks require a plain numeric answer and empty acceptedAnswers; use exact choice or self assessment for symbolic reasoning"));
    }
    let mut deterministic = false;
    if q.answer_format == AnswerFormat::Text && grading::numeric(&q.answer).is_some() {
        let template = q.template.as_ref().ok_or_else(|| {
            ApiError::bad("Numeric AI answers require independently computable template parameters")
        })?;
        if template.parameters.len() > 8
            || template
                .parameters
                .values()
                .any(|p| !p.is_finite() || p.abs() > 30.0)
        {
            return Err(ApiError::bad("Unsafe template parameters"));
        }
        let required: &[&str] = match template.family.as_str() {
            "linear-evaluation" | "linear-equation" => &["a", "b", "x"],
            "arithmetic-sequence" => &["a", "d", "n"],
            "power" => &["base", "exponent"],
            "derivative-at" => &["a", "n", "x"],
            "integral-power" => &["a", "n", "lower", "upper"],
            _ => return Err(ApiError::bad("Unsupported numeric family")),
        };
        if required
            .iter()
            .any(|k| !template.parameters.contains_key(*k))
        {
            return Err(ApiError::bad("Missing deterministic template parameters"));
        }
        let computed = variants::reconstruct(&q);
        if computed.id == q.id {
            return Err(ApiError::bad("Unsupported numeric family"));
        }
        if !grading::equivalent(&computed.answer, &q.answer)
            || q.accepted_answers
                .iter()
                .any(|a| !grading::equivalent(&computed.answer, a))
        {
            return Err(ApiError::bad(
                "Generated answer disagrees with independent mathematical calculation",
            ));
        }
        q = computed;
        deterministic = true;
    } else if q.template.is_some() {
        return Err(ApiError::bad(
            "Templates are restricted to independently computed numeric tasks",
        ));
    }
    // Blind verifier receives NO proposed answer, explanation, accepted answers,
    // hints, parameters, generator identity, or generator reasoning.
    let topic = state
        .catalog
        .units
        .iter()
        .flat_map(|u| &u.topics)
        .find(|t| t.id == plan.topic_id)
        .ok_or_else(|| ApiError::bad("Unknown topic"))?;
    let blind = json!({"topic":topic.title,"prompt":q.prompt,"choices":q.choices,"answerFormat":q.answer_format,"graph":q.graph,"formulaOnly":plan.formula_only});
    let solved=completion(state,SOLVER_MODEL,"Independently solve this mathematics problem. You have not been given any candidate answer. Output JSON {\"answer\":string,\"reasoning\":string,\"unambiguous\":boolean,\"topicAligned\":boolean,\"formulaCard\":boolean}. Set formulaCard true ONLY when the prompt asks to recall an actual general formula, identity or mathematical rule with appropriate assumptions. Plain definitions, descriptive facts and calculations/applications at specific values are NOT formula recall; set formulaCard false for those, even if the input requests formulaOnly. For choice return exact choice text. For numerical text answers, return only the number or arithmetic expression in answer, with no variable assignment, units or LaTeX delimiters; explain the calculation in reasoning. For self-assessed reasoning, answer may be a mathematical explanation. Mark unambiguous false if underspecified, invalid, or not solvable. Never invent missing assumptions.",&blind.to_string()).await?;
    validate_formula_verdict(plan.formula_only, &q, &solved)?;
    let answer = solved
        .get("answer")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= 4000)
        .ok_or_else(|| ApiError::bad("Verifier did not provide an answer"))?;
    let reasoning = solved
        .get("reasoning")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= 12000)
        .ok_or_else(|| ApiError::bad("Verifier did not provide reasoning"))?;
    if solved.get("unambiguous").and_then(Value::as_bool) != Some(true)
        || solved.get("topicAligned").and_then(Value::as_bool) != Some(true)
    {
        return Err(ApiError::bad(
            "Verifier rejected ambiguity or topic mismatch",
        ));
    }
    if q.answer_format != AnswerFormat::SelfAssessed && grading::correct(&q, answer) != Some(true) {
        return Err(ApiError::bad(
            "Independent solver and deterministic answer disagree",
        ));
    }
    if q.answer_format == AnswerFormat::SelfAssessed {
        let comparison = json!({"prompt":q.prompt,"independentAnswer":answer,"independentReasoning":reasoning,"proposedAnswer":q.answer,"proposedExplanation":q.explanation});
        let verdict=completion(state,SOLVER_MODEL,"You already solved this problem independently. Compare that solution with the proposed answer and explanation. Return JSON {\"equivalent\":boolean}. Reject mathematical disagreements, wrong reasoning, misleading assumptions, and incomplete answers. Do not defer to the proposed answer.",&comparison.to_string()).await?;
        if verdict.get("equivalent").and_then(Value::as_bool) != Some(true) {
            return Err(ApiError::bad(
                "Independent conceptual solution disagrees with the proposed answer",
            ));
        }
        q.verification.method = "self-assessed".into();
        q.explanation = format!(
            "{}\n\nIndependent solution (not an automatic proof checker): {}",
            q.explanation, reasoning
        );
    } else {
        q.verification.method = if deterministic {
            "deterministic"
        } else {
            "independent-model"
        }
        .into();
    }
    q.verification.models = vec![AUTHOR_MODEL.into(), SOLVER_MODEL.into()];
    q.verification.checked_at = Some(now());
    q.flagged = false;
    validate_question(&q, &state.catalog)?;
    Ok(q)
}

pub fn validate_formula_verdict(
    formula_only: bool,
    question: &Question,
    verdict: &Value,
) -> ApiResult<()> {
    if formula_only
        && (question.kind != Kind::Flashcard
            || question.answer_format != AnswerFormat::SelfAssessed
            || verdict.get("formulaCard").and_then(Value::as_bool) != Some(true))
    {
        return Err(ApiError::bad("Independent solver rejected this as not a formula, identity or mathematical-rule recall flashcard"));
    }
    Ok(())
}
