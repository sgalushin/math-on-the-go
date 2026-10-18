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

/// Only `session` is public. A generation ID fences workers across migrations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session: PracticeSession,
    pub creation: CreateSession,
    pub plans: Vec<GenerationPlan>,
    pub queue: Vec<String>,
    pub worker_started: bool,
    #[serde(default)]
    pub handbook_version: u8,
    #[serde(default)]
    pub generation_id: String,
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

/// Prepare immutable handbook snapshots and task-only plans before committing.
pub async fn prepare(
    state: &AppState,
    user: &str,
    request: &CreateSession,
    created_at: i64,
) -> ApiResult<(SessionRecord, Vec<Write>)> {
    let settings = state.settings(user).await?;
    let eligible = practice::validate_selection(&state.catalog, &settings, &request.selection)?;
    let timestamp = now();
    let review = request.selection.mode == PracticeMode::Review;
    let formula = request.selection.kind == RequestedKind::Formula;
    if review && request.selection.kind == RequestedKind::Task {
        return Err(ApiError::bad("Scheduled review contains flashcards only"));
    }
    let fixed_limit = if review || formula {
        request.length
    } else if request.selection.kind == RequestedKind::Mixed {
        request.length / 2
    } else {
        0
    };
    let mut fixed = vec![];
    if fixed_limit > 0 {
        let cards: HashMap<_, _> = state
            .store
            .list::<CardRecord>(user, "CARD#")
            .await?
            .into_iter()
            .map(|r| (r.value.question_id, r.value.schedule))
            .collect();
        let questions = state.questions(user).await?;
        fixed = if review {
            practice::review_queue(&questions, &eligible, &cards, timestamp, fixed_limit)?
        } else {
            practice::formula_queue(&questions, &eligible, &cards, timestamp, fixed_limit, true)?
        };
    }
    if formula && fixed.is_empty() {
        return Err(ApiError::conflict(
            "No handbook formulas are available in this scope; choose Tasks instead",
        ));
    }
    let length = if review || formula {
        fixed.len() as u32
    } else {
        request.length
    };
    let mut record = SessionRecord {
        session: PracticeSession {
            id: request.request_id.clone(),
            status: SessionStatus::Ready,
            request: request.selection.clone(),
            length,
            generated: fixed.len() as u32,
            attempt_ids: vec![],
            error: None,
            created_at,
            updated_at: timestamp,
        },
        creation: request.clone(),
        plans: vec![],
        queue: vec![String::new(); length as usize],
        worker_started: false,
        handbook_version: 1,
        generation_id: uuid::Uuid::new_v4().to_string(),
    };
    let mut writes = vec![];
    for (index, question) in fixed.iter().enumerate() {
        let slot = if !review && !formula {
            index * 2 + 1
        } else {
            index
        };
        let question_key = format!("SESSIONQUESTION#{}#handbook-1#{slot}", request.request_id);
        writes.push(Write::new(&question_key, question, None)?);
        record.queue[slot] = question_key;
    }
    let task_slots: Vec<_> = record
        .queue
        .iter()
        .enumerate()
        .filter_map(|(slot, key)| key.is_empty().then_some(slot))
        .collect();
    if !task_slots.is_empty() {
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
            task_slots.len() as u32,
        )?;
        for (plan, slot) in record.plans.iter_mut().zip(task_slots) {
            plan.slot = slot;
        }
        record.session.status = SessionStatus::Generating;
    }
    Ok((record, writes))
}

async fn dispatch(state: &AppState, user: &str, record: &SessionRecord) -> ApiResult<()> {
    if record.session.status == SessionStatus::Generating {
        if let Err(error) = ai::dispatch(
            state.clone(),
            WorkerEvent {
                user_id: user.into(),
                session_id: record.session.id.clone(),
            },
        )
        .await
        {
            fail(
                state,
                user,
                &record.session.id,
                &record.generation_id,
                error.1,
            )
            .await?;
        }
    }
    Ok(())
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
    if let Some(previous) = state
        .store
        .get::<SessionRecord>(user, &key(&request.request_id))
        .await?
    {
        // Reject a changed replay before making any migration writes.
        replay(previous.value.clone(), &request)?;
        return replay(
            load(state, user, &request.request_id).await?.value,
            &request,
        );
    }
    let (record, mut writes) = prepare(state, user, &request, now()).await?;
    writes.push(Write::new(key(&request.request_id), &record, None)?);
    match state.store.transaction(user, writes).await {
        Ok(()) => {}
        Err(error) if error.0 == StatusCode::CONFLICT => {
            return replay(
                load(state, user, &request.request_id).await?.value,
                &request,
            );
        }
        Err(error) => return Err(error),
    }
    dispatch(state, user, &record).await?;
    Ok(state
        .store
        .get::<SessionRecord>(user, &key(&record.session.id))
        .await?
        .ok_or_else(ApiError::missing)?
        .value
        .session)
}

