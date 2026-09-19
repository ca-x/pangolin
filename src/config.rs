use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub database_url: String,
    pub observation_path: PathBuf,
    pub observation_retention_days: u64,
    pub public_url: Option<String>,
    pub session_secure: bool,
    pub capture_payloads: bool,
    pub upstream_timeout: Duration,
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub master_key: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let data_dir =
            PathBuf::from(env::var("PANGOLIN_DATA_DIR").unwrap_or_else(|_| "./data".into()));
        let bind = env::var("PANGOLIN_BIND")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()
            .context("PANGOLIN_BIND must be a socket address")?;
        let database_url = env::var("PANGOLIN_DATABASE_URL").unwrap_or_else(|_| {
            format!(
                "sqlite://{}?mode=rwc",
                data_dir.join("pangolin.db").display()
            )
        });
        let observation_path = env::var_os("PANGOLIN_OBSERVATION_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("observability.duckdb"));
        let observation_retention_days = env::var("PANGOLIN_OBSERVATION_RETENTION_DAYS")
            .unwrap_or_else(|_| "30".into())
            .parse()
            .context("PANGOLIN_OBSERVATION_RETENTION_DAYS must be a positive integer")?;
        if observation_retention_days == 0 {
            anyhow::bail!("PANGOLIN_OBSERVATION_RETENTION_DAYS must be greater than zero");
        }
        let public_url = env::var("PANGOLIN_PUBLIC_URL").ok();
        let session_secure = env_bool(
            "PANGOLIN_SESSION_SECURE",
            public_url
                .as_deref()
                .is_some_and(|url| url.starts_with("https://")),
        );

        let upstream_timeout = Duration::from_secs(
            env::var("PANGOLIN_UPSTREAM_TIMEOUT_SECS")
                .unwrap_or_else(|_| "600".into())
                .parse()
                .context("PANGOLIN_UPSTREAM_TIMEOUT_SECS must be a positive integer")?,
        );
        if upstream_timeout.is_zero() {
            anyhow::bail!("PANGOLIN_UPSTREAM_TIMEOUT_SECS must be greater than zero");
        }

        Ok(Self {
            bind,
            data_dir,
            database_url,
            observation_path,
            observation_retention_days,
            public_url,
            session_secure,
            capture_payloads: env_bool("PANGOLIN_CAPTURE_PAYLOADS", false),
            upstream_timeout,
            admin_email: env::var("PANGOLIN_ADMIN_EMAIL").ok(),
            admin_password: env::var("PANGOLIN_ADMIN_PASSWORD").ok(),
            master_key: env::var("PANGOLIN_MASTER_KEY").ok(),
        })
    }
}

fn env_bool(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}
