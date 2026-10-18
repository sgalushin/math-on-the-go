use math_api::{
    actions, grading,
    model::*,
    practice::{eligible_topics, formula_queue, generation_plan, review_queue, topic_stats},
    schedule::{self, CardState},
    variants,
};
use std::collections::HashMap;
fn catalog() -> Catalog {
    Catalog {
        units: vec![Unit {
            id: "u".into(),
            module: "MST124".into(),
            number: 1,
            title: "Algebra".into(),
            source_page: 1,
            topics: vec![
                Topic {
                    id: "weak".into(),
                    title: "Weak".into(),
                    prerequisites: vec![],
                },
                Topic {
                    id: "strong".into(),
                    title: "Strong".into(),
                    prerequisites: vec![],
                },
            ],
        }],
    }
}
fn question(id: &str, topic: &str) -> Question {
    Question {
        id: id.into(),
        topic_id: topic.into(),
        kind: Kind::Task,
        difficulty: Difficulty::Standard,
        prompt: "Evaluate $2x+1$ at $x=3$.".into(),
        answer: "7".into(),
        accepted_answers: vec![],
        choices: vec![],
        hints: vec!["Substitute x.".into()],
        explanation: "2(3)+1=7".into(),
        answer_format: AnswerFormat::Text,
        source: Source {
            handbook: "MST124".into(),
            page: 1,
            section: "Algebra".into(),
        },
        verification: Verification {
            method: "deterministic".into(),
            models: vec![],
            checked_at: None,
        },
        graph: None,
        flagged: false,
        template: Some(Template {
            family: "linear-evaluation".into(),
            parameters: HashMap::from([("a".into(), 2.0), ("b".into(), 1.0), ("x".into(), 3.0)]),
        }),
    }
}
fn handbook_card(id: &str, topic: &str) -> Question {
    let mut q = question(id, topic);
    q.kind = Kind::Flashcard;
    q.difficulty = Difficulty::Quick;
    q.answer_format = AnswerFormat::SelfAssessed;
    q.source.handbook = "MST124 Handbook".into();
    q.verification.method = "handbook".into();
    q.template = None;
    q
}
fn attempt(id: &str, topic: &str, outcome: Outcome, at: i64) -> Attempt {
    let mut a = Attempt::new(question(id, topic), at);
    a.outcome = outcome;
    if outcome != Outcome::Unfinished {
        a.completed_at = Some(at);
    }
    a
}
fn settings() -> Settings {
    Settings {
        studied_unit_ids: vec!["u".into()],
        ..Settings::default()
    }
}
fn request(mode: PracticeMode) -> PracticeSelection {
    PracticeSelection {
        mode,
        kind: RequestedKind::Task,
        unit_ids: vec!["u".into()],
        topic_ids: vec![],
        difficulty: RequestedDifficulty::Any,
    }
}

