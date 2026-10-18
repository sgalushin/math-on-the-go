use crate::{
    ai,
    model::*,
    now,
    practice::{self, CardRecord, GenerationPlan},
    store::{ApiError, ApiResult, Record, Write},
    AppState,
};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Only `session` is returned over the wire. Queue keys and worker claims are private.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session: PracticeSession,
    pub creation: CreateSession,
    pub plans: Vec<GenerationPlan>,
    pub queue: Vec<String>,
    pub worker_started: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NextEvent {
    session_id: String,
    request: SessionNextRequest,
    result: Attempt,
}
pub fn key(id: &str) -> String {
    format!("SESSION#{id}")
}
fn valid_id(id: &str) -> ApiResult<()> {
    uuid::Uuid::parse_str(id).map_err(|_| ApiError::bad("Session and event IDs must be UUIDs"))?;
    Ok(())
}
fn replay(record: SessionRecord, request: &CreateSession) -> ApiResult<PracticeSession> {
    if record.creation != *request {
        return Err(ApiError::conflict(
            "requestId was already used for another session selection",
        ));
    }
    Ok(record.session)
}

pub async fn create(
    state: &AppState,
    user: &str,
    request: CreateSession,
) -> ApiResult<PracticeSession> {
    valid_id(&request.request_id)?;
    if !(1..=20).contains(&request.length) {
        return Err(ApiError::bad("Session length must be between 1 and 20"));
    }
    let session_key = key(&request.request_id);
    if let Some(previous) = state.store.get::<SessionRecord>(user, &session_key).await? {
        return replay(previous.value, &request);
    }
    let settings = state.settings(user).await?;
    practice::validate_selection(&state.catalog, &settings, &request.selection)?;
    let timestamp = now();
    let review = request.selection.mode == PracticeMode::Review;
    let mut record = SessionRecord {
        session: PracticeSession {
            id: request.request_id.clone(),
            status: if review {
                SessionStatus::Ready
            } else {
                SessionStatus::Generating
            },
            request: request.selection.clone(),
            length: request.length,
            generated: 0,
            attempt_ids: vec![],
            error: None,
            created_at: timestamp,
            updated_at: timestamp,
        },
        creation: request.clone(),
        plans: vec![],
        queue: vec![],
        worker_started: false,
    };
    let mut writes = vec![];
    if review {
        let cards: HashMap<_, _> = state
            .store
            .list::<CardRecord>(user, "CARD#")
            .await?
            .into_iter()
            .map(|r| (r.value.question_id, r.value.schedule))
            .collect();
        let questions = state.questions(user).await?;
        let queue = practice::review_queue(
            &questions,
            &state.catalog,
            &settings,
            &request.selection,
            &cards,
            timestamp,
            request.length,
        )?;
        record.session.length = queue.len() as u32;
        record.session.generated = queue.len() as u32;
        for (slot, question) in queue.iter().enumerate() {
            let question_key = format!("SESSIONQUESTION#{}#{slot}", request.request_id);
            writes.push(Write::new(&question_key, question, None)?);
            record.queue.push(question_key);
        }
    } else {
        if state.openrouter_key.is_none() && std::env::var_os("AWS_LAMBDA_RUNTIME_API").is_none() {
            return Err(ApiError::unavailable(
                "AI generation is not configured on this server",
            ));
        }
        record.plans = practice::generation_plan(
            &state.catalog,
            &settings,
            &request.selection,
            &state.attempts(user).await?,
            request.length,
        )?;
    }
    writes.push(Write::new(&session_key, &record, None)?);
    match state.store.transaction(user, writes).await {
        Ok(()) => {}
        Err(error) if error.0 == StatusCode::CONFLICT => {
            let previous = state
                .store
                .get::<SessionRecord>(user, &session_key)
                .await?
                .ok_or(error)?;
            return replay(previous.value, &request);
        }
        Err(error) => return Err(error),
    }
    // Only the winner of the conditional create dispatches. A crashed dispatch is
    // failed by stale-session handling, never retried into duplicate model calls.
    if !review {
        if let Err(error) = ai::dispatch(
            state.clone(),
            WorkerEvent {
                user_id: user.into(),
                session_id: request.request_id,
            },
        )
        .await
        {
            fail(state, user, &record.session.id, error.1).await?;
            return get(state, user, &record.session.id).await;
        }
    }
    Ok(record.session)
}