const RETIRED_NOTICE: &str = "This session used retired generated formula cards. Your attempts and review history are preserved. Start a new session to practise the fixed handbook formulas.";

/// Conditional replacement fences old queue transitions and workers in the same
/// transaction as its immutable fixed snapshots. No historical rows are removed.
async fn load(state: &AppState, user: &str, id: &str) -> ApiResult<Record<SessionRecord>> {
    for _ in 0..5 {
        let mut stored = state
            .store
            .get::<SessionRecord>(user, &key(id))
            .await?
            .ok_or_else(ApiError::missing)?;
        if stored.value.handbook_version == 1
            || stored.value.session.status == SessionStatus::Finished
        {
            return Ok(stored);
        }
        let record = &mut stored.value;
        let formula = record.session.request.kind == RequestedKind::Formula;
        let review = record.session.request.mode == PracticeMode::Review;
        let mut incompatible = formula || record.plans.iter().any(|p| p.kind == Kind::Flashcard);
        let mut presented_generated = false;
        for attempt_id in &record.session.attempt_ids {
            if let Some(attempt) = state
                .store
                .get::<Attempt>(user, &format!("ATTEMPT#{attempt_id}"))
                .await?
            {
                presented_generated |= attempt.value.question.kind == Kind::Flashcard
                    && !state.bank.iter().any(|q| {
                        practice::is_handbook_card(q) && q.id == attempt.value.question.id
                    });
            }
        }
        for question_key in &record.queue {
            if let Some(question) = state.store.get::<Question>(user, question_key).await? {
                incompatible |= question.value.kind == Kind::Flashcard
                    && !state
                        .bank
                        .iter()
                        .any(|q| practice::is_handbook_card(q) && q.id == question.value.id);
            }
        }
        let mut writes = vec![];
        if record.session.attempt_ids.is_empty() && (formula || review) {
            match prepare(state, user, &record.creation, record.session.created_at).await {
                Ok((replacement, snapshots)) => {
                    *record = replacement;
                    writes = snapshots;
                }
                Err(error)
                    if error.0 == StatusCode::CONFLICT || error.0 == StatusCode::BAD_REQUEST =>
                {
                    record.session.status = SessionStatus::Finished;
                    record.session.error = Some(format!("{RETIRED_NOTICE} {}", error.1));
                    record.plans.clear();
                    record.session.updated_at = now();
                }
                Err(error) => return Err(error),
            }
        } else if presented_generated || incompatible {
            record.session.status = SessionStatus::Finished;
            record.session.error = Some(RETIRED_NOTICE.into());
            record.session.updated_at = now();
            record.plans.clear();
        }
        record.handbook_version = 1;
        writes.push(Write::new(key(id), record, Some(stored.version))?);
        match state.store.transaction(user, writes).await {
            Ok(()) => {
                stored.version += 1;
                return Ok(stored);
            }
            Err(error) if error.0 == StatusCode::CONFLICT => continue,
            Err(error) => return Err(error),
        }
    }
    Err(ApiError::conflict(
        "Session changed concurrently; reload the session",
    ))
}

