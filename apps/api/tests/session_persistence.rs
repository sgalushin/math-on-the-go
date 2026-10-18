//! Run explicitly against an existing isolated DynamoDB test table.
use math_api::{
    model::*,
    sessions::{self, SessionRecord},
    AppState,
};

#[tokio::test]
#[ignore = "requires DYNAMODB_ENDPOINT_URL and TABLE_NAME pointing to an isolated existing test table"]
async fn dynamodb_session_create_claim_next_and_finish_are_idempotent() {
    let state = AppState::connect().await.unwrap();
    let user = format!("session-regression-{}", uuid::Uuid::new_v4());
    let settings = Settings {
        studied_unit_ids: state
            .catalog
            .units
            .iter()
            .map(|unit| unit.id.clone())
            .collect(),
        ..Settings::default()
    };
    state
        .store
        .save(&user, "SETTINGS", &settings, None)
        .await
        .unwrap();
    let request = CreateSession {
        request_id: uuid::Uuid::new_v4().to_string(),
        length: 2,
        selection: PracticeSelection {
            mode: PracticeMode::Review,
            kind: RequestedKind::Mixed,
            unit_ids: settings.studied_unit_ids.clone(),
            topic_ids: vec![],
            difficulty: RequestedDifficulty::Stretch,
        },
    };
    let (first, duplicate) = tokio::join!(
        sessions::create(&state, &user, request.clone()),
        sessions::create(&state, &user, request.clone()),
    );
    let first = first.unwrap();
    assert_eq!(
        serde_json::to_value(first.clone()).unwrap(),
        serde_json::to_value(duplicate.unwrap()).unwrap()
    );
    assert_eq!(first.status, SessionStatus::Ready);
    assert_eq!(first.length, 2);
    assert!(first.attempt_ids.is_empty());
    assert!(state.attempts(&user).await.unwrap().is_empty());
    let mut changed = request.clone();
    changed.length = 3;
    assert_eq!(
        sessions::create(&state, &user, changed)
            .await
            .unwrap_err()
            .0
            .as_u16(),
        409
    );

    let next = SessionNextRequest {
        event_id: uuid::Uuid::new_v4().to_string(),
        after_attempt_id: None,
    };
    let (attempt, repeated) = tokio::join!(
        sessions::next(&state, &user, &first.id, next.clone()),
        sessions::next(&state, &user, &first.id, next.clone()),
    );
    let attempt = attempt.unwrap();
    assert_eq!(
        serde_json::to_value(&attempt).unwrap(),
        serde_json::to_value(repeated.unwrap()).unwrap()
    );
    assert_eq!(state.attempts(&user).await.unwrap().len(), 1);
    let early = SessionNextRequest {
        event_id: uuid::Uuid::new_v4().to_string(),
        after_attempt_id: Some(attempt.id.clone()),
    };
    assert_eq!(
        sessions::next(&state, &user, &first.id, early.clone())
            .await
            .unwrap_err()
            .0
            .as_u16(),
        409
    );
    let attempt_key = format!("ATTEMPT#{}", attempt.id);
    let mut stored = state
        .store
        .get::<Attempt>(&user, &attempt_key)
        .await
        .unwrap()
        .unwrap();
    stored.value.outcome = Outcome::Skipped;
    stored.value.completed_at = Some(math_api::now());
    state
        .store
        .save(&user, &attempt_key, &stored.value, Some(stored.version))
        .await
        .unwrap();
    let second = sessions::next(&state, &user, &first.id, early)
        .await
        .unwrap();
    assert_ne!(second.question.id, attempt.question.id);
    assert_eq!(state.attempts(&user).await.unwrap().len(), 2);
    // Event replay returns the exact original snapshot, even after its attempt
    // has changed and the session has advanced.
    assert_eq!(
        serde_json::to_value(
            sessions::next(&state, &user, &first.id, next)
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&attempt).unwrap()
    );
    let finished = sessions::finish(&state, &user, &first.id).await.unwrap();
    assert_eq!(finished.status, SessionStatus::Finished);
    assert_eq!(
        serde_json::to_value(&finished).unwrap(),
        serde_json::to_value(sessions::finish(&state, &user, &first.id).await.unwrap()).unwrap()
    );
    assert_eq!(state.attempts(&user).await.unwrap().len(), 2);

    let id = uuid::Uuid::new_v4().to_string();
    let generation = SessionRecord {
        session: PracticeSession {
            id: id.clone(),
            status: SessionStatus::Generating,
            request: request.selection.clone(),
            length: 2,
            generated: 0,
            attempt_ids: vec![],
            error: None,
            created_at: math_api::now(),
            updated_at: math_api::now(),
        },
        creation: CreateSession {
            request_id: id.clone(),
            ..request
        },
        plans: vec![],
        queue: vec![],
        worker_started: false,
    };
    state
        .store
        .save(&user, &sessions::key(&id), &generation, None)
        .await
        .unwrap();
    let (claim, duplicate_claim) = tokio::join!(
        sessions::claim(&state, &user, &id),
        sessions::claim(&state, &user, &id),
    );
    assert_eq!(
        u8::from(claim.unwrap().is_some()) + u8::from(duplicate_claim.unwrap().is_some()),
        1
    );
    assert!(sessions::claim(&state, &user, &id).await.unwrap().is_none());
    let interrupted_id = uuid::Uuid::new_v4().to_string();
    let mut interrupted = generation.clone();
    interrupted.session.id = interrupted_id.clone();
    interrupted.creation.request_id = interrupted_id.clone();
    interrupted.session.updated_at = math_api::now() - math_api::ai::STALE_SESSION_MILLIS - 1;
    state
        .store
        .save(&user, &sessions::key(&interrupted_id), &interrupted, None)
        .await
        .unwrap();
    let expired = sessions::get(&state, &user, &interrupted_id).await.unwrap();
    assert_eq!(expired.status, SessionStatus::Failed);
    assert!(expired.attempt_ids.is_empty());
    assert!(expired.error.is_some());
    // A duplicate worker must return before touching providers, even with no key.
    let mut unconfigured = state.clone();
    unconfigured.openrouter_key = None;
    math_api::ai::run_session(
        &unconfigured,
        WorkerEvent {
            user_id: user,
            session_id: id,
        },
    )
    .await
    .unwrap();
}