pub async fn get(state: &AppState, user: &str, id: &str) -> ApiResult<PracticeSession> {
    valid_id(id)?;
    for _ in 0..5 {
        let mut record = state
            .store
            .get::<SessionRecord>(user, &key(id))
            .await?
            .ok_or_else(ApiError::missing)?;
        if record.value.session.status != SessionStatus::Generating
            || now() - record.value.session.updated_at <= ai::STALE_SESSION_MILLIS
        {
            return Ok(record.value.session);
        }
        record.value.session.status = SessionStatus::Failed;
        record.value.session.error = Some("The preparation worker was interrupted or exceeded its deadline; no partial question queue was published".into());
        record.value.session.updated_at = now();
        match state
            .store
            .save(user, &key(id), &record.value, Some(record.version))
            .await
        {
            Ok(()) => return Ok(record.value.session),
            Err(error) if error.0 == StatusCode::CONFLICT => continue,
            Err(error) => return Err(error),
        }
    }
    Err(ApiError::conflict(
        "Session changed concurrently; reload the session",
    ))
}

/// A conditional claim is durable before any provider request. Duplicate worker
/// deliveries (including after interruption) must never rerun a pipeline.
pub async fn claim(state: &AppState, user: &str, id: &str) -> ApiResult<Option<SessionRecord>> {
    let Some(mut record) = state.store.get::<SessionRecord>(user, &key(id)).await? else {
        return Err(ApiError::missing());
    };
    if record.value.worker_started || record.value.session.status != SessionStatus::Generating {
        return Ok(None);
    }
    record.value.worker_started = true;
    record.value.session.updated_at = now();
    match state
        .store
        .save(user, &key(id), &record.value, Some(record.version))
        .await
    {
        Ok(()) => Ok(Some(record.value)),
        Err(error) if error.0 == StatusCode::CONFLICT => Ok(None),
        Err(error) => Err(error),
    }
}

pub async fn progress(state: &AppState, user: &str, id: &str, generated: u32) -> ApiResult<()> {
    let mut record = state
        .store
        .get::<SessionRecord>(user, &key(id))
        .await?
        .ok_or_else(ApiError::missing)?;
    if record.value.session.status != SessionStatus::Generating {
        return Err(ApiError::conflict(
            "Session preparation is no longer active",
        ));
    }
    record.value.session.generated = generated;
    record.value.session.updated_at = now();
    state
        .store
        .save(user, &key(id), &record.value, Some(record.version))
        .await
}

pub fn publish_queue(record: &mut SessionRecord, questions: &[Question]) -> ApiResult<()> {
    if record.session.status != SessionStatus::Generating
        || questions.len() != record.session.length as usize
    {
        return Err(ApiError::conflict(
            "Only a complete active session can be published",
        ));
    }
    let mut prompts = std::collections::HashSet::new();
    if questions.iter().any(|q| {
        !prompts.insert(
            q.prompt
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase(),
        )
    }) {
        return Err(ApiError::bad("Independent pipelines produced duplicate question prompts; the session was not published"));
    }
    record.queue = questions
        .iter()
        .map(|q| format!("QUESTION#{}", q.id))
        .collect();
    record.session.generated = questions.len() as u32;
    record.session.status = SessionStatus::Ready;
    record.session.updated_at = now();
    Ok(())
}

pub async fn publish(
    state: &AppState,
    user: &str,
    id: &str,
    questions: Vec<Question>,
) -> ApiResult<()> {
    let mut record = state
        .store
        .get::<SessionRecord>(user, &key(id))
        .await?
        .ok_or_else(ApiError::missing)?;
    publish_queue(&mut record.value, &questions)?;
    let mut writes = questions
        .iter()
        .map(|q| Write::new(format!("QUESTION#{}", q.id), q, None))
        .collect::<ApiResult<Vec<_>>>()?;
    writes.push(Write::new(key(id), &record.value, Some(record.version))?);
    // All questions and the ready metadata become visible in the same DynamoDB
    // transaction. No partial bank additions or partial ready queue can escape.
    state.store.transaction(user, writes).await
}

pub async fn fail(state: &AppState, user: &str, id: &str, message: String) -> ApiResult<()> {
    for _ in 0..5 {
        let mut record = state
            .store
            .get::<SessionRecord>(user, &key(id))
            .await?
            .ok_or_else(ApiError::missing)?;
        if record.value.session.status != SessionStatus::Generating {
            return Ok(());
        }
        record.value.session.status = SessionStatus::Failed;
        record.value.session.error = Some(message.clone());
        record.value.session.updated_at = now();
        match state
            .store
            .save(user, &key(id), &record.value, Some(record.version))
            .await
        {
            Ok(()) => return Ok(()),
            Err(error) if error.0 == StatusCode::CONFLICT => continue,
            Err(error) => return Err(error),
        }
    }
    Err(ApiError::conflict("Session changed concurrently"))
}