pub async fn get(state: &AppState, user: &str, id: &str) -> ApiResult<PracticeSession> {
    valid_id(id)?;
    for _ in 0..5 {
        let mut record = load(state, user, id).await?;
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

/// A durable conditional claim precedes every provider request.
pub async fn claim(state: &AppState, user: &str, id: &str) -> ApiResult<Option<SessionRecord>> {
    let mut record = load(state, user, id).await?;
    if record.value.worker_started || record.value.session.status != SessionStatus::Generating {
        return Ok(None);
    }
    if record.value.plans.is_empty() || record.value.plans.iter().any(|p| p.kind != Kind::Task) {
        return Err(ApiError::conflict(
            "Only task generation plans may reach the worker",
        ));
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

fn active_worker(record: &SessionRecord, generation_id: &str) -> ApiResult<()> {
    if record.session.status != SessionStatus::Generating
        || record.handbook_version != 1
        || record.generation_id != generation_id
    {
        return Err(ApiError::conflict(
            "Session preparation is no longer active",
        ));
    }
    Ok(())
}
pub async fn progress(
    state: &AppState,
    user: &str,
    id: &str,
    generation_id: &str,
    generated: u32,
) -> ApiResult<()> {
    let mut record = state
        .store
        .get::<SessionRecord>(user, &key(id))
        .await?
        .ok_or_else(ApiError::missing)?;
    active_worker(&record.value, generation_id)?;
    let fixed = record.value.session.length - record.value.plans.len() as u32;
    record.value.session.generated = fixed + generated;
    record.value.session.updated_at = now();
    state
        .store
        .save(user, &key(id), &record.value, Some(record.version))
        .await
}

pub fn publish_queue(record: &mut SessionRecord, questions: &[Question]) -> ApiResult<()> {
    if record.session.status != SessionStatus::Generating
        || record.handbook_version != 1
        || questions.len() != record.plans.len()
        || record.queue.len() != record.session.length as usize
    {
        return Err(ApiError::conflict(
            "Only a complete active session can be published",
        ));
    }
    let mut prompts = std::collections::HashSet::new();
    let mut slots = std::collections::HashSet::new();
    for (plan, question) in record.plans.iter().zip(questions) {
        if question.kind != Kind::Task
            || plan.kind != Kind::Task
            || plan.slot >= record.queue.len()
            || !record.queue[plan.slot].is_empty()
            || !slots.insert(plan.slot)
        {
            return Err(ApiError::bad("Only planned task slots may be generated"));
        }
        if !prompts.insert(
            question
                .prompt
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase(),
        ) {
            return Err(ApiError::bad("Independent pipelines produced duplicate question prompts; the session was not published"));
        }
    }
    if record
        .queue
        .iter()
        .enumerate()
        .any(|(slot, key)| key.is_empty() && !slots.contains(&slot))
    {
        return Err(ApiError::bad("A prepared question slot is missing"));
    }
    for (plan, question) in record.plans.iter().zip(questions) {
        record.queue[plan.slot] = format!("QUESTION#{}", question.id);
    }
    record.session.generated = record.session.length;
    record.session.status = SessionStatus::Ready;
    record.session.updated_at = now();
    Ok(())
}

pub async fn publish(
    state: &AppState,
    user: &str,
    id: &str,
    generation_id: &str,
    questions: Vec<Question>,
) -> ApiResult<()> {
    let mut record = state
        .store
        .get::<SessionRecord>(user, &key(id))
        .await?
        .ok_or_else(ApiError::missing)?;
    active_worker(&record.value, generation_id)?;
    publish_queue(&mut record.value, &questions)?;
    let mut writes = questions
        .iter()
        .map(|q| Write::new(format!("QUESTION#{}", q.id), q, None))
        .collect::<ApiResult<Vec<_>>>()?;
    writes.push(Write::new(key(id), &record.value, Some(record.version))?);
    state.store.transaction(user, writes).await
}

pub async fn fail(
    state: &AppState,
    user: &str,
    id: &str,
    generation_id: &str,
    message: String,
) -> ApiResult<()> {
    for _ in 0..5 {
        let mut record = state
            .store
            .get::<SessionRecord>(user, &key(id))
            .await?
            .ok_or_else(ApiError::missing)?;
        if active_worker(&record.value, generation_id).is_err() {
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
            record
                .session
                .error
                .clone()
                .unwrap_or_else(|| "Session is not ready for another question".into()),
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
        // Migrate even when replaying an old event, but preserve its exact snapshot.
        let mut record = load(state, user, id).await?;
        if let Some(previous) = state.store.get::<NextEvent>(user, &event_key).await? {
            if previous.value.session_id != id || previous.value.request != request {
                return Err(ApiError::conflict(
                    "eventId was already used for a different session transition",
                ));
            }
            return Ok(previous.value.result);
        }
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
        if question.kind == Kind::Flashcard
            && !state
                .bank
                .iter()
                .any(|q| practice::is_handbook_card(q) && q.id == question.id)
        {
            return Err(ApiError::conflict(RETIRED_NOTICE));
        }
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
        let Record { mut value, version } = load(state, user, id).await?;
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
