//! Whole-instance disaster recovery. SQLite's online backup API supplies snapshot
//! consistency and atomic replacement; no hand-written identity graph migration.
use crate::{
    access,
    api::{ApiError, AppState},
    crypto::SecretBox,
    db,
};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use rand::RngCore;
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
    params,
};
use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use zeroize::Zeroizing;

const MAX_DATABASE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ARTIFACT_BYTES: usize = 128 * 1024 * 1024;
const TRANSIENT: &[&str] = &[
    "sessions",
    "oidc_auth_states",
    "backup_job_targets",
    "operation_jobs",
    "backup_artifacts",
    "backup_runs",
    "backup_restores",
    "webhook_deliveries",
];
const HISTORY: &[&str] = &[
    "request_contents",
    "usage_cost_items",
    "usage_logs",
    "request_executions",
    "requests",
    "traces",
    "threads",
    "execution_facts",
    "request_facts",
    "response_sessions",
    "session_summaries",
    "channel_probes",
];

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    #[serde(default)]
    pub include_history: bool,
    pub passphrase: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub version: u32,
    pub id: String,
    pub created_at: i64,
    pub schema_digest: String,
    pub database_digest: String,
    pub include_history: bool,
    pub key_mode: String,
    pub salt: Option<String>,
    pub envelope: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    format: String,
    id: String,
    schema_digest: String,
    database_digest: String,
    include_history: bool,
    database: String,
    source_key_envelope: Option<String>,
}
#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Fail,
    Replace,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Restore {
    pub artifact: Artifact,
    pub passphrase: Option<String>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub force: bool,
}
#[derive(Clone, Serialize)]
pub struct Preflight {
    pub artifact_id: String,
    pub schema_version: i64,
    pub tables: BTreeMap<String, i64>,
    pub destination_has_data: bool,
    pub portable: bool,
    pub active_sessions_restored: bool,
    pub derived_available: bool,
}
struct Staged {
    file: NamedTempFile,
    preflight: Preflight,
}
fn invalid(message: &str) -> ApiError {
    ApiError::BadRequest(message.into())
}
fn internal(error: impl Into<anyhow::Error>) -> ApiError {
    ApiError::Internal(error.into())
}
fn database_file(state: &AppState) -> Result<PathBuf, ApiError> {
    let options = state.db.get_sqlite_connection_pool().connect_options();
    let path = options.get_filename();
    if path == Path::new(":memory:") || !path.is_file() {
        return Err(invalid(
            "full-instance backup/restore requires an on-disk SQLite database",
        ));
    }
    std::fs::canonicalize(path).map_err(internal)
}
fn schema(connection: &Connection) -> anyhow::Result<String> {
    let mut statement=connection.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL ORDER BY type,name")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(blake3::hash(&serde_json::to_vec(&rows)?)
        .to_hex()
        .to_string())
}
fn copy_database(source: &Connection, destination: &mut Connection) -> anyhow::Result<()> {
    let started = Instant::now();
    let backup = Backup::new(source, destination)?;
    loop {
        if started.elapsed() > Duration::from_secs(60) {
            anyhow::bail!("SQLite snapshot timed out")
        }
        match backup.step(256)? {
            StepResult::Done => break,
            StepResult::More => (),
            StepResult::Busy | StepResult::Locked => std::thread::sleep(Duration::from_millis(10)),
            _ => anyhow::bail!("unexpected SQLite backup result"),
        }
    }
    Ok(())
}
fn counts(connection: &Connection) -> anyhow::Result<BTreeMap<String, i64>> {
    let names=connection.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?.query_map([],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut result = BTreeMap::new();
    for name in names {
        if !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            anyhow::bail!("invalid database table name")
        };
        let count =
            connection.query_row(&format!("SELECT COUNT(*) FROM \"{name}\""), [], |row| {
                row.get(0)
            })?;
        result.insert(name, count);
    }
    Ok(result)
}
fn has_data(connection: &Connection) -> anyhow::Result<bool> {
    let counts = counts(connection)?;
    Ok(counts.get("users").copied().unwrap_or(0) > 1
        || counts.get("projects").copied().unwrap_or(0) > 1
        || counts.get("roles").copied().unwrap_or(0) > 3
        || counts.iter().any(|(name, count)| {
            *count > 0
                && !matches!(
                    name.as_str(),
                    "users"
                        | "projects"
                        | "roles"
                        | "permissions"
                        | "role_permissions"
                        | "project_memberships"
                        | "user_role_bindings"
                        | "instance_state"
                        | "settings"
                        | "sessions"
                        | "schema_migrations"
                        | "audit_events"
                        | "instance_restore_receipts"
                        | "operation_jobs"
                )
        }))
}
fn validate(
    connection: &Connection,
    expected_schema: &str,
) -> anyhow::Result<BTreeMap<String, i64>> {
    if schema(connection)? != expected_schema {
        anyhow::bail!("backup schema differs from the running Pangolin schema")
    }
    let check: String = connection.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok"
        || connection
            .prepare("PRAGMA foreign_key_check")?
            .query([])?
            .next()?
            .is_some()
    {
        anyhow::bail!("backup fails SQLite integrity or foreign-key validation")
    }
    let owners:i64=connection.query_row("SELECT COUNT(*) FROM users u JOIN user_role_bindings b ON b.user_id=u.id WHERE u.enabled=1 AND b.project_id IS NULL AND b.role_id=?",[db::SYSTEM_OWNER_ROLE_ID],|r|r.get(0))?;
    let defaults: i64 = connection.query_row(
        "SELECT COUNT(*) FROM projects WHERE is_default=1",
        [],
        |r| r.get(0),
    )?;
    if owners < 1 || defaults != 1 {
        anyhow::bail!("backup must contain an active system owner and one default project")
    }
    let invalid_scopes:i64=connection.query_row("SELECT (SELECT COUNT(*) FROM project_memberships m JOIN roles r ON r.id=m.role_id WHERE r.project_id IS NOT NULL AND r.project_id!=m.project_id)+(SELECT COUNT(*) FROM project_invitations i JOIN roles r ON r.id=i.role_id WHERE r.project_id IS NOT NULL AND r.project_id!=i.project_id)+(SELECT COUNT(*) FROM user_role_bindings b JOIN roles r ON r.id=b.role_id WHERE r.project_id IS NOT NULL AND (b.project_id IS NULL OR b.project_id!=r.project_id))+(SELECT COUNT(*) FROM service_group_channels c JOIN service_groups g ON g.id=c.group_id JOIN providers p ON p.id=c.provider_id WHERE g.project_id!=p.project_id)",[],|row|row.get(0))?;
    if invalid_scopes != 0 {
        anyhow::bail!("backup violates project ownership constraints")
    }
    counts(connection)
}
fn strip_transient(connection: &mut Connection, history: bool) -> anyhow::Result<()> {
    connection.execute_batch("PRAGMA journal_mode=DELETE;")?;
    let tx = connection.transaction()?;
    for table in TRANSIENT {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
    }
    if !history {
        // Preserve enforcement when optional history is omitted from a point-in-time
        // snapshot containing in-flight work with an unknown eventual outcome.
        tx.execute("UPDATE api_keys SET spent_micros=spent_micros+COALESCE((SELECT SUM(e.reserved_micros) FROM execution_facts e JOIN request_facts r ON r.id=e.request_id WHERE r.api_key_id=api_keys.id AND e.status='running' AND e.contacted=1),0)",[])?;
        for table in HISTORY {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
    }
    tx.commit()?;
    connection.execute_batch("VACUUM;")?;
    Ok(())
}
fn rekey(
    connection: &mut Connection,
    source: &SecretBox,
    destination: &SecretBox,
) -> anyhow::Result<()> {
    let tables = counts(connection)?;
    let mut changes = vec![];
    for (table, count) in tables {
        if count == 0 {
            continue;
        }
        let columns = connection
            .prepare(&format!("PRAGMA table_info(\"{table}\")"))?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for column in columns
            .into_iter()
            .filter(|c| c.ends_with("_envelope") || c == "envelope")
        {
            let rows = connection
                .prepare(&format!(
                    "SELECT rowid,\"{column}\" FROM \"{table}\" WHERE \"{column}\" IS NOT NULL"
                ))?
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (row, envelope) in rows {
                let plaintext = source.decrypt(&envelope)?;
                changes.push((
                    table.clone(),
                    column.clone(),
                    row,
                    destination.encrypt(&plaintext)?,
                ));
            }
        }
    }
    let triggers=connection.prepare("SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND tbl_name IN ('session_summaries','backup_artifacts')")?.query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let tx = connection.transaction()?;
    for (name, _) in &triggers {
        tx.execute_batch(&format!("DROP TRIGGER \"{name}\";"))?;
    }
    for (table, column, row, envelope) in changes {
        tx.execute(
            &format!("UPDATE \"{table}\" SET \"{column}\"=? WHERE rowid=?"),
            params![envelope, row],
        )?;
    }
    for (_, sql) in triggers {
        tx.execute_batch(&sql)?;
    }
    tx.commit()?;
    Ok(())
}
fn settle_interrupted(connection: &mut Connection) -> anyhow::Result<()> {
    let tx = connection.transaction()?;
    tx.execute("UPDATE api_keys SET spent_micros=spent_micros+COALESCE((SELECT SUM(e.reserved_micros) FROM execution_facts e JOIN request_facts r ON r.id=e.request_id WHERE r.api_key_id=api_keys.id AND e.status='running' AND e.contacted=1 AND NOT EXISTS(SELECT 1 FROM usage_logs u WHERE u.execution_id=e.id)),0)",[])?;
    tx.execute("INSERT INTO usage_logs(id,execution_id,model_id,price_id,total_cost_micros,created_at,settlement_kind) SELECT lower(hex(randomblob(16))),e.id,e.model_id,json_extract(e.price_json,'$.id'),CASE WHEN e.contacted=1 THEN e.reserved_micros ELSE 0 END,unixepoch(),'interrupted' FROM execution_facts e WHERE e.status='running' AND NOT EXISTS(SELECT 1 FROM usage_logs u WHERE u.execution_id=e.id)",[])?;
    tx.execute("INSERT INTO usage_cost_items(id,usage_log_id,quantity,subtotal_micros) SELECT lower(hex(randomblob(16))),u.id,1,u.total_cost_micros FROM usage_logs u JOIN execution_facts e ON e.id=u.execution_id WHERE e.status='running' AND u.total_cost_micros>0 AND NOT EXISTS(SELECT 1 FROM usage_cost_items c WHERE c.usage_log_id=u.id)",[])?;
    for table in ["execution_facts", "request_facts", "requests", "traces"] {
        tx.execute(&format!("UPDATE {table} SET status='interrupted',finished_at=unixepoch() WHERE status='running'"),[])?;
    }
    // The execution row is what the observation projection is rebuilt from, so it
    // records the classification that belongs to the status it just received: an
    // interrupted request is a failure, and a NULL here would report it as neither.
    tx.execute("UPDATE request_executions SET status='interrupted',error_kind='interrupted',finished_at=unixepoch() WHERE status='running'",[])?;
    tx.commit()?;
    Ok(())
}
pub async fn export(
    state: &AppState,
    actor: &access::Principal,
    input: Export,
) -> Result<Artifact, ApiError> {
    access::authorize(&state.db, actor, None, "*")
        .await
        .map_err(|_| ApiError::Forbidden)?;
    let path = database_file(state)?;
    let directory = state.config.data_dir.clone();
    let key = state.secrets.clone();
    let artifact=tokio::task::spawn_blocking(move||->anyhow::Result<Artifact>{
  let source=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)?;source.busy_timeout(Duration::from_secs(5))?;
  let schema_digest=schema(&source)?;source.execute_batch("BEGIN;")?;
  let file=NamedTempFile::new_in(directory)?;let mut snapshot=Connection::open(file.path())?;copy_database(&source,&mut snapshot)?;source.execute_batch("COMMIT;")?;
  snapshot.execute_batch("PRAGMA foreign_keys=ON;")?;strip_transient(&mut snapshot,input.include_history)?;
  validate(&snapshot,&schema_digest)?;
  let metadata=std::fs::metadata(file.path())?;if metadata.len()>MAX_DATABASE_BYTES as u64{anyhow::bail!("instance snapshot exceeds 64 MiB; export without request history")}
  let bytes=std::fs::read(file.path())?;let digest=blake3::hash(&bytes).to_hex().to_string();let id=super::id();
  let (wrapping,salt,source_key_envelope,mode)=if let Some(passphrase)=input.passphrase {let passphrase=Zeroizing::new(passphrase);let mut salt=[0u8;32];rand::rng().fill_bytes(&mut salt);let wrapping=SecretBox::passphrase(&passphrase,&salt)?;let source_key=key.wrap_key(&wrapping)?;(wrapping,Some(STANDARD_NO_PAD.encode(salt)),Some(source_key),"passphrase")}else{(key,None,None,"instance")};
  let payload=Payload{format:"pangolin-instance-v1".into(),id:id.clone(),schema_digest:schema_digest.clone(),database_digest:digest.clone(),include_history:input.include_history,database:STANDARD_NO_PAD.encode(bytes),source_key_envelope};
  let plaintext=Zeroizing::new(serde_json::to_string(&payload)?);
  Ok(Artifact{version:1,id,created_at:db::now(),schema_digest,database_digest:digest,include_history:input.include_history,key_mode:mode.into(),salt,envelope:wrapping.encrypt(&plaintext)?})
 }).await.map_err(internal)?.map_err(|_|invalid("instance backup could not be created; check source integrity, secrets and snapshot size"))?;
    super::audit(&state.db, Some(actor), "", "instance.backup", &artifact.id).await?;
    Ok(artifact)
}
async fn stage(state: &AppState, input: Restore) -> Result<Staged, ApiError> {
    let target = database_file(state)?;
    let directory = state.config.data_dir.clone();
    let destination_key = state.secrets.clone();
    tokio::task::spawn_blocking(move||->anyhow::Result<Staged>{
  let artifact=input.artifact;if artifact.version!=1||artifact.envelope.len()>MAX_ARTIFACT_BYTES{anyhow::bail!("invalid artifact version or size")}
  let (wrapping,portable)=match artifact.key_mode.as_str(){
   "instance" if artifact.salt.is_none()=>(destination_key.clone(),false),
   "passphrase"=>{let salt=STANDARD_NO_PAD.decode(artifact.salt.as_deref().ok_or_else(||anyhow::anyhow!("missing salt"))?)?;let passphrase=Zeroizing::new(input.passphrase.ok_or_else(||anyhow::anyhow!("missing passphrase"))?);(SecretBox::passphrase(&passphrase,&salt)?,true)},
   _=>anyhow::bail!("invalid encryption mode")
  };
  let plaintext=wrapping.decrypt(&artifact.envelope)?;let payload:Payload=serde_json::from_str(&plaintext)?;
  if payload.format!="pangolin-instance-v1"||payload.id!=artifact.id||payload.schema_digest!=artifact.schema_digest||payload.database_digest!=artifact.database_digest||payload.include_history!=artifact.include_history||payload.database.len()>MAX_DATABASE_BYTES*4/3+4{anyhow::bail!("artifact binding mismatch")}
  let bytes=STANDARD_NO_PAD.decode(&payload.database)?;if bytes.len()>MAX_DATABASE_BYTES||!bytes.starts_with(b"SQLite format 3\0")||blake3::hash(&bytes).to_hex().as_str()!=artifact.database_digest{anyhow::bail!("invalid database image")}
  let target=Connection::open_with_flags(target,OpenFlags::SQLITE_OPEN_READ_ONLY)?;let expected=schema(&target)?;
  if expected!=artifact.schema_digest{anyhow::bail!("incompatible schema")}
  let mut file=NamedTempFile::new_in(directory)?;std::io::Write::write_all(&mut file,&bytes)?;file.as_file().sync_all()?;
  let mut staged=Connection::open(file.path())?;staged.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;validate(&staged,&expected)?;
  let source_key=if portable{SecretBox::unwrap_key(&wrapping,payload.source_key_envelope.as_deref().ok_or_else(||anyhow::anyhow!("missing source key envelope"))?)?}else{destination_key.clone()};
  rekey(&mut staged,&source_key,&destination_key)?;
  strip_transient(&mut staged,true)?;
  settle_interrupted(&mut staged)?;
  let tables=validate(&staged,&expected)?;let version=staged.query_row("SELECT MAX(version) FROM schema_migrations",[],|row|row.get(0))?;
  Ok(Staged{file,preflight:Preflight{artifact_id:artifact.id,schema_version:version,tables,destination_has_data:has_data(&target)?,portable,active_sessions_restored:false,derived_available:true}})
 }).await.map_err(internal)?.map_err(|_|invalid("backup preflight failed: verify schema, checksum, identities, secret envelopes and the original master key or export passphrase"))
}
pub async fn preflight(
    state: &AppState,
    actor: &access::Principal,
    input: Restore,
) -> Result<Preflight, ApiError> {
    access::authorize(&state.db, actor, None, "*")
        .await
        .map_err(|_| ApiError::Forbidden)?;
    Ok(stage(state, input).await?.preflight)
}
pub async fn restore(
    state: &AppState,
    actor: &access::Principal,
    input: Restore,
) -> Result<Preflight, ApiError> {
    access::authorize(&state.db, actor, None, "*")
        .await
        .map_err(|_| ApiError::Forbidden)?;
    let mode = input.mode;
    let force = input.force;
    let digest = input.artifact.database_digest.clone();
    let staged = stage(state, input).await?;
    let maintenance = state.maintenance.clone().try_write_owned().map_err(|_| {
        ApiError::Conflict(
            "instance restore requires no in-flight requests or worker I/O; retry when idle".into(),
        )
    })?;
    access::authorize(&state.db, actor, None, "*")
        .await
        .map_err(|_| ApiError::Forbidden)?;
    let path = database_file(state)?;
    let actor_id = actor.subject_id.clone();
    let runtime = state.orchestrator.clone();
    let state = state.clone();
    tokio::task::spawn_blocking(move||->Result<Preflight,ApiError>{
  // The guard lives in the blocking task: client disconnect cannot release the
  // maintenance boundary while SQLite is still committing the replacement.
  let _maintenance=maintenance;let mut destination=Connection::open(path).map_err(internal)?;destination.busy_timeout(Duration::from_secs(5)).map_err(internal)?;
  let already:i64=destination.query_row("SELECT COUNT(*) FROM instance_restore_receipts WHERE artifact_id=? AND digest=?",params![staged.preflight.artifact_id,digest],|r|r.get(0)).map_err(internal)?;
  if already>0&&!force{return Ok(staged.preflight)}
  if mode==Mode::Fail&&has_data(&destination).map_err(internal)?{return Err(ApiError::Conflict("destination has data; choose replace to restore the entire instance".into()))}
  let active:i64=destination.query_row("SELECT (SELECT COUNT(*) FROM execution_facts WHERE status='running')+(SELECT COUNT(*) FROM request_facts WHERE status='running')+(SELECT COUNT(*) FROM operation_jobs WHERE status='running')",[],|r|r.get(0)).map_err(internal)?;
  if active>0{return Err(ApiError::Conflict("running executions or jobs must finish before instance restore".into()))}
  let source=Connection::open(staged.file.path()).map_err(internal)?;
  source.execute("INSERT INTO instance_restore_receipts(artifact_id,digest,restored_at) VALUES(?,?,?)",params![staged.preflight.artifact_id,digest,db::now()]).map_err(internal)?;
  source.execute("INSERT INTO audit_events(id,action,resource_type,resource_id,details,created_at) VALUES(?,'instance.restore','instance',?,?,?)",params![super::id(),staged.preflight.artifact_id,json!({"principal_id":actor_id,"principal_kind":"restore_operator","mode":mode}).to_string(),db::now()]).map_err(internal)?;
  source.execute("INSERT INTO settings(key,value,updated_at) VALUES('_internal.observation_reset_required','true',unixepoch()) ON CONFLICT(key) DO UPDATE SET value='true',updated_at=unixepoch()",[]).map_err(internal)?;
  copy_database(&source,&mut destination).map_err(internal)?;runtime.reset_derived();
  let mut result=staged.preflight;
  result.derived_available=tokio::runtime::Handle::current().block_on(reset_projection(&state)).unwrap_or(false);
  Ok(result)
 }).await.map_err(internal)?
}
/// Re-derive the retained window of the observation projection from the
/// authoritative SQLite record system. This is the recovery path for a cleared
/// or restored projection, so it must read only durable records.
pub(crate) async fn rebuild_projection(state: &AppState) -> Result<usize, ApiError> {
    let cutoff = db::now() - state.config.observation_retention_days as i64 * 86400;
    let rows = state
        .db
        .query_all(super::sql(
            r#"SELECT f.id AS id, f.project_id AS project_id, f.api_key_id AS api_key_id,
                      f.started_at AS started_at, f.finished_at AS finished_at,
                      COALESCE(json_extract(r.request_metadata_json,'$.external_id'), f.id) AS external_request,
                      COALESCE(t.external_id, r.trace_id, '') AS external_trace,
                      COALESCE(r.endpoint, '') AS endpoint,
                      r.source_ip AS source_ip,
                      r.requested_model AS requested_model,
                      e.model AS resolved_model,
                      e.provider_name AS provider_name,
                      e.http_status AS http_status,
                      e.error_kind AS error_kind,
                      COALESCE(e.latency_ms, 0) AS latency_ms,
                      -- `first_token_at` is milliseconds since the epoch, built as
                      -- `started_at * 1000 + elapsed`. Subtracting the second-based
                      -- `started_at` and multiplying the difference by 1000 — as
                      -- this read used to — reported a first byte a thousand
                      -- centuries away. The analytics breakdown has always used
                      -- this form.
                      CASE WHEN e.first_token_at IS NULL THEN NULL
                           ELSE e.first_token_at - e.started_at * 1000 END AS ttft_ms,
                      COALESCE(u.input_tokens, 0) AS input_tokens,
                      COALESCE(u.output_tokens, 0) AS output_tokens,
                      COALESCE(u.cache_read_tokens, 0) AS cached_tokens,
                      -- Cache-write and reasoning tokens are the settled usage
                      -- counts. A cost component is what a price charged for, not
                      -- what the provider reported, so none is read here.
                      COALESCE(u.cache_write_tokens, 0) AS cache_write_tokens,
                      COALESCE(u.reasoning_tokens, 0) AS reasoning_tokens,
                      -- The stream decision as recorded at admission. The stored
                      -- JSON type is what separates a decision from its absence: a
                      -- missing or non-boolean value stays NULL rather than turning
                      -- into a guessed false.
                      CASE json_type(r.request_metadata_json, '$.stream')
                           WHEN 'true' THEN 1 WHEN 'false' THEN 0 END AS streamed,
                      COALESCE(u.total_cost_micros, 0) AS cost_micros,
                      c.request_json AS request_json, c.response_json AS response_json
               FROM request_facts f
               LEFT JOIN requests r ON r.id = f.id
               LEFT JOIN traces t ON t.id = r.trace_id
               LEFT JOIN request_executions e
                      ON e.request_id = f.id
                     AND e.attempt = (SELECT MAX(x.attempt) FROM request_executions x WHERE x.request_id = f.id)
               LEFT JOIN execution_facts ef ON ef.id = e.id
               LEFT JOIN usage_logs u ON u.execution_id = ef.id
               LEFT JOIN request_contents c ON c.request_id = f.id
               WHERE f.started_at >= ? AND f.log_level <> 'off'
               ORDER BY f.started_at"#,
            vec![cutoff.into()],
        ))
        .await?;
    let mut events = Vec::with_capacity(rows.len());
    for row in &rows {
        let payload = row
            .try_get::<Option<String>>("", "request_json")
            .ok()
            .flatten();
        events.push(crate::observability::RequestEvent {
            id: row.try_get("", "id").unwrap_or_default(),
            project_id: row.try_get("", "project_id").unwrap_or_default(),
            request_id: row.try_get("", "external_request").unwrap_or_default(),
            trace_id: row.try_get("", "external_trace").unwrap_or_default(),
            started_at: row.try_get("", "started_at").unwrap_or_default(),
            finished_at: row
                .try_get::<Option<i64>>("", "finished_at")
                .ok()
                .flatten()
                .unwrap_or_else(|| row.try_get("", "started_at").unwrap_or_default()),
            endpoint: row.try_get("", "endpoint").unwrap_or_default(),
            source_ip: row.try_get("", "source_ip").ok().flatten(),
            api_key_id: row.try_get("", "api_key_id").ok().flatten(),
            // Every one of these is read from the execution snapshot, verbatim. The
            // rebuild used to synthesize `200`/`502` from the execution status, and to
            // label a row with the provider's UUID because that was all it had. A row
            // that recorded none of it stays unmeasured: inventing a plausible answer
            // here is what made the console contradict the request that actually ran.
            provider: row.try_get("", "provider_name").ok().flatten(),
            requested_model: row.try_get("", "requested_model").ok().flatten(),
            resolved_model: row.try_get("", "resolved_model").ok().flatten(),
            status_code: row.try_get("", "http_status").ok().flatten(),
            error_kind: row.try_get("", "error_kind").ok().flatten(),
            latency_ms: row.try_get("", "latency_ms").unwrap_or_default(),
            ttft_ms: row.try_get("", "ttft_ms").ok().flatten(),
            input_tokens: row.try_get("", "input_tokens").unwrap_or_default(),
            output_tokens: row.try_get("", "output_tokens").unwrap_or_default(),
            cached_tokens: row.try_get("", "cached_tokens").unwrap_or_default(),
            cache_write_tokens: row.try_get("", "cache_write_tokens").unwrap_or_default(),
            reasoning_tokens: row.try_get("", "reasoning_tokens").unwrap_or_default(),
            // Read back, never inferred. A row that recorded no stream decision —
            // written before the fact existed, or an endpoint with no stream choice
            // — stays unmeasured.
            stream: row
                .try_get::<Option<i64>>("", "streamed")
                .ok()
                .flatten()
                .map(|value| value != 0),
            cost_micros: row.try_get("", "cost_micros").unwrap_or_default(),
            payload_captured: payload.is_some(),
            request_json: payload,
            response_json: row.try_get("", "response_json").ok().flatten(),
        });
    }
    Ok(state.observations.rebuild(events).await)
}

