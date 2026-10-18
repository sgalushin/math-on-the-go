use math_api::{routes, AppState};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "math_api=info,tower_http=info".into()),
        )
        .init();
    let state = AppState::connect().await?;
    let app = routes::router(state);
    if std::env::var_os("AWS_LAMBDA_RUNTIME_API").is_some() {
        lambda_http::run(app).await?;
    } else {
        let port = std::env::var("PORT").unwrap_or_else(|_| "8300".into());
        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
        tracing::info!(%port,"API listening");
        axum::serve(listener, app).await?;
    }
    Ok(())
}
