use crate::{
    model::*,
    schedule::CardState,
    store::{ApiError, ApiResult},
    variants, AppState,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardRecord {
    pub question_id: String,
    pub schedule: CardState,
}
pub fn eligible_topics(
    catalog: &Catalog,
    settings: &Settings,
    units: &[String],
    topics: &[String],
) -> HashSet<String> {
    catalog
        .units
        .iter()
        .filter(|u| settings.studied_unit_ids.contains(&u.id) && units.contains(&u.id))
        .flat_map(|u| u.topics.iter())
        .filter(|t| {
            (topics.is_empty() || topics.contains(&t.id))
                && t.prerequisites
                    .iter()
                    .all(|p| settings.studied_unit_ids.contains(p))
        })
        .map(|t| t.id.clone())
        .collect()
}
pub fn topic_stats(catalog: &Catalog, attempts: &[Attempt]) -> Vec<TopicProgress> {
    let mut result: Vec<_> = catalog
        .units
        .iter()
        .flat_map(|u| u.topics.iter())
        .map(|t| TopicProgress::new(t.id.clone()))
        .collect();
    for topic in &mut result {
        let mut relevant: Vec<_> = attempts
            .iter()
            .filter(|a| a.question.topic_id == topic.topic_id && a.question.kind == Kind::Task)
            .collect();
        relevant.sort_by(|a, b| {
            b.started_at
                .cmp(&a.started_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        let mut accuracy = vec![];
        for a in relevant {
            if a.question.flagged || a.reported {
                topic.flagged += 1;
                continue;
            }
            topic.attempts += 1;
            topic.last_practised = Some(topic.last_practised.unwrap_or(0).max(a.started_at));
            let d = topic
                .by_difficulty
                .get_mut(&a.question.difficulty)
                .expect("all difficulties initialized");
            d.attempts += 1;
            match a.outcome {
                Outcome::Independent => {
                    topic.independent += 1;
                    d.independent += 1
                }
                Outcome::Assisted => {
                    topic.assisted += 1;
                    d.assisted += 1
                }
                Outcome::Incorrect => {
                    topic.incorrect += 1;
                    d.incorrect += 1
                }
                Outcome::SelfAssessed => topic.self_assessed += 1,
                Outcome::Skipped => topic.skipped += 1,
                Outcome::Unfinished => topic.unfinished += 1,
            }
            if a.assessment == Assessment::Automatic
                && matches!(
                    a.outcome,
                    Outcome::Independent | Outcome::Assisted | Outcome::Incorrect
                )
            {
                accuracy.push(a.outcome == Outcome::Independent)
            }
        }
        topic.recent_accuracy = ratio(&accuracy[..accuracy.len().min(10)]);
        if accuracy.len() > 10 {
            topic.previous_accuracy = ratio(&accuracy[10..accuracy.len().min(20)]);
        }
    }
    result
}
fn ratio(values: &[bool]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().filter(|&&v| v).count() as f64 / values.len() as f64)
}
pub async fn progress(state: &AppState, user: &str, now: i64) -> ApiResult<Progress> {
    let settings = state.settings(user).await?;
    let attempts = state.attempts(user).await?;
    let questions = state.questions(user).await?;
    let cards: HashMap<_, _> = state
        .store
        .list::<CardRecord>(user, "CARD#")
        .await?
        .into_iter()
        .map(|r| (r.value.question_id, r.value.schedule))
        .collect();
    let topics = topic_stats(&state.catalog, &attempts);
    let eligible = eligible_topics(&state.catalog, &settings, &settings.studied_unit_ids, &[]);
    let available: Vec<_> = questions
        .iter()
        .filter(|q| q.kind == Kind::Flashcard && !q.flagged && eligible.contains(&q.topic_id))
        .collect();
    let new_flashcards = available
        .iter()
        .filter(|q| !cards.contains_key(&q.id))
        .count();
    let due_flashcards = available
        .iter()
        .filter(|q| cards.get(&q.id).is_some_and(|c| c.due <= now))
        .count();
    let reviewed_flashcards = questions
        .iter()
        .filter(|q| {
            q.kind == Kind::Flashcard && !q.flagged && cards.get(&q.id).is_some_and(|c| c.reps > 0)
        })
        .count();
    let independent = topics.iter().map(|t| t.independent).sum();
    let assisted = topics.iter().map(|t| t.assisted).sum();
    let incorrect = topics.iter().map(|t| t.incorrect).sum();
    let mut days = BTreeMap::new();
    for offset in (0..30).rev() {
        if let Some(date) = chrono::DateTime::from_timestamp_millis(now - offset * 86_400_000) {
            let date = date.format("%Y-%m-%d").to_string();
            days.insert(
                date.clone(),
                RecentDay {
                    date,
                    tasks: 0,
                    flashcards: 0,
                },
            );
        }
    }
    for a in &attempts {
        if a.question.flagged || a.reported || a.outcome == Outcome::Skipped {
            continue;
        }
        if let Some(done) = a
            .completed_at
            .and_then(chrono::DateTime::from_timestamp_millis)
        {
            if let Some(day) = days.get_mut(&done.format("%Y-%m-%d").to_string()) {
                match a.question.kind {
                    Kind::Task => day.tasks += 1,
                    Kind::Flashcard => day.flashcards += 1,
                }
            }
        }
    }
    Ok(Progress {
        total_attempts: attempts
            .iter()
            .filter(|a| !a.question.flagged && !a.reported)
            .count(),
        completed_tasks: independent
            + assisted
            + incorrect
            + topics.iter().map(|t| t.self_assessed).sum::<u64>(),
        independent,
        assisted,
        incorrect,
        studied_units: settings.studied_unit_ids.len(),
        covered_topics: topics
            .iter()
            .filter(|t| t.independent + t.assisted + t.incorrect + t.self_assessed > 0)
            .count(),
        total_topics: topics.len(),
        due_flashcards,
        new_flashcards,
        reviewed_flashcards,
        topics,
        recent_days: days.into_values().collect(),
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationPlan {
    pub slot: usize,
    pub topic_id: String,
    pub kind: Kind,
    pub difficulty: Difficulty,
    pub formula_only: bool,
}

pub fn validate_selection(
    catalog: &Catalog,
    settings: &Settings,
    request: &PracticeSelection,
) -> ApiResult<HashSet<String>> {
    if request.unit_ids.len() > 30
        || request.topic_ids.len() > 200
        || request.unit_ids.iter().collect::<HashSet<_>>().len() != request.unit_ids.len()
        || request.topic_ids.iter().collect::<HashSet<_>>().len() != request.topic_ids.len()
        || request.unit_ids.iter().any(|id| {
            !catalog.units.iter().any(|u| u.id == *id) || !settings.studied_unit_ids.contains(id)
        })
    {
        return Err(ApiError::bad(
            "Unknown, unstudied or duplicate practice selection",
        ));
    }
    let eligible = eligible_topics(catalog, settings, &request.unit_ids, &request.topic_ids);
    if eligible.is_empty() || request.topic_ids.iter().any(|id| !eligible.contains(id)) {
        return Err(ApiError::conflict(
            "Choose studied units and topics whose prerequisites are studied",
        ));
    }
    Ok(eligible)
}

pub fn generation_plan(
    catalog: &Catalog,
    settings: &Settings,
    request: &PracticeSelection,
    attempts: &[Attempt],
    length: u32,
) -> ApiResult<Vec<GenerationPlan>> {
    let eligible = validate_selection(catalog, settings, request)?;
    let stats = topic_stats(catalog, attempts);
    let mut topics: Vec<_> = stats
        .iter()
        .filter(|s| eligible.contains(&s.topic_id))
        .collect();
    // Both focused and suggested practice use actual topic performance, without
    // ever widening the caller's selected scope.
    topics.sort_by(|a, b| {
        let score = |t: &TopicProgress| {
            (
                (t.recent_accuracy.unwrap_or(0.0) * 100.0) as u64 + t.attempts.min(20) * 3,
                t.last_practised.unwrap_or(0),
            )
        };
        score(a)
            .cmp(&score(b))
            .then_with(|| a.topic_id.cmp(&b.topic_id))
    });
    Ok((0..length as usize)
        .map(|slot| GenerationPlan {
            slot,
            topic_id: topics[slot % topics.len()].topic_id.clone(),
            kind: match request.kind {
                RequestedKind::Formula => Kind::Flashcard,
                RequestedKind::Task => Kind::Task,
                RequestedKind::Mixed if slot % 2 == 1 => Kind::Flashcard,
                RequestedKind::Mixed => Kind::Task,
            },
            difficulty: match request.difficulty {
                RequestedDifficulty::Quick => Difficulty::Quick,
                RequestedDifficulty::Standard => Difficulty::Standard,
                RequestedDifficulty::Stretch => Difficulty::Stretch,
                RequestedDifficulty::Any => {
                    [Difficulty::Quick, Difficulty::Standard, Difficulty::Stretch][slot % 3]
                }
            },
            formula_only: request.kind == RequestedKind::Formula,
        })
        .collect())
}

pub fn review_queue(
    questions: &[Question],
    catalog: &Catalog,
    settings: &Settings,
    request: &PracticeSelection,
    cards: &HashMap<String, CardState>,
    now: i64,
    length: u32,
) -> ApiResult<Vec<Question>> {
    let eligible = validate_selection(catalog, settings, request)?;
    if request.kind == RequestedKind::Task {
        return Err(ApiError::bad("Scheduled review contains flashcards only"));
    }
    let mut candidates: Vec<_> = questions
        .iter()
        .filter(|q| {
            !q.flagged
                && q.kind == Kind::Flashcard
                && eligible.contains(&q.topic_id)
                && cards
                    .get(variants::canonical(&q.id))
                    .is_none_or(|c| c.due <= now)
        })
        .collect();
    candidates.sort_by(|a, b| {
        let score = |q: &Question| {
            let card = cards.get(variants::canonical(&q.id));
            (u8::from(card.is_none()), card.map_or(0, |c| c.due))
        };
        score(a).cmp(&score(b)).then_with(|| a.id.cmp(&b.id))
    });
    let mut seen = HashSet::new();
    let queue: Vec<_> = candidates
        .into_iter()
        .filter(|q| seen.insert(variants::canonical(&q.id).to_owned()))
        .take(length as usize)
        .cloned()
        .collect();
    if queue.is_empty() {
        return Err(ApiError::conflict(
            "No distinct due or new flashcards are available in this scope",
        ));
    }
    Ok(queue)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prerequisite_and_selection_are_strict() {
        let c = Catalog {
            units: vec![Unit {
                id: "u".into(),
                module: "MST124".into(),
                number: 1,
                title: "U".into(),
                source_page: 1,
                topics: vec![Topic {
                    id: "t".into(),
                    title: "T".into(),
                    prerequisites: vec!["p".into()],
                }],
            }],
        };
        let mut s = Settings {
            studied_unit_ids: vec!["u".into()],
            ..Settings::default()
        };
        assert!(eligible_topics(&c, &s, &["u".into()], &[]).is_empty());
        s.studied_unit_ids.push("p".into());
        assert!(eligible_topics(&c, &s, &["u".into()], &[]).contains("t"));
        assert!(eligible_topics(&c, &s, &[], &[]).is_empty());
        assert!(eligible_topics(&c, &s, &["u".into()], &["other".into()]).is_empty());
    }
    #[test]
    fn accuracy_has_no_fictitious_zero() {
        assert_eq!(ratio(&[]), None);
        assert_eq!(ratio(&[true, false, false]), Some(1.0 / 3.0));
    }
}