#[test]
fn flags_self_assessment_and_flashcards_never_inflate_auto_accuracy() {
    let mut attempts = vec![
        attempt("a", "weak", Outcome::Independent, 10),
        attempt("b", "weak", Outcome::Assisted, 20),
        attempt("c", "weak", Outcome::Incorrect, 30),
    ];
    let mut flagged = attempt("d", "weak", Outcome::Independent, 40);
    flagged.question.flagged = true;
    attempts.push(flagged);
    let mut self_assessed = attempt("e", "weak", Outcome::SelfAssessed, 50);
    self_assessed.assessment = Assessment::SelfAssessed;
    attempts.push(self_assessed);
    let mut flash = attempt("f", "weak", Outcome::Independent, 60);
    flash.question.kind = Kind::Flashcard;
    attempts.push(flash);
    let stats = topic_stats(&catalog(), &attempts);
    let t = &stats[0];
    assert_eq!(
        (
            t.attempts,
            t.independent,
            t.assisted,
            t.incorrect,
            t.self_assessed,
            t.flagged
        ),
        (4, 1, 1, 1, 1, 1)
    );
    assert_eq!(t.recent_accuracy, Some(1.0 / 3.0));
    assert_eq!(t.by_difficulty[&Difficulty::Standard].attempts, 4);
    assert_eq!(stats[1].recent_accuracy, None);
}
#[test]
fn trend_uses_latest_ten_and_previous_ten_without_dropping_history() {
    let mut attempts = vec![];
    for n in 0..35 {
        attempts.push(attempt(
            &format!("a{n}"),
            "weak",
            if n >= 25 {
                Outcome::Independent
            } else {
                Outcome::Incorrect
            },
            n,
        ));
    }
    let stats = topic_stats(&catalog(), &attempts);
    assert_eq!(stats[0].attempts, 35);
    assert_eq!(stats[0].recent_accuracy, Some(1.0));
    assert_eq!(stats[0].previous_accuracy, Some(0.0));
}
#[test]
fn suggested_tasks_follow_topic_performance_not_fsrs() {
    let attempts = vec![
        attempt("a", "weak", Outcome::Incorrect, 1),
        attempt("b", "strong", Outcome::Independent, 2),
    ];
    let plans = generation_plan(
        &catalog(),
        &settings(),
        &request(PracticeMode::Suggested),
        &attempts,
        3,
    )
    .unwrap();
    assert_eq!(plans[0].topic_id, "weak");
    assert_eq!(plans[1].topic_id, "strong");
    assert_eq!(plans.len(), 3);
}
#[test]
fn review_is_due_or_new_flashcards_only() {
    let task = question("task", "weak");
    let flash = handbook_card("formula-MST124-1-flash", "weak");
    let mut r = request(PracticeMode::Review);
    r.kind = RequestedKind::Mixed;
    let eligible = eligible_topics(&catalog(), &settings(), &r.unit_ids, &r.topic_ids);
    let card = schedule::grade(&CardState::fresh(), Rating::Good, 100).unwrap();
    let cards = HashMap::from([(flash.id.clone(), card)]);
    assert!(review_queue(&[task.clone(), flash.clone()], &eligible, &cards, 101, 20).is_err());
    // Difficulty never hides a due review card; a session is finite and distinct.
    r.difficulty = RequestedDifficulty::Stretch;
    let chosen = review_queue(
        &[task, flash.clone(), flash.clone()],
        &eligible,
        &cards,
        card.due,
        20,
    )
    .unwrap();
    assert_eq!(chosen.len(), 1);
    assert_eq!(chosen[0].id, "formula-MST124-1-flash");
    assert_eq!(
        review_queue(&[flash], &eligible, &HashMap::new(), 101, 20)
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn no_eligibility_fallback_and_topic_narrowing() {
    let c = catalog();
    let mut s = settings();
    s.studied_unit_ids.clear();
    assert!(eligible_topics(&c, &s, &["u".into()], &[]).is_empty());
    assert!(generation_plan(&c, &s, &request(PracticeMode::Focused), &[], 3).is_err());
}
#[test]
fn deterministic_family_is_reconstructed_not_string_substituted() {
    let q = question("q", "weak");
    let v = variants::reconstruct(&q);
    let again = variants::reconstruct(&q);
    assert_eq!(v.id, again.id);
    assert_eq!(v.prompt, again.prompt);
    assert_eq!(v.answer, again.answer);
    assert_ne!(v.id, q.id);
    let p = &v.template.as_ref().unwrap().parameters;
    let expected = p["a"] * p["x"] + p["b"];
    assert_eq!(grading::numeric(&v.answer), Some(expected));
}

#[test]
fn numeric_templates_reject_nonmental_or_undefined_parameters() {
    let mut q = question("q", "weak");
    for (family, parameters) in [
        ("power", vec![("base", -2.0), ("exponent", 2.0)]),
        ("power", vec![("base", 3.0), ("exponent", 30.0)]),
        (
            "arithmetic-sequence",
            vec![("a", 1.0), ("d", 2.0), ("n", 2.5)],
        ),
        (
            "integral-power",
            vec![("a", 1.0), ("n", -2.0), ("lower", -1.0), ("upper", 1.0)],
        ),
        ("linear-equation", vec![("a", 0.0), ("b", 1.0), ("x", 2.0)]),
    ] {
        q.template = Some(Template {
            family: family.into(),
            parameters: parameters.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        });
        assert_eq!(variants::reconstruct(&q).id, q.id, "{family}");
    }
}
#[test]
fn exact_choices_do_not_accept_numeric_or_symbolic_guesswork() {
    let mut q = question("q", "weak");
    q.answer_format = AnswerFormat::Choice;
    q.answer = "1/2".into();
    q.choices = vec!["1/2".into(), "2".into()];
    assert_eq!(grading::correct(&q, "0.5"), Some(false));
    assert_eq!(grading::correct(&q, "1/2"), Some(true));
    q.answer_format = AnswerFormat::SelfAssessed;
    assert_eq!(grading::correct(&q, "1/2"), None);
}
#[test]
fn immutable_outcome_preserves_wrong_submission_after_reveal() {
    let a = Attempt::new(question("q", "weak"), 1);
    let e = AttemptAction {
        action: ActionKind::Answer,
        answer: Some("999".into()),
        rating: None,
        self_correct: None,
        event_id: uuid::Uuid::new_v4().to_string(),
    };
    let a = actions::apply(&a, &e, 2).unwrap();
    let reveal = AttemptAction {
        action: ActionKind::Reveal,
        event_id: uuid::Uuid::new_v4().to_string(),
        ..e.clone()
    };
    let after = actions::apply(&a, &reveal, 3).unwrap();
    assert_eq!(after.outcome, Outcome::Incorrect);
    assert_eq!(after.completed_at, Some(2));
    assert_eq!(after.submissions[0].answer, "999");
    assert_eq!(after.submissions[0].correct, Some(false));
    assert!(actions::apply(&after, &e, 4).is_err());
}

#[test]
fn settings_ignore_old_model_fields_without_resetting_preferences() {
    let settings: Settings = serde_json::from_value(serde_json::json!({
        "studiedUnitIds":["u"], "defaultDifficulty":"stretch", "sessionLength":17,
        "generatorModel":"old/custom-author", "verifierModel":"old/custom-solver"
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(settings).unwrap(),
        serde_json::json!({
            "studiedUnitIds":["u"], "defaultDifficulty":"stretch", "sessionLength":17
        })
    );
}

#[test]
fn fixed_formulas_prioritize_due_new_future_ignore_task_difficulty_and_private_cards() {
    let mut r = request(PracticeMode::Focused);
    r.kind = RequestedKind::Formula;
    r.difficulty = RequestedDifficulty::Stretch;
    r.topic_ids = vec!["weak".into()];
    let eligible = eligible_topics(&catalog(), &settings(), &r.unit_ids, &r.topic_ids);
    assert!(generation_plan(&catalog(), &settings(), &r, &[], 20).is_err());
    let due = handbook_card("formula-MST124-1-due", "weak");
    let new = handbook_card("formula-MST124-1-new", "weak");
    let future = handbook_card("formula-MST124-1-future", "weak");
    let excluded = handbook_card("formula-MST124-1-outside", "strong");
    let mut private = due.clone();
    private.id = "ai-old-formula".into();
    let mut due_state = CardState::fresh();
    due_state.due = 50;
    let mut future_state = CardState::fresh();
    future_state.due = 200;
    let cards = HashMap::from([
        (due.id.clone(), due_state),
        (future.id.clone(), future_state),
    ]);
    let bank = vec![future, new, due, excluded, private];
    let queue = formula_queue(&bank, &eligible, &cards, 100, 20, true).unwrap();
    assert_eq!(
        queue.iter().map(|q| q.id.as_str()).collect::<Vec<_>>(),
        [
            "formula-MST124-1-due",
            "formula-MST124-1-new",
            "formula-MST124-1-future"
        ]
    );
    let review = review_queue(&bank, &eligible, &cards, 100, 20).unwrap();
    assert_eq!(review.len(), 2);
    assert_eq!(review[0].id, queue[0].id);
}

#[test]
fn mixed_generation_plans_are_tasks_only_and_keep_difficulty_and_scope() {
    let mut r = request(PracticeMode::Suggested);
    r.kind = RequestedKind::Mixed;
    r.topic_ids = vec!["strong".into()];
    let plans = generation_plan(&catalog(), &settings(), &r, &[], 6).unwrap();
    assert!(plans
        .iter()
        .all(|p| p.topic_id == "strong" && p.kind == Kind::Task));
    assert_eq!(
        plans
            .iter()
            .map(|p| p.difficulty)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
    r.topic_ids.push("not-in-scope".into());
    assert!(generation_plan(&catalog(), &settings(), &r, &[], 6).is_err());
}

#[tokio::test]
async fn all_twenty_author_solver_pipelines_overlap_and_queue_keeps_slot_order() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    let authors = Arc::new(tokio::sync::Barrier::new(20));
    let solvers = Arc::new(tokio::sync::Barrier::new(20));
    let author_count = Arc::new(AtomicUsize::new(0));
    let solver_count = Arc::new(AtomicUsize::new(0));
    let pipelines: Vec<_> = (0..20)
        .map(|slot| {
            let (authors, solvers, author_count, solver_count) = (
                authors.clone(),
                solvers.clone(),
                author_count.clone(),
                solver_count.clone(),
            );
            async move {
                // This barrier times out if the implementation awaits any pipeline
                // before starting all N authors, or introduces a smaller cap.
                author_count.fetch_add(1, Ordering::SeqCst);
                authors.wait().await;
                solver_count.fetch_add(1, Ordering::SeqCst);
                solvers.wait().await;
                tokio::time::sleep(Duration::from_millis((20 - slot) as u64)).await;
                let mut q = question(&format!("q{slot}"), "weak");
                q.prompt = format!("Distinct slot {slot}");
                Ok(q)
            }
        })
        .collect();
    let mut counts = vec![];
    let questions = tokio::time::timeout(
        Duration::from_secs(2),
        math_api::ai::prepare_parallel(pipelines, |count| {
            counts.push(count);
            std::future::ready(Ok(()))
        }),
    )
    .await
    .expect("all twenty pipelines must run concurrently")
    .unwrap();
    assert_eq!(author_count.load(Ordering::SeqCst), 20);
    assert_eq!(solver_count.load(Ordering::SeqCst), 20);
    assert_eq!(counts, (1..=20).collect::<Vec<_>>());
    assert_eq!(
        questions.iter().map(|q| q.id.clone()).collect::<Vec<_>>(),
        (0..20).map(|slot| format!("q{slot}")).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_rejected_slot_fails_the_full_queue_without_shorter_fallback() {
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(4));
    let pipelines: Vec<_> = (0..4)
        .map(|slot| {
            let barrier = barrier.clone();
            async move {
                barrier.wait().await;
                if slot == 1 {
                    return Err(math_api::store::ApiError::bad(
                        "Independent task solution disagrees",
                    ));
                }
                Ok(question(&format!("q{slot}"), "weak"))
            }
        })
        .collect();
    let mut counts = vec![];
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        math_api::ai::prepare_parallel(pipelines, |count| {
            counts.push(count);
            std::future::ready(Ok(()))
        }),
    )
    .await
    .unwrap();
    assert!(result.is_err());
    assert_eq!(counts, vec![1, 2, 3]);
}

fn preparing_session(length: u32) -> math_api::sessions::SessionRecord {
    let selection = request(PracticeMode::Focused);
    let id = uuid::Uuid::new_v4().to_string();
    let plans = generation_plan(&catalog(), &settings(), &selection, &[], length).unwrap();
    math_api::sessions::SessionRecord {
        session: PracticeSession {
            id: id.clone(),
            status: SessionStatus::Generating,
            request: selection.clone(),
            length,
            generated: 0,
            attempt_ids: vec![],
            error: None,
            created_at: 1,
            updated_at: 1,
        },
        creation: CreateSession {
            selection,
            request_id: id,
            length,
        },
        plans,
        queue: vec![String::new(); length as usize],
        worker_started: true,
        handbook_version: 1,
        generation_id: uuid::Uuid::new_v4().to_string(),
    }
}

#[test]
fn publication_is_complete_unique_ordered_and_does_not_create_attempts() {
    let mut record = preparing_session(2);
    let first = question("first", "weak");
    let mut second = question("second", "weak");
    second.prompt = "A different verified prompt".into();
    assert!(math_api::sessions::publish_queue(&mut record, std::slice::from_ref(&first)).is_err());
    assert_eq!(record.session.status, SessionStatus::Generating);
    assert!(record.queue.iter().all(String::is_empty));
    assert!(
        math_api::sessions::publish_queue(&mut record, &[first.clone(), first.clone()]).is_err()
    );
    assert!(record.queue.iter().all(String::is_empty));
    math_api::sessions::publish_queue(&mut record, &[first, second]).unwrap();
    assert_eq!(record.session.status, SessionStatus::Ready);
    assert_eq!(record.session.generated, 2);
    assert_eq!(record.queue, ["QUESTION#first", "QUESTION#second"]);
    assert!(record.session.attempt_ids.is_empty());
    assert!(math_api::sessions::publish_queue(&mut record, &[]).is_err());
}

#[test]
fn next_requires_exact_predecessor_and_terminal_outcome_and_never_reuses_queue_slots() {
    let mut record = preparing_session(2);
    let mut first = question("first", "weak");
    first.prompt = "First".into();
    let mut second = question("second", "weak");
    second.prompt = "Second".into();
    let request = SessionNextRequest {
        event_id: uuid::Uuid::new_v4().to_string(),
        after_attempt_id: None,
    };
    assert!(math_api::sessions::next_slot(&record, &request, None).is_err());
    math_api::sessions::publish_queue(&mut record, &[first.clone(), second.clone()]).unwrap();
    assert_eq!(
        math_api::sessions::next_slot(&record, &request, None).unwrap(),
        0
    );
    let mut current = Attempt::new(first, 1);
    record.session.attempt_ids.push(current.id.clone());
    assert!(math_api::sessions::next_slot(&record, &request, Some(&current)).is_err());
    let next = SessionNextRequest {
        after_attempt_id: Some(current.id.clone()),
        ..request
    };
    assert!(math_api::sessions::next_slot(&record, &next, Some(&current)).is_err());
    current.outcome = Outcome::Skipped;
    assert_eq!(
        math_api::sessions::next_slot(&record, &next, Some(&current)).unwrap(),
        1
    );
    let mut last = Attempt::new(second, 2);
    last.outcome = Outcome::Independent;
    record.session.attempt_ids.push(last.id.clone());
    let end = SessionNextRequest {
        after_attempt_id: Some(last.id.clone()),
        ..next
    };
    assert!(math_api::sessions::next_slot(&record, &end, Some(&last)).is_err());
}
