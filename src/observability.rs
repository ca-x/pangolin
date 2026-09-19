use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use anyhow::{Context, Result};
use duckdb::{Connection, params};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEvent {
    pub request_id: String,
    pub trace_id: String,
    pub started_at: i64,
    pub finished_at: i64,
    pub endpoint: String,
    pub api_key_id: Option<String>,
    pub provider: Option<String>,
    pub requested_model: Option<String>,
    pub resolved_model: Option<String>,
    pub status_code: i32,
    pub error_kind: Option<String>,
    pub latency_ms: i64,
    pub ttft_ms: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cost_micros: i64,
    pub payload_captured: bool,
    pub request_json: Option<String>,
    pub response_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Summary {
    pub requests: i64,
    pub errors: i64,
    pub error_rate: f64,
    pub p95_latency_ms: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_micros: i64,
    pub series: Vec<SummaryPoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryPoint {
    pub bucket: i64,
    pub requests: i64,
    pub errors: i64,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestListItem {
    pub request_id: String,
    pub started_at: i64,
    pub endpoint: String,
    pub provider: Option<String>,
    pub requested_model: Option<String>,
    pub resolved_model: Option<String>,
    pub status_code: i32,
    pub latency_ms: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_micros: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RequestFilter {
    pub status_code: Option<i32>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub limit: Option<usize>,
}

enum Command {
    Record(Box<RequestEvent>),
    Flush(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct ObservationStore {
    path: Arc<PathBuf>,
    sender: Option<mpsc::Sender<Command>>,
    failure: Option<Arc<str>>,
    dropped: Arc<AtomicU64>,
}

impl ObservationStore {
    pub fn open(path: impl AsRef<Path>, retention_days: u64) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        initialize(&path, retention_days)?;
        let (sender, mut receiver) = mpsc::channel::<Command>(4096);
        let writer_path = path.clone();
        let dropped = Arc::new(AtomicU64::new(0));
        let writer_dropped = Arc::clone(&dropped);
        thread::Builder::new().name("pangolin-observation-writer".into()).spawn(move || {
            let connection = match Connection::open(&writer_path) {
                Ok(connection) => connection,
                Err(error) => {
                    tracing::error!(%error, "observation writer failed to open");
                    return;
                }
            };
            while let Some(command) = receiver.blocking_recv() {
                match command {
                    Command::Record(event) => {
                        let mut batch = vec![event];
                        let mut flush_after = None;
                        while batch.len() < 64 {
                            match receiver.try_recv() {
                                Ok(Command::Record(event)) => batch.push(event),
                                Ok(Command::Flush(done)) => { flush_after = Some(done); break; }
                                Err(_) => break,
                            }
                        }
                        if let Err(error) = insert_batch(&connection, &batch) {
                            writer_dropped.fetch_add(batch.len() as u64, Ordering::Relaxed);
                            tracing::warn!(%error, count = batch.len(), "observation event batch was dropped");
                        }
                        if let Some(done) = flush_after {
                            let _ = connection.execute_batch("CHECKPOINT");
                            let _ = done.send(());
                        }
                    }
                    Command::Flush(done) => {
                        let _ = connection.execute_batch("CHECKPOINT");
                        let _ = done.send(());
                    }
                }
            }
        }).context("failed to spawn observation writer")?;
        Ok(Self {
            path: Arc::new(path),
            sender: Some(sender),
            failure: None,
            dropped,
        })
    }

    pub fn degraded(path: impl AsRef<Path>, error: impl std::fmt::Display) -> Self {
        Self {
            path: Arc::new(path.as_ref().to_path_buf()),
            sender: None,
            failure: Some(Arc::from(error.to_string())),
            dropped: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn is_available(&self) -> bool {
        self.failure.is_none()
    }

    pub fn dropped_events(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn record(&self, event: RequestEvent) {
        let Some(sender) = &self.sender else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        if sender.try_send(Command::Record(Box::new(event))).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            tracing::warn!("observation queue is full; event dropped");
        }
    }

    pub async fn flush(&self) {
        let (sender, receiver) = oneshot::channel();
        if let Some(command_sender) = &self.sender
            && command_sender.send(Command::Flush(sender)).await.is_ok()
        {
            let _ = receiver.await;
        }
    }

    pub async fn summary(&self) -> Result<Summary> {
        self.ensure_available()?;
        let path = Arc::clone(&self.path);
        tokio::task::spawn_blocking(move || query_summary(&path)).await?
    }

    pub async fn list(&self, filter: RequestFilter) -> Result<Vec<RequestListItem>> {
        self.ensure_available()?;
        let path = Arc::clone(&self.path);
        tokio::task::spawn_blocking(move || query_list(&path, &filter)).await?
    }

    pub async fn get(&self, request_id: String) -> Result<Option<RequestEvent>> {
        self.ensure_available()?;
        let path = Arc::clone(&self.path);
        tokio::task::spawn_blocking(move || query_one(&path, &request_id)).await?
    }

    fn ensure_available(&self) -> Result<()> {
        if let Some(error) = &self.failure {
            anyhow::bail!("observation store unavailable: {error}");
        }
        Ok(())
    }
}

fn initialize(path: &Path, retention_days: u64) -> Result<()> {
    let connection = Connection::open(path).context("failed to open DuckDB observation store")?;
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS observation_schema(version INTEGER PRIMARY KEY, applied_at BIGINT NOT NULL);
        CREATE TABLE IF NOT EXISTS request_events(
            request_id VARCHAR PRIMARY KEY,
            trace_id VARCHAR NOT NULL,
            started_at BIGINT NOT NULL,
            finished_at BIGINT NOT NULL,
            endpoint VARCHAR NOT NULL,
            api_key_id VARCHAR,
            provider VARCHAR,
            requested_model VARCHAR,
            resolved_model VARCHAR,
            status_code INTEGER NOT NULL,
            error_kind VARCHAR,
            latency_ms BIGINT NOT NULL,
            ttft_ms BIGINT,
            input_tokens BIGINT NOT NULL,
            output_tokens BIGINT NOT NULL,
            cached_tokens BIGINT NOT NULL,
            cost_micros BIGINT NOT NULL,
            payload_captured BOOLEAN NOT NULL,
            request_json VARCHAR,
            response_json VARCHAR
        );
        CREATE INDEX IF NOT EXISTS idx_request_events_started ON request_events(started_at);
        CREATE INDEX IF NOT EXISTS idx_request_events_model ON request_events(requested_model);
        INSERT INTO observation_schema SELECT 1, epoch(current_timestamp)::BIGINT WHERE NOT EXISTS (SELECT 1 FROM observation_schema WHERE version=1);
        "#,
    ).context("failed to migrate DuckDB observation schema")?;
    let retention_seconds = retention_days.min((i64::MAX / 86_400) as u64) as i64 * 86_400;
    let cutoff = time::OffsetDateTime::now_utc()
        .unix_timestamp()
        .saturating_sub(retention_seconds);
    connection.execute("DELETE FROM request_events WHERE started_at < ?", [cutoff])?;
    Ok(())
}

fn insert_batch(connection: &Connection, events: &[Box<RequestEvent>]) -> Result<()> {
    connection.execute_batch("BEGIN TRANSACTION")?;
    for event in events {
        if let Err(error) = insert(connection, event) {
            let _ = connection.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    connection.execute_batch("COMMIT")?;
    Ok(())
}

fn insert(connection: &Connection, event: &RequestEvent) -> Result<()> {
    connection.execute(
        "INSERT OR REPLACE INTO request_events VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![
            event.request_id,
            event.trace_id,
            event.started_at,
            event.finished_at,
            event.endpoint,
            event.api_key_id,
            event.provider,
            event.requested_model,
            event.resolved_model,
            event.status_code,
            event.error_kind,
            event.latency_ms,
            event.ttft_ms,
            event.input_tokens,
            event.output_tokens,
            event.cached_tokens,
            event.cost_micros,
            event.payload_captured,
            event.request_json,
            event.response_json
        ],
    )?;
    Ok(())
}

fn query_summary(path: &Path) -> Result<Summary> {
    let connection = Connection::open(path)?;
    let since = time::OffsetDateTime::now_utc().unix_timestamp() - 24 * 3600;
    let mut summary = connection.query_row(
        "SELECT count(*),count(*) FILTER (WHERE status_code >= 400),coalesce(quantile_cont(latency_ms,0.95),0),coalesce(sum(input_tokens),0),coalesce(sum(output_tokens),0),coalesce(sum(cost_micros),0) FROM request_events WHERE started_at >= ?",
        [since],
        |row| Ok(Summary {
            requests: row.get(0)?, errors: row.get(1)?, error_rate: 0.0,
            p95_latency_ms: row.get(2)?, input_tokens: row.get(3)?, output_tokens: row.get(4)?, cost_micros: row.get(5)?, series: vec![],
        }),
    )?;
    summary.error_rate = if summary.requests == 0 {
        0.0
    } else {
        summary.errors as f64 / summary.requests as f64
    };
    let mut statement = connection.prepare(
        "SELECT floor(started_at/3600)*3600 AS bucket,count(*),count(*) FILTER (WHERE status_code >= 400),avg(latency_ms) FROM request_events WHERE started_at >= ? GROUP BY bucket ORDER BY bucket",
    )?;
    let points = statement.query_map([since], |row| {
        Ok(SummaryPoint {
            bucket: row.get(0)?,
            requests: row.get(1)?,
            errors: row.get(2)?,
            latency_ms: row.get(3)?,
        })
    })?;
    summary.series = points.collect::<duckdb::Result<Vec<_>>>()?;
    Ok(summary)
}

fn query_list(path: &Path, filter: &RequestFilter) -> Result<Vec<RequestListItem>> {
    let connection = Connection::open(path)?;
    let limit = filter.limit.unwrap_or(100).clamp(1, 500) as i64;
    let mut statement = connection.prepare(
        "SELECT request_id,started_at,endpoint,provider,requested_model,resolved_model,status_code,latency_ms,input_tokens,output_tokens,cost_micros FROM request_events WHERE (? IS NULL OR status_code=?) AND (? IS NULL OR provider=?) AND (? IS NULL OR requested_model=? OR resolved_model=?) ORDER BY started_at DESC LIMIT ?",
    )?;
    let rows = statement.query_map(
        params![
            filter.status_code,
            filter.status_code,
            filter.provider,
            filter.provider,
            filter.model,
            filter.model,
            filter.model,
            limit,
        ],
        |row| {
            Ok(RequestListItem {
                request_id: row.get(0)?,
                started_at: row.get(1)?,
                endpoint: row.get(2)?,
                provider: row.get(3)?,
                requested_model: row.get(4)?,
                resolved_model: row.get(5)?,
                status_code: row.get(6)?,
                latency_ms: row.get(7)?,
                input_tokens: row.get(8)?,
                output_tokens: row.get(9)?,
                cost_micros: row.get(10)?,
            })
        },
    )?;
    Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
}

fn query_one(path: &Path, request_id: &str) -> Result<Option<RequestEvent>> {
    let connection = Connection::open(path)?;
    let mut statement = connection.prepare("SELECT * FROM request_events WHERE request_id=?")?;
    let mut rows = statement.query([request_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(RequestEvent {
        request_id: row.get(0)?,
        trace_id: row.get(1)?,
        started_at: row.get(2)?,
        finished_at: row.get(3)?,
        endpoint: row.get(4)?,
        api_key_id: row.get(5)?,
        provider: row.get(6)?,
        requested_model: row.get(7)?,
        resolved_model: row.get(8)?,
        status_code: row.get(9)?,
        error_kind: row.get(10)?,
        latency_ms: row.get(11)?,
        ttft_ms: row.get(12)?,
        input_tokens: row.get(13)?,
        output_tokens: row.get(14)?,
        cached_tokens: row.get(15)?,
        cost_micros: row.get(16)?,
        payload_captured: row.get(17)?,
        request_json: row.get(18)?,
        response_json: row.get(19)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_and_queries_events() {
        let directory = tempfile::tempdir().unwrap();
        let store =
            ObservationStore::open(directory.path().join("observations.duckdb"), 30).unwrap();
        store.record(RequestEvent {
            request_id: "req-1".into(),
            trace_id: "trace-1".into(),
            started_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            finished_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            endpoint: "/v1/chat/completions".into(),
            api_key_id: None,
            provider: Some("OpenAI".into()),
            requested_model: Some("fast".into()),
            resolved_model: Some("gpt-test".into()),
            status_code: 200,
            error_kind: None,
            latency_ms: 42,
            ttft_ms: None,
            input_tokens: 10,
            output_tokens: 4,
            cached_tokens: 0,
            cost_micros: 3,
            payload_captured: false,
            request_json: None,
            response_json: None,
        });
        store.flush().await;
        let summary = store.summary().await.unwrap();
        assert_eq!(summary.requests, 1);
        assert_eq!(store.list(RequestFilter::default()).await.unwrap().len(), 1);
        assert_eq!(
            store.get("req-1".into()).await.unwrap().unwrap().latency_ms,
            42
        );
    }

    #[tokio::test]
    async fn degraded_store_drops_without_blocking_gateway_work() {
        let store = ObservationStore::degraded("/unavailable", "permission denied");
        store.record(RequestEvent {
            request_id: "req-degraded".into(),
            trace_id: "trace-degraded".into(),
            started_at: 0,
            finished_at: 0,
            endpoint: "/v1/models".into(),
            api_key_id: None,
            provider: None,
            requested_model: None,
            resolved_model: None,
            status_code: 200,
            error_kind: None,
            latency_ms: 0,
            ttft_ms: None,
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            cost_micros: 0,
            payload_captured: false,
            request_json: None,
            response_json: None,
        });
        assert!(!store.is_available());
        assert_eq!(store.dropped_events(), 1);
        assert!(store.summary().await.is_err());
    }
}
