use super::{backup, id, jobs, sql};
use crate::{
    api::{ApiError, AppState},
    db,
};
use futures_util::StreamExt;
use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

const AUTH_FAILURE_AUDIT_RETENTION_DAYS: i64 = 90;

pub fn start(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let owner = id();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tick.tick().await;
            if schedule(&state).await.is_err() {
                tracing::warn!("scheduler enqueue failed");
            }
            for _ in 0..16 {
                let claim = match jobs::claim(&state.db, &owner, db::now(), 120).await {
                    Ok(Some(c)) => c,
                    Ok(None) => break,
                    Err(_) => {
                        tracing::warn!("job claim failed");
                        break;
                    }
                };
                let work = execute(&state, &claim);
                tokio::pin!(work);
                let result = loop {
                    tokio::select! {result=&mut work=>break result,_=tokio::time::sleep(Duration::from_secs(30))=>{if !jobs::heartbeat(&state.db,&claim,db::now(),120).await.unwrap_or(false){break Err(ApiError::Conflict("job lease lost".into()))}}}
                };
                if jobs::finish(&state.db, &claim, db::now(), result.is_ok())
                    .await
                    .is_err()
                {
                    tracing::warn!("job finalization failed")
                }
            }
        }
    })
}
async fn schedule(state: &AppState) -> Result<(), ApiError> {
    let now = db::now();
    jobs::enqueue_due(&state.db, now).await?;
    for source in crate::catalog::repository::due_sources(&state.db, now).await? {
        jobs::enqueue(
            &state.db,
            None,
            "catalog_refresh",
            &format!(
                "catalog:{}:{}:{}",
                source.id,
                source.revision,
                now / source.refresh_interval_secs
            ),
            &json!({"source_id":source.id,"revision":source.revision}),
            now,
        )
        .await?;
    }
    jobs::enqueue(
        &state.db,
        None,
        "gc",
        &format!("gc:{}", now / 3600),
        &json!({}),
        now,
    )
    .await?;
    Ok(())
}
pub async fn execute(state: &AppState, claim: &jobs::Claim) -> Result<(), ApiError> {
    let _maintenance = state.maintenance.clone().read_owned().await;
    jobs::fence(&state.db, claim).await?;
    let payload: Value =
        serde_json::from_str(&claim.payload_json).map_err(|e| ApiError::Internal(e.into()))?;
    match claim.kind.as_str() {
        "catalog_refresh" => {
            let source = payload["source_id"].as_str().ok_or(ApiError::NotFound)?;
            let current = crate::catalog::repository::source(&state.db, source).await?;
            if payload["revision"].as_i64() == Some(current.revision) {
                // A scheduled refresh has no interactive actor, so it records no audit
                // row (as before); the mutation still goes through the same
                // transactional repository call as the admin-triggered paths.
                let proxy =
                    super::proxy::resolve(state, current.proxy_preset_id.as_deref()).await?;
                crate::catalog::refresh::refresh_with_proxy(&state.db, source, proxy, None).await?;
            }
            Ok(())
        }
        "backup" => backup::execute(state, claim).await,
        "automatic_backup" => {
            let project = claim.project_id.as_deref().ok_or(ApiError::Forbidden)?;
            let targets: Vec<String> = serde_json::from_value(payload["targets"].clone())
                .map_err(|_| ApiError::BadRequest("invalid targets".into()))?;
            let resources: Vec<String> = serde_json::from_value(payload["resources"].clone())
                .map_err(|_| ApiError::BadRequest("invalid resources".into()))?;
            backup::enqueue(
                state,
                project,
                &targets,
                &backup::Selection { resources },
                &format!("backup:{}", claim.id),
            )
            .await?;
            Ok(())
        }
        "probe" => probe(state, claim, &payload).await,
        "quota" => quota(state, claim, &payload).await,
        "model_sync" => {
            let result = model_sync(state, claim, &payload).await;
            if result.is_err() {
                record_model_sync_error(state, claim, &payload).await?;
            }
            result
        }
        "webhook" => deliver(state, claim, &payload).await,
        "gc" => gc(state).await,
        "backup_retention" => {
            backup::retain(
                state,
                claim.project_id.as_deref().ok_or(ApiError::Forbidden)?,
                payload["storage_id"].as_str().ok_or(ApiError::NotFound)?,
                payload["keep"].as_u64().unwrap_or(7) as usize,
            )
            .await?;
            Ok(())
        }
        _ => Err(ApiError::BadRequest("unsupported durable job kind".into())),
    }
}

