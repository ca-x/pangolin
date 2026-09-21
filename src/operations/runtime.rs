use super::{backup, id, jobs, sql};
use crate::{
    api::{ApiError, AppState},
    db,
};
use futures_util::StreamExt;
use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

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
                crate::catalog::refresh::refresh(&state.db, source).await?;
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
        tx.execute(sql("UPDATE request_executions SET status='interrupted',retry_reason='process_restart',finished_at=? WHERE id=?",vec![db::now().into(),execution.into()])).await?;
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
    let mut rules = vec![crate::observability::Retention {
        project_id: None,
        days: state.config.observation_retention_days.min(i64::MAX as u64) as i64,
        payloads_only: false,
    }];
    for row in state.db.query_all(sql("SELECT project_id,resource_type,retention_days FROM data_retention_policies WHERE resource_type IN ('requests','payloads')",vec![])).await? {
        rules.push(crate::observability::Retention {
            project_id: row.try_get("", "project_id")?,
            days: row.try_get("", "retention_days")?,
            payloads_only: row.try_get::<String>("", "resource_type")? == "payloads",
        });
    }
    Ok(state.observations.apply_retention(rules).await)
}
async fn target(
    state: &AppState,
    project: &str,
    provider: &str,
) -> Result<(crate::models::RouteTarget, String, String), ApiError> {
    let row=state.db.query_one(sql("SELECT p.name,p.kind,p.base_url,c.id AS credential_id,c.secret_envelope,m.public_name,m.upstream_name FROM providers p JOIN channel_credentials c ON c.provider_id=p.id AND c.enabled=1 JOIN models m ON m.provider_id=p.id AND m.enabled=1 WHERE p.id=? AND p.project_id=? AND p.enabled=1 ORDER BY c.priority,m.priority LIMIT 1",vec![provider.into(),project.into()])).await?.ok_or(ApiError::NotFound)?;
    let credential = row.try_get("", "credential_id")?;
    let secret: String = row.try_get("", "secret_envelope")?;
    Ok((
        crate::models::RouteTarget {
            public_name: row.try_get("", "public_name")?,
            upstream_name: row.try_get("", "upstream_name")?,
            provider_name: row.try_get("", "name")?,
            provider_kind: row.try_get("", "kind")?,
            base_url: row.try_get("", "base_url")?,
            secret_envelope: secret.clone(),
            input_price_micros: 0,
            output_price_micros: 0,
        },
        credential,
        state.secrets.decrypt(&secret)?.to_string(),
    ))
}
async fn probe(state: &AppState, claim: &jobs::Claim, payload: &Value) -> Result<(), ApiError> {
    let project = claim.project_id.as_deref().ok_or(ApiError::Forbidden)?;
    let provider = payload["provider_id"].as_str().ok_or(ApiError::NotFound)?;
    let (target, credential, secret) = target(state, project, provider).await?;
    let endpoint = match target.provider_kind.as_str() {
        "anthropic" => "/v1/messages",
        "gemini" => "/v1beta/models:generateContent",
        _ => "/v1/chat/completions",
    };
    let request = match target.provider_kind.as_str() {
        "gemini" => {
            json!({"model":target.upstream_name,"contents":[{"role":"user","parts":[{"text":"Reply OK"}]}],"generationConfig":{"maxOutputTokens":8}})
        }
        _ => {
            json!({"model":target.upstream_name,"messages":[{"role":"user","content":"Reply OK"}],"max_tokens":8})
        }
    };
    let prepared = crate::providers::prepare_routed(
        &target,
        crate::providers::Route {
            protocol: endpoint,
            path: endpoint,
            native_version: None,
        },
        &request,
        &secret,
        http::HeaderMap::new(),
        &http::HeaderMap::new(),
    )
    .await?;
    let started = Instant::now();
    let response = state
        .oidc_client
        .post(prepared.url)
        .headers(prepared.headers)
        .json(&prepared.payload)
        .timeout(Duration::from_secs(30))
        .send()
        .await;
    let code = response
        .as_ref()
        .ok()
        .map(|r| i32::from(r.status().as_u16()));
    let mut success = response.as_ref().is_ok_and(|r| r.status().is_success());
    let body = if let Ok(response) = response {
        bounded_json(response).await.ok()
    } else {
        None
    };
    success &= body
        .as_ref()
        .is_some_and(|v| v.get("error").is_none() && v.is_object());
    let output = body
        .as_ref()
        .map(|v| super::pricing::Usage::parse(v).output);
    let elapsed = started.elapsed().as_millis() as i64;
    let tx = state.db.begin().await?;
    jobs::fence(&tx, claim).await?;
    tx.execute(sql("INSERT INTO channel_probes(id,provider_id,credential_id,model,success,status_code,latency_ms,output_tokens,error_code,probed_at) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",vec![format!("{}:{}",claim.id,claim.attempts).into(),provider.into(),credential.into(),target.upstream_name.into(),success.into(),code.into(),elapsed.into(),output.into(),(!success).then_some("probe_failed").into(),db::now().into()])).await?;
    tx.commit().await?;
    health(
        state,
        project,
        provider,
        success,
        code.map(|v| v as u16),
        "probe_failed",
    )
    .await?;
    if success {
        Ok(())
    } else {
        Err(ApiError::Upstream("probe failed".into()))
    }
}
async fn quota(state: &AppState, claim: &jobs::Claim, payload: &Value) -> Result<(), ApiError> {
    let project = claim.project_id.as_deref().ok_or(ApiError::Forbidden)?;
    let provider = payload["provider_id"].as_str().ok_or(ApiError::NotFound)?;
    let (target, credential, secret) = target(state, project, provider).await?;
    let path = payload["path"].as_str().unwrap_or("/api/v1/key");
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
    let request = state.oidc_client.get(url);
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
    let pointer = payload["remaining_pointer"]
        .as_str()
        .unwrap_or("/data/limit_remaining");
    let number = value
        .pointer(pointer)
        .filter(|v| v.is_number() || v.is_string())
        .ok_or_else(|| ApiError::Upstream("quota amount missing".into()))?;
    let scale = payload["scale_micros"].as_i64().unwrap_or(1_000_000);
    let amount = number
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| number.to_string());
    let remaining = decimal_micros(&amount, scale)?;
    let tx = state.db.begin().await?;
    jobs::fence(&tx, claim).await?;
    let inserted=tx.execute(sql("INSERT INTO provider_quota_snapshots(id,provider_id,credential_id,period_start,period_end,remaining_micros,quota_json,collected_at,sequence) VALUES(?,?,?,?,?,?,?,?,(SELECT COALESCE(MAX(sequence),0)+1 FROM provider_quota_snapshots)) ON CONFLICT(id) DO NOTHING",vec![claim.id.clone().into(),provider.into(),credential.into(),payload["period_start"].as_i64().into(),payload["period_end"].as_i64().into(),remaining.into(),json!({"version":1,"remaining_micros":remaining}).to_string().into(),db::now().into()])).await?.rows_affected();
    if inserted == 0 {
        tx.rollback().await?;
        return Ok(());
    }
    if remaining <= 0 {
        notify(
            &tx,
            project,
            "quota.exhausted",
            &claim.id,
            &json!({"provider_id":provider,"remaining_micros":remaining}),
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
    let row=state.db.query_one(sql("SELECT url,secret_envelope,headers_json,body_template_json FROM webhooks WHERE id=? AND project_id=? AND enabled=1",vec![payload["webhook_id"].as_str().into(),claim.project_id.clone().into()])).await?.ok_or(ApiError::NotFound)?;
    let url: String = row.try_get("", "url")?;
    let mut request = state
        .oidc_client
        .post(url)
        .header("idempotency-key", &claim.id)
        .timeout(Duration::from_secs(20));
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
    let policies=tx.query_all(sql("SELECT project_id,resource_type,retention_days,retain_payloads FROM data_retention_policies",vec![])).await?;
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
        let (scope, args) = if let Some(project) = project {
            (
                format!(" AND ({scope})"),
                vec![before.into(), project.into()],
            )
        } else {
            (String::new(), vec![before.into()])
        };
        tx.execute(sql(
            format!("DELETE FROM {table} WHERE {time}<?{scope}{preserve}"),
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
            format!("DELETE FROM request_facts WHERE started_at<? AND status!='running'{scope}"),
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
    tx.execute(sql("DELETE FROM traces WHERE finished_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM requests WHERE trace_id=traces.id)",vec![])).await?;
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
