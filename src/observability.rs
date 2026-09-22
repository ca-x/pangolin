use std::{
    collections::HashSet,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use anyhow::{Context, Result};
use duckdb::{Connection, params, params_from_iter};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEvent {
    /// Pangolin's own request UUID. This is the projection's identity: the
    /// client-supplied `x-request-id` is caller-controlled, so keying analytics
    /// on it let one client reusing an id overwrite another request's row.
    pub id: String,
    pub project_id: String,
    /// The client-supplied `x-request-id`, kept for correlation with clients.
    pub request_id: String,
    pub trace_id: String,
    /// The direct network peer recorded by the authoritative request row, or
    /// `None` when admission had no server-supplied peer address. This is metadata,
    /// never a forwarded header guessed from the request.
    #[serde(default)]
    pub source_ip: Option<String>,
    pub started_at: i64,
    pub finished_at: i64,
    pub endpoint: String,
    pub api_key_id: Option<String>,
    pub provider: Option<String>,
    pub requested_model: Option<String>,
    pub resolved_model: Option<String>,
    /// The exact upstream status, or `None` when none was observed — a legacy row, a
    /// locally answered request, or a failure before the provider answered. `0` is
    /// never a status, so it is never a value here either.
    pub status_code: Option<i32>,
    pub error_kind: Option<String>,
    pub latency_ms: i64,
    pub ttft_ms: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    /// Cache-write and reasoning tokens are settled usage, not cost components:
    /// they are the counts the provider reported, whether or not a price uses them.
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    /// Whether this request asked for a streamed response, decided at admission and
    /// read back from the authoritative request metadata. `None` means the record
    /// has no such decision — a row written before it was recorded, or an endpoint
    /// with no stream choice — and never a guessed `false`.
    pub stream: Option<bool>,
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
    /// Token and cost totals of the window, or `None` when the window settled no
    /// measured usage at all — a window of unmeasured failures has no total, and
    /// `0` would be a measurement nobody made. A measured zero stays `Some(0)`.
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost_micros: Option<i64>,
    pub series: Vec<SummaryPoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryPoint {
    pub bucket: i64,
    pub requests: i64,
    pub errors: i64,
    pub latency_ms: f64,
    /// The bucket's own totals, nullable by the same rule as the aggregate: an
    /// unmeasured bucket is a gap in a trend, never a plotted zero.
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost_micros: Option<i64>,
}

/// The window a summary is read over. Bounds are unix seconds on the projection's
/// event time (`started_at`) and the window is half-open — `from` is included,
/// `until` is not — the same convention `/analytics` reads its own facts with, so
/// the cards and the breakdown describe one set of events. An absent end is no
/// bound at all: the default window has never had an upper one.
#[derive(Debug, Clone, Copy, Deserialize, Default)]
pub struct SummaryFilter {
    /// Inclusive lower bound on `started_at`, in unix seconds.
    pub from: Option<i64>,
    /// Exclusive upper bound on `started_at`, in unix seconds.
    pub until: Option<i64>,
}

impl SummaryFilter {
    /// Whether the caller's own bounds are ordered. A window that ends before it
    /// starts cannot describe anything, so the routes refuse it rather than
    /// answering an empty summary that looks like a quiet instance. A missing end
    /// is not inverted: the clock fills it.
    pub fn is_ordered(&self) -> bool {
        match (self.from, self.until) {
            (Some(from), Some(until)) => from <= until,
            _ => true,
        }
    }
}

/// The window every caller without one still gets: the last 24 hours.
const DEFAULT_WINDOW_SECONDS: i64 = 24 * 3600;
const HOUR_SECONDS: i64 = 3600;
const DAY_SECONDS: i64 = 86_400;

/// The exact event-time bounds one summary read uses, and the bucket width that
/// span calls for. The window is half-open: `from` is in it, `until` is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryWindow {
    pub from: i64,
    /// The caller's own end, or `None` when they asked for none. The default
    /// window has never had an upper bound and keeps that behaviour: a row
    /// stamped a second ahead by clock skew is recorded traffic, not a window
    /// violation. A caller who names an end gets a half-open window ending there,
    /// so an event stamped exactly at `until` belongs to the next window — the
    /// same boundary `/analytics` draws.
    pub until: Option<i64>,
    pub bucket_seconds: i64,
}

impl SummaryWindow {
    /// Resolves the caller's bounds against the clock once per read: an absent
    /// start is the default 24 hours ending at the caller's end (or at now), and
    /// the bucket width follows the span that results.
    pub fn resolve(filter: SummaryFilter, now: i64) -> Self {
        let until = filter.until;
        let end = until.unwrap_or(now);
        let from = filter.from.unwrap_or(end - DEFAULT_WINDOW_SECONDS);
        Self {
            from,
            until,
            bucket_seconds: bucket_seconds(end.saturating_sub(from)),
        }
    }
}

/// Buckets follow the *span* of the window, not the number of rows in it: a short
/// window is read hourly, a fortnight six-hourly, and anything longer — including
/// all retained history, whose ultimate bound is the projection's own retention —
/// daily. A response is therefore a bounded aggregate however much traffic the
/// window holds.
fn bucket_seconds(span: i64) -> i64 {
    if span <= 48 * HOUR_SECONDS {
        HOUR_SECONDS
    } else if span <= 14 * DAY_SECONDS {
        6 * HOUR_SECONDS
    } else {
        DAY_SECONDS
    }
}

/// The projection's form of the console's `usageMeasured` rule. A row that never
/// failed is measured; `usage_unavailable` is the gateway's own verdict that the
/// terminal usage report was lost; and a failure that settled as no tokens and no
/// cost is the same fact from the other side. A partial settlement — a stream that
/// broke after it had reported usage — keeps its real numbers.
const MEASURED_USAGE: &str = "(error_kind IS NULL OR (error_kind <> 'usage_unavailable' AND (input_tokens + output_tokens > 0 OR cost_micros > 0)))";

#[derive(Debug, Clone, Serialize)]
pub struct RequestListItem {
    /// Pangolin's own request UUID: unique, and the identity the console keys rows
    /// and detail links by. The external `request_id` cannot do that job — a client
    /// chooses it, so it repeats.
    pub internal_id: String,
    /// The client-supplied `x-request-id`, kept so an operator can correlate a row
    /// with the client's own logs.
    pub request_id: String,
    pub started_at: i64,
    pub endpoint: String,
    pub provider: Option<String>,
    pub requested_model: Option<String>,
    pub resolved_model: Option<String>,
    /// The upstream status the attempt recorded, or `None` when nothing was measured.
    pub status_code: Option<i32>,
    /// Why the request failed, when the record system knows. Without it a failure
    /// row is only a number.
    pub error_kind: Option<String>,
    /// Which key made the request, so the log can be filtered by it.
    pub api_key_id: Option<String>,
    pub latency_ms: i64,
    /// Time to the first streamed byte, or `None` when none was measured: a
    /// non-stream response legitimately has no first byte to time.
    pub ttft_ms: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Cache-read tokens the provider reported for this request.
    pub cached_tokens: i64,
    /// Cache-write and reasoning tokens, from the settled usage row rather than
    /// from the cost components a price happened to charge for.
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    /// Whether the provider request was streamed, as decided at admission. `None`
    /// when nothing recorded that decision — never a guessed `false`.
    pub stream: Option<bool>,
    pub cost_micros: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RequestFilter {
    #[serde(skip)]
    pub project_id: Option<String>,
    pub status_code: Option<i32>,
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Which key made the requests. The projection has always carried the key id;
    /// nothing could filter on it.
    pub api_key_id: Option<String>,
    /// Inclusive lower bound on `started_at`, in unix seconds.
    pub from: Option<i64>,
    /// Inclusive upper bound on `started_at`, in unix seconds.
    pub until: Option<i64>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

enum Command {
    Record(Box<RequestEvent>, u64),
    Flush(oneshot::Sender<()>),
    Reset(u64, oneshot::Sender<bool>),
    Retain(Vec<Retention>, oneshot::Sender<bool>),
    /// Replace the projection with rows re-derived from the record system.
    Rebuild(Vec<RequestEvent>, oneshot::Sender<usize>),
    /// Every read goes through the writer's connection. Opening the file a second
    /// time checkpoints and deletes the WAL the writer holds, which silently
    /// freezes the projection for the life of the process.
    Read(Read, oneshot::Sender<Result<ReadOutcome>>),
}

/// A read the writer thread performs on its own connection.
enum Read {
    Summary {
        project: Option<String>,
        filter: SummaryFilter,
    },
    List(RequestFilter),
    Count(RequestFilter),
    /// One request by identity. `project_id` scopes the query itself; `None` is the
    /// unscoped read diagnostics use.
    One {
        project_id: Option<String>,
        identifier: String,
    },
}

enum ReadOutcome {
    Summary(Box<Summary>),
    List(Vec<RequestListItem>),
    Count(i64),
    One(Option<Box<RequestEvent>>),
}

#[derive(Clone)]
pub struct Retention {
    pub project_id: Option<String>,
    pub days: i64,
    pub payloads_only: bool,
    /// The requests the record system has pinned, by Pangolin's own request uuid.
    ///
    /// `RequestEvent.id` **is** `requests.id` — the projection's identity and the
    /// record system's identity are the same value — so a retained trace names the
    /// events that must not expire without any caller-supplied string reaching a
    /// statement. The set is shared by every rule of one pass, because the
    /// instance-wide window is itself a rule: a pin that only survived the
    /// project's own policy would still be expired by the default window.
    pub preserve: Arc<HashSet<String>>,
}
impl Retention {
    pub fn new(project_id: Option<String>, days: i64, payloads_only: bool) -> Self {
        Self {
            project_id,
            days,
            payloads_only,
            preserve: Arc::new(HashSet::new()),
        }
    }
    /// The same rule with the requests a retained trace owns excluded from it.
    pub fn preserving(mut self, preserve: Arc<HashSet<String>>) -> Self {
        self.preserve = preserve;
        self
    }
    fn cutoff(&self) -> i64 {
        time::OffsetDateTime::now_utc()
            .unix_timestamp()
            .saturating_sub(self.days.saturating_mul(86400))
    }
    fn matches(&self, event: &RequestEvent) -> bool {
        // A pinned request is never expired and its payload is never stripped,
        // whatever window it falls in: the promise is about this request, and
        // admission reads the same predicate, so a late event is spared too.
        !self.preserve.contains(&event.id)
            && event.started_at < self.cutoff()
            && self
                .project_id
                .as_ref()
                .is_none_or(|project| project == &event.project_id || event.project_id.is_empty())
    }
}

/// How many pinned request ids one statement carries. The statement text is built
/// from this constant and never from data, and the ids themselves are bound values.
const PRESERVE_CHUNK: usize = 500;

/// The union of every rule's pinned ids, one entry per unique request.
///
/// Built as a set rather than by flattening the rules: `sync_retention` puts the
/// same `Arc` set on every rule of a pass, so a flattened intermediate would hold
/// `rules × pins` borrowed entries before it was reduced — memory that scales with
/// the number of project policies, for a set that is identical in all of them.
/// Bounded here by the unique pins, and sorted so the statement is deterministic.
fn pinned_ids(rules: &[Retention]) -> Vec<&str> {
    let mut unique: HashSet<&str> = HashSet::new();
    for rule in rules {
        unique.extend(rule.preserve.iter().map(String::as_str));
    }
    let mut ids: Vec<&str> = unique.into_iter().collect();
    ids.sort_unstable();
    ids
}

fn retain(connection: &Connection, rules: &[Retention]) -> Result<()> {
    // The pinned ids go into a temp table rather than an `IN` list: nothing bounds
    // how many requests an operator may pin, and one statement cannot carry an
    // unbounded parameter list. An empty table leaves every rule exactly as it was.
    connection
        .execute_batch("CREATE OR REPLACE TEMP TABLE retained_requests(id VARCHAR PRIMARY KEY)")?;
    let preserved = pinned_ids(rules);
    for chunk in preserved.chunks(PRESERVE_CHUNK) {
        let values = vec!["(?)"; chunk.len()].join(",");
        connection.execute(
            &format!("INSERT INTO retained_requests(id) VALUES {values}"),
            params_from_iter(chunk.iter()),
        )?;
    }
    for rule in rules {
        let mutation = if rule.payloads_only {
            "UPDATE request_events SET payload_captured=false,request_json=NULL,response_json=NULL"
        } else {
            "DELETE FROM request_events"
        };
        connection.execute(
            &format!(
                "{mutation} WHERE started_at<? AND (? IS NULL OR project_id=? OR project_id='') AND id NOT IN (SELECT id FROM retained_requests)"
            ),
            params![rule.cutoff(), rule.project_id, rule.project_id],
        )?;
    }
    Ok(())
}

/// Projection writes can be lost for different reasons; keeping them apart makes the loss
/// diagnosable instead of collapsing every cause into one opaque counter.
#[derive(Default)]
struct DropCounters {
    queue_full: AtomicU64,
    writer_down: AtomicU64,
    stale_generation: AtomicU64,
    batch_failed: AtomicU64,
    /// Events refused because the writer was marked unusable after repeated
    /// batch failures — distinct from a generation mismatch, which the store
    /// recovers from on its own.
    poisoned: AtomicU64,
}

impl DropCounters {
    fn total(&self) -> u64 {
        self.queue_full.load(Ordering::Relaxed)
            + self.writer_down.load(Ordering::Relaxed)
            + self.stale_generation.load(Ordering::Relaxed)
            + self.batch_failed.load(Ordering::Relaxed)
            + self.poisoned.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
pub struct ObservationStore {
    sender: Option<mpsc::Sender<Command>>,
    failure: Option<Arc<str>>,
    dropped: Arc<DropCounters>,
    generation: Arc<AtomicU64>,
    poisoned: Arc<AtomicBool>,
    writer_down: Arc<AtomicBool>,
    /// The projection shape changed on disk, so the retained window has to be
    /// re-derived from the record system before the console can be trusted.
    rebuild_required: bool,
}

impl ObservationStore {
    pub fn open(path: impl AsRef<Path>, retention_days: u64) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let outdated = initialize(&path)?;
        let (sender, mut receiver) = mpsc::channel::<Command>(4096);
        let writer_path = path.clone();
        let dropped = Arc::new(DropCounters::default());
        let writer_dropped = Arc::clone(&dropped);
        let generation = Arc::new(AtomicU64::new(0));
        let poisoned = Arc::new(AtomicBool::new(false));
        let writer_poisoned = poisoned.clone();
        let writer_down = Arc::new(AtomicBool::new(false));
        let writer_down_flag = Arc::clone(&writer_down);
        thread::Builder::new().name("pangolin-observation-writer".into()).spawn(move || {
            let connection = match Connection::open(&writer_path) {
                Ok(connection) => connection,
                Err(error) => {
                    // The projection is dead for the life of the process; report it instead of
                    // pretending the store is healthy while every event is dropped.
                    tracing::error!(%error, "observation writer failed to open");
                    writer_down_flag.store(true, Ordering::Release);
                    writer_poisoned.store(true, Ordering::Release);
                    return;
                }
            };
            let mut pending=None;let mut epoch=0;let mut consecutive_failures=0;
            let mut retention=vec![Retention::new(None,retention_days.min(i64::MAX as u64) as i64,false)];
            while let Some(command) = pending.take().or_else(||receiver.blocking_recv()) {
                match command {
                    Command::Record(event,version) => {
                        if version!=epoch {writer_dropped.stale_generation.fetch_add(1,Ordering::Relaxed);continue}
                        let mut batch = vec![event];
                        while batch.len() < 64 {
                            match receiver.try_recv() {
                                Ok(Command::Record(event,version)) if version==epoch => batch.push(event),
                                Ok(Command::Record(_, _))=>{writer_dropped.stale_generation.fetch_add(1,Ordering::Relaxed);},
                                Ok(control) => { pending=Some(control);break; }
                                Err(_) => break,
                            }
                        }
                        batch.retain_mut(|event| {
                            for rule in &retention {
                                if rule.matches(event) {
                                    if !rule.payloads_only {return false;}
                                    event.payload_captured=false;event.request_json=None;event.response_json=None;
                                }
                            }
                            true
                        });
                        match insert_batch(&connection, &batch) {
                            Ok(()) => {
                                consecutive_failures = 0;
                                // A batch that lands proves the writer works again.
                                writer_poisoned.store(false, Ordering::Release);
                            }
                            Err(error) => {
                                consecutive_failures += 1;
                                writer_dropped.batch_failed.fetch_add(batch.len() as u64, Ordering::Relaxed);
                                tracing::warn!(%error, count = batch.len(), consecutive_failures, "observation event batch was dropped");
                                if consecutive_failures >= 3 {
                                    // Fail closed: an operator must see that analytics stopped
                                    // rather than read an empty-but-plausible dashboard.
                                    tracing::error!(%error, "observation writer is failing repeatedly; analytics are degraded");
                                    writer_poisoned.store(true, Ordering::Release);
                                }
                            }
                        }
                    }
                    Command::Flush(done) => {
                        let _ = connection.execute_batch("CHECKPOINT");
                        let _ = done.send(());
                    }
                    Command::Reset(version,done)=>{
                        epoch=version;let cleared=connection.execute_batch("DELETE FROM request_events; CHECKPOINT;").is_ok();
                        writer_poisoned.store(!cleared,Ordering::Release);let _=done.send(cleared);
                    }
                    Command::Retain(rules,done)=>{
                        retention=rules;
                        let applied=retain(&connection,&retention).is_ok();
                        writer_poisoned.store(!applied,Ordering::Release);
                        let _=done.send(applied);
                    }
                    Command::Read(read,done)=>{
                        let outcome = match read {
                            Read::Summary { project, filter } => {
                                query_summary_for(&connection, project.as_deref(), filter)
                                    .map(|value| ReadOutcome::Summary(Box::new(value)))
                            }
                            Read::List(filter) => query_list(&connection, &filter)
                                .map(ReadOutcome::List),
                            Read::Count(filter) => query_count(&connection, &filter)
                                .map(ReadOutcome::Count),
                            Read::One {
                                project_id,
                                identifier,
                            } => query_one(&connection, project_id.as_deref(), &identifier)
                                .map(|value| ReadOutcome::One(value.map(Box::new))),
                        };
                        let _ = done.send(outcome);
                    }
                    Command::Rebuild(events,done)=>{
                        let expected=events.len();
                        let rebuilt=(|| -> Result<usize> {
                            connection.execute_batch("DELETE FROM request_events")?;
                            if !events.is_empty() {
                                let boxed: Vec<Box<RequestEvent>> =
                                    events.iter().cloned().map(Box::new).collect();
                                insert_batch(&connection,&boxed)?;
                            }
                            Ok(expected)
                        })().unwrap_or_else(|error| {
                            tracing::error!(%error,"observation projection rebuild failed");
                            0
                        });
                        writer_poisoned.store(rebuilt!=expected,Ordering::Release);
                        if rebuilt==expected { consecutive_failures=0; }
                        let _=done.send(rebuilt);
                    }
                }
            }
        }).context("failed to spawn observation writer")?;
        Ok(Self {
            sender: Some(sender),
            failure: None,
            dropped,
            generation,
            poisoned,
            writer_down,
            rebuild_required: outdated,
        })
    }

    /// True when the on-disk projection predates the current shape and must be
    /// rebuilt from the record system.
    pub fn needs_rebuild(&self) -> bool {
        self.rebuild_required
    }

    pub fn degraded(_path: impl AsRef<Path>, error: impl std::fmt::Display) -> Self {
        Self {
            sender: None,
            rebuild_required: false,
            failure: Some(Arc::from(error.to_string())),
            dropped: Arc::new(DropCounters::default()),
            generation: Arc::new(AtomicU64::new(0)),
            poisoned: Arc::new(AtomicBool::new(true)),
            writer_down: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn is_available(&self) -> bool {
        self.failure.is_none()
            && !self.poisoned.load(Ordering::Acquire)
            && !self.writer_down.load(Ordering::Acquire)
            && self
                .sender
                .as_ref()
                .is_some_and(|sender| !sender.is_closed())
    }

    pub fn dropped_events(&self) -> u64 {
        self.dropped.total()
    }

    /// Drops by cause: queue pressure, a dead writer, stale generations, failed batches.
    pub fn dropped_breakdown(&self) -> [(&'static str, u64); 5] {
        [
            (
                "queue_full",
                self.dropped.queue_full.load(Ordering::Relaxed),
            ),
            (
                "writer_down",
                self.dropped.writer_down.load(Ordering::Relaxed),
            ),
            (
                "stale_generation",
                self.dropped.stale_generation.load(Ordering::Relaxed),
            ),
            (
                "batch_failed",
                self.dropped.batch_failed.load(Ordering::Relaxed),
            ),
            ("poisoned", self.dropped.poisoned.load(Ordering::Relaxed)),
        ]
    }

    #[cfg(test)]
    pub fn record(&self, event: RequestEvent) {
        self.record_at(event, self.generation());
    }
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub fn record_at(&self, event: RequestEvent, generation: u64) {
        if generation != self.generation() {
            self.dropped
                .stale_generation
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        if self.poisoned.load(Ordering::Acquire) {
            self.dropped.poisoned.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let Some(sender) = &self.sender else {
            self.dropped.writer_down.fetch_add(1, Ordering::Relaxed);
            return;
        };
        match sender.try_send(Command::Record(Box::new(event), generation)) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.dropped.queue_full.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("observation queue is full; event dropped");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // The writer thread is gone: every later event would be lost silently.
                self.dropped.writer_down.fetch_add(1, Ordering::Relaxed);
                self.writer_down.store(true, Ordering::Release);
                tracing::error!("observation writer is gone; analytics are degraded");
            }
        }
    }
    pub async fn clear_for_restore(&self) -> bool {
        self.poisoned.store(true, Ordering::Release);
        let epoch = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let (done, result) = oneshot::channel();
        if let Some(sender) = &self.sender
            && sender.send(Command::Reset(epoch, done)).await.is_ok()
        {
            return result.await.unwrap_or(false);
        }
        false
    }

    /// Replace the projection with rows re-derived from the record system.
    ///
    /// The projection is derived data: after an instance restore it is cleared,
    /// and leaving it empty made the console report zero traffic for the whole
    /// retained window. Returns the number of rows now projected.
    pub async fn rebuild(&self, events: Vec<RequestEvent>) -> usize {
        let expected = events.len();
        let (done, result) = oneshot::channel();
        if let Some(sender) = &self.sender
            && sender.send(Command::Rebuild(events, done)).await.is_ok()
        {
            let rebuilt = result.await.unwrap_or(0);
            self.poisoned.store(rebuilt != expected, Ordering::Release);
            return rebuilt;
        }
        0
    }

    pub async fn apply_retention(&self, rules: Vec<Retention>) -> bool {
        let (done, result) = oneshot::channel();
        if let Some(sender) = &self.sender
            && sender.send(Command::Retain(rules, done)).await.is_ok()
        {
            let applied = result.await.unwrap_or(false);
            if applied {
                // A pass that succeeds proves the writer is usable again, so a
                // transient failure earlier must not latch it off forever.
                self.poisoned.store(false, Ordering::Release);
            } else {
                self.poisoned.store(true, Ordering::Release);
            }
            return applied;
        }
        self.poisoned.store(true, Ordering::Release);
        false
    }

    pub async fn flush(&self) {
        let (sender, receiver) = oneshot::channel();
        if let Some(command_sender) = &self.sender
            && command_sender.send(Command::Flush(sender)).await.is_ok()
        {
            let _ = receiver.await;
        }
    }

    async fn read(&self, read: Read) -> Result<ReadOutcome> {
        self.ensure_available()?;
        let (done, result) = oneshot::channel();
        let Some(sender) = &self.sender else {
            anyhow::bail!("observation projection is unavailable")
        };
        sender
            .send(Command::Read(read, done))
            .await
            .map_err(|_| anyhow::anyhow!("observation writer is unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("observation writer dropped the read"))?
    }

    /// The unscoped summary, over the caller's window. `SummaryFilter::default()`
    /// is the projection's own default: the last 24 hours.
    pub async fn summary(&self, filter: SummaryFilter) -> Result<Summary> {
        match self
            .read(Read::Summary {
                project: None,
                filter,
            })
            .await?
        {
            ReadOutcome::Summary(value) => Ok(*value),
            _ => anyhow::bail!("observation writer returned the wrong answer"),
        }
    }

    /// One project's summary, over the caller's window.
    pub async fn summary_for(&self, project_id: String, filter: SummaryFilter) -> Result<Summary> {
        match self
            .read(Read::Summary {
                project: Some(project_id),
                filter,
            })
            .await?
        {
            ReadOutcome::Summary(value) => Ok(*value),
            _ => anyhow::bail!("observation writer returned the wrong answer"),
        }
    }

    pub async fn list(&self, filter: RequestFilter) -> Result<Vec<RequestListItem>> {
        match self.read(Read::List(filter)).await? {
            ReadOutcome::List(value) => Ok(value),
            _ => anyhow::bail!("observation writer returned the wrong answer"),
        }
    }

    pub async fn count(&self, filter: RequestFilter) -> Result<i64> {
        match self.read(Read::Count(filter)).await? {
            ReadOutcome::Count(value) => Ok(value),
            _ => anyhow::bail!("observation writer returned the wrong answer"),
        }
    }

    /// The unscoped read, for diagnostics and tests. The console uses `get_for`.
    pub async fn get(&self, request_id: String) -> Result<Option<RequestEvent>> {
        self.resolve(None, request_id).await
    }

    /// One request inside a project, by Pangolin's internal id or by the external
    /// `x-request-id`. The project is part of the query: a newer row in another
    /// project that happens to share the external id must not decide this project's
    /// answer, and no foreign row is read to produce it.
    pub async fn get_for(
        &self,
        project_id: String,
        request_id: String,
    ) -> Result<Option<RequestEvent>> {
        self.resolve(Some(project_id), request_id).await
    }

    async fn resolve(
        &self,
        project_id: Option<String>,
        identifier: String,
    ) -> Result<Option<RequestEvent>> {
        match self
            .read(Read::One {
                project_id,
                identifier,
            })
            .await?
        {
            ReadOutcome::One(value) => Ok(value.map(|event| *event)),
            _ => anyhow::bail!("observation writer returned the wrong answer"),
        }
    }

    fn ensure_available(&self) -> Result<()> {
        if self.poisoned.load(Ordering::Acquire) {
            anyhow::bail!("observation projection is unavailable")
        }
        if let Some(error) = &self.failure {
            anyhow::bail!("observation store unavailable: {error}");
        }
        Ok(())
    }
}

/// Bump whenever `request_events` changes shape. A mismatch drops the projection
/// and asks the caller to re-derive it from the record system.
const SCHEMA_VERSION: i64 = 5;

fn initialize(path: &Path) -> Result<bool> {
    let connection = Connection::open(path).context("failed to open DuckDB observation store")?;
    let stored: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM observation_schema",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let outdated = stored != SCHEMA_VERSION;
    if outdated {
        // Derived data only: dropping it is safe because it is re-derived.
        let _ = connection.execute_batch("DROP TABLE IF EXISTS request_events");
    }
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS observation_schema(version INTEGER PRIMARY KEY, applied_at BIGINT NOT NULL);
        CREATE TABLE IF NOT EXISTS request_events(
            id VARCHAR PRIMARY KEY,
            request_id VARCHAR NOT NULL,
            project_id VARCHAR NOT NULL,
            trace_id VARCHAR NOT NULL,
            started_at BIGINT NOT NULL,
            finished_at BIGINT NOT NULL,
            endpoint VARCHAR NOT NULL,
            api_key_id VARCHAR,
            provider VARCHAR,
            requested_model VARCHAR,
            resolved_model VARCHAR,
            -- Nullable: an unobserved status is NULL, never an invented 200 or 502.
            status_code INTEGER,
            error_kind VARCHAR,
            latency_ms BIGINT NOT NULL,
            ttft_ms BIGINT,
            input_tokens BIGINT NOT NULL,
            output_tokens BIGINT NOT NULL,
            cached_tokens BIGINT NOT NULL,
            cost_micros BIGINT NOT NULL,
            payload_captured BOOLEAN NOT NULL,
            request_json VARCHAR,
            response_json VARCHAR,
            -- Appended, never inserted: `query_one` reads this table positionally.
            cache_write_tokens BIGINT NOT NULL,
            reasoning_tokens BIGINT NOT NULL,
            -- Nullable: a stream decision nobody recorded is NULL, never a false.
            stream BOOLEAN,
            -- Appended: the direct peer recorded by SQLite, never a forwarding header.
            source_ip VARCHAR
        );
        CREATE INDEX IF NOT EXISTS idx_request_events_started ON request_events(started_at);
        CREATE INDEX IF NOT EXISTS idx_request_events_external ON request_events(request_id);
        -- Every console read is scoped by project and windowed by time.
        CREATE INDEX IF NOT EXISTS idx_request_events_project_started ON request_events(project_id, started_at);
        CREATE INDEX IF NOT EXISTS idx_request_events_model ON request_events(requested_model);
        ALTER TABLE request_events ADD COLUMN IF NOT EXISTS project_id VARCHAR DEFAULT '';
        "#,
    ).context("failed to migrate DuckDB observation schema")?;
    connection
        .execute(
            "DELETE FROM observation_schema WHERE version <> ?",
            [SCHEMA_VERSION],
        )
        .context("failed to stamp the DuckDB observation schema version")?;
    connection
        .execute(
            "INSERT INTO observation_schema(version, applied_at) SELECT ?, epoch(current_timestamp)::BIGINT WHERE NOT EXISTS (SELECT 1 FROM observation_schema WHERE version = ?)",
            [SCHEMA_VERSION, SCHEMA_VERSION],
        )
        .context("failed to stamp the DuckDB observation schema version")?;
    // Expiry belongs to the retention pass, not to opening the file: that pass is
    // the only place that knows which requests the record system pinned, and a
    // trim here deleted a pinned run's events before it could ever be told. The
    // instance's own observation window is applied by the same pass — it is the
    // first rule `sync_retention` builds, and the writer's initial rule already
    // refuses to admit an event older than it — so the bound is unchanged.
    Ok(outdated)
}

fn insert_batch(connection: &Connection, events: &[Box<RequestEvent>]) -> Result<()> {
    // Every failure path must end the transaction: DuckDB keeps an aborted
    // transaction open, so a bare `?` on COMMIT made the next BEGIN fail and the
    // writer latch itself off permanently from one transient conflict.
    connection.execute_batch("BEGIN TRANSACTION")?;
    for event in events {
        if let Err(error) = insert(connection, event) {
            let _ = connection.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    if let Err(error) = connection.execute_batch("COMMIT") {
        // DuckDB keeps an aborted transaction open, so a bare `?` here made the
        // next BEGIN fail and the writer latch itself off permanently.
        let _ = connection.execute_batch("ROLLBACK");
        return Err(error.into());
    }
    Ok(())
}

fn insert(connection: &Connection, event: &RequestEvent) -> Result<()> {
    connection.execute(
        "INSERT OR REPLACE INTO request_events VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![
            event.id,
            event.request_id,
            event.project_id,
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
            event.response_json,
            event.cache_write_tokens,
            event.reasoning_tokens,
            event.stream,
            event.source_ip
        ],
    )?;
    Ok(())
}

/// Reads the window's totals and its trend.
///
/// Both queries use the same resolved bounds and the same event time, so the
/// series and the aggregate describe one set of events — and both draw the window
/// half-open (`started_at >= from AND started_at < until`), which is the boundary
/// `/analytics` draws over its own facts. Bucket width follows the span, so a
/// window over all retained history returns daily aggregates instead of every
/// request in it.
fn query_summary_for(
    connection: &Connection,
    project_id: Option<&str>,
    filter: SummaryFilter,
) -> Result<Summary> {
    let window = SummaryWindow::resolve(filter, time::OffsetDateTime::now_utc().unix_timestamp());
    let mut summary = connection.query_row(
        &format!(
            "SELECT count(*),count(*) FILTER (WHERE error_kind IS NOT NULL OR status_code >= 400),coalesce(quantile_cont(latency_ms,0.95),0),sum(input_tokens) FILTER (WHERE {MEASURED_USAGE}),sum(output_tokens) FILTER (WHERE {MEASURED_USAGE}),sum(cost_micros) FILTER (WHERE {MEASURED_USAGE}) FROM request_events WHERE started_at >= ? AND (? IS NULL OR started_at < ?) AND (? IS NULL OR project_id=?)"
        ),
        params![window.from, window.until, window.until, project_id, project_id],
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
    let mut statement = connection.prepare(&format!(
        "SELECT CAST(floor(started_at/?)*? AS BIGINT) AS bucket,count(*),count(*) FILTER (WHERE error_kind IS NOT NULL OR status_code >= 400),avg(latency_ms),sum(input_tokens) FILTER (WHERE {MEASURED_USAGE}),sum(output_tokens) FILTER (WHERE {MEASURED_USAGE}),sum(cost_micros) FILTER (WHERE {MEASURED_USAGE}) FROM request_events WHERE started_at >= ? AND (? IS NULL OR started_at < ?) AND (? IS NULL OR project_id=?) GROUP BY bucket ORDER BY bucket"
    ))?;
    let points = statement.query_map(
        params![
            window.bucket_seconds,
            window.bucket_seconds,
            window.from,
            window.until,
            window.until,
            project_id,
            project_id
        ],
        |row| {
            Ok(SummaryPoint {
                bucket: row.get(0)?,
                requests: row.get(1)?,
                errors: row.get(2)?,
                latency_ms: row.get(3)?,
                input_tokens: row.get(4)?,
                output_tokens: row.get(5)?,
                cost_micros: row.get(6)?,
            })
        },
    )?;
    summary.series = points.collect::<duckdb::Result<Vec<_>>>()?;
    Ok(summary)
}

fn query_list(connection: &Connection, filter: &RequestFilter) -> Result<Vec<RequestListItem>> {
    let limit = filter.limit.unwrap_or(100).clamp(1, 500) as i64;
    let mut statement = connection.prepare(
        "SELECT id,request_id,started_at,endpoint,provider,requested_model,resolved_model,status_code,error_kind,api_key_id,latency_ms,input_tokens,output_tokens,cost_micros,ttft_ms,cached_tokens,cache_write_tokens,reasoning_tokens,stream FROM request_events WHERE (? IS NULL OR project_id=?) AND (? IS NULL OR status_code=?) AND (? IS NULL OR provider=?) AND (? IS NULL OR requested_model=? OR resolved_model=?) AND (? IS NULL OR api_key_id=?) AND (? IS NULL OR started_at>=?) AND (? IS NULL OR started_at<=?) ORDER BY started_at DESC,id DESC LIMIT ? OFFSET ?",
    )?;
    let rows = statement.query_map(
        params![
            filter.project_id,
            filter.project_id,
            filter.status_code,
            filter.status_code,
            filter.provider,
            filter.provider,
            filter.model,
            filter.model,
            filter.model,
            filter.api_key_id,
            filter.api_key_id,
            filter.from,
            filter.from,
            filter.until,
            filter.until,
            limit,
            filter.offset.unwrap_or(0).min(100_000) as i64,
        ],
        |row| {
            Ok(RequestListItem {
                internal_id: row.get(0)?,
                request_id: row.get(1)?,
                started_at: row.get(2)?,
                endpoint: row.get(3)?,
                provider: row.get(4)?,
                requested_model: row.get(5)?,
                resolved_model: row.get(6)?,
                status_code: row.get(7)?,
                error_kind: row.get(8)?,
                api_key_id: row.get(9)?,
                latency_ms: row.get(10)?,
                input_tokens: row.get(11)?,
                output_tokens: row.get(12)?,
                cost_micros: row.get(13)?,
                ttft_ms: row.get(14)?,
                cached_tokens: row.get(15)?,
                cache_write_tokens: row.get(16)?,
                reasoning_tokens: row.get(17)?,
                stream: row.get(18)?,
            })
        },
    )?;
    Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
}

fn query_count(connection: &Connection, filter: &RequestFilter) -> Result<i64> {
    Ok(connection.query_row(
        "SELECT count(*) FROM request_events WHERE (? IS NULL OR project_id=?) AND (? IS NULL OR status_code=?) AND (? IS NULL OR provider=?) AND (? IS NULL OR requested_model=? OR resolved_model=?) AND (? IS NULL OR api_key_id=?) AND (? IS NULL OR started_at>=?) AND (? IS NULL OR started_at<=?)",
        params![
            filter.project_id,
            filter.project_id,
            filter.status_code,
            filter.status_code,
            filter.provider,
            filter.provider,
            filter.model,
            filter.model,
            filter.model,
            filter.api_key_id,
            filter.api_key_id,
            filter.from,
            filter.from,
            filter.until,
            filter.until,
        ],
        |row| row.get(0),
    )?)
}

/// Reads one request by identity.
///
/// `identifier` is either Pangolin's internal request UUID or the caller-controlled
/// external `x-request-id`. The internal id is unique, so an exact internal match wins:
/// a client that sends an id equal to another request's internal UUID must not shadow
/// that request. Otherwise the newest external match wins, with the internal `id`
/// breaking a timestamp tie so the answer never depends on row order. The project is
/// part of this query rather than a filter applied to its result — the global
/// newest-first lookup this replaced let a newer row in another project hide this
/// project's row and turn its detail read into a 404.
fn query_one(
    connection: &Connection,
    project_id: Option<&str>,
    identifier: &str,
) -> Result<Option<RequestEvent>> {
    let mut statement = connection.prepare(
        "SELECT * FROM request_events WHERE (? IS NULL OR project_id=?) AND (id=? OR request_id=?) ORDER BY CASE WHEN id=? THEN 0 ELSE 1 END,started_at DESC,id DESC LIMIT 1",
    )?;
    let mut rows = statement.query(params![
        project_id, project_id, identifier, identifier, identifier
    ])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(RequestEvent {
        id: row.get(0)?,
        request_id: row.get(1)?,
        project_id: row.get(2)?,
        trace_id: row.get(3)?,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        endpoint: row.get(6)?,
        api_key_id: row.get(7)?,
        provider: row.get(8)?,
        requested_model: row.get(9)?,
        resolved_model: row.get(10)?,
        status_code: row.get(11)?,
        error_kind: row.get(12)?,
        latency_ms: row.get(13)?,
        ttft_ms: row.get(14)?,
        input_tokens: row.get(15)?,
        output_tokens: row.get(16)?,
        cached_tokens: row.get(17)?,
        cost_micros: row.get(18)?,
        payload_captured: row.get(19)?,
        request_json: row.get(20)?,
        response_json: row.get(21)?,
        // Appended to the table, so `SELECT *` keeps every earlier position.
        cache_write_tokens: row.get(22)?,
        reasoning_tokens: row.get(23)?,
        stream: row.get(24)?,
        source_ip: row.get(25)?,
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
            id: "internal-1".into(),
            project_id: "test-project".into(),
            request_id: "req-1".into(),
            trace_id: "trace-1".into(),
            source_ip: Some("203.0.113.7".into()),
            started_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            finished_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            endpoint: "/v1/chat/completions".into(),
            api_key_id: None,
            provider: Some("OpenAI".into()),
            requested_model: Some("fast".into()),
            resolved_model: Some("gpt-test".into()),
            status_code: Some(200),
            error_kind: None,
            latency_ms: 42,
            ttft_ms: None,
            input_tokens: 10,
            output_tokens: 4,
            cached_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            stream: None,
            cost_micros: 3,
            payload_captured: false,
            request_json: None,
            response_json: None,
        });
        store.flush().await;
        let summary = store.summary(SummaryFilter::default()).await.unwrap();
        assert_eq!(summary.requests, 1);
        assert_eq!(store.list(RequestFilter::default()).await.unwrap().len(), 1);
        assert_eq!(
            store.get("req-1".into()).await.unwrap().unwrap().latency_ms,
            42
        );
        assert_eq!(
            store
                .get("req-1".into())
                .await
                .unwrap()
                .unwrap()
                .source_ip
                .as_deref(),
            Some("203.0.113.7")
        );
        let old = store.get("req-1".into()).await.unwrap().unwrap();
        let epoch = store.generation();
        assert!(store.clear_for_restore().await);
        store.record_at(old.clone(), epoch);
        store.record(RequestEvent {
            id: "internal-new".into(),
            request_id: "req-new".into(),
            ..old
        });
        store.flush().await;
        assert!(store.get("req-1".into()).await.unwrap().is_none());
        assert_eq!(
            store
                .summary(SummaryFilter::default())
                .await
                .unwrap()
                .requests,
            1
        );
        assert_eq!(store.dropped_events(), 1);
    }

    /// A failure row used to be a bare status code, and there was no way to ask
    /// for a window narrower than "the newest N".
    #[tokio::test]
    async fn the_list_carries_the_failure_reason_and_honours_a_time_window() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("window.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut ok = sample("internal-ok", "req-ok", now);
        ok.api_key_id = Some("key-1".into());
        ok.status_code = Some(200);
        ok.error_kind = None;
        let mut failed = sample("internal-failed", "req-failed", now + 600);
        failed.api_key_id = Some("key-2".into());
        failed.status_code = Some(429);
        failed.error_kind = Some("upstream_failure".into());
        store.record(ok);
        store.record(failed);
        store.flush().await;

        let all = store.list(RequestFilter::default()).await.unwrap();
        assert_eq!(all.len(), 2);
        let newest = &all[0];
        assert_eq!(newest.request_id, "req-failed");
        assert_eq!(
            newest.error_kind.as_deref(),
            Some("upstream_failure"),
            "the list must say why a request failed, not only that it did"
        );
        assert!(all[1].error_kind.is_none());

        let by_key = RequestFilter {
            api_key_id: Some("key-nobody-used".into()),
            ..Default::default()
        };
        assert!(
            store.list(by_key.clone()).await.unwrap().is_empty(),
            "filtering by a key that made no requests must return nothing"
        );
        assert_eq!(store.count(by_key).await.unwrap(), 0);
        let by_own_key = RequestFilter {
            api_key_id: Some("key-1".into()),
            ..Default::default()
        };
        assert_eq!(
            store.list(by_own_key.clone()).await.unwrap().len(),
            1,
            "the key filter must select the rows that key made"
        );
        assert_eq!(store.count(by_own_key).await.unwrap(), 1);

        let window = RequestFilter {
            from: Some(now + 300),
            ..Default::default()
        };
        let narrowed = store.list(window.clone()).await.unwrap();
        assert_eq!(narrowed.len(), 1);
        assert_eq!(narrowed[0].request_id, "req-failed");
        assert_eq!(
            store.count(window).await.unwrap(),
            1,
            "the count must describe the same window as the list"
        );
    }

    fn sample(id: &str, request_id: &str, started_at: i64) -> RequestEvent {
        RequestEvent {
            id: id.into(),
            project_id: "test-project".into(),
            request_id: request_id.into(),
            trace_id: "trace-1".into(),
            source_ip: None,
            started_at,
            finished_at: started_at,
            endpoint: "/v1/chat/completions".into(),
            api_key_id: None,
            provider: Some("OpenAI".into()),
            requested_model: Some("fast".into()),
            resolved_model: Some("gpt-test".into()),
            status_code: Some(200),
            error_kind: None,
            latency_ms: 42,
            ttft_ms: None,
            input_tokens: 10,
            output_tokens: 4,
            cached_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            stream: None,
            cost_micros: 3,
            payload_captured: false,
            request_json: None,
            response_json: None,
        }
    }

    /// Paging must visit every row exactly once. Without a deterministic
    /// tiebreaker, rows that share `started_at` can be returned in a different
    /// order per query, which duplicates some rows across pages and makes others
    /// unreachable — 8 of 169 rows were unreachable in a browser run.
    #[tokio::test]
    async fn paging_visits_every_row_exactly_once_when_timestamps_tie() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("paging.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        for index in 0..25 {
            let mut event = sample(&format!("internal-{index}"), &format!("req-{index}"), now);
            event.started_at = now; // every row ties
            event.finished_at = now;
            store.record(event);
        }
        store.flush().await;

        let mut seen: Vec<String> = Vec::new();
        for page in 0..3 {
            let rows = store
                .list(RequestFilter {
                    limit: Some(10),
                    offset: Some(page * 10),
                    ..Default::default()
                })
                .await
                .unwrap();
            assert!(rows.len() <= 10, "a page never exceeds its limit");
            seen.extend(rows.into_iter().map(|row| row.request_id));
        }
        let mut unique = seen.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            seen.len(),
            "no row may appear on two pages: {seen:?}"
        );
        assert_eq!(unique.len(), 25, "every row must be reachable");
    }

    /// One `x-request-id` is caller-controlled, so the same external id can exist in
    /// two projects. Each project must resolve its own row: the read used to take the
    /// globally newest row with that external id and filter by project afterwards.
    #[tokio::test]
    async fn the_same_external_id_resolves_inside_each_project() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("identity.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut alpha = sample("internal-alpha", "shared-id", now);
        alpha.project_id = "project-a".into();
        let mut beta = sample("internal-beta", "shared-id", now + 60);
        beta.project_id = "project-b".into();
        store.record(alpha);
        store.record(beta);
        store.flush().await;

        let resolved = store
            .get_for("project-a".into(), "shared-id".into())
            .await
            .unwrap()
            .expect("project A must resolve its own row, not another project's");
        assert_eq!(resolved.id, "internal-alpha");
        assert_eq!(resolved.project_id, "project-a");
        let resolved = store
            .get_for("project-b".into(), "shared-id".into())
            .await
            .unwrap()
            .expect("project B must resolve its own row");
        assert_eq!(resolved.id, "internal-beta");
        assert_eq!(resolved.project_id, "project-b");
        // An internal id is unique, so another project's row is simply not this
        // project's request: the answer is "no such request here", not a foreign row.
        assert!(
            store
                .get_for("project-a".into(), "internal-beta".into())
                .await
                .unwrap()
                .is_none(),
            "a foreign internal id must not resolve inside this project"
        );
    }

    /// The reported failure mode: project B's newer row sharing the external id made
    /// project A's own detail read return nothing, because the lookup took the newest
    /// global match first and only then filtered by project.
    #[tokio::test]
    async fn a_newer_foreign_row_never_hides_the_local_row() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("foreign.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut local = sample("internal-local", "shared-id", now);
        local.project_id = "project-a".into();
        let mut foreign = sample("internal-foreign", "shared-id", now + 86_400);
        foreign.project_id = "project-b".into();
        store.record(local);
        store.record(foreign);
        store.flush().await;

        let resolved = store
            .get_for("project-a".into(), "shared-id".into())
            .await
            .unwrap()
            .expect("a newer foreign row must not turn the local detail into a 404");
        assert_eq!(resolved.id, "internal-local");
    }

    /// A caller can send an `x-request-id` equal to another request's internal UUID.
    /// The exact internal match is unambiguous and must win over any external match,
    /// even when the external match is newer.
    #[tokio::test]
    async fn an_exact_internal_id_beats_another_rows_external_id() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("collision.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut exact = sample("internal-exact", "req-exact", now);
        exact.project_id = "project-a".into();
        let mut impostor = sample("internal-impostor", "internal-exact", now + 3_600);
        impostor.project_id = "project-a".into();
        store.record(exact);
        store.record(impostor);
        store.flush().await;

        let resolved = store
            .get_for("project-a".into(), "internal-exact".into())
            .await
            .unwrap()
            .expect("the internal id must resolve");
        assert_eq!(
            resolved.id, "internal-exact",
            "an exact internal id must not be shadowed by a newer external id"
        );
        assert_eq!(resolved.request_id, "req-exact");
        // The other row is untouched and still reachable by its own identity.
        let other = store
            .get_for("project-a".into(), "internal-impostor".into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(other.request_id, "internal-exact");
    }

    /// Two requests can carry the same external id inside one project — a retrying
    /// client, or a client that sends a constant id. The external lookup has to stay
    /// deterministic, and neither row may become unopenable from the list.
    #[tokio::test]
    async fn repeated_external_ids_stay_deterministic_and_listable() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("repeated.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        for (id, started_at) in [
            ("internal-1", now),
            ("internal-2", now + 60),
            ("internal-3", now + 60),
        ] {
            let mut event = sample(id, "repeat", started_at);
            event.project_id = "project-a".into();
            store.record(event);
        }
        store.flush().await;

        let newest = store
            .get_for("project-a".into(), "repeat".into())
            .await
            .unwrap()
            .expect("the external id must resolve inside its project");
        assert_eq!(
            newest.id, "internal-3",
            "the newest external match wins, with id DESC breaking the timestamp tie"
        );
        for _ in 0..3 {
            assert_eq!(
                store
                    .get_for("project-a".into(), "repeat".into())
                    .await
                    .unwrap()
                    .unwrap()
                    .id,
                "internal-3",
                "the same question must get the same answer"
            );
        }

        let rows = store
            .list(RequestFilter {
                project_id: Some("project-a".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(rows.len(), 3, "every row stays listable");
        let mut internal: Vec<String> = rows.iter().map(|row| row.internal_id.clone()).collect();
        internal.sort();
        internal.dedup();
        assert_eq!(
            internal,
            vec!["internal-1", "internal-2", "internal-3"],
            "the list must expose the internal id of every row, not only the external one"
        );
        for row in &rows {
            assert_eq!(row.request_id, "repeat");
            assert_eq!(
                store
                    .get_for("project-a".into(), row.internal_id.clone())
                    .await
                    .unwrap()
                    .expect("every listed row must be openable by its own internal id")
                    .id,
                row.internal_id
            );
        }
    }

    /// The read path used to open the database file itself, which checkpoints and
    /// deletes the write-ahead log the writer thread still holds open. Measured on
    /// four instances, the first read froze the projection permanently: later
    /// writes landed in an unlinked inode, nothing was reported dropped, and the
    /// store kept reporting itself available.
    #[tokio::test]
    async fn reading_the_projection_does_not_freeze_the_writer() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("freeze.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        store.record(sample("internal-before", "req-before", now));
        store.flush().await;
        assert_eq!(store.list(RequestFilter::default()).await.unwrap().len(), 1);

        // The first read has now happened. Everything written afterwards must
        // still be visible to the next reader.
        for index in 0..5 {
            store.record(sample(
                &format!("internal-after-{index}"),
                &format!("req-after-{index}"),
                now + 1 + index,
            ));
        }
        store.flush().await;
        let rows = store.list(RequestFilter::default()).await.unwrap();
        assert_eq!(
            rows.len(),
            6,
            "a read must not stop later writes from reaching the projection"
        );
        assert_eq!(
            store
                .summary(SummaryFilter::default())
                .await
                .unwrap()
                .requests,
            6,
            "the summary must see them too"
        );
        assert_eq!(store.dropped_events(), 0);
    }

    /// A row is an error when the record system classified it or when a measured
    /// status is `>= 400`. A status nobody observed is neither a failure nor a
    /// success, so counting it as one would be an invention.
    #[tokio::test]
    async fn the_summary_counts_errors_by_classification_or_measured_status() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("errors.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut unmeasured = sample("internal-unmeasured", "req-unmeasured", now);
        unmeasured.status_code = None;
        unmeasured.error_kind = None;
        let mut classified = sample("internal-classified", "req-classified", now + 1);
        classified.status_code = None;
        classified.error_kind = Some("usage_unavailable".into());
        let mut measured = sample("internal-measured", "req-measured", now + 2);
        measured.status_code = Some(500);
        measured.error_kind = None;
        let mut created = sample("internal-created", "req-created", now + 3);
        created.status_code = Some(201);
        created.error_kind = None;
        for event in [unmeasured, classified, measured, created] {
            store.record(event);
        }
        store.flush().await;

        let summary = store.summary(SummaryFilter::default()).await.unwrap();
        assert_eq!(summary.requests, 4);
        assert_eq!(
            summary.errors, 2,
            "the classified row and the 500 are errors; a NULL status is not"
        );
        assert_eq!(
            summary.series.iter().map(|point| point.errors).sum::<i64>(),
            2,
            "the trend must count the same failures as the summary"
        );
    }

    /// The console reads this payload, not the struct: a trend that returns only
    /// requests, errors and latency cannot draw a token or a cost series at all.
    #[tokio::test]
    async fn the_summary_payload_carries_the_token_and_cost_series() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("series.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        store.record(sample("internal-1", "req-1", now));
        store.flush().await;

        let payload = serde_json::to_value(
            store
                .summary_for("test-project".into(), SummaryFilter::default())
                .await
                .unwrap(),
        )
        .unwrap();
        let point = &payload["series"][0];
        for key in ["input_tokens", "output_tokens", "cost_micros"] {
            assert!(
                point.get(key).is_some(),
                "the trend has to return {key}, or no token or cost series can be drawn: {point}"
            );
        }
        assert_eq!(point["input_tokens"], 10);
        assert_eq!(point["output_tokens"], 4);
        assert_eq!(point["cost_micros"], 3);
    }

    /// A window whose every settlement was unmeasured has no token or cost total.
    /// Reporting `0` there is the gateway's normalized rejection read as a
    /// measurement, and it is exactly the zero a KPI card must not print.
    #[tokio::test]
    async fn an_unmeasured_window_reports_no_token_or_cost_total() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("unmeasured.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut lost = sample("internal-lost", "req-lost", now);
        lost.status_code = Some(502);
        lost.error_kind = Some("usage_unavailable".into());
        lost.input_tokens = 0;
        lost.output_tokens = 0;
        lost.cost_micros = 0;
        store.record(lost);
        store.flush().await;

        let payload =
            serde_json::to_value(store.summary(SummaryFilter::default()).await.unwrap()).unwrap();
        assert_eq!(payload["requests"], 1);
        assert_eq!(payload["errors"], 1, "the failure itself is still counted");
        for key in ["input_tokens", "output_tokens", "cost_micros"] {
            assert!(
                payload[key].is_null(),
                "a window with no measured usage must report {key} as unmeasured, not zero: {payload}"
            );
        }
        let point = &payload["series"][0];
        for key in ["input_tokens", "output_tokens", "cost_micros"] {
            assert!(
                point[key].is_null(),
                "the unmeasured bucket must stay unmeasured in the trend too: {point}"
            );
        }
    }

    /// The bucket width is a function of the window's span, not of how much
    /// traffic it holds: a day is read hourly, a fortnight six-hourly, and all
    /// retained history daily. The span boundaries are the projection's contract
    /// with the console, so they are pinned without a clock.
    #[test]
    fn the_bucket_width_follows_the_span_of_the_window() {
        let day = SummaryWindow::resolve(
            SummaryFilter {
                from: Some(0),
                until: Some(86_400),
            },
            0,
        );
        assert_eq!(day.bucket_seconds, 3_600);
        assert_eq!(day.from, 0);
        assert_eq!(day.until, Some(86_400));

        for (span, expected) in [
            (0, 3_600),
            (48 * 3_600, 3_600),
            (48 * 3_600 + 1, 6 * 3_600),
            (14 * 86_400, 6 * 3_600),
            (14 * 86_400 + 1, 86_400),
        ] {
            let window = SummaryWindow::resolve(
                SummaryFilter {
                    from: Some(0),
                    until: Some(span),
                },
                0,
            );
            assert_eq!(
                window.bucket_seconds, expected,
                "a {span}s window must bucket at {expected}s"
            );
        }

        // The default window, and a caller who names no end: 24 hours from the
        // read's own clock, hourly, with no upper bound.
        let default = SummaryWindow::resolve(SummaryFilter::default(), 1_000_000);
        assert_eq!(default.from, 1_000_000 - 86_400);
        assert_eq!(default.until, None);
        assert_eq!(default.bucket_seconds, 3_600);
        // An end without a start is the same 24 hours ending there.
        let ended = SummaryWindow::resolve(
            SummaryFilter {
                from: None,
                until: Some(500_000),
            },
            1_000_000,
        );
        assert_eq!(ended.from, 500_000 - 86_400);
        assert_eq!(ended.until, Some(500_000));
        // All retained history is the widest span there is, so it reads daily.
        let retained = SummaryWindow::resolve(
            SummaryFilter {
                from: Some(0),
                until: Some(30 * 86_400),
            },
            0,
        );
        assert_eq!(retained.bucket_seconds, 86_400);
    }

    /// A window that ends before it starts describes nothing. A missing end is
    /// not inverted — the clock fills it — and an empty window (`from == until`)
    /// is a real, if empty, question.
    #[test]
    fn an_inverted_window_is_refused_while_a_missing_end_is_not() {
        assert!(
            !SummaryFilter {
                from: Some(10),
                until: Some(9)
            }
            .is_ordered()
        );
        for filter in [
            SummaryFilter {
                from: Some(10),
                until: Some(10),
            },
            SummaryFilter {
                from: Some(10),
                until: None,
            },
            SummaryFilter {
                from: None,
                until: Some(9),
            },
            SummaryFilter::default(),
        ] {
            assert!(
                filter.is_ordered(),
                "{filter:?} is a question, not a mistake"
            );
        }
    }

    /// The trend of a window is read with the same bounds as its totals, and the
    /// console's own truth class decides which buckets carry a measurement: a
    /// successful measured zero is a real zero, a partial measured failure keeps
    /// its numbers, and a settlement the gateway could not measure is a gap.
    #[tokio::test]
    async fn the_trend_carries_measured_buckets_and_leaves_the_rest_as_gaps() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("truth.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let hour = now / 3_600 * 3_600;
        // Measured hour: a successful zero and a partial failure that reported
        // usage before it broke.
        let mut zero = sample("internal-zero", "req-zero", hour);
        zero.input_tokens = 0;
        zero.output_tokens = 0;
        zero.cost_micros = 0;
        let mut partial = sample("internal-partial", "req-partial", hour + 60);
        partial.status_code = Some(502);
        partial.error_kind = Some("upstream_failure".into());
        partial.input_tokens = 5;
        partial.output_tokens = 2;
        partial.cost_micros = 7;
        // Unmeasured hour: the terminal usage report was lost, and a failure that
        // settled as no tokens and no cost is the same fact from the other side.
        let mut lost = sample("internal-lost", "req-lost", hour + 3_600);
        lost.status_code = Some(502);
        lost.error_kind = Some("usage_unavailable".into());
        lost.input_tokens = 0;
        lost.output_tokens = 0;
        lost.cost_micros = 0;
        let mut normalized = sample("internal-normalized", "req-normalized", hour + 3_660);
        normalized.status_code = Some(502);
        normalized.error_kind = Some("upstream_failure".into());
        normalized.input_tokens = 0;
        normalized.output_tokens = 0;
        normalized.cost_micros = 0;
        for event in [zero, partial, lost, normalized] {
            store.record(event);
        }
        store.flush().await;

        let summary = store
            .summary(SummaryFilter {
                from: Some(hour),
                until: Some(hour + 7_200),
            })
            .await
            .unwrap();
        assert_eq!(summary.requests, 4, "every event is in the window");
        assert_eq!(summary.errors, 3);
        assert_eq!(
            summary.input_tokens,
            Some(5),
            "the measured zero and the partial failure are measured; the lost and \
             normalized failures are not"
        );
        assert_eq!(summary.output_tokens, Some(2));
        assert_eq!(summary.cost_micros, Some(7));

        assert_eq!(summary.series.len(), 2, "one bucket per hour");
        let measured = &summary.series[0];
        assert_eq!(measured.bucket, hour);
        assert_eq!(measured.requests, 2);
        assert_eq!(measured.input_tokens, Some(5));
        assert_eq!(measured.output_tokens, Some(2));
        assert_eq!(measured.cost_micros, Some(7));
        let unmeasured = &summary.series[1];
        assert_eq!(unmeasured.bucket, hour + 3_600);
        assert_eq!(unmeasured.requests, 2);
        for (key, value) in [
            ("input_tokens", unmeasured.input_tokens),
            ("output_tokens", unmeasured.output_tokens),
            ("cost_micros", unmeasured.cost_micros),
        ] {
            assert!(
                value.is_none(),
                "an unmeasured bucket is a gap in {key}, not a zero"
            );
        }

        // The measured hour on its own is a real measurement of zero: a window
        // whose only settlement succeeded with no tokens reports Some(0).
        let only_zero = store
            .summary(SummaryFilter {
                from: Some(hour),
                until: Some(hour + 30),
            })
            .await
            .unwrap();
        assert_eq!(only_zero.requests, 1);
        assert_eq!(only_zero.input_tokens, Some(0));
        assert_eq!(only_zero.output_tokens, Some(0));
        assert_eq!(only_zero.cost_micros, Some(0));
    }

    /// The bucket the query returns is the window's own width, not the hourly one
    /// the projection started with: three events two hours apart fall into three
    /// hour-aligned buckets of a day, into six-hour buckets of a fortnight, and
    /// into day-aligned buckets of all retained history — with every event still
    /// counted exactly once.
    #[tokio::test]
    async fn the_returned_buckets_are_aligned_to_the_window_width() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("aligned.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let hour = now / 3_600 * 3_600;
        for index in 0..3 {
            store.record(sample(
                &format!("internal-{index}"),
                &format!("req-{index}"),
                hour + index * 3_600,
            ));
        }
        store.flush().await;

        let day = store
            .summary(SummaryFilter {
                from: Some(hour),
                until: Some(hour + 3 * 3_600),
            })
            .await
            .unwrap();
        assert_eq!(
            day.series
                .iter()
                .map(|point| point.bucket)
                .collect::<Vec<_>>(),
            vec![hour, hour + 3_600, hour + 2 * 3_600],
            "a day is read in hour-aligned buckets"
        );

        let six_hours = 6 * 3_600;
        let fortnight = store
            .summary(SummaryFilter {
                from: Some(hour - 6 * 86_400),
                until: Some(hour + 3 * 3_600),
            })
            .await
            .unwrap();
        let buckets: Vec<i64> = fortnight.series.iter().map(|point| point.bucket).collect();
        assert!(
            buckets.iter().all(|bucket| bucket % six_hours == 0),
            "a fortnight is read in six-hour buckets: {buckets:?}"
        );
        assert_eq!(
            fortnight
                .series
                .iter()
                .map(|point| point.requests)
                .sum::<i64>(),
            3,
            "a wider bucket must not lose an event"
        );

        let retained = store
            .summary(SummaryFilter {
                from: Some(0),
                until: Some(hour + 3 * 3_600),
            })
            .await
            .unwrap();
        let buckets: Vec<i64> = retained.series.iter().map(|point| point.bucket).collect();
        assert!(
            buckets.iter().all(|bucket| bucket % 86_400 == 0),
            "all retained history is read in day-aligned buckets: {buckets:?}"
        );
        assert_eq!(
            retained
                .series
                .iter()
                .map(|point| point.requests)
                .sum::<i64>(),
            3
        );
        assert_eq!(retained.requests, 3);
    }

    /// A status filter selects measured values only: an unmeasured row cannot be
    /// matched by a code it never recorded, and no zero bucket is invented for it.
    #[tokio::test]
    async fn an_unmeasured_status_is_not_a_zero_bucket() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("facet.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut unmeasured = sample("internal-unmeasured", "req-unmeasured", now);
        unmeasured.status_code = None;
        let mut measured = sample("internal-measured", "req-measured", now + 1);
        measured.status_code = Some(429);
        store.record(unmeasured);
        store.record(measured);
        store.flush().await;

        let rows = store
            .list(RequestFilter {
                status_code: Some(429),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status_code, Some(429));
        let zero = store
            .list(RequestFilter {
                status_code: Some(0),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(zero.is_empty(), "0 was never an observed status");
        let all = store.list(RequestFilter::default()).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(
            all.iter().filter(|row| row.status_code.is_none()).count(),
            1,
            "the unmeasured row stays listable and stays unmeasured"
        );
    }

    /// The console reads this payload, not the struct. Every fact the list is
    /// supposed to show has to be a key on the row: a key that is absent is a fact
    /// the operator can never see, and an unmeasured one has to be `null` rather
    /// than missing, so `—` is distinguishable from "not sent".
    #[tokio::test]
    async fn the_list_payload_exposes_the_telemetry_the_projection_records() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("payload.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        store.record(sample("internal-1", "req-1", now));
        store.flush().await;

        let rows = store.list(RequestFilter::default()).await.unwrap();
        assert_eq!(rows.len(), 1);
        let payload = serde_json::to_value(&rows[0]).unwrap();
        let missing = [
            "ttft_ms",
            "cached_tokens",
            "cache_write_tokens",
            "reasoning_tokens",
            "stream",
        ]
        .into_iter()
        .filter(|key| payload.get(key).is_none())
        .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "the list row must return the recorded telemetry; missing {missing:?}"
        );
    }

    /// The list's shape grew by the token facts, TTFT and the stream decision. The
    /// stream fact has to be able to say "this request's stream choice was never
    /// recorded", which a NOT NULL column cannot; the token facts follow the
    /// existing token columns and stay NOT NULL.
    #[test]
    fn the_list_shape_carries_the_token_facts_and_a_nullable_stream() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shape.duckdb");
        assert!(
            initialize(&path).unwrap(),
            "a fresh file is stamped with the current shape"
        );
        assert!(
            !initialize(&path).unwrap(),
            "the shape is stamped exactly once"
        );

        let connection = Connection::open(&path).unwrap();
        let mut statement = connection
            .prepare("SELECT column_name,is_nullable FROM information_schema.columns WHERE table_name='request_events'")
            .unwrap();
        let columns = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<duckdb::Result<Vec<(String, String)>>>()
            .unwrap();
        let column = |name: &str| {
            columns
                .iter()
                .find(|(column, _)| column == name)
                .map(|(_, nullable)| nullable.clone())
        };
        assert_eq!(
            column("stream").as_deref(),
            Some("YES"),
            "a stream choice that was never recorded has to be storable"
        );
        assert_eq!(
            column("source_ip").as_deref(),
            Some("YES"),
            "a request without a recorded direct peer stays nullable"
        );
        for name in ["cache_write_tokens", "reasoning_tokens"] {
            assert!(
                column(name).is_some(),
                "the projection must store {name} to return it"
            );
        }
    }

    /// The projection shape changed: an older file is dropped and re-derived once,
    /// and the new shape has to be able to say "no status was observed" instead of
    /// forcing a NOT NULL value.
    #[test]
    fn the_observation_schema_upgrades_once_and_allows_a_null_status() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("upgrade.duckdb");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE observation_schema(version INTEGER PRIMARY KEY, applied_at BIGINT NOT NULL);
                     INSERT INTO observation_schema(version,applied_at) VALUES(2,0);
                     CREATE TABLE request_events(id VARCHAR PRIMARY KEY, status_code INTEGER NOT NULL);",
                )
                .unwrap();
        }
        assert!(
            initialize(&path).unwrap(),
            "a projection from the previous shape must be re-derived"
        );
        assert!(
            !initialize(&path).unwrap(),
            "the new schema version is stamped once"
        );
        assert!(
            !initialize(&path).unwrap(),
            "opening the current schema again asks for nothing"
        );
        let connection = Connection::open(&path).unwrap();
        let nullable: String = connection
            .query_row(
                "SELECT is_nullable FROM information_schema.columns WHERE table_name='request_events' AND column_name='status_code'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(nullable, "YES", "an unmeasured status has to be storable");
        let versions: i64 = connection
            .query_row("SELECT COUNT(*) FROM observation_schema", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(versions, 1);
    }

    #[tokio::test]
    async fn degraded_store_drops_without_blocking_gateway_work() {
        let store = ObservationStore::degraded("/unavailable", "permission denied");
        store.record(RequestEvent {
            id: "internal-degraded".into(),
            project_id: "test-project".into(),
            request_id: "req-degraded".into(),
            trace_id: "trace-degraded".into(),
            source_ip: None,
            started_at: 0,
            finished_at: 0,
            endpoint: "/v1/models".into(),
            api_key_id: None,
            provider: None,
            requested_model: None,
            resolved_model: None,
            status_code: None,
            error_kind: None,
            latency_ms: 0,
            ttft_ms: None,
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            stream: None,
            cost_micros: 0,
            payload_captured: false,
            request_json: None,
            response_json: None,
        });
        assert!(!store.is_available());
        assert_eq!(store.dropped_events(), 1);
        assert!(store.summary(SummaryFilter::default()).await.is_err());
    }

    /// A pinned request is pinned in the derived projection too. `RequestEvent.id`
    /// is Pangolin's own request uuid — the same value `requests.id` holds — so the
    /// record system can name the requests a retained trace owns and the projection
    /// can spare them by identity. Without it the trace detail survived retention
    /// while the request log and the summary quietly stopped showing the run the
    /// operator pinned.
    #[tokio::test]
    async fn derived_retention_spares_a_pinned_request_from_the_delete_and_payload_rules() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObservationStore::open(directory.path().join("pinned.duckdb"), 30).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let aged = now - 10 * 86_400;
        let captured = |id: &str, request_id: &str| {
            let mut event = sample(id, request_id, aged);
            event.payload_captured = true;
            event.request_json = Some(r#"{"messages":[{"role":"user","content":"hi"}]}"#.into());
            event.response_json = Some(r#"{"choices":[]}"#.into());
            event
        };
        store.record(captured("internal-pinned", "req-pinned"));
        store.record(captured("internal-ordinary", "req-ordinary"));
        store.flush().await;
        assert_eq!(store.list(RequestFilter::default()).await.unwrap().len(), 2);

        let preserve = Arc::new(HashSet::from(["internal-pinned".to_string()]));
        let rules = vec![
            Retention::new(Some("test-project".into()), 1, false).preserving(preserve.clone()),
            Retention::new(Some("test-project".into()), 1, true).preserving(preserve.clone()),
        ];
        assert!(store.apply_retention(rules).await);

        let rows = store.list(RequestFilter::default()).await.unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row.internal_id.as_str())
                .collect::<Vec<_>>(),
            vec!["internal-pinned"],
            "the window expired the ordinary row and kept the pinned one"
        );
        let pinned = store.get("internal-pinned".into()).await.unwrap().unwrap();
        assert!(
            pinned.request_json.is_some() && pinned.response_json.is_some(),
            "a payload rule must not strip a pinned request's stored body"
        );
        assert!(pinned.payload_captured);

        // A late event for the pinned request is admitted, not dropped: the rule set
        // the writer holds is what decides admission, so the pin has to reach it.
        store.record(captured("internal-pinned", "req-pinned"));
        store.record(captured("internal-ordinary", "req-ordinary"));
        store.flush().await;
        assert_eq!(
            store.list(RequestFilter::default()).await.unwrap().len(),
            1,
            "an ordinary aged event is still expired at admission, a pinned one is not"
        );
        let late = store.get("internal-pinned".into()).await.unwrap().unwrap();
        assert!(
            late.request_json.is_some(),
            "the payload rule must not strip a late pinned event either"
        );
    }

    /// A global rule is not a different promise: a pinned request survives the
    /// instance-wide observation window and a project-scoped rule alike, and the
    /// same row expires the moment the pin is released.
    #[tokio::test]
    async fn derived_retention_is_the_same_promise_for_a_global_rule_and_a_released_pin() {
        let directory = tempfile::tempdir().unwrap();
        // A wide instance window, so the aged row is admitted and the rule under
        // test is the one that decides its fate.
        let store = ObservationStore::open(directory.path().join("global.duckdb"), 3650).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let aged = now - 400 * 86_400;
        store.record(sample("internal-pinned", "req-pinned", aged));
        store.flush().await;
        assert_eq!(store.list(RequestFilter::default()).await.unwrap().len(), 1);

        let preserve = Arc::new(HashSet::from(["internal-pinned".to_string()]));
        assert!(
            store
                .apply_retention(vec![
                    Retention::new(None, 30, false).preserving(preserve.clone())
                ])
                .await
        );
        assert_eq!(
            store.list(RequestFilter::default()).await.unwrap().len(),
            1,
            "the instance-wide window must spare a pinned request"
        );

        // Releasing the pin returns the request to the ordinary behaviour.
        assert!(
            store
                .apply_retention(vec![Retention::new(None, 30, false)])
                .await
        );
        assert_eq!(store.list(RequestFilter::default()).await.unwrap().len(), 0);
    }

    /// Opening the projection must not expire a row on its own. The pass that knows
    /// which requests the record system pinned is the one that owns expiry —
    /// otherwise a restart silently deleted a pinned run's events before that pass
    /// ever ran, and the request log disagreed with the trace detail.
    #[test]
    fn opening_the_projection_leaves_expiry_to_the_pass_that_knows_what_is_pinned() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("no-pretrim.duckdb");
        initialize(&path).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let connection = Connection::open(&path).unwrap();
        insert(
            &connection,
            &sample("internal-old", "req-old", now - 400 * 86_400),
        )
        .unwrap();
        drop(connection);

        initialize(&path).unwrap();
        let connection = Connection::open(&path).unwrap();
        let rows: i64 = connection
            .query_row("SELECT count(*) FROM request_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            rows, 1,
            "opening the projection must not expire an aged row before the retention pass"
        );
    }

    /// `sync_retention` puts the same pinned set on every rule of one pass — one
    /// `Arc` per rule, one shared set — so the union has to be built once.
    /// Flattening the rules first multiplied the pin list by the rule count before
    /// de-duplicating it: memory that scales with the number of project policies,
    /// for a set that is identical in all of them, which is what can exhaust the
    /// pass. The union is asserted directly, and then as the set the statement
    /// actually reads.
    #[test]
    fn the_pinned_ids_are_unioned_once_however_many_rules_carry_them() {
        let shared = Arc::new(HashSet::from([
            "pinned-a".to_string(),
            "pinned-b".to_string(),
        ]));
        let other = Arc::new(HashSet::from([
            "pinned-b".to_string(),
            "pinned-c".to_string(),
        ]));
        let rules = vec![
            Retention::new(None, 1, false).preserving(shared.clone()),
            Retention::new(Some("test-project".into()), 1, true).preserving(shared.clone()),
            Retention::new(None, 1, false).preserving(other.clone()),
        ];

        // One entry per unique pinned request, whatever carried it.
        assert_eq!(pinned_ids(&rules), vec!["pinned-a", "pinned-b", "pinned-c"]);

        // …and the same union is what the statement reads: the temp table holds one
        // row per unique id, every pinned row survives both the delete rule and the
        // payload rule, and the unpinned row is still expired.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("union.duckdb");
        initialize(&path).unwrap();
        let connection = Connection::open(&path).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let aged = now - 10 * 86_400;
        for id in ["pinned-a", "pinned-b", "pinned-c", "ordinary"] {
            let mut event = sample(id, id, aged);
            event.payload_captured = true;
            event.request_json = Some(r#"{"messages":[{"role":"user","content":"hi"}]}"#.into());
            insert(&connection, &event).unwrap();
        }

        retain(&connection, &rules).unwrap();

        let ids = |query: &str| -> Vec<String> {
            let mut statement = connection.prepare(query).unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(|row| row.unwrap())
                .collect()
        };
        assert_eq!(
            ids("SELECT id FROM retained_requests ORDER BY id"),
            vec!["pinned-a", "pinned-b", "pinned-c"],
            "the temp table carries the union once, not one copy per rule"
        );
        assert_eq!(
            ids("SELECT id FROM request_events ORDER BY id"),
            vec!["pinned-a", "pinned-b", "pinned-c"],
            "the union is what spares a row; the unpinned row is still expired"
        );
        let body: Option<String> = connection
            .query_row(
                "SELECT request_json FROM request_events WHERE id='pinned-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            body.is_some(),
            "a payload rule must not strip a body the union pins"
        );
    }
}
