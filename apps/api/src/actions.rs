use crate::{
    grading,
    model::*,
    store::{ApiError, ApiResult},
};

pub fn apply(attempt: &Attempt, action: &AttemptAction, now: i64) -> ApiResult<Attempt> {
    if uuid::Uuid::parse_str(&action.event_id).is_err() {
        return Err(ApiError::bad("eventId must be a UUID"));
    }
    if action.answer.as_ref().is_some_and(|s| s.len() > 4096) {
        return Err(ApiError::bad("Answer is too long"));
    }
    let mut a = attempt.clone();
    if a.outcome != Outcome::Unfinished && action.action != ActionKind::Reveal {
        return Err(ApiError::conflict(
            "This attempt is already complete; its outcome cannot be changed",
        ));
    }
    match action.action {
        ActionKind::Hint => {
            if a.hints_used >= a.question.hints.len() {
                return Err(ApiError::conflict("No more hints available"));
            }
            a.hints_used += 1;
        }
        ActionKind::Reveal => a.solution_revealed = true,
        ActionKind::Answer => {
            let answer = action
                .answer
                .as_ref()
                .filter(|x| !x.trim().is_empty())
                .ok_or_else(|| ApiError::bad("Answer is required"))?;
            let correct = grading::correct(&a.question, answer);
            a.submissions.push(Submission {
                answer: answer.clone(),
                at: now,
                correct,
            });
            if a.question.kind == Kind::Task
                && a.question.answer_format != AnswerFormat::SelfAssessed
            {
                a.outcome = if correct == Some(true) {
                    if a.hints_used > 0 || a.solution_revealed {
                        Outcome::Assisted
                    } else {
                        Outcome::Independent
                    }
                } else {
                    Outcome::Incorrect
                };
                a.completed_at = Some(now);
            }
        }
        ActionKind::Grade => {
            if !a.solution_revealed {
                return Err(ApiError::conflict(
                    "Reveal the worked answer before grading",
                ));
            }
            if a.question.kind == Kind::Flashcard {
                let rating = action
                    .rating
                    .ok_or_else(|| ApiError::bad("A flashcard rating is required"))?;
                a.rating = Some(rating);
                a.outcome = match rating {
                    Rating::Again => Outcome::Incorrect,
                    Rating::Good => {
                        if a.hints_used > 0 {
                            Outcome::Assisted
                        } else {
                            Outcome::Independent
                        }
                    }
                };
            } else {
                if a.question.answer_format != AnswerFormat::SelfAssessed {
                    return Err(ApiError::bad("Automatic tasks cannot be self graded"));
                }
                let correct = action
                    .self_correct
                    .ok_or_else(|| ApiError::bad("selfCorrect is required"))?;
                let submission = a.submissions.last_mut().ok_or_else(|| {
                    ApiError::conflict("Record your reasoning before revealing and assessing it")
                })?;
                submission.correct = Some(correct);
                a.outcome = Outcome::SelfAssessed;
            }
            a.completed_at = Some(now);
        }
        ActionKind::Skip => {
            a.outcome = Outcome::Skipped;
            a.completed_at = Some(now);
        }
    }
    a.updated_at = now;
    Ok(a)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Attempt {
        Attempt::new(
            Question {
                id: "q".into(),
                topic_id: "t".into(),
                kind: Kind::Task,
                difficulty: Difficulty::Quick,
                prompt: "1+1".into(),
                answer: "2".into(),
                accepted_answers: vec![],
                choices: vec![],
                hints: vec!["Add".into()],
                explanation: "two".into(),
                answer_format: AnswerFormat::Text,
                source: Source {
                    handbook: "h".into(),
                    page: 1,
                    section: "s".into(),
                },
                verification: Verification {
                    method: "deterministic".into(),
                    models: vec![],
                    checked_at: None,
                },
                graph: None,
                flagged: false,
                template: None,
            },
            1,
        )
    }
    fn event(action: ActionKind) -> AttemptAction {
        AttemptAction {
            action,
            answer: None,
            rating: None,
            self_correct: None,
            event_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    #[test]
    fn terminal_wrong_is_preserved() {
        let a = fixture();
        let mut e = event(ActionKind::Answer);
        e.answer = Some("3".into());
        let a = apply(&a, &e, 2).unwrap();
        assert_eq!(a.outcome, Outcome::Incorrect);
        e.answer = Some("2".into());
        assert!(apply(&a, &e, 3).is_err());
        let a = apply(&a, &event(ActionKind::Reveal), 3).unwrap();
        assert_eq!(a.submissions[0].answer, "3");
        assert_eq!(a.completed_at, Some(2));
    }
    #[test]
    fn hint_changes_correct_outcome() {
        let a = apply(&fixture(), &event(ActionKind::Hint), 2).unwrap();
        let mut e = event(ActionKind::Answer);
        e.answer = Some("4/2".into());
        assert_eq!(apply(&a, &e, 3).unwrap().outcome, Outcome::Assisted);
    }
    #[test]
    fn reveal_required_but_not_assistance_for_flashcard() {
        let mut a = fixture();
        a.question.kind = Kind::Flashcard;
        a.question.answer_format = AnswerFormat::SelfAssessed;
        let mut e = event(ActionKind::Grade);
        e.rating = Some(Rating::Good);
        assert!(apply(&a, &e, 2).is_err());
        let a = apply(&a, &event(ActionKind::Reveal), 2).unwrap();
        assert_eq!(apply(&a, &e, 3).unwrap().outcome, Outcome::Independent);
    }
    #[test]
    fn no_synthetic_self_correctness() {
        let mut a = fixture();
        a.question.answer_format = AnswerFormat::SelfAssessed;
        let a = apply(&a, &event(ActionKind::Reveal), 2).unwrap();
        let mut e = event(ActionKind::Grade);
        e.self_correct = Some(true);
        assert!(apply(&a, &e, 3).is_err());
    }
}
