mod access;
mod access_api;
mod api;
mod build_info;
mod catalog;
mod config;
mod crypto;
mod db;
mod models;
mod oauth;
mod observability;
mod oidc;
mod operations;
mod orchestration;
mod providers;
mod web;

use std::{collections::HashMap, fs, sync::Arc};

use anyhow::{Context, Result, bail};
use axum::Router;
use sea_orm::ConnectionTrait;
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
    validate_request_logging_policy_at_startup(&database).await?;
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
    if observations.needs_rebuild() {
        // The projection's shape changed. Mark it so startup re-derives it from
        // the record system instead of leaving the console reporting no traffic.
        database
            .execute(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Sqlite,
                "INSERT INTO settings(key,value,updated_at) VALUES('_internal.observation_reset_required','true',unixepoch()) ON CONFLICT(key) DO UPDATE SET value='true'".to_owned(),
            ))
            .await?;
    }
    let state = AppState {
        db: database,
        config: Arc::new(config.clone()),
        secrets,
        observations: observations.clone(),
        client: providers::http_client(config.upstream_timeout)?,
        oidc_client: reqwest::Client::builder()
            .user_agent(concat!("pangolin/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(config.upstream_timeout)
            .build()?,
        budget_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        maintenance: Arc::new(tokio::sync::RwLock::new(())),
        orchestrator: Arc::new(orchestration::Runtime::default()),
    };
    operations::instance_backup::reset_projection(&state).await?;
    operations::runtime::recover(&state).await?;
    let operations = operations::runtime::start(state.clone());
    let app = Router::new()
        .merge(api::router(state))
        .fallback(web::serve)
        .layer(CompressionLayer::new())
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http().make_span_with(http_span));
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("failed to bind {}", config.bind))?;
    // Logged on every start: the first question about a misbehaving instance is
    // which build it actually is.
    let build = crate::build_info::build_info();
    tracing::info!(
        address = %config.bind,
        product = "Pangolin / 鲮鲤",
        version = build.version,
        commit = build.commit,
        built_at = %build.built_at,
        target = build.target,
        "server listening"
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    operations.abort();
    let _ = operations.await;
    observations.flush().await;
    Ok(())
}

fn http_span<B>(request: &http::Request<B>) -> tracing::Span {
    tracing::debug_span!("http.request",method=%request.method(),path=%request.uri().path())
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

async fn validate_request_logging_policy_at_startup(
    database: &sea_orm::DatabaseConnection,
) -> Result<()> {
    if let Some(problem) = operations::logging::startup_problem(database).await? {
        tracing::warn!(
            problem = problem.code(),
            "stored request-logging policy is invalid; admission will use logging off until an owner saves a supported policy in System > Request logging"
        );
    }
    Ok(())
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

#[cfg(test)]
mod startup_tests {
    use super::*;
    use sea_orm::{DbBackend, Statement};
    use std::sync::{Arc, Mutex};
    use tracing::instrument::WithSubscriber;

    #[derive(Clone)]
    struct LogCapture(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for LogCapture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
        type Writer = Self;
        fn make_writer(&'a self) -> Self {
            self.clone()
        }
    }

    #[tokio::test]
    async fn malformed_or_unknown_stored_logging_policy_is_reported_without_stopping_startup() {
        let database = db::connect("sqlite::memory:").await.unwrap();
        for (document, code, secret) in [
            (
                "{not-json:startup-secret}",
                "malformed_document",
                "startup-secret",
            ),
            (
                r#"{"version":99,"future_secret":"version-secret"}"#,
                "unsupported_version",
                "version-secret",
            ),
        ] {
            database
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO settings(key,value,updated_at) VALUES('request_logging',?,0) ON CONFLICT(key) DO UPDATE SET value=excluded.value"
                        .to_owned(),
                    [document.into()],
                ))
                .await
                .unwrap();
            let capture = LogCapture(Arc::new(Mutex::new(vec![])));
            let subscriber = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::WARN)
                .with_ansi(false)
                .with_writer(capture.clone())
                .finish();

            validate_request_logging_policy_at_startup(&database)
                .with_subscriber(subscriber)
                .await
                .expect("an invalid document must not stop startup");

            let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
            assert!(logs.contains(code), "{logs}");
            assert!(logs.contains("System > Request logging"), "{logs}");
            assert!(logs.contains("logging off"), "{logs}");
            assert!(!logs.contains(secret), "stored content leaked: {logs}");
            assert!(!logs.contains("serde") && logs.len() < 1024, "{logs}");
        }
    }

    #[tokio::test]
    async fn a_non_text_stored_logging_policy_is_reported_without_stopping_startup() {
        let database = db::connect("sqlite::memory:").await.unwrap();
        for stored_value in ["X'7B7D'", "CAST(X'80' AS TEXT)", "17"] {
            database
                .execute(Statement::from_string(
                    DbBackend::Sqlite,
                    format!(
                        "INSERT INTO settings(key,value,updated_at) VALUES('request_logging',{stored_value},0) ON CONFLICT(key) DO UPDATE SET value=excluded.value"
                    ),
                ))
                .await
                .unwrap();
            let capture = LogCapture(Arc::new(Mutex::new(vec![])));
            let subscriber = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::WARN)
                .with_ansi(false)
                .with_writer(capture.clone())
                .finish();

            validate_request_logging_policy_at_startup(&database)
                .with_subscriber(subscriber)
                .await
                .expect("an invalid SQLite value must not stop startup");

            let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
            assert!(logs.contains("malformed_document"), "{logs}");
            assert!(logs.contains("logging off"), "{logs}");
            assert!(logs.len() < 1024, "{logs}");
        }
    }
}