async fn model_sync(
    state: &AppState,
    claim: &jobs::Claim,
    payload: &Value,
) -> Result<(), ApiError> {
    let project = claim.project_id.as_deref().ok_or(ApiError::Forbidden)?;
    let provider = payload["provider_id"].as_str().ok_or(ApiError::NotFound)?;
    let row=state.db.query_one(sql("SELECT p.name,p.kind,p.base_url,c.credential_type,c.secret_envelope,s.proxy_url,s.proxy_username,s.proxy_secret_envelope,COALESCE(s.proxy_reuse_connections,1) AS proxy_reuse_connections,s.proxy_preset_id FROM providers p JOIN channel_credentials c ON c.provider_id=p.id AND c.enabled=1 LEFT JOIN channel_settings s ON s.provider_id=p.id WHERE p.id=? AND p.project_id=? AND p.enabled=1 ORDER BY c.priority,c.id LIMIT 1",vec![provider.into(),project.into()])).await?.ok_or(ApiError::NotFound)?;
    let secret_envelope: String = row.try_get("", "secret_envelope")?;
    let target = crate::models::RouteTarget {
        public_name: String::new(),
        upstream_name: String::new(),
        provider_name: row.try_get("", "name")?,
        provider_kind: row.try_get("", "kind")?,
        base_url: row.try_get("", "base_url")?,
        credential_type: row.try_get("", "credential_type")?,
        secret_envelope: secret_envelope.clone(),
        proxy_url: row.try_get("", "proxy_url")?,
        proxy_username: row.try_get("", "proxy_username")?,
        proxy_secret_envelope: row.try_get("", "proxy_secret_envelope")?,
        proxy_reuse_connections: row.try_get("", "proxy_reuse_connections")?,
        proxy_preset_id: row.try_get("", "proxy_preset_id")?,
        input_price_micros: 0,
        output_price_micros: 0,
    };
    let secret = crate::oauth::credential_secret(
        &target.credential_type,
        state.secrets.decrypt(&secret_envelope)?,
    )?;
    let client = state.upstream_client(&target).await?;
    let discovered = crate::providers::discovery::models(
        &client,
        &target.provider_kind,
        &target.base_url,
        &secret,
    )
    .await?;

    let discovered_names = json!(discovered.iter().map(|model| &model.id).collect::<Vec<_>>());
    let tx = state.db.begin().await?;
    jobs::fence(&tx, claim).await?;
    if tx
        .query_one(sql(
            "SELECT id FROM providers WHERE id=? AND project_id=?",
            vec![provider.into(), project.into()],
        ))
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    tx.execute(sql("UPDATE models SET enabled=0,lifecycle='archived' WHERE provider_id=? AND discovery_managed=1 AND upstream_name NOT IN (SELECT value FROM json_each(?))",vec![provider.into(),discovered_names.to_string().into()])).await?;
    for model in &discovered {
        let manual = tx.query_one(sql("SELECT id FROM models WHERE provider_id=? AND upstream_name=? AND discovery_managed=0 LIMIT 1",vec![provider.into(),model.id.clone().into()])).await?.is_some();
        if manual {
            tx.execute(sql("UPDATE models SET enabled=0,lifecycle='archived' WHERE provider_id=? AND upstream_name=? AND discovery_managed=1",vec![provider.into(),model.id.clone().into()])).await?;
            continue;
        }
        let digest = blake3::hash(format!("{provider}\0{}", model.id).as_bytes())
            .to_hex()
            .to_string();
        let model_id = format!("discovered-{}", &digest[..32]);
        tx.execute(sql("INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,input_price_micros,output_price_micros,priority,enabled,created_at,catalog_metadata_json,lifecycle,discovery_managed) VALUES(?,?,?,?,?,0,0,100,1,?,?,'active',1) ON CONFLICT(id) DO UPDATE SET public_name=excluded.public_name,upstream_name=excluded.upstream_name,capabilities=excluded.capabilities,enabled=1,lifecycle='active',catalog_metadata_json=excluded.catalog_metadata_json WHERE models.provider_id=excluded.provider_id AND models.discovery_managed=1",vec![model_id.into(),provider.into(),model.id.clone().into(),model.id.clone().into(),json!(&model.capabilities).to_string().into(),db::now().into(),json!({"version":1,"discovery":{"adapter":&target.provider_kind}}).to_string().into()])).await?;
    }
    tx.execute(sql("UPDATE channel_settings SET model_sync_error=NULL,model_synced_at=?,model_sync_count=?,updated_at=? WHERE provider_id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![db::now().into(),i64::try_from(discovered.len()).unwrap_or(i64::MAX).into(),db::now().into(),provider.into(),project.into()])).await?;
    super::audit(&tx, None, project, "model.sync", provider).await?;
    tx.commit().await?;
    Ok(())
}

async fn record_model_sync_error(
    state: &AppState,
    claim: &jobs::Claim,
    payload: &Value,
) -> Result<(), ApiError> {
    let project = claim.project_id.as_deref().ok_or(ApiError::Forbidden)?;
    let provider = payload["provider_id"].as_str().ok_or(ApiError::NotFound)?;
    let tx = state.db.begin().await?;
    jobs::fence(&tx, claim).await?;
    let changed=tx.execute(sql("UPDATE channel_settings SET model_sync_error='model_sync_failed',updated_at=? WHERE provider_id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![db::now().into(),provider.into(),project.into()])).await?.rows_affected();
    if changed == 1 {
        super::audit(&tx, None, project, "model.sync_failed", provider).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn recover(state: &AppState) -> Result<(), ApiError> {
    // Single-node startup only. A crash after provider commitment has ambiguous
    // usage; charge the durable reservation once and retain an explicit status.
    let tx = state.db.begin().await?;
    recover_in(&tx, None, None).await?;
    tx.commit().await?;
    sync_retention(state).await?;
    Ok(())
}

/// The caller owns the transaction. Restore supplies only newly inserted IDs;
/// startup supplies no scope because no live request tasks exist yet.
pub(super) async fn recover_in(
    tx: &impl ConnectionTrait,
    executions: Option<&[String]>,
    requests: Option<&[String]>,
) -> Result<(), ApiError> {
    let execution_scope = json!(executions).to_string();
    let request_scope = json!(requests).to_string();
    let rows=tx.query_all(sql("UPDATE execution_facts SET status='interrupted',finished_at=? WHERE status='running' AND (?='null' OR id IN (SELECT value FROM json_each(?))) RETURNING id,request_id,model_id,json_extract(price_json,'$.id') AS price_id,CASE WHEN contacted=1 THEN reserved_micros ELSE 0 END AS reserved_micros",vec![db::now().into(),execution_scope.clone().into(),execution_scope.into()])).await?;
    for row in rows {
        let execution: String = row.try_get("", "id")?;
        let request: String = row.try_get("", "request_id")?;
        let cost: i64 = row.try_get("", "reserved_micros")?;
        let usage = id();
        let inserted=tx.execute(sql("INSERT INTO usage_logs(id,execution_id,model_id,price_id,total_cost_micros,created_at,settlement_kind) VALUES(?,?,?,?,?,?,'interrupted') ON CONFLICT(execution_id) DO NOTHING",vec![usage.clone().into(),execution.clone().into(),row.try_get::<Option<String>>("","model_id")?.into(),row.try_get::<Option<String>>("","price_id")?.into(),cost.into(),db::now().into()])).await?.rows_affected();
        if inserted > 0 && cost > 0 {
            tx.execute(sql("INSERT INTO usage_cost_items(id,usage_log_id,quantity,subtotal_micros) VALUES(?,?,1,?)",vec![id().into(),usage.into(),cost.into()])).await?;
            tx.execute(sql("UPDATE api_keys SET spent_micros=spent_micros+? WHERE id=(SELECT api_key_id FROM request_facts WHERE id=?)",vec![cost.into(),request.into()])).await?;
        }
        tx.execute(sql("UPDATE request_executions SET status='interrupted',retry_reason='process_restart',error_kind='interrupted',finished_at=? WHERE id=?",vec![db::now().into(),execution.into()])).await?;
    }
    for table in ["request_facts", "requests"] {
        tx.execute(sql(
            format!("UPDATE {table} SET status='interrupted',finished_at=? WHERE status='running' AND (?='null' OR id IN (SELECT value FROM json_each(?))) AND NOT EXISTS(SELECT 1 FROM execution_facts WHERE request_id={table}.id AND status='running')"),
            vec![db::now().into(),request_scope.clone().into(),request_scope.clone().into()],
        ))
        .await?;
    }
    tx.execute(sql("UPDATE traces SET status='interrupted',finished_at=? WHERE status='running' AND (?='null' OR id IN(SELECT trace_id FROM requests WHERE id IN(SELECT value FROM json_each(?)))) AND NOT EXISTS(SELECT 1 FROM requests WHERE trace_id=traces.id AND status='running')",vec![db::now().into(),request_scope.clone().into(),request_scope.into()])).await?;
    Ok(())
}

pub(super) async fn sync_retention(state: &AppState) -> Result<bool, ApiError> {
    sync_retention_within(state, RETAINED_ID_PAGE, MAX_RETAINED_REQUESTS).await
}

/// How many pinned request ids one page carries out of the record system, and how
/// many may be pinned before the derived projection refuses to apply retention at
/// all. Both are hard bounds: the projection is a convenience, and an unbounded
/// read would let one operator's pin list decide the memory of a background pass.
const RETAINED_ID_PAGE: i64 = 500;
const MAX_RETAINED_REQUESTS: usize = 50_000;

/// The retention pass with its bounds named, so the ceiling is testable without
/// pinning fifty thousand requests.
///
/// Returns whether derived retention was applied. A pass that cannot name every
/// pinned request disables derived retention entirely instead of cleaning up with
/// an incomplete preserve set: a partial pin list would delete the events of the
/// pins it did not carry, which is the very promise the operator asked for.
pub(crate) async fn sync_retention_within(
    state: &AppState,
    page: i64,
    ceiling: usize,
) -> Result<bool, ApiError> {
    let Some(pinned) = retained_request_ids(state, page, ceiling).await? else {
        // Fail safe: no rule deletes a row and no rule clears a payload — for the
        // events already stored and for the ones still arriving — so nothing pinned
        // is expired by a pass that cannot see all of it. The record system keeps
        // enforcing the pin and the projection stays readable; only the projection's
        // own expiry is off until the pin set fits again.
        tracing::warn!(
            ceiling,
            "more requests are pinned than the derived projection will track; derived retention is disabled for this pass so no pinned run is expired"
        );
        state.observations.apply_retention(Vec::new()).await;
        return Ok(false);
    };
    let mut rules = vec![crate::observability::Retention::new(
        None,
        state.config.observation_retention_days.min(i64::MAX as u64) as i64,
        false,
    )];
    for row in state.db.query_all(sql("SELECT project_id,resource_type,retention_days FROM data_retention_policies WHERE resource_type IN ('requests','payloads')",vec![])).await? {
        rules.push(crate::observability::Retention::new(
            row.try_get("", "project_id")?,
            row.try_get("", "retention_days")?,
            row.try_get::<String>("", "resource_type")? == "payloads",
        ));
    }
    // Every rule carries the pin, the instance-wide window included: a pin that
    // only survived the project's own policy would still be expired by the default
    // window, which is the rule that reaches every event.
    let rules = rules
        .into_iter()
        .map(|rule| rule.preserving(pinned.clone()))
        .collect();
    Ok(state.observations.apply_retention(rules).await)
}

/// The requests a retained trace owns, by Pangolin's own request uuid.
///
/// This is the whole bridge between the record system and the derived projection:
/// `RequestEvent.id` is the same value as `requests.id`, so the projection can
/// spare exactly those events by identity, with no caller-supplied string and no
/// per-event query. The read is paged by keyset rather than offset — the tables are
/// live, and a row inserted between two pages must not shift the window and hide
/// another pinned request — and `None` says the pin set does not fit, never a
/// truncated set.
async fn retained_request_ids(
    state: &AppState,
    page: i64,
    ceiling: usize,
) -> Result<Option<Arc<HashSet<String>>>, ApiError> {
    let mut ids = HashSet::new();
    let mut after: Option<String> = None;
    loop {
        let rows = state
            .db
            .query_all(sql(
                "SELECT r.id AS id FROM requests r JOIN traces t ON t.id=r.trace_id WHERE t.lifecycle='retained' AND (? IS NULL OR r.id>?) ORDER BY r.id LIMIT ?",
                vec![after.clone().into(), after.clone().into(), page.into()],
            ))
            .await?;
        let filled = rows.len() as i64 >= page;
        for row in rows {
            let id: String = row.try_get("", "id")?;
            after = Some(id.clone());
            ids.insert(id);
        }
        // The ceiling is checked before the partial page can succeed: a set that
        // only exceeds it on the last, partial page is still a set that does not
        // fit, and accepting it here applied rules whose pin list had just been
        // declared unbounded.
        if ids.len() > ceiling {
            return Ok(None);
        }
        if !filled {
            return Ok(Some(Arc::new(ids)));
        }
    }
}
async fn target(
    state: &AppState,
    project: &str,
    provider: &str,
    model_id: Option<&str>,
) -> Result<(crate::models::RouteTarget, String, String), ApiError> {
    let (query, values) = if let Some(model_id) = model_id {
        (
            "SELECT p.name,p.kind,p.base_url,c.id AS credential_id,c.credential_type,c.secret_envelope,m.public_name,m.upstream_name,s.proxy_url,s.proxy_username,s.proxy_secret_envelope,COALESCE(s.proxy_reuse_connections,1) AS proxy_reuse_connections,s.proxy_preset_id FROM providers p JOIN channel_credentials c ON c.provider_id=p.id AND c.enabled=1 JOIN models m ON m.provider_id=p.id AND m.enabled=1 AND m.lifecycle='active' LEFT JOIN channel_settings s ON s.provider_id=p.id WHERE p.id=? AND p.project_id=? AND p.enabled=1 AND m.id=? ORDER BY c.priority LIMIT 1",
            vec![provider.into(), project.into(), model_id.into()],
        )
    } else {
        (
            "SELECT p.name,p.kind,p.base_url,c.id AS credential_id,c.credential_type,c.secret_envelope,m.public_name,m.upstream_name,s.proxy_url,s.proxy_username,s.proxy_secret_envelope,COALESCE(s.proxy_reuse_connections,1) AS proxy_reuse_connections,s.proxy_preset_id FROM providers p JOIN channel_credentials c ON c.provider_id=p.id AND c.enabled=1 JOIN models m ON m.provider_id=p.id AND m.enabled=1 AND m.lifecycle='active' LEFT JOIN channel_settings s ON s.provider_id=p.id WHERE p.id=? AND p.project_id=? AND p.enabled=1 ORDER BY c.priority,m.priority LIMIT 1",
            vec![provider.into(), project.into()],
        )
    };
    let row = state
        .db
        .query_one(sql(query, values))
        .await?
        .ok_or(ApiError::NotFound)?;
    let credential = row.try_get("", "credential_id")?;
    let secret: String = row.try_get("", "secret_envelope")?;
    Ok((
        crate::models::RouteTarget {
            public_name: row.try_get("", "public_name")?,
            upstream_name: row.try_get("", "upstream_name")?,
            provider_name: row.try_get("", "name")?,
            provider_kind: row.try_get("", "kind")?,
            base_url: row.try_get("", "base_url")?,
            credential_type: row.try_get("", "credential_type")?,
            secret_envelope: secret.clone(),
            proxy_url: row.try_get("", "proxy_url")?,
            proxy_username: row.try_get("", "proxy_username")?,
            proxy_secret_envelope: row.try_get("", "proxy_secret_envelope")?,
            proxy_reuse_connections: row.try_get("", "proxy_reuse_connections")?,
            proxy_preset_id: row.try_get("", "proxy_preset_id")?,
            input_price_micros: 0,
            output_price_micros: 0,
        },
        credential,
        crate::oauth::credential_secret(
            &row.try_get::<String>("", "credential_type")?,
            state.secrets.decrypt(&secret)?,
        )?
        .to_string(),
    ))
}
pub(crate) struct ProbeSelection {
    target: crate::models::RouteTarget,
    credential: String,
    endpoint: String,
    path: String,
    stream: bool,
}

pub(crate) async fn validate_probe_selection<C: ConnectionTrait>(
    connection: &C,
    project: &str,
    payload: &Value,
) -> Result<ProbeSelection, ApiError> {
    let provider = payload["provider_id"].as_str().ok_or(ApiError::NotFound)?;
    let model = payload["model_id"].as_str().ok_or(ApiError::NotFound)?;
    let credential = match payload.get("credential_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if !id.is_empty() => Some(id.as_str()),
        _ => return Err(ApiError::BadRequest("invalid probe credential".into())),
    };
    let requested_endpoint = match payload.get("endpoint") {
        None | Some(Value::Null) => None,
        Some(Value::String(endpoint)) => Some(endpoint.as_str()),
        _ => return Err(ApiError::BadRequest("invalid probe endpoint".into())),
    };
    let requested_stream = match payload.get("stream") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(stream)) => Some(*stream),
        _ => return Err(ApiError::BadRequest("invalid probe stream choice".into())),
    };
    let row = connection.query_one(sql(
        "SELECT p.name,p.kind,p.base_url,c.id AS credential_id,c.credential_type,c.secret_envelope,m.public_name,m.upstream_name,m.capabilities,s.endpoint_mappings_json,s.model_rules_json,s.proxy_url,s.proxy_username,s.proxy_secret_envelope,COALESCE(s.proxy_reuse_connections,1) AS proxy_reuse_connections,s.proxy_preset_id FROM providers p JOIN projects project ON project.id=p.project_id AND project.enabled=1 JOIN models m ON m.provider_id=p.id AND m.id=? AND m.enabled=1 AND m.lifecycle='active' JOIN channel_credentials c ON c.provider_id=p.id AND c.enabled=1 LEFT JOIN channel_settings s ON s.provider_id=p.id WHERE p.id=? AND p.project_id=? AND p.enabled=1 AND (? IS NULL OR c.id=?) ORDER BY c.priority,c.id LIMIT 1",
        vec![model.into(),provider.into(),project.into(),credential.into(),credential.into()],
    )).await?.ok_or_else(|| ApiError::BadRequest("probe model or credential is not enabled for this channel".into()))?;
    let kind: String = row.try_get("", "kind")?;
    let endpoint = requested_endpoint.unwrap_or(match kind.as_str() {
        "anthropic" => "/v1/messages",
        "gemini" | "vertex" | "gcp" => "/v1beta/models:generateContent",
        _ => "/v1/chat/completions",
    });
    if !matches!(
        endpoint,
        "/v1/chat/completions"
            | "/v1/responses"
            | "/v1/messages"
            | "/v1beta/models:generateContent"
            | "/v1beta/models:streamGenerateContent"
    ) {
        return Err(ApiError::BadRequest("unsupported probe endpoint".into()));
    }
    let stream = requested_stream.unwrap_or(endpoint == "/v1beta/models:streamGenerateContent");
    if matches!(
        endpoint,
        "/v1beta/models:generateContent" | "/v1beta/models:streamGenerateContent"
    ) && stream != (endpoint == "/v1beta/models:streamGenerateContent")
    {
        return Err(ApiError::BadRequest(
            "Gemini probe stream must match endpoint".into(),
        ));
    }
    let capabilities: Vec<String> =
        serde_json::from_str(&row.try_get::<String>("", "capabilities")?)
            .map_err(|_| ApiError::BadRequest("invalid model capabilities".into()))?;
    if !crate::providers::supports(&kind, &capabilities, endpoint, stream) {
        return Err(ApiError::BadRequest(
            "model does not support probe endpoint or stream".into(),
        ));
    }
    let rules: Value = serde_json::from_str(
        &row.try_get::<Option<String>>("", "model_rules_json")?
            .unwrap_or_else(|| "{\"version\":1}".into()),
    )
    .map_err(|_| ApiError::BadRequest("invalid channel model rules".into()))?;
    if !rules.is_object() || rules["version"] != 1 {
        return Err(ApiError::BadRequest("invalid channel model rules".into()));
    }
    if stream && rules["stream"] == false {
        return Err(ApiError::BadRequest(
            "streaming is disabled for this channel".into(),
        ));
    }
    let mappings: Value = serde_json::from_str(
        &row.try_get::<Option<String>>("", "endpoint_mappings_json")?
            .unwrap_or_else(|| "{\"version\":1}".into()),
    )
    .map_err(|_| ApiError::BadRequest("invalid endpoint mappings".into()))?;
    if !mappings.is_object() || mappings["version"] != 1 {
        return Err(ApiError::BadRequest("invalid endpoint mappings".into()));
    }
    let path = mappings
        .pointer(&format!(
            "/paths/{}",
            endpoint.replace('~', "~0").replace('/', "~1")
        ))
        .and_then(Value::as_str)
        .unwrap_or(endpoint);
    if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#']) {
        return Err(ApiError::BadRequest(
            "invalid probe endpoint mapping".into(),
        ));
    }
    Ok(ProbeSelection {
        target: crate::models::RouteTarget {
            public_name: row.try_get("", "public_name")?,
            upstream_name: row.try_get("", "upstream_name")?,
            provider_name: row.try_get("", "name")?,
            provider_kind: kind,
            base_url: row.try_get("", "base_url")?,
            credential_type: row.try_get("", "credential_type")?,
            secret_envelope: row.try_get("", "secret_envelope")?,
            proxy_url: row.try_get("", "proxy_url")?,
            proxy_username: row.try_get("", "proxy_username")?,
            proxy_secret_envelope: row.try_get("", "proxy_secret_envelope")?,
            proxy_reuse_connections: row.try_get("", "proxy_reuse_connections")?,
            proxy_preset_id: row.try_get("", "proxy_preset_id")?,
            input_price_micros: 0,
            output_price_micros: 0,
        },
        credential: row.try_get("", "credential_id")?,
        endpoint: endpoint.into(),
        path: path.into(),
        stream,
    })
}

#[derive(Default)]
struct ProbeMeasurement {
    started: Option<Instant>,
    status_code: Option<i32>,
    response_headers_ms: Option<i64>,
    first_event_ms: Option<i64>,
    first_text_ms: Option<i64>,
    output_tokens: Option<i64>,
}

fn probe_error_status(status: reqwest::StatusCode) -> &'static str {
    match status.as_u16() {
        401 | 403 => "authentication",
        429 => "rate_limited",
        404 => "model_unavailable",
        _ => "upstream_http",
    }
}

fn probe_shape(endpoint: &str, value: &Value) -> bool {
    match endpoint {
        "/v1/chat/completions" => value["choices"].is_array(),
        "/v1/responses" => value["status"].is_string() && value["output"].is_array(),
        "/v1/messages" => value["content"].is_array() && value["stop_reason"].is_string(),
        "/v1beta/models:generateContent" => value["candidates"].is_array(),
        _ => false,
    }
}

fn probe_output(endpoint: &str, value: &Value) -> bool {
    match endpoint {
        "/v1/chat/completions" => value["choices"].as_array().is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice["message"].is_object()
                    && (choice["message"]["content"]
                        .as_str()
                        .is_some_and(|text| !text.trim().is_empty())
                        || choice["message"]["tool_calls"]
                            .as_array()
                            .is_some_and(|calls| !calls.is_empty()))
            })
        }),
        "/v1/responses" => {
            value["status"] == "completed"
                && value["output"].as_array().is_some_and(|items| {
                    items.iter().any(|item| {
                        item["type"] == "function_call"
                            || item["content"].as_array().is_some_and(|parts| {
                                parts.iter().any(|part| {
                                    part["type"] == "output_text"
                                        && part["text"]
                                            .as_str()
                                            .is_some_and(|text| !text.trim().is_empty())
                                })
                            })
                    })
                })
        }
        "/v1/messages" => value["content"].as_array().is_some_and(|parts| {
            parts.iter().any(|part| {
                part["type"] == "tool_use"
                    || part["type"] == "text"
                        && part["text"]
                            .as_str()
                            .is_some_and(|text| !text.trim().is_empty())
            })
        }),
        "/v1beta/models:generateContent" => {
            value["candidates"].as_array().is_some_and(|candidates| {
                candidates.iter().any(|candidate| {
                    candidate["content"]["parts"]
                        .as_array()
                        .is_some_and(|parts| {
                            parts.iter().any(|part| {
                                part.get("functionCall").is_some()
                                    || part["thought"] != true
                                        && part["text"]
                                            .as_str()
                                            .is_some_and(|text| !text.trim().is_empty())
                            })
                        })
                })
            })
        }
        _ => false,
    }
}

