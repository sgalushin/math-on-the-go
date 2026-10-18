// Adapted from Moonrabbit apps/api/src/schedule.rs: FSRS-6 default weights,
// 0.9 retention, explicit learning/relearning, one fuzz per mature interval.
use crate::{
    model::Rating,
    store::{ApiError, ApiResult},
};
use fsrs::{MemoryState, DEFAULT_PARAMETERS, FSRS};
use serde::{Deserialize, Serialize};
const MINUTE: i64 = 60_000;
const DAY: i64 = 86_400_000;
const MAX_DAYS: f32 = 36_500.0;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardPhase {
    New,
    Learning,
    Review,
    Relearning,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardState {
    pub state: CardPhase,
    pub step: u32,
    pub stability: f32,
    pub difficulty: f32,
    pub due: i64,
    pub last_review: i64,
    pub reps: u32,
    pub lapses: u32,
    pub version: u32,
}
impl CardState {
    pub fn fresh() -> Self {
        Self {
            state: CardPhase::New,
            step: 0,
            stability: 0.0,
            difficulty: 0.0,
            due: 0,
            last_review: 0,
            reps: 0,
            lapses: 0,
            version: 0,
        }
    }
}
pub fn grade(card: &CardState, rating: Rating, reviewed_at: i64) -> ApiResult<CardState> {
    let state = card.state;
    if reviewed_at < 0 || card.last_review < 0 || reviewed_at < card.last_review {
        return Err(ApiError::bad(
            "review timestamp must be nonnegative and chronological",
        ));
    }
    if state == CardPhase::Learning && card.step > 2 {
        return Err(ApiError::bad("unknown learning step"));
    }
    let reps = card
        .reps
        .checked_add(1)
        .ok_or_else(|| ApiError::bad("repetition count overflow"))?;
    let version = card
        .version
        .checked_add(1)
        .ok_or_else(|| ApiError::bad("card version overflow"))?;
    let lapses = if state == CardPhase::Review && rating == Rating::Again {
        card.lapses
            .checked_add(1)
            .ok_or_else(|| ApiError::bad("lapse count overflow"))?
    } else {
        card.lapses
    };
    let forecast = forecast(card, reviewed_at)?;
    let item = match rating {
        Rating::Again => &forecast.again,
        Rating::Good => &forecast.good,
    };
    let (next_state, step, delay) = match (state, rating) {
        (CardPhase::New, Rating::Again) | (CardPhase::Learning, Rating::Again) => {
            (CardPhase::Learning, 0, MINUTE)
        }
        (CardPhase::New, Rating::Good) => (CardPhase::Learning, 1, MINUTE),
        (CardPhase::Learning, Rating::Good) => match card.step {
            0 => (CardPhase::Learning, 1, MINUTE),
            1 => (CardPhase::Learning, 2, 10 * MINUTE),
            2 => (
                CardPhase::Review,
                0,
                interval_ms(item.interval, reviewed_at)?,
            ),
            _ => unreachable!(),
        },
        (CardPhase::Review, Rating::Again) | (CardPhase::Relearning, Rating::Again) => {
            (CardPhase::Relearning, 0, 10 * MINUTE)
        }
        (CardPhase::Review, Rating::Good) | (CardPhase::Relearning, Rating::Good) => (
            CardPhase::Review,
            0,
            interval_ms(item.interval, reviewed_at)?,
        ),
    };
    let due = reviewed_at
        .checked_add(delay)
        .ok_or_else(|| ApiError::bad("review due timestamp overflow"))?;
    Ok(CardState {
        state: next_state,
        step,
        stability: item.memory.stability,
        difficulty: item.memory.difficulty,
        due,
        last_review: reviewed_at,
        reps,
        lapses,
        version,
    })
}
fn forecast(card: &CardState, reviewed_at: i64) -> ApiResult<fsrs::NextStates> {
    let memory = if card.stability == 0.0 && card.difficulty == 0.0 {
        if matches!(card.state, CardPhase::Review | CardPhase::Relearning) {
            return Err(ApiError::bad("review card is missing FSRS memory"));
        }
        None
    } else {
        if !card.stability.is_finite()
            || card.stability <= 0.0
            || !card.difficulty.is_finite()
            || !(1.0..=10.0).contains(&card.difficulty)
        {
            return Err(ApiError::bad("invalid FSRS memory"));
        }
        Some(MemoryState {
            stability: card.stability,
            difficulty: card.difficulty,
        })
    };
    let elapsed = if memory.is_some() {
        u32::try_from((reviewed_at - card.last_review) / DAY)
            .map_err(|_| ApiError::bad("elapsed review time exceeds FSRS range"))?
    } else {
        0
    };
    thread_local! {static MODEL:Result<FSRS,fsrs::FSRSError>=FSRS::new(Some(&DEFAULT_PARAMETERS));}
    MODEL.with(|m| {
        m.as_ref()
            .map_err(|_| ApiError::unavailable("FSRS initialization failed"))?
            .next_states(memory, 0.9, elapsed)
            .map_err(|_| ApiError::unavailable("FSRS forecast failed"))
    })
}
fn interval_ms(days: f32, seed: i64) -> ApiResult<i64> {
    if !days.is_finite() || days <= 0.0 {
        return Err(ApiError::unavailable("FSRS returned an invalid interval"));
    }
    let interval = (days.clamp(1.0, MAX_DAYS) as f64 * DAY as f64) as i64;
    let unit = (seed.unsigned_abs() % 1000) as f64 / 1000.0;
    Ok(((interval as f64 * (0.95 + unit * 0.10)) as i64)
        .clamp(DAY, (MAX_DAYS as f64 * DAY as f64) as i64))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn learning_and_relearning() {
        let mut c = CardState::fresh();
        c = grade(&c, Rating::Good, 1000).unwrap();
        assert_eq!((c.state, c.step, c.due), (CardPhase::Learning, 1, 61000));
        c = grade(&c, Rating::Good, 61000).unwrap();
        assert_eq!((c.state, c.step), (CardPhase::Learning, 2));
        c = grade(&c, Rating::Good, 661000).unwrap();
        assert_eq!(c.state, CardPhase::Review);
        assert!(c.stability > 0.0);
        c = grade(&c, Rating::Again, c.due).unwrap();
        assert_eq!((c.state, c.lapses), (CardPhase::Relearning, 1));
        c = grade(&c, Rating::Again, c.due).unwrap();
        assert_eq!(c.lapses, 1);
        c = grade(&c, Rating::Good, c.due).unwrap();
        assert_eq!(c.state, CardPhase::Review);
    }
    #[test]
    fn invalid_clock_rejected() {
        let c = grade(&CardState::fresh(), Rating::Good, 1000).unwrap();
        assert!(grade(&c, Rating::Good, 999).is_err());
    }
}
