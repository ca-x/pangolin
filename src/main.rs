mod access;
mod access_api;
mod api;
mod config;
mod crypto;
mod db;
mod models;
mod observability;
mod oidc;
mod web;

use std::{collections::HashMap, fs, sync::Arc};

use anyhow::{Context, Result, bail};
use axum::Router;
use tower_http::{catch_panic::CatchPanicLayer, compression::CompressionLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

use crate::{
    api::AppState, config::Config, crypto::SecretBox, models::SetupRequest,
    observability::ObservationStore,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pangolin=info,tower_http=info".into()),
        )
        .init();
    let config = Config::from_env()?;
    fs::create_dir_all(&config.data_dir).context("failed to create data directory")?;
    let database = db::connect(&config.database_url).await?;
    bootstrap_from_environment(&database, &config).await?;
    let secrets = SecretBox::load(&config.data_dir, config.master_key.as_deref())?;
    let observations = match ObservationStore::open(
        &config.observation_path,
        config.observation_retention_days,
    ) {
        Ok(store) => store,
        Err(error) => {
            tracing::error!(%error, "observation store is unavailable; gateway will run in degraded mode");
            ObservationStore::degraded(&config.observation_path, error)
        }
    };
    let state = AppState {
        db: database,
        config: Arc::new(config.clone()),
        secrets,
        observations: observations.clone(),
        client: reqwest::Client::builder()
            .user_agent(concat!("pangolin/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(config.upstream_timeout)
            .build()?,
        oidc_client: reqwest::Client::builder()
            .user_agent(concat!("pangolin/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(config.upstream_timeout)
            .build()?,
        budget_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    };
    let app = Router::new()
        .merge(api::router(state))
        .fallback(web::serve)
        .layer(CompressionLayer::new())
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("failed to bind {}", config.bind))?;
    tracing::info!(address = %config.bind, product = "Pangolin / 鲮鲤", "server listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    observations.flush().await;
    Ok(())
}

async fn bootstrap_from_environment(
    database: &sea_orm::DatabaseConnection,
    config: &Config,
) -> Result<()> {
    if db::is_initialized(database).await? {
        return Ok(());
    }
    match (&config.admin_email, &config.admin_password) {
        (Some(email), Some(password)) => {
            db::create_initial_admin(
                database,
                &SetupRequest {
                    email: email.clone(),
                    password: password.clone(),
                    instance_name: Some("Pangolin".into()),
                    language: Some("zh-CN".into()),
                },
            )
            .await?;
            tracing::info!("initialized administrator from environment");
            Ok(())
        }
        (None, None) => Ok(()),
        _ => bail!("PANGOLIN_ADMIN_EMAIL and PANGOLIN_ADMIN_PASSWORD must be set together"),
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler")
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