fn probe_stream_shape(endpoint: &str, value: &Value) -> bool {
    if matches!(value["type"].as_str(), Some("ping" | "heartbeat")) {
        return true;
    }
    match endpoint {
        "/v1/chat/completions" => value["choices"].is_array(),
        "/v1/responses" => value["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("response.")),
        "/v1/messages" => matches!(
            value["type"].as_str(),
            Some(
                "message_start"
                    | "message_delta"
                    | "message_stop"
                    | "content_block_start"
                    | "content_block_delta"
                    | "content_block_stop"
            )
        ),
        "/v1beta/models:streamGenerateContent" => {
            value["candidates"].is_array() || value["usageMetadata"].is_object()
        }
        _ => false,
    }
}

fn probe_stream_output(endpoint: &str, value: &Value) -> bool {
    crate::providers::timing::visible_text(value)
        || match endpoint {
            "/v1/chat/completions" => value["choices"].as_array().is_some_and(|choices| {
                choices.iter().any(|choice| {
                    choice["delta"]["tool_calls"]
                        .as_array()
                        .is_some_and(|calls| !calls.is_empty())
                })
            }),
            "/v1/responses" => {
                matches!(
                    value["type"].as_str(),
                    Some("response.function_call_arguments.delta" | "response.output_item.added")
                ) && (value["type"] != "response.output_item.added"
                    || value["item"]["type"] == "function_call")
            }
            "/v1/messages" => {
                value["type"] == "content_block_start"
                    && value["content_block"]["type"] == "tool_use"
            }
            "/v1beta/models:streamGenerateContent" => {
                value["candidates"].as_array().is_some_and(|candidates| {
                    candidates.iter().any(|candidate| {
                        candidate["content"]["parts"]
                            .as_array()
                            .is_some_and(|parts| {
                                parts.iter().any(|part| part.get("functionCall").is_some())
                            })
                    })
                })
            }
            _ => false,
        }
}

fn probe_transport_error(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else {
        "network"
    }
}

async fn probe_json(response: reqwest::Response) -> Result<Value, &'static str> {
    let mut chunks = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|error| probe_transport_error(&error))?;
        if bytes.len().saturating_add(chunk.len()) > 1024 * 1024 {
            return Err("invalid_response");
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid_response")
}