pub async fn reset_projection(state: &AppState) -> Result<bool, ApiError> {
    let reset = state
        .db
        .query_one(super::sql(
            "SELECT value FROM settings WHERE key='_internal.observation_reset_required'",
            vec![],
        ))
        .await?
        .is_some_and(|row| {
            row.try_get::<String>("", "value")
                .is_ok_and(|v| v == "true")
        });
    if !reset {
        return Ok(state.observations.is_available());
    }
    let mut cleared = tokio::time::timeout(
        Duration::from_secs(5),
        state.observations.clear_for_restore(),
    )
    .await
    .unwrap_or(false);
    if cleared {
        cleared = super::runtime::sync_retention(state).await?;
    }
    if cleared {
        // A restore leaves the projection empty. Derived data has to be
        // re-derived from the record system, or the console reports zero traffic
        // for the entire retained window even though the requests are all there.
        match rebuild_projection(state).await {
            Ok(count) => tracing::info!(
                count,
                "rebuilt the observation projection from the record system"
            ),
            Err(error) => tracing::error!(%error, "could not rebuild the observation projection"),
        }
    }
    if cleared {
        state
            .db
            .execute(super::sql(
                "DELETE FROM settings WHERE key='_internal.observation_reset_required'",
                vec![],
            ))
            .await?;
    }
    Ok(cleared)
}
