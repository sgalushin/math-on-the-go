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
    let mut task_selection = request.selection.clone();
    task_selection.mode = PracticeMode::Focused;
    task_selection.kind = RequestedKind::Task;
    let generation = SessionRecord {
        session: PracticeSession {
            id: id.clone(),
            status: SessionStatus::Generating,
            request: task_selection.clone(),
            length: 2,
            generated: 0,
            attempt_ids: vec![],
            error: None,
            created_at: math_api::now(),
            updated_at: math_api::now(),
        },
        creation: CreateSession {
            selection: task_selection.clone(),
            request_id: id.clone(),
            length: request.length,
        },
        plans: math_api::practice::generation_plan(
            &state.catalog,
            &settings,
            &task_selection,
            &[],
            2,
        )
        .unwrap(),
        queue: vec![String::new(); 2],
        worker_started: false,
        handbook_version: 1,
        generation_id: uuid::Uuid::new_v4().to_string(),
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

async fn handbook_fixture() -> (AppState, String, CreateSession) {
    let mut state = AppState::connect().await.unwrap();
    state.openrouter_key = None;
    let user = format!("handbook-regression-{}", uuid::Uuid::new_v4());
    let settings = Settings {
        studied_unit_ids: state.catalog.units.iter().map(|u| u.id.clone()).collect(),
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
            mode: PracticeMode::Focused,
            kind: RequestedKind::Formula,
            unit_ids: settings.studied_unit_ids,
            topic_ids: vec![],
            difficulty: RequestedDifficulty::Stretch,
        },
    };
    (state, user, request)
}

fn legacy_formula(request: CreateSession) -> SessionRecord {
    SessionRecord {
        session: PracticeSession {
            id: request.request_id.clone(),
            status: SessionStatus::Failed,
            request: request.selection.clone(),
            length: request.length,
            generated: request.length - 1,
            attempt_ids: vec![],
            error: Some(
                "Independent conceptual solution disagrees with the proposed answer".into(),
            ),
            created_at: math_api::now() - 60_000,
            updated_at: math_api::now() - 30_000,
        },
        plans: vec![math_api::practice::GenerationPlan {
            slot: 0,
            topic_id: "MST124-1-equations".into(),
            kind: Kind::Flashcard,
            difficulty: Difficulty::Standard,
        }],
        creation: request,
        queue: vec![],
        worker_started: true,
        handbook_version: 0,
        generation_id: String::new(),
    }
}