async fn probe_response(
    state: &AppState,
    selection: &ProbeSelection,
    secret: &str,
    measured: &mut ProbeMeasurement,
) -> Result<(), &'static str> {
    let endpoint = selection.endpoint.as_str();
    // Anthropic chat uses the gateway's native Messages upstream protocol. The
    // selected public endpoint remains Chat in the persisted probe record.
    let wire_endpoint =
        if selection.target.provider_kind == "anthropic" && endpoint == "/v1/chat/completions" {
            "/v1/messages"
        } else {
            endpoint
        };
    let wire_path = if wire_endpoint != endpoint && selection.path == endpoint {
        wire_endpoint
    } else {
        &selection.path
    };
    let request = match wire_endpoint {
        "/v1/responses" => {
            json!({"model":selection.target.upstream_name,"input":"Reply OK","max_output_tokens":8,"stream":selection.stream})
        }
        "/v1/messages" => {
            json!({"model":selection.target.upstream_name,"messages":[{"role":"user","content":"Reply OK"}],"max_tokens":8,"stream":selection.stream})
        }
        "/v1beta/models:generateContent" | "/v1beta/models:streamGenerateContent" => {
            json!({"model":selection.target.upstream_name,"contents":[{"role":"user","parts":[{"text":"Reply OK"}]}],"generationConfig":{"maxOutputTokens":8},"stream":selection.stream})
        }
        _ => {
            json!({"model":selection.target.upstream_name,"messages":[{"role":"user","content":"Reply OK"}],"max_tokens":8,"stream":selection.stream})
        }
    };
    let prepared = crate::providers::prepare_routed(
        &selection.target,
        crate::providers::Route {
            protocol: wire_endpoint,
            path: wire_path,
            native_version: None,
        },
        &request,
        secret,
        http::HeaderMap::new(),
        &http::HeaderMap::new(),
    )
    .await
    .map_err(|_| "invalid_response")?;
    if selection.stream && !prepared.response.identity() {
        return Err("invalid_response");
    }
    let client = state
        .upstream_client(&selection.target)
        .await
        .map_err(|_| "network")?;
    measured.started = Some(Instant::now());
    let started = measured.started.expect("probe send has a clock origin");
    let response = client
        .post(prepared.url)
        .headers(prepared.headers)
        .json(&prepared.payload)
        .send()
        .await
        .map_err(|error| probe_transport_error(&error))?;
    crate::providers::timing::record_headers_at(
        started,
        crate::providers::timing::capture_now(),
        &mut measured.response_headers_ms,
    );
    #[cfg(test)]
    crate::providers::timing::observe_for_test(
        measured.response_headers_ms,
        measured.first_event_ms,
        measured.first_text_ms,
    );
    measured.status_code = Some(i32::from(response.status().as_u16()));
    if !response.status().is_success() {
        return Err(probe_error_status(response.status()));
    }
    if selection.stream {
        if !response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .is_some_and(|content| {
                content
                    .to_str()
                    .is_ok_and(|text| text.starts_with("text/event-stream"))
            })
        {
            return Err("invalid_response");
        }
        let bytes_seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = bytes_seen.clone();
        let transport_failure = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let transport_flag = transport_failure.clone();
        let chunks = response.bytes_stream().map(move |chunk| {
            let chunk = chunk.map_err(|error| {
                transport_flag.store(
                    if error.is_timeout() { 2 } else { 1 },
                    std::sync::atomic::Ordering::Relaxed,
                );
                std::io::Error::other("probe transport")
            })?;
            if counter
                .fetch_add(chunk.len(), std::sync::atomic::Ordering::Relaxed)
                .saturating_add(chunk.len())
                > 1024 * 1024
            {
                return Err(std::io::Error::other("probe response limit"));
            }
            Ok(chunk)
        });
        let mut frames = Box::pin(crate::providers::framing::frames(chunks));
        let mut terminal = crate::orchestration::stream::TerminalState::new(&request);
        let mut events = 0;
        let mut output = false;
        while let Some(frame) = frames.next().await {
            let frame = frame.map_err(|_| {
                match transport_failure.load(std::sync::atomic::Ordering::Relaxed) {
                    2 => "timeout",
                    1 => "network",
                    _ => "invalid_response",
                }
            })?;
            events += 1;
            if events > 2048 {
                return Err("invalid_response");
            }
            let Some(data) = frame.data else { continue };
            let observed = crate::providers::timing::capture_now();
            crate::providers::timing::record_headers_at(
                started,
                observed,
                &mut measured.first_event_ms,
            );
            let event = eventsource_stream::Event {
                event: frame.event.unwrap_or_else(|| "message".into()),
                data,
                ..Default::default()
            };
            if crate::orchestration::stream::failed(&event) {
                return Err("invalid_response");
            }
            if let Ok(value) = serde_json::from_str::<Value>(&event.data) {
                if !probe_stream_shape(endpoint, &value) {
                    return Err("invalid_response");
                }
                crate::providers::timing::record_event_at(
                    started,
                    observed,
                    &event.data,
                    &mut measured.first_event_ms,
                    &mut measured.first_text_ms,
                );
                output |= probe_stream_output(endpoint, &value);
                let usage = super::pricing::Usage::parse(&value);
                if usage.reported {
                    measured.output_tokens = Some(usage.output);
                }
            } else if event.data.trim() != "[DONE]" {
                return Err("invalid_response");
            }
            #[cfg(test)]
            crate::providers::timing::observe_for_test(
                measured.response_headers_ms,
                measured.first_event_ms,
                measured.first_text_ms,
            );
            if terminal.terminal(&event, endpoint) {
                return if output {
                    Ok(())
                } else {
                    Err("empty_response")
                };
            }
        }
        Err("missing_terminal")
    } else {
        let raw = probe_json(response).await?;
        if raw.get("error").is_some() {
            return Err("invalid_response");
        }
        let value = prepared
            .response
            .apply(raw)
            .map_err(|_| "invalid_response")?;
        if !probe_shape(wire_endpoint, &value) {
            return Err("invalid_response");
        }
        if !probe_output(wire_endpoint, &value) {
            return Err("empty_response");
        }
        let usage = super::pricing::Usage::parse(&value);
        if usage.reported {
            measured.output_tokens = Some(usage.output);
        }
        Ok(())
    }
}

