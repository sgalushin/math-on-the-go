use math_api::{ai, model::WorkerEvent, AppState};
#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "math_api=info".into()),
        )
        .init();
    let state = AppState::connect().await?;
    lambda_runtime::run(lambda_runtime::service_fn(
        move |event: lambda_runtime::LambdaEvent<WorkerEvent>| {
            let state = state.clone();
            async move {
                ai::run_session(&state, event.payload)
                    .await
                    .map_err(|e| lambda_runtime::Error::from(e.1))?;
                Ok::<_, lambda_runtime::Error>(serde_json::json!({"processed":true}))
            }
        },
    ))
    .await
}
