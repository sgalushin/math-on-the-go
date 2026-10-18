pub mod actions;
pub mod ai;
pub mod grading;
pub mod model;
pub mod practice;
pub mod routes;
pub mod schedule;
pub mod sessions;
pub mod store;
pub mod variants;

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use lambda_http::RequestExt;
use model::*;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use store::{ApiError, ApiResult, Store};
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub catalog: Arc<Catalog>,
    pub bank: Arc<Vec<Question>>,
    pub http: reqwest::Client,
    pub lambda: aws_sdk_lambda::Client,
    pub dev_user: Option<String>,
    pub openrouter_key: Option<Arc<str>>,
}
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock predates epoch")
        .as_millis() as i64
}
impl AppState {
    pub async fn connect() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let in_lambda = std::env::var_os("AWS_LAMBDA_RUNTIME_API").is_some();
        if !in_lambda {
            let _ = dotenvy::from_filename(".env");
            let _ = dotenvy::from_filename("../../.env");
        }
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
        if let Ok(endpoint) =
            std::env::var("DYNAMODB_ENDPOINT_URL").or_else(|_| std::env::var("AWS_ENDPOINT_URL"))
        {
            loader = loader.endpoint_url(endpoint);
        }
        let config = loader.load().await;
        let catalog: Catalog = serde_json::from_str(include_str!("../../../data/catalog.json"))?;
        let bank: Vec<Question> =
            serde_json::from_str(include_str!("../../../data/questions.json"))?;
        for q in &bank {
            validate_question(q, &catalog)?;
        }
        Ok(Self {
            store: Store {
                client: aws_sdk_dynamodb::Client::new(&config),
                table: std::env::var("TABLE_NAME").unwrap_or_else(|_| "math-on-the-go".into()),
            },
            catalog: Arc::new(catalog),
            bank: Arc::new(bank),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .connect_timeout(Duration::from_secs(10))
                .build()?,
            lambda: aws_sdk_lambda::Client::new(&config),
            dev_user: if in_lambda {
                None
            } else {
                std::env::var("DEV_USER").ok().filter(|s| !s.is_empty())
            },
            openrouter_key: std::env::var("OPENROUTER_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
                .map(Arc::from),
        })
    }
    pub async fn settings(&self, user: &str) -> ApiResult<Settings> {
        Ok(self
            .store
            .get::<Settings>(user, "SETTINGS")
            .await?
            .map_or_else(Settings::default, |record| record.value))
    }
    pub async fn questions(&self, user: &str) -> ApiResult<Vec<Question>> {
        let mut qs = self.bank.as_ref().clone();
        qs.extend(
            self.store
                .list::<Question>(user, "QUESTION#")
                .await?
                .into_iter()
                .map(|r| r.value),
        );
        let flags = self.store.list::<QuestionFlag>(user, "FLAG#").await?;
        let excluded: std::collections::HashSet<_> =
            flags.into_iter().map(|r| r.value.question_id).collect();
        for q in &mut qs {
            q.flagged |= excluded.contains(variants::canonical(&q.id));
        }
        Ok(qs)
    }
    pub async fn attempts(&self, user: &str) -> ApiResult<Vec<Attempt>> {
        let mut attempts: Vec<_> = self
            .store
            .list::<Attempt>(user, "ATTEMPT#")
            .await?
            .into_iter()
            .map(|r| r.value)
            .collect();
        let flags = self.store.list::<QuestionFlag>(user, "FLAG#").await?;
        let excluded: std::collections::HashSet<_> =
            flags.into_iter().map(|r| r.value.question_id).collect();
        for a in &mut attempts {
            a.question.flagged |= excluded.contains(variants::canonical(&a.question.id));
        }
        attempts.sort_by(|a, b| {
            b.started_at
                .cmp(&a.started_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(attempts)
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionFlag {
    pub question_id: String,
    pub reason: Option<String>,
    pub at: i64,
}
pub struct User(pub String);
impl FromRequestParts<AppState> for User {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let sub = parts
            .request_context_ref()
            .and_then(|c| c.authorizer())
            .and_then(|a| a.jwt.as_ref())
            .and_then(|j| j.claims.get("sub"))
            .cloned();
        if let Some(sub) = sub.filter(|s| valid_user(s)) {
            return Ok(User(sub));
        }
        if std::env::var_os("AWS_LAMBDA_RUNTIME_API").is_none() {
            if let Some(sub) = state.dev_user.as_ref().filter(|s| valid_user(s)) {
                return Ok(User(sub.clone()));
            }
        }
        Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "Authentication required".into(),
        ))
    }
}
pub fn valid_user(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b))
}
pub fn validate_question(q: &Question, catalog: &Catalog) -> ApiResult<()> {
    let unit = catalog
        .units
        .iter()
        .find(|u| u.topics.iter().any(|t| t.id == q.topic_id))
        .ok_or_else(|| ApiError::bad("Unknown question topic"))?;
    if q.id.is_empty()
        || q.id.len() > 160
        || q.prompt.trim().is_empty()
        || q.prompt.len() > 8000
        || q.answer.trim().is_empty()
        || q.answer.len() > 2000
        || q.explanation.trim().is_empty()
        || q.explanation.len() > 12000
        || q.hints.len() > 5
        || q.hints.iter().any(|s| s.len() > 3000)
        || q.accepted_answers.len() > 20
        || q.accepted_answers.iter().any(|s| s.len() > 2000)
        || q.choices.len() > 8
        || q.choices.iter().any(|s| s.len() > 2000)
    {
        return Err(ApiError::bad(
            "Question content is missing or exceeds size limits",
        ));
    }
    if !q.source.handbook.contains(&unit.module)
        || q.source.page == 0
        || q.source.section.trim().is_empty()
    {
        return Err(ApiError::bad("Question requires a valid handbook source"));
    }
    if q.answer_format == AnswerFormat::Choice
        && (q.choices.len() < 2 || !q.choices.contains(&q.answer))
    {
        return Err(ApiError::bad(
            "Choice answer must exactly match a supplied choice",
        ));
    }
    if let Some(g) = &q.graph {
        if ![g.x_min, g.x_max, g.y_min, g.y_max]
            .into_iter()
            .all(f64::is_finite)
            || g.x_min >= g.x_max
            || g.y_min >= g.y_max
            || g.series.is_empty()
            || g.series.len() > 3
            || g.series.iter().any(|s| {
                s.label.len() > 100
                    || s.points.len() > 200
                    || s.points
                        .iter()
                        .any(|p| !p.x.is_finite() || !p.y.is_finite())
            })
        {
            return Err(ApiError::bad("Invalid or oversized graph"));
        }
    }
    Ok(())
}