async fn probe(state: &AppState, claim: &jobs::Claim, payload: &Value) -> Result<(), ApiError> {
    let project = claim.project_id.as_deref().ok_or(ApiError::Forbidden)?;
    // Recheck selection after queueing and before any upstream I/O. An operator may
    // disable a credential, model or provider while this durable job is pending.
    let selection = validate_probe_selection(&state.db, project, payload).await?;
    let secret = crate::oauth::credential_secret(
        &selection.target.credential_type,
        state.secrets.decrypt(&selection.target.secret_envelope)?,
    )?
    .to_string();
    let mut measured = ProbeMeasurement::default();
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        probe_response(state, &selection, &secret, &mut measured),
    )
    .await
    .unwrap_or(Err("timeout"));
    let success = result.is_ok();
    let error = result.err();
    let provider = payload["provider_id"].as_str().ok_or(ApiError::NotFound)?;
    let tx = state.db.begin().await?;
    jobs::fence(&tx, claim).await?;
    tx.execute(sql("INSERT INTO channel_probes(id,provider_id,credential_id,model,endpoint,stream,success,status_code,latency_ms,response_headers_ms,first_event_ms,first_text_ms,ttft_ms,output_tokens,error_code,probed_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",vec![format!("{}:{}",claim.id,claim.attempts).into(),provider.into(),selection.credential.into(),selection.target.upstream_name.into(),selection.endpoint.into(),selection.stream.into(),success.into(),measured.status_code.into(),measured.started.map(|started| started.elapsed().as_millis() as i64).into(),measured.response_headers_ms.into(),measured.first_event_ms.into(),measured.first_text_ms.into(),measured.first_text_ms.into(),measured.output_tokens.into(),error.into(),db::now().into()])).await?;
    tx.commit().await?;
    health(
        state,
        project,
        provider,
        success,
        measured.status_code.map(|code| code as u16),
        error.unwrap_or("probe_failed"),
    )
    .await?;
    if success {
        Ok(())
    } else {
        Err(ApiError::Upstream("probe failed".into()))
    }
}
#[derive(Debug)]
pub(crate) struct NormalizedQuota {
    pub remaining_micros: Option<i64>,
    pub period_start: Option<i64>,
    pub period_end: Option<i64>,
    pub document: Value,
}