pub fn next_slot(
    record: &SessionRecord,
    request: &SessionNextRequest,
    current: Option<&Attempt>,
) -> ApiResult<usize> {
    if record.session.status != SessionStatus::Ready {
        return Err(ApiError::conflict(
            "Session is not ready for another question",
        ));
    }
    if record.session.attempt_ids.last() != request.after_attempt_id.as_ref() {
        return Err(ApiError::conflict(
            "afterAttemptId must identify the current last attempt",
        ));
    }
    if let Some(id) = record.session.attempt_ids.last() {
        let attempt = current
            .filter(|a| a.id == *id)
            .ok_or_else(ApiError::missing)?;
        if attempt.outcome == Outcome::Unfinished {
            return Err(ApiError::conflict(
                "Complete or skip the current question before advancing",
            ));
        }
    }
    let slot = record.session.attempt_ids.len();
    if slot >= record.queue.len() {
        return Err(ApiError::conflict(
            "Every prepared question has been presented; finish this session",
        ));
    }
    Ok(slot)
}

pub async fn next(
    state: &AppState,
    user: &str,
    id: &str,
    request: SessionNextRequest,
) -> ApiResult<Attempt> {
    valid_id(id)?;
    valid_id(&request.event_id)?;
    let event_key = format!("SESSIONNEXT#{}", request.event_id);
    for _ in 0..5 {
        if let Some(previous) = state.store.get::<NextEvent>(user, &event_key).await? {
            if previous.value.session_id != id || previous.value.request != request {
                return Err(ApiError::conflict(
                    "eventId was already used for a different session transition",
                ));
            }
            return Ok(previous.value.result);
        }
        let mut record = state
            .store
            .get::<SessionRecord>(user, &key(id))
            .await?
            .ok_or_else(ApiError::missing)?;
        let current = match record.value.session.attempt_ids.last() {
            Some(attempt_id) => {
                state
                    .store
                    .get::<Attempt>(user, &format!("ATTEMPT#{attempt_id}"))
                    .await?
            }
            None => None,
        };
        let slot = next_slot(&record.value, &request, current.as_ref().map(|r| &r.value))?;
        let question = state
            .store
            .get::<Question>(user, &record.value.queue[slot])
            .await?
            .ok_or_else(|| ApiError::unavailable("Prepared question is missing"))?
            .value;
        let attempt = Attempt::new(question, now());
        record.value.session.attempt_ids.push(attempt.id.clone());
        record.value.session.updated_at = now();
        let event = NextEvent {
            session_id: id.into(),
            request: request.clone(),
            result: attempt.clone(),
        };
        let writes = vec![
            Write::new(key(id), &record.value, Some(record.version))?,
            Write::new(format!("ATTEMPT#{}", attempt.id), &attempt, None)?,
            Write::new(&event_key, &event, None)?,
        ];
        match state.store.transaction(user, writes).await {
            Ok(()) => return Ok(attempt),
            Err(error) if error.0 == StatusCode::CONFLICT => continue,
            Err(error) => return Err(error),
        }
    }
    Err(ApiError::conflict(
        "Concurrent transitions; retry the same eventId",
    ))
}

pub async fn finish(state: &AppState, user: &str, id: &str) -> ApiResult<PracticeSession> {
    valid_id(id)?;
    for _ in 0..5 {
        let Record { mut value, version } = state
            .store
            .get::<SessionRecord>(user, &key(id))
            .await?
            .ok_or_else(ApiError::missing)?;
        if value.session.status == SessionStatus::Finished {
            return Ok(value.session);
        }
        if value.session.status == SessionStatus::Generating {
            return Err(ApiError::conflict(
                "Preparation cannot be cancelled; navigate away and resume it later",
            ));
        }
        value.session.status = SessionStatus::Finished;
        value.session.updated_at = now();
        match state
            .store
            .save(user, &key(id), &value, Some(version))
            .await
        {
            Ok(()) => return Ok(value.session),
            Err(error) if error.0 == StatusCode::CONFLICT => continue,
            Err(error) => return Err(error),
        }
    }
    Err(ApiError::conflict(
        "Session changed concurrently; retry finishing",
    ))
}
