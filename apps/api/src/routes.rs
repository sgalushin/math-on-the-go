use crate::{
    actions,
    model::*,
    now,
    practice::{self, CardRecord},
    schedule, sessions,
    store::{ApiError, ApiResult, Write},
    variants, AppState, QuestionFlag, User,
};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(serde_json::json!({"status":"ok"})) }),
        )
        .route("/api/catalog", get(catalog))
        .route("/api/settings", get(settings).put(update_settings))
        .route("/api/progress", get(progress))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{id}", get(session))
        .route("/api/sessions/{id}/next", post(session_next))
        .route("/api/sessions/{id}/finish", post(session_finish))
        .route("/api/attempts/{id}", get(attempt))
        .route("/api/attempts/{id}/actions", post(action))
        .route("/api/history", get(history))
        .route("/api/history/export", get(export))
        .fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error":"Endpoint not found"})),
            )
        })
        .layer(DefaultBodyLimit::max(32_768))
        .layer(axum::middleware::map_response(json_errors))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}
async fn json_errors(response: axum::response::Response) -> axum::response::Response {
    let status = response.status();
    if (status.is_client_error() || status.is_server_error())
        && !response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_some_and(|v| v.as_bytes().starts_with(b"application/json"))
    {
        return (
            status,
            Json(
                serde_json::json!({"error":status.canonical_reason().unwrap_or("Request failed")}),
            ),
        )
            .into_response();
    }
    response
}
async fn catalog(User(_): User, State(s): State<AppState>) -> Json<Catalog> {
    Json(s.catalog.as_ref().clone())
}
async fn settings(User(u): User, State(s): State<AppState>) -> ApiResult<Json<Settings>> {
    Ok(Json(s.settings(&u).await?))
}
async fn update_settings(
    User(u): User,
    State(s): State<AppState>,
    Json(value): Json<Settings>,
) -> ApiResult<Json<Settings>> {
    let unit_ids: HashSet<_> = s.catalog.units.iter().map(|u| &u.id).collect();
    let ids = &value.studied_unit_ids;
    if ids.len() > unit_ids.len()
        || ids.iter().any(|id| !unit_ids.contains(id))
        || ids.iter().collect::<HashSet<_>>().len() != ids.len()
    {
        return Err(ApiError::bad("Unknown or duplicate studied units"));
    }
    if !(1..=20).contains(&value.session_length) {
        return Err(ApiError::bad("Session length must be 1–20"));
    }
    let old = s.store.get::<Settings>(&u, "SETTINGS").await?;
    s.store
        .save(&u, "SETTINGS", &value, old.map(|r| r.version))
        .await?;
    Ok(Json(value))
}
async fn progress(User(u): User, State(s): State<AppState>) -> ApiResult<Json<Progress>> {
    Ok(Json(practice::progress(&s, &u, now()).await?))
}
async fn create_session(
    User(u): User,
    State(s): State<AppState>,
    Json(request): Json<CreateSession>,
) -> ApiResult<(StatusCode, Json<PracticeSession>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(sessions::create(&s, &u, request).await?),
    ))
}
async fn session(
    User(u): User,
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<PracticeSession>> {
    Ok(Json(sessions::get(&s, &u, &id).await?))
}
async fn session_next(
    User(u): User,
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<SessionNextRequest>,
) -> ApiResult<Json<Attempt>> {
    Ok(Json(sessions::next(&s, &u, &id, request).await?))
}
async fn session_finish(
    User(u): User,
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<PracticeSession>> {
    Ok(Json(sessions::finish(&s, &u, &id).await?))
}
async fn attempt(
    User(u): User,
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Attempt>> {
    let mut a = s
        .store
        .get::<Attempt>(&u, &format!("ATTEMPT#{id}"))
        .await?
        .ok_or_else(ApiError::missing)?
        .value;
    if s.store
        .get::<QuestionFlag>(&u, &format!("FLAG#{}", variants::canonical(&a.question.id)))
        .await?
        .is_some()
    {
        a.question.flagged = true;
    }
    Ok(Json(a))
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionEvent {
    attempt_id: String,
    action: serde_json::Value,
    result: Attempt,
}
async fn action(
    User(u): User,
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(event): Json<AttemptAction>,
) -> ApiResult<Json<Attempt>> {
    if uuid::Uuid::parse_str(&event.event_id).is_err() {
        return Err(ApiError::bad("eventId must be a UUID"));
    }
    let event_key = format!("EVENT#{}", event.event_id);
    for _ in 0..5 {
        if let Some(previous) = s.store.get::<ActionEvent>(&u, &event_key).await? {
            if previous.value.attempt_id != id
                || previous.value.action
                    != serde_json::to_value(&event).map_err(|_| ApiError::bad("Invalid action"))?
            {
                return Err(ApiError::conflict(
                    "eventId was already used for another action",
                ));
            }
            return Ok(Json(previous.value.result));
        }
        let key = format!("ATTEMPT#{id}");
        let record = s
            .store
            .get::<Attempt>(&u, &key)
            .await?
            .ok_or_else(ApiError::missing)?;
        let timestamp = now();
        let result = actions::apply(&record.value, &event, timestamp)?;
        if serde_json::to_vec(&result)
            .map_err(|_| ApiError::bad("Invalid attempt"))?
            .len()
            > 250_000
        {
            return Err(ApiError::bad(
                "Attempt has reached its immutable history size limit; start another attempt",
            ));
        }
        let action_record = ActionEvent {
            attempt_id: id.clone(),
            action: serde_json::to_value(&event).map_err(|_| ApiError::bad("Invalid action"))?,
            result: result.clone(),
        };
        let mut writes = vec![
            Write::new(key, &result, Some(record.version))?,
            Write::new(&event_key, &action_record, None)?,
        ];
        if event.action == ActionKind::Grade
            && result.question.kind == Kind::Flashcard
            && s.bank
                .iter()
                .any(|q| practice::is_handbook_card(q) && q.id == result.question.id)
        {
            let card_key = format!("CARD#{}", result.question.id);
            let card = s.store.get::<CardRecord>(&u, &card_key).await?;
            let schedule = card
                .as_ref()
                .map_or_else(schedule::CardState::fresh, |r| r.value.schedule);
            let next = schedule::grade(
                &schedule,
                result.rating.expect("grade validates rating"),
                timestamp,
            )?;
            writes.push(Write::new(
                card_key,
                &CardRecord {
                    question_id: result.question.id.clone(),
                    schedule: next,
                },
                card.map(|r| r.version),
            )?);
        }
        match s.store.transaction(&u, writes).await {
            Ok(()) => return Ok(Json(result)),
            Err(e) if e.0 == StatusCode::CONFLICT => continue,
            Err(e) => return Err(e),
        }
    }
    Err(ApiError::conflict(
        "Concurrent changes; retry the same eventId",
    ))
}
fn filter_history(mut attempts: Vec<Attempt>, q: &HistoryQuery, catalog: &Catalog) -> Vec<Attempt> {
    attempts.retain(|a| {
        q.topic_id
            .as_ref()
            .is_none_or(|x| *x == a.question.topic_id)
            && q.unit_id.as_ref().is_none_or(|x| {
                catalog
                    .units
                    .iter()
                    .any(|u| u.id == *x && u.topics.iter().any(|t| t.id == a.question.topic_id))
            })
            && q.kind.is_none_or(|x| x == a.question.kind)
            && q.outcome.is_none_or(|x| x == a.outcome)
            && q.difficulty.is_none_or(|x| x == a.question.difficulty)
            && q.from.is_none_or(|x| a.started_at >= x)
            && q.to.is_none_or(|x| a.started_at <= x)
    });
    attempts
}
#[derive(Serialize, Deserialize)]
struct Cursor {
    at: i64,
    id: String,
}
async fn history(
    User(u): User,
    State(s): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<HistoryResponse>> {
    let limit = q.limit.unwrap_or(30);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::bad("History limit must be between 1 and 100"));
    }
    if q.from.zip(q.to).is_some_and(|(a, b)| a > b) {
        return Err(ApiError::bad("from must not exceed to"));
    }
    let mut attempts = filter_history(s.attempts(&u).await?, &q, &s.catalog);
    if let Some(cursor) = &q.cursor {
        if cursor.len() > 1024 {
            return Err(ApiError::bad("Invalid history cursor"));
        }
        let cursor: Cursor = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(cursor)
                .map_err(|_| ApiError::bad("Invalid history cursor"))?,
        )
        .map_err(|_| ApiError::bad("Invalid history cursor"))?;
        attempts.retain(|a| (a.started_at, &a.id) < (cursor.at, &cursor.id));
    }
    let has_more = attempts.len() > limit;
    attempts.truncate(limit);
    let next_cursor = if has_more {
        attempts.last().map(|a| {
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&Cursor {
                    at: a.started_at,
                    id: a.id.clone(),
                })
                .expect("cursor serialization"),
            )
        })
    } else {
        None
    };
    Ok(Json(HistoryResponse {
        attempts,
        next_cursor,
    }))
}
async fn export(
    User(u): User,
    State(s): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<impl IntoResponse> {
    let attempts = filter_history(s.attempts(&u).await?, &q, &s.catalog);
    let ids: HashSet<_> = attempts.iter().map(|a| a.id.as_str()).collect();
    let mut actions: Vec<_> = s
        .store
        .list::<ActionEvent>(&u, "EVENT#")
        .await?
        .into_iter()
        .map(|r| r.value)
        .filter(|e| ids.contains(e.attempt_id.as_str()))
        .collect();
    actions.sort_by(|a, b| {
        a.result.updated_at.cmp(&b.result.updated_at).then_with(|| {
            a.action
                .get("eventId")
                .and_then(serde_json::Value::as_str)
                .cmp(&b.action.get("eventId").and_then(serde_json::Value::as_str))
        })
    });
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            "attachment; filename=math-on-the-go-history.json",
        )],
        Json(serde_json::json!({"exportedAt":now(),"attempts":attempts,"actions":actions})),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_report_actions_round_trip_without_reintroducing_a_write_action() {
        let questions: Vec<Question> =
            serde_json::from_str(include_str!("../../../data/questions.json")).unwrap();
        let mut attempt = Attempt::new(questions[0].clone(), 1);
        attempt.reported = true;
        attempt.question.flagged = true;
        let historical = serde_json::json!({
            "attemptId": attempt.id,
            "action": {"action":"report","reason":"Old historical exclusion",
                "eventId":"87a44eaa-c480-41ba-a356-048154f87817","answer":null,
                "rating":null,"selfCorrect":null},
            "result":attempt
        });
        let event: ActionEvent = serde_json::from_value(historical.clone()).unwrap();
        assert_eq!(serde_json::to_value(event).unwrap(), historical);
        assert!(serde_json::from_value::<AttemptAction>(historical["action"].clone()).is_err());
    }
}