fn quota_number(value: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        value.pointer(pointer).and_then(|number| {
            if number.is_number() {
                Some(number.to_string())
            } else {
                number.as_str().map(str::to_owned)
            }
        })
    })
}

pub(crate) fn normalize_quota(
    value: &Value,
    payload: &Value,
    source_url: &str,
) -> Result<NormalizedQuota, ApiError> {
    let mut safe_source_url = reqwest::Url::parse(source_url)
        .map_err(|_| ApiError::BadRequest("invalid quota source URL".into()))?;
    safe_source_url
        .set_password(None)
        .map_err(|_| ApiError::BadRequest("invalid quota source URL".into()))?;
    safe_source_url
        .set_username("")
        .map_err(|_| ApiError::BadRequest("invalid quota source URL".into()))?;
    let scale = payload["scale_micros"].as_i64().unwrap_or(1_000_000);
    let custom = payload["remaining_pointer"].as_str();
    let mut remaining_pointers = vec![
        "/data/limit_remaining",
        "/remaining",
        "/remaining_amount",
        "/quota/remaining",
    ];
    if let Some(pointer) = custom {
        remaining_pointers.insert(0, pointer);
    }
    let direct = quota_number(value, &remaining_pointers)
        .and_then(|amount| decimal_micros(&amount, scale).ok());
    let limit = quota_number(value, &["/limit", "/total", "/usage/limit", "/quota/limit"])
        .and_then(|amount| decimal_micros(&amount, scale).ok());
    let used = quota_number(value, &["/used", "/usage/used", "/quota/used"])
        .and_then(|amount| decimal_micros(&amount, scale).ok());
    let remaining = direct.or_else(|| {
        limit
            .zip(used)
            .map(|(limit, used)| limit.saturating_sub(used))
    });
    let period_start = payload["period_start"]
        .as_i64()
        .or_else(|| value.pointer("/period_start").and_then(Value::as_i64));
    let period_end = payload["period_end"]
        .as_i64()
        .or_else(|| value.pointer("/period_end").and_then(Value::as_i64))
        .or_else(|| value.pointer("/reset_at").and_then(Value::as_i64))
        .or_else(|| value.pointer("/data/reset_at").and_then(Value::as_i64));
    let period = payload["period"]
        .as_str()
        .or_else(|| value.pointer("/period").and_then(Value::as_str))
        .or_else(|| value.pointer("/window").and_then(Value::as_str))
        .unwrap_or("unspecified");
    let measured = remaining.is_some();
    let status = match remaining {
        Some(amount) if amount <= 0 => "exhausted",
        Some(_) => "available",
        None => "unknown",
    };
    Ok(NormalizedQuota {
        remaining_micros: remaining,
        period_start,
        period_end,
        document: json!({"version":1,"measured":measured,"status":status,"period":period,"remaining_micros":remaining,"limit_micros":limit,"period_start":period_start,"period_end":period_end,"source_url":safe_source_url.as_str()}),
    })
}

