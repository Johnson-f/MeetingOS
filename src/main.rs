mod config;
mod models;
mod routes;
mod service;

use std::error::Error;

use config::{AppConfig, AppRole};
use routes::{AppState, app_router};
use service::{ServiceRegistry, jobs};
use service::{auth::create_jwks_provider, turso::client::TursoClient};
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    match dotenvy::dotenv() {
        Ok(path) => eprintln!("loaded .env from: {}", path.display()),
        Err(e) => eprintln!("warning: .env not loaded: {e}"),
    }
    init_tracing();

    // must be before any TLS connection — fixes rustls crypto provider conflict
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    let config = AppConfig::from_env();
    let turso = TursoClient::connect(
        config.turso_database_url.clone(),
        config.turso_auth_token.clone(),
    )
    .await?;
    let (sse_tx, _) = tokio::sync::broadcast::channel(256);
    let services = ServiceRegistry::new(config.clone(), turso, sse_tx.clone()).await;

    match config.role {
        AppRole::Api => run_api(config, services).await?,
        AppRole::Worker => jobs::run_worker_loop(services).await?,
        AppRole::All => run_all(config, services).await?,
    }

    Ok(())
}

async fn run_api(config: AppConfig, services: ServiceRegistry) -> Result<(), Box<dyn Error>> {
    let jwks_provider = create_jwks_provider(&config.clerk_secret_key);
    let app = app_router(AppState::new(config.clone(), services), jwks_provider);
    let listener = TcpListener::bind(config.socket_address()).await?;

    info!(
        address = %config.socket_address(),
        environment = %config.environment,
        role = "api",
        "starting meeting-bot api"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn run_all(config: AppConfig, services: ServiceRegistry) -> Result<(), Box<dyn Error>> {
    let worker_services = services.clone();
    let worker_handle = tokio::spawn(async move { jobs::run_worker_loop(worker_services).await });
    let result = run_api(config, services).await;
    worker_handle.abort();

    match worker_handle.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => error!(%error, "worker exited with an error"),
        Err(error) if error.is_cancelled() => {}
        Err(error) => jobs::log_worker_shutdown(&error),
    }

    result
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install Ctrl+C handler");
        return;
    }

    info!("shutdown signal received");
}