async fn save_legacy(state: &AppState, user: &str, record: &SessionRecord) {
    let mut old = serde_json::to_value(record).unwrap();
    old.as_object_mut().unwrap().remove("handbookVersion");
    old.as_object_mut().unwrap().remove("generationId");
    state
        .store
        .save(user, &sessions::key(&record.session.id), &old, None)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires DYNAMODB_ENDPOINT_URL and TABLE_NAME pointing to an isolated existing test table"]
async fn formulas_open_without_a_provider_and_reuse_the_same_fsrs_card() {
    let (state, user, request) = handbook_fixture().await;
    let first = sessions::create(&state, &user, request.clone())
        .await
        .unwrap();
    assert_eq!(first.status, SessionStatus::Ready);
    assert_eq!((first.length, first.generated), (2, 2));
    assert!(sessions::claim(&state, &user, &first.id)
        .await
        .unwrap()
        .is_none());
    let opening = SessionNextRequest {
        event_id: uuid::Uuid::new_v4().to_string(),
        after_attempt_id: None,
    };
    let initial = sessions::next(&state, &user, &first.id, opening)
        .await
        .unwrap();
    assert!(math_api::practice::is_handbook_card(&initial.question));
    let later = sessions::create(
        &state,
        &user,
        CreateSession {
            request_id: uuid::Uuid::new_v4().to_string(),
            ..request.clone()
        },
    )
    .await
    .unwrap();
    let repeated = sessions::next(
        &state,
        &user,
        &later.id,
        SessionNextRequest {
            event_id: uuid::Uuid::new_v4().to_string(),
            after_attempt_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(initial.question.id, repeated.question.id);
    assert_ne!(initial.id, repeated.id);

    let schedule = math_api::schedule::grade(
        &math_api::schedule::CardState::fresh(),
        Rating::Again,
        math_api::now() - 120_000,
    )
    .unwrap();
    let card = math_api::practice::CardRecord {
        question_id: initial.question.id.clone(),
        schedule,
    };
    let card_key = format!("CARD#{}", initial.question.id);
    state
        .store
        .save(&user, &card_key, &card, None)
        .await
        .unwrap();
    let mut review = request;
    review.request_id = uuid::Uuid::new_v4().to_string();
    review.selection.mode = PracticeMode::Review;
    review.selection.kind = RequestedKind::Mixed;
    review.selection.topic_ids = vec![initial.question.topic_id.clone()];
    review.length = 1;
    let due = sessions::create(&state, &user, review).await.unwrap();
    let recalled = sessions::next(
        &state,
        &user,
        &due.id,
        SessionNextRequest {
            event_id: uuid::Uuid::new_v4().to_string(),
            after_attempt_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(recalled.question.id, initial.question.id);
    let persisted = state
        .store
        .get::<math_api::practice::CardRecord>(&user, &card_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(persisted.value.schedule).unwrap(),
        serde_json::to_value(schedule).unwrap()
    );
}

#[tokio::test]
#[ignore = "requires DYNAMODB_ENDPOINT_URL and TABLE_NAME pointing to an isolated existing test table"]
async fn failed_unpresented_formulas_upgrade_atomically_and_finished_sessions_stay_finished() {
    let (state, user, mut request) = handbook_fixture().await;
    request.length = 5;
    let old = legacy_formula(request.clone());
    save_legacy(&state, &user, &old).await;
    let (a, b) = tokio::join!(
        sessions::get(&state, &user, &old.session.id),
        sessions::get(&state, &user, &old.session.id)
    );
    let a = a.unwrap();
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(b.unwrap()).unwrap()
    );
    assert_eq!(a.status, SessionStatus::Ready);
    assert_eq!((a.length, a.generated), (5, 5));
    assert_eq!(a.created_at, old.session.created_at);
    assert!(a.error.is_none());
    assert!(state.attempts(&user).await.unwrap().is_empty());
    assert_eq!(
        sessions::create(&state, &user, request).await.unwrap().id,
        a.id
    );
    assert!(sessions::progress(&state, &user, &a.id, "", 5)
        .await
        .is_err());
    sessions::fail(&state, &user, &a.id, "", "stale worker failure".into())
        .await
        .unwrap();
    assert_eq!(
        sessions::get(&state, &user, &a.id).await.unwrap().status,
        SessionStatus::Ready
    );
    assert!(sessions::claim(&state, &user, &a.id)
        .await
        .unwrap()
        .is_none());
    let opening = sessions::next(
        &state,
        &user,
        &a.id,
        SessionNextRequest {
            event_id: uuid::Uuid::new_v4().to_string(),
            after_attempt_id: None,
        },
    )
    .await
    .unwrap();
    assert!(math_api::practice::is_handbook_card(&opening.question));

    let mut closed = old;
    closed.session.id = uuid::Uuid::new_v4().to_string();
    closed.creation.request_id = closed.session.id.clone();
    closed.session.status = SessionStatus::Finished;
    closed.session.error = None;
    save_legacy(&state, &user, &closed).await;
    let restored = sessions::get(&state, &user, &closed.session.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(closed.session).unwrap()
    );
}

#[tokio::test]
#[ignore = "requires DYNAMODB_ENDPOINT_URL and TABLE_NAME pointing to an isolated existing test table"]
async fn retired_generated_cards_keep_history_and_legacy_mixed_workers_cannot_publish() {
    let (state, user, request) = handbook_fixture().await;
    let mut old = legacy_formula(request.clone());
    let mut question = state
        .bank
        .iter()
        .find(|q| math_api::practice::is_handbook_card(q))
        .unwrap()
        .clone();
    question.id = format!("ai-{}", uuid::Uuid::new_v4());
    question.verification.method = "self-assessed".into();
    question.verification.models = vec![AUTHOR_MODEL.into(), SOLVER_MODEL.into()];
    let attempt = Attempt::new(question.clone(), math_api::now() - 10_000);
    let question_key = format!("QUESTION#{}", question.id);
    state
        .store
        .save(&user, &question_key, &question, None)
        .await
        .unwrap();
    state
        .store
        .save(&user, &format!("ATTEMPT#{}", attempt.id), &attempt, None)
        .await
        .unwrap();
    old.session.status = SessionStatus::Ready;
    old.session.attempt_ids = vec![attempt.id.clone()];
    old.queue = vec![question_key];
    save_legacy(&state, &user, &old).await;
    let retired = sessions::get(&state, &user, &old.session.id).await.unwrap();
    assert_eq!(retired.status, SessionStatus::Finished);
    assert!(retired.error.is_some());
    assert!(sessions::next(
        &state,
        &user,
        &old.session.id,
        SessionNextRequest {
            event_id: uuid::Uuid::new_v4().to_string(),
            after_attempt_id: Some(attempt.id.clone()),
        }
    )
    .await
    .is_err());
    let history = state.attempts(&user).await.unwrap();
    assert_eq!(
        serde_json::to_value(&history).unwrap(),
        serde_json::to_value(vec![attempt]).unwrap()
    );
    assert!(!state
        .questions(&user)
        .await
        .unwrap()
        .iter()
        .any(|q| q.id == question.id));

    let mut mixed = legacy_formula(CreateSession {
        request_id: uuid::Uuid::new_v4().to_string(),
        ..request
    });
    mixed.creation.selection.kind = RequestedKind::Mixed;
    mixed.session.request.kind = RequestedKind::Mixed;
    mixed.session.status = SessionStatus::Generating;
    save_legacy(&state, &user, &mixed).await;
    assert_eq!(
        sessions::get(&state, &user, &mixed.session.id)
            .await
            .unwrap()
            .status,
        SessionStatus::Finished
    );
    assert!(sessions::claim(&state, &user, &mixed.session.id)
        .await
        .unwrap()
        .is_none());
    assert!(
        sessions::publish(&state, &user, &mixed.session.id, "", vec![])
            .await
            .is_err()
    );
    assert_eq!(state.attempts(&user).await.unwrap().len(), 1);
}

#[tokio::test]
#[ignore = "requires DYNAMODB_ENDPOINT_URL and TABLE_NAME pointing to an isolated existing test table"]
async fn mixed_publication_keeps_handbook_slots_fixed_and_blocks_partial_sessions() {
    let (mut state, user, mut request) = handbook_fixture().await;
    state.openrouter_key = Some("unused-by-preparation".into());
    request.selection.kind = RequestedKind::Mixed;
    request.length = 4;
    let (record, mut writes) = sessions::prepare(&state, &user, &request, math_api::now())
        .await
        .unwrap();
    assert_eq!(
        record.plans.iter().map(|p| p.slot).collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert_eq!(record.session.generated, 2);
    writes.push(
        math_api::store::Write::new(sessions::key(&record.session.id), &record, None).unwrap(),
    );
    state.store.transaction(&user, writes).await.unwrap();
    let opening = SessionNextRequest {
        event_id: uuid::Uuid::new_v4().to_string(),
        after_attempt_id: None,
    };
    assert!(
        sessions::next(&state, &user, &record.session.id, opening.clone())
            .await
            .is_err()
    );
    let mut tasks: Vec<_> = state
        .bank
        .iter()
        .filter(|q| q.kind == Kind::Task)
        .take(2)
        .cloned()
        .collect();
    for question in &mut tasks {
        question.id = format!("ai-{}", uuid::Uuid::new_v4());
    }
    sessions::publish(
        &state,
        &user,
        &record.session.id,
        &record.generation_id,
        tasks,
    )
    .await
    .unwrap();
    let ready = sessions::get(&state, &user, &record.session.id)
        .await
        .unwrap();
    assert_eq!(
        (ready.status, ready.generated, ready.length),
        (SessionStatus::Ready, 4, 4)
    );
    let mut next = opening;
    for kind in [Kind::Task, Kind::Flashcard, Kind::Task, Kind::Flashcard] {
        let mut attempt = sessions::next(&state, &user, &record.session.id, next)
            .await
            .unwrap();
        assert_eq!(attempt.question.kind, kind);
        if kind == Kind::Flashcard {
            assert!(math_api::practice::is_handbook_card(&attempt.question));
        }
        let attempt_key = format!("ATTEMPT#{}", attempt.id);
        let stored = state
            .store
            .get::<Attempt>(&user, &attempt_key)
            .await
            .unwrap()
            .unwrap();
        attempt.outcome = Outcome::Skipped;
        attempt.completed_at = Some(math_api::now());
        state
            .store
            .save(&user, &attempt_key, &attempt, Some(stored.version))
            .await
            .unwrap();
        next = SessionNextRequest {
            event_id: uuid::Uuid::new_v4().to_string(),
            after_attempt_id: Some(attempt.id),
        };
    }
}