async fn quota(state: &AppState, claim: &jobs::Claim, payload: &Value) -> Result<(), ApiError> {
    if !crate::operations::settings::load(&state.db)
        .await?
        .quota_collection_enabled
    {
        return Ok(());
    }
    let project = claim.project_id.as_deref().ok_or(ApiError::Forbidden)?;
    let provider = payload["provider_id"].as_str().ok_or(ApiError::NotFound)?;
    let (target, credential, secret) = target(state, project, provider, None).await?;
    let provider_settings = state
        .db
        .query_one(sql(
            "SELECT settings_json FROM providers WHERE id=? AND project_id=?",
            vec![provider.into(), project.into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?
        .try_get::<String>("", "settings_json")?;
    let provider_settings: Value = serde_json::from_str(&provider_settings)
        .map_err(|error| ApiError::Internal(error.into()))?;
    let path = payload["path"]
        .as_str()
        .or_else(|| {
            provider_settings
                .pointer("/quota/path")
                .and_then(Value::as_str)
        })
        .unwrap_or("/api/v1/key");
    if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#']) {
        return Err(ApiError::BadRequest(
            "quota path must stay on configured provider origin".into(),
        ));
    }
    let origin = reqwest::Url::parse(&target.base_url)
        .map_err(|_| ApiError::BadRequest("invalid provider URL".into()))?;
    let url = origin
        .join(path)
        .map_err(|_| ApiError::BadRequest("invalid quota path".into()))?;
    if url.origin() != origin.origin()
        || secret.trim_start().starts_with(['{', '['])
        || matches!(
            target.provider_kind.as_str(),
            "bedrock" | "vertex" | "gcp" | "azure"
        )
    {
        return Err(ApiError::BadRequest("quota collection requires a supported API-key credential and the configured provider origin".into()));
    }
    let request = state.upstream_client(&target).await?.get(url.clone());
    let request = match target.provider_kind.as_str() {
        "anthropic" => request
            .header("x-api-key", &secret)
            .header("anthropic-version", "2023-06-01"),
        "gemini" => request.header("x-goog-api-key", &secret),
        _ => request.bearer_auth(&secret),
    };
    let response = request.timeout(Duration::from_secs(20)).send().await;
    let response = match response {
        Ok(r) if r.status().is_success() => r,
        _ => {
            health(
                state,
                project,
                provider,
                false,
                Some(429),
                "quota_collection_failed",
            )
            .await?;
            return Err(ApiError::Upstream("quota collection failed".into()));
        }
    };
    let value = bounded_json(response).await?;
    let normalized = normalize_quota(&value, payload, url.as_str())?;
    let tx = state.db.begin().await?;
    jobs::fence(&tx, claim).await?;
    let inserted=tx.execute(sql("INSERT INTO provider_quota_snapshots(id,provider_id,credential_id,period_start,period_end,remaining_micros,quota_json,collected_at,sequence) VALUES(?,?,?,?,?,?,?,?,(SELECT COALESCE(MAX(sequence),0)+1 FROM provider_quota_snapshots)) ON CONFLICT(id) DO NOTHING",vec![claim.id.clone().into(),provider.into(),credential.into(),normalized.period_start.into(),normalized.period_end.into(),normalized.remaining_micros.into(),normalized.document.to_string().into(),db::now().into()])).await?.rows_affected();
    if inserted == 0 {
        tx.rollback().await?;
        return Ok(());
    }
    if normalized
        .remaining_micros
        .is_some_and(|remaining| remaining <= 0)
    {
        notify(
            &tx,
            project,
            "quota.exhausted",
            &claim.id,
            &json!({"provider_id":provider,"remaining_micros":normalized.remaining_micros}),
        )
        .await?
    }
    tx.commit().await?;
    Ok(())
}
async fn bounded_json(response: reqwest::Response) -> Result<Value, ApiError> {
    let mut chunks = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| ApiError::Upstream("operation response failed".into()))?;
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(ApiError::Upstream(
                "operation response exceeds 1 MiB".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::Upstream("operation response is not JSON".into()))
}
pub fn decimal_micros(value: &str, scale: i64) -> Result<i64, ApiError> {
    use rust_decimal::{Decimal, prelude::ToPrimitive};
    use std::str::FromStr;
    if !(1..=1_000_000_000).contains(&scale) {
        return Err(ApiError::BadRequest("invalid quota scale".into()));
    }
    Decimal::from_str(value)
        .or_else(|_| Decimal::from_scientific(value))
        .ok()
        .and_then(|amount| amount.checked_mul(Decimal::from(scale)))
        .and_then(|amount| amount.floor().to_i64())
        .ok_or_else(|| ApiError::BadRequest("invalid or overflowing quota amount".into()))
}
pub async fn health(
    state: &AppState,
    project: &str,
    provider: &str,
    success: bool,
    status: Option<u16>,
    error: &str,
) -> Result<(), ApiError> {
    health_for(state, project, provider, None, success, status, error).await
}
pub async fn health_for(
    state: &AppState,
    project: &str,
    provider: &str,
    credential: Option<&str>,
    success: bool,
    status: Option<u16>,
    error: &str,
) -> Result<(), ApiError> {
    let tx = state.db.begin().await?;
    let policy=tx.query_one(sql("SELECT auto_disable_policy_json FROM channel_settings s JOIN providers p ON p.id=s.provider_id WHERE p.id=? AND p.project_id=?",vec![provider.into(),project.into()])).await?;
    let policy: Value = policy
        .map(|r| r.try_get::<String>("", "auto_disable_policy_json"))
        .transpose()?
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| ApiError::Internal(e.into()))?
        .unwrap_or(json!({}));
    let matched = policy["statuses"].as_array().is_none_or(|values| {
        status.is_some_and(|s| values.iter().any(|v| v.as_u64() == Some(s as u64)))
    }) && policy["pattern"]
        .as_str()
        .is_none_or(|p| p.len() <= 512 && regex::Regex::new(p).is_ok_and(|r| r.is_match(error)));
    let counted = !success && matched;
    let row=tx.query_one(sql("INSERT INTO channel_health_state(provider_id,consecutive_failures,backoff_until,reason,updated_at) VALUES(?,?,?,?,?) ON CONFLICT(provider_id) DO UPDATE SET consecutive_failures=CASE WHEN ? THEN channel_health_state.consecutive_failures+1 WHEN ? THEN 0 ELSE channel_health_state.consecutive_failures END,backoff_until=excluded.backoff_until,reason=excluded.reason,updated_at=excluded.updated_at RETURNING consecutive_failures",vec![provider.into(),i64::from(counted).into(),(status==Some(429)&&(policy["enabled"]==true||error=="quota_collection_failed")).then_some(db::now()+30).into(),(!success).then_some(error).into(),db::now().into(),counted.into(),success.into()])).await?.ok_or(ApiError::NotFound)?;
    let mut failures: i64 = row.try_get("", "consecutive_failures")?;
    if let Some(credential) = credential {
        let row=tx.query_one(sql("INSERT INTO credential_health_state(credential_id,consecutive_failures,updated_at) VALUES(?,?,?) ON CONFLICT(credential_id) DO UPDATE SET consecutive_failures=CASE WHEN ? THEN credential_health_state.consecutive_failures+1 WHEN ? THEN 0 ELSE credential_health_state.consecutive_failures END,updated_at=excluded.updated_at RETURNING consecutive_failures",vec![credential.into(),i64::from(counted).into(),db::now().into(),counted.into(),success.into()])).await?.ok_or(ApiError::NotFound)?;
        if policy["action"] == "credential" {
            failures = row.try_get("", "consecutive_failures")?;
        }
    }
    if counted
        && policy["enabled"] == true
        && failures >= policy["threshold"].as_i64().unwrap_or(3).max(1)
    {
        let mut until = db::now()
            + policy["duration_secs"]
                .as_i64()
                .unwrap_or(300)
                .clamp(1, 86400);
        if let Some(expression) = policy["recovery_cron"].as_str() {
            if expression.len() > 128 {
                return Err(ApiError::BadRequest("recovery cron too long".into()));
            }
            let expression = if expression.split_whitespace().count() == 5 {
                format!("0 {expression}")
            } else {
                expression.into()
            };
            let schedule = expression
                .parse::<cron::Schedule>()
                .map_err(|_| ApiError::BadRequest("invalid recovery cron".into()))?;
            let timezone = policy["timezone"]
                .as_str()
                .unwrap_or("UTC")
                .parse::<chrono_tz::Tz>()
                .map_err(|_| ApiError::BadRequest("invalid recovery timezone".into()))?;
            let now = chrono::DateTime::from_timestamp(db::now(), 0)
                .ok_or(ApiError::NotFound)?
                .with_timezone(&timezone);
            until = schedule
                .after(&now)
                .next()
                .ok_or_else(|| ApiError::BadRequest("recovery cron has no future time".into()))?
                .timestamp();
        }
        if policy["action"] == "credential" {
            if let Some(credential) = credential {
                tx.execute(sql(
                    "UPDATE credential_health_state SET disabled_until=? WHERE credential_id=?",
                    vec![until.into(), credential.into()],
                ))
                .await?;
            }
        } else {
            tx.execute(sql(
                "UPDATE channel_health_state SET disabled_until=? WHERE provider_id=?",
                vec![until.into(), provider.into()],
            ))
            .await?;
        }
        notify(
            &tx,
            project,
            "channel.disabled",
            &format!("{provider}:{until}"),
            &json!({"provider_id":provider,"disabled_until":until}),
        )
        .await?;
    }
    let notify_after = policy["notify_after"].as_i64().unwrap_or(3).clamp(1, 1000);
    if counted && failures % notify_after == 0 {
        notify(
            &tx,
            project,
            "request.failed",
            &format!("{provider}:{failures}:{}", db::now() / 60),
            &json!({"provider_id":provider,"consecutive_failures":failures}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn notify(
    db: &impl ConnectionTrait,
    project: &str,
    event: &str,
    operation: &str,
    payload: &Value,
) -> Result<(), ApiError> {
    for row in db
        .query_all(sql(
            "SELECT id,subscriptions_json FROM webhooks WHERE project_id=? AND enabled=1",
            vec![project.into()],
        ))
        .await?
    {
        let subscriptions: Value =
            serde_json::from_str(&row.try_get::<String>("", "subscriptions_json")?)
                .map_err(|e| ApiError::Internal(e.into()))?;
        if !subscriptions["events"]
            .as_array()
            .is_some_and(|events| events.iter().any(|v| v == event))
        {
            continue;
        }
        let target: String = row.try_get("", "id")?;
        let job = jobs::enqueue(
            db,
            Some(project),
            "webhook",
            &format!("webhook:{target}:{operation}"),
            &json!({"webhook_id":target,"event":event,"data":payload}),
            db::now(),
        )
        .await?;
        db.execute(sql("INSERT INTO webhook_deliveries(id,webhook_id,event_type,status,created_at) VALUES(?,?,?,'pending',?) ON CONFLICT(id) DO NOTHING",vec![job.into(),target.into(),event.into(),db::now().into()])).await?;
    }
    Ok(())
}
async fn deliver(state: &AppState, claim: &jobs::Claim, payload: &Value) -> Result<(), ApiError> {
    let row=state.db.query_one(sql("SELECT url,secret_envelope,headers_json,body_template_json,timeout_secs,proxy_preset_id FROM webhooks WHERE id=? AND project_id=? AND enabled=1",vec![payload["webhook_id"].as_str().into(),claim.project_id.clone().into()])).await?.ok_or(ApiError::NotFound)?;
    let url: String = row.try_get("", "url")?;
    let timeout = row.try_get::<i64>("", "timeout_secs")?.clamp(1, 300) as u64;
    let proxy_preset: Option<String> = row.try_get("", "proxy_preset_id")?;
    let resolved_proxy = super::proxy::resolve(state, proxy_preset.as_deref()).await?;
    let client = super::proxy::client(resolved_proxy.as_ref(), Duration::from_secs(timeout))?;
    let mut request = client
        .post(url)
        .header("idempotency-key", &claim.id)
        .timeout(Duration::from_secs(timeout));
    let public_headers: Value = serde_json::from_str(&row.try_get::<String>("", "headers_json")?)
        .map_err(|e| ApiError::Internal(e.into()))?;
    if let Some(headers) = public_headers["headers"].as_object() {
        for (name, value) in headers {
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "host" | "content-length" | "connection" | "transfer-encoding"
            ) {
                return Err(ApiError::BadRequest("forbidden webhook header".into()));
            }
            request = request.header(
                name,
                value
                    .as_str()
                    .ok_or_else(|| ApiError::BadRequest("invalid webhook header".into()))?,
            );
        }
    }
    if let Some(envelope) = row.try_get::<Option<String>>("", "secret_envelope")? {
        let secret: Value = serde_json::from_str(&state.secrets.decrypt(&envelope)?)
            .map_err(|e| ApiError::Internal(e.into()))?;
        if secret["project_id"] != claim.project_id.as_deref().unwrap_or("")
            || secret["webhook_id"] != payload["webhook_id"]
        {
            return Err(ApiError::Forbidden);
        }
        if let Some(headers) = secret["headers"].as_object() {
            for (name, value) in headers {
                request = request.header(name, value.as_str().ok_or(ApiError::Forbidden)?);
            }
        }
    }
    let template: Value = serde_json::from_str(&row.try_get::<String>("", "body_template_json")?)
        .map_err(|e| ApiError::Internal(e.into()))?;
    let body = if let Some(body) = template.get("body") {
        render(body.clone(), payload)
    } else {
        payload.clone()
    };
    let response = request.json(&body).send().await;
    let status = response
        .as_ref()
        .ok()
        .map(|v| i32::from(v.status().as_u16()));
    let success = response.as_ref().is_ok_and(|v| v.status().is_success());
    state.db.execute(sql("UPDATE webhook_deliveries SET status=?,attempt=?,response_status=?,finished_at=?,next_attempt_at=? WHERE id=? AND EXISTS(SELECT 1 FROM operation_jobs WHERE id=? AND owner=? AND fence=? AND lease_until>?)",vec![if success{"succeeded"}else{"failed"}.into(),claim.attempts.into(),status.into(),success.then_some(db::now()).into(),(!success).then_some(db::now()+2i64.pow(claim.attempts.min(10) as u32)).into(),claim.id.clone().into(),claim.id.clone().into(),claim.owner.clone().into(),claim.fence.into(),db::now().into()])).await?;
    if success {
        Ok(())
    } else {
        Err(ApiError::Upstream("webhook delivery failed".into()))
    }
}
fn render(mut value: Value, event: &Value) -> Value {
    match &mut value {
        Value::String(s) if s == "$event" => event.clone(),
        Value::Object(o) => {
            for v in o.values_mut() {
                *v = render(v.take(), event)
            }
            value
        }
        Value::Array(a) => {
            for v in a {
                *v = render(v.take(), event)
            }
            value
        }
        _ => value,
    }
}
pub async fn gc(state: &AppState) -> Result<(), ApiError> {
    let now = db::now();
    let tx = state.db.begin().await?;
    tx.execute(sql(
        "DELETE FROM response_sessions WHERE expires_at<=?",
        vec![now.into()],
    ))
    .await?;
    // Anonymous password failures are valuable threat evidence, but retaining them forever makes
    // the unauthenticated endpoint an unbounded record-system growth vector. Successful login and
    // every authenticated/control-plane audit remain durable; only this narrow class expires.
    tx.execute(sql(
        "DELETE FROM audit_events WHERE actor_user_id IS NULL AND action='login_failed' AND resource_type='authentication' AND json_extract(details,'$.method')='password' AND created_at<?",
        vec![(now - AUTH_FAILURE_AUDIT_RETENTION_DAYS * 86_400).into()],
    ))
    .await?;
    let policies = tx
        .query_all(sql(
            "SELECT project_id,resource_type,retention_days FROM data_retention_policies",
            vec![],
        ))
        .await?;
    let mut policies_for_facts: Vec<(Option<String>, i64)> = vec![];
    for row in policies.iter() {
        let resource: String = row.try_get("", "resource_type")?;
        if resource == "requests" {
            let project: Option<String> = row.try_get("", "project_id")?;
            let days: i64 = row.try_get("", "retention_days")?;
            policies_for_facts.push((project, now - days.saturating_mul(86400)));
        }
    }
    for row in policies {
        let project: Option<String> = row.try_get("", "project_id")?;
        let resource: String = row.try_get("", "resource_type")?;
        let days: i64 = row.try_get("", "retention_days")?;
        let before = now - days.saturating_mul(86400);
        let (table, scope, time) = match resource.as_str() {
            "requests" => (
                "requests",
                "trace_id IN (SELECT id FROM traces WHERE project_id=?)",
                "started_at",
            ),
            "payloads" => (
                "request_contents",
                "request_id IN (SELECT r.id FROM requests r JOIN traces t ON t.id=r.trace_id WHERE t.project_id=?)",
                "(SELECT started_at FROM requests WHERE id=request_contents.request_id)",
            ),
            "probes" => (
                "channel_probes",
                "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                "probed_at",
            ),
            "quota" => (
                "provider_quota_snapshots",
                "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                "collected_at",
            ),
            _ => continue,
        };
        // Quota snapshots govern eligibility; never GC the newest provider/credential fact.
        let preserve = if resource == "quota" {
            " AND id NOT IN (SELECT id FROM provider_quota_snapshots q WHERE NOT EXISTS(SELECT 1 FROM provider_quota_snapshots newer WHERE newer.provider_id=q.provider_id AND newer.credential_id IS q.credential_id AND (newer.sequence>q.sequence OR newer.sequence=q.sequence AND (newer.collected_at>q.collected_at OR newer.collected_at=q.collected_at AND newer.id<q.id))))"
        } else {
            ""
        };
        // A retained trace is the operator's one way to keep a run out of the
        // retention policy. Every row needed to open it stays, and the guard is
        // written against the row the policy is about to delete rather than through
        // a table the same pass is emptying. `requests.id` is `request_facts.id`, so
        // one predicate reaches the request, its captured body and the authoritative
        // ledger alike — for a project-scoped policy and a global one, because the
        // guard is added to the scope instead of replacing it.
        let retained = match resource.as_str() {
            "requests" => {
                " AND NOT EXISTS(SELECT 1 FROM traces t WHERE t.id=requests.trace_id AND t.lifecycle='retained')"
            }
            "payloads" => {
                " AND NOT EXISTS(SELECT 1 FROM requests r JOIN traces t ON t.id=r.trace_id WHERE r.id=request_contents.request_id AND t.lifecycle='retained')"
            }
            _ => "",
        };
        let (scope, args) = if let Some(project) = project {
            (
                format!(" AND ({scope})"),
                vec![before.into(), project.into()],
            )
        } else {
            (String::new(), vec![before.into()])
        };
        tx.execute(sql(
            format!("DELETE FROM {table} WHERE {time}<?{scope}{preserve}{retained}"),
            args,
        ))
        .await?;
    }
    // The requests policy must reach the authoritative ledger too, otherwise "delete after N
    // days" is a promise the instance does not keep. Children (execution_facts -> usage_logs ->
    // usage_cost_items) cascade, and running rows are never touched so in-flight work keeps its
    // reservation. Lifetime counters (api_keys.spent_micros) stay authoritative.
    for row in policies_for_facts {
        let project: Option<String> = row.0;
        let before: i64 = row.1;
        let (scope, args) = if let Some(project) = project {
            (" AND project_id=?", vec![before.into(), project.into()])
        } else {
            ("", vec![before.into()])
        };
        tx.execute(sql(
            format!("DELETE FROM request_facts WHERE started_at<? AND status!='running'{scope} AND NOT EXISTS(SELECT 1 FROM requests r JOIN traces t ON t.id=r.trace_id WHERE r.id=request_facts.id AND t.lifecycle='retained')"),
            args,
        ))
        .await?;
    }
    // Settlement fingerprints only matter while their execution is retained.
    tx.execute(sql(
        "DELETE FROM provider_response_settlements WHERE execution_id NOT IN (SELECT id FROM execution_facts)",
        vec![],
    ))
    .await?;
    // A trace whose requests survived because it is retained must survive with
    // them; the lifecycle is the only thing that keeps it here.
    tx.execute(sql("DELETE FROM traces WHERE lifecycle!='retained' AND finished_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM requests WHERE trace_id=traces.id)",vec![])).await?;
    tx.execute(sql(
        "DELETE FROM threads WHERE NOT EXISTS(SELECT 1 FROM traces WHERE thread_id=threads.id)",
        vec![],
    ))
    .await?;
    tx.commit().await?;
    // No network calls or active transaction while checkpointing/reclaiming storage.
    if !sync_retention(state).await? {
        tracing::warn!("derived retention unavailable; analytics remain degraded");
    }
    state
        .db
        .execute_unprepared("PRAGMA wal_checkpoint(PASSIVE); PRAGMA incremental_vacuum;")
        .await?;
    Ok(())
}
