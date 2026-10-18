use serde::{Deserialize, Serialize};
use std::collections::HashMap;

macro_rules! strings { ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum $name { $(#[serde(rename=$wire)] $variant),+ }
}; }
strings!(Kind { Flashcard => "flashcard", Task => "task" });
strings!(Difficulty { Quick => "quick", Standard => "standard", Stretch => "stretch" });
strings!(AnswerFormat { Text => "text", Choice => "choice", SelfAssessed => "self" });
strings!(Outcome { Unfinished => "unfinished", Independent => "independent", Assisted => "assisted", Incorrect => "incorrect", SelfAssessed => "self_assessed", Skipped => "skipped" });
strings!(Assessment { Automatic => "automatic", SelfAssessed => "self" });
strings!(Rating { Again => "again", Good => "good" });
strings!(PracticeMode { Review => "review", Focused => "focused", Suggested => "suggested" });
strings!(RequestedKind { Mixed => "mixed", Formula => "formula", Task => "task" });
strings!(RequestedDifficulty { Any => "any", Quick => "quick", Standard => "standard", Stretch => "stretch" });
impl RequestedDifficulty {
    pub fn accepts(self, d: Difficulty) -> bool {
        match self {
            Self::Any => true,
            Self::Quick => d == Difficulty::Quick,
            Self::Standard => d == Difficulty::Standard,
            Self::Stretch => d == Difficulty::Stretch,
        }
    }
}
impl RequestedKind {
    pub fn accepts(self, k: Kind) -> bool {
        self == Self::Mixed
            || (self == Self::Task && k == Kind::Task)
            || (self == Self::Formula && k == Kind::Flashcard)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub units: Vec<Unit>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unit {
    pub id: String,
    pub module: String,
    pub number: u32,
    pub title: String,
    pub source_page: u32,
    pub topics: Vec<Topic>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Topic {
    pub id: String,
    pub title: String,
    pub prerequisites: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub handbook: String,
    pub page: u32,
    pub section: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Verification {
    pub method: String,
    pub models: Vec<String>,
    pub checked_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    #[serde(rename = "type")]
    pub family: String,
    pub parameters: HashMap<String, f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Series {
    pub label: String,
    pub points: Vec<Point>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
    pub series: Vec<Series>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub id: String,
    pub topic_id: String,
    pub kind: Kind,
    pub difficulty: Difficulty,
    pub prompt: String,
    pub answer: String,
    pub accepted_answers: Vec<String>,
    pub choices: Vec<String>,
    pub hints: Vec<String>,
    pub explanation: String,
    pub answer_format: AnswerFormat,
    pub source: Source,
    pub verification: Verification,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<Graph>,
    pub flagged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Submission {
    pub answer: String,
    pub at: i64,
    pub correct: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub id: String,
    pub question: Question,
    pub started_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub submissions: Vec<Submission>,
    pub hints_used: usize,
    pub solution_revealed: bool,
    pub outcome: Outcome,
    pub assessment: Assessment,
    pub rating: Option<Rating>,
    pub reported: bool,
}
impl Attempt {
    pub fn new(question: Question, now: i64) -> Self {
        let assessment = if question.answer_format == AnswerFormat::SelfAssessed
            || question.kind == Kind::Flashcard
        {
            Assessment::SelfAssessed
        } else {
            Assessment::Automatic
        };
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            question,
            started_at: now,
            updated_at: now,
            completed_at: None,
            submissions: vec![],
            hints_used: 0,
            solution_revealed: false,
            outcome: Outcome::Unfinished,
            assessment,
            rating: None,
            reported: false,
        }
    }
}
pub const AUTHOR_MODEL: &str = "anthropic/claude-opus-5.5";
pub const SOLVER_MODEL: &str = "openai/gpt-6.1-sol";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub studied_unit_ids: Vec<String>,
    pub default_difficulty: RequestedDifficulty,
    pub session_length: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            studied_unit_ids: vec![],
            default_difficulty: RequestedDifficulty::Any,
            session_length: 5,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeSelection {
    pub mode: PracticeMode,
    pub kind: RequestedKind,
    pub unit_ids: Vec<String>,
    pub topic_ids: Vec<String>,
    pub difficulty: RequestedDifficulty,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSession {
    #[serde(flatten)]
    pub selection: PracticeSelection,
    pub request_id: String,
    pub length: u32,
}
strings!(SessionStatus { Generating => "generating", Ready => "ready", Failed => "failed", Finished => "finished" });
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeSession {
    pub id: String,
    pub status: SessionStatus,
    pub request: PracticeSelection,
    pub length: u32,
    pub generated: u32,
    pub attempt_ids: Vec<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionNextRequest {
    pub event_id: String,
    pub after_attempt_id: Option<String>,
}
strings!(ActionKind { Hint=>"hint", Reveal=>"reveal", Answer=>"answer", Grade=>"grade", Skip=>"skip" });
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttemptAction {
    pub action: ActionKind,
    pub answer: Option<String>,
    pub rating: Option<Rating>,
    pub self_correct: Option<bool>,
    pub event_id: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DifficultyProgress {
    pub attempts: u64,
    pub independent: u64,
    pub assisted: u64,
    pub incorrect: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicProgress {
    pub topic_id: String,
    pub attempts: u64,
    pub independent: u64,
    pub assisted: u64,
    pub incorrect: u64,
    pub self_assessed: u64,
    pub skipped: u64,
    pub unfinished: u64,
    pub flagged: u64,
    pub recent_accuracy: Option<f64>,
    pub previous_accuracy: Option<f64>,
    pub last_practised: Option<i64>,
    pub by_difficulty: HashMap<Difficulty, DifficultyProgress>,
}
impl TopicProgress {
    pub fn new(id: String) -> Self {
        Self {
            topic_id: id,
            attempts: 0,
            independent: 0,
            assisted: 0,
            incorrect: 0,
            self_assessed: 0,
            skipped: 0,
            unfinished: 0,
            flagged: 0,
            recent_accuracy: None,
            previous_accuracy: None,
            last_practised: None,
            by_difficulty: [Difficulty::Quick, Difficulty::Standard, Difficulty::Stretch]
                .into_iter()
                .map(|d| (d, DifficultyProgress::default()))
                .collect(),
        }
    }
}
#[derive(Serialize)]
pub struct RecentDay {
    pub date: String,
    pub tasks: u64,
    pub flashcards: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub total_attempts: usize,
    pub completed_tasks: u64,
    pub independent: u64,
    pub assisted: u64,
    pub incorrect: u64,
    pub studied_units: usize,
    pub covered_topics: usize,
    pub total_topics: usize,
    pub due_flashcards: usize,
    pub new_flashcards: usize,
    pub reviewed_flashcards: usize,
    pub topics: Vec<TopicProgress>,
    pub recent_days: Vec<RecentDay>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub topic_id: Option<String>,
    pub unit_id: Option<String>,
    pub kind: Option<Kind>,
    pub outcome: Option<Outcome>,
    pub difficulty: Option<Difficulty>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResponse {
    pub attempts: Vec<Attempt>,
    pub next_cursor: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEvent {
    pub user_id: String,
    pub session_id: String,
}
