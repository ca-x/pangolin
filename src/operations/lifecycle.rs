use super::{
    id,
    logging::{self, Level},
    pricing::{self, Price, Usage},
    sql,
};
use crate::{
    api::{ApiError, AppState},
    db,
    models::ApiKeyCredential,
    observability::RequestEvent,
    orchestration::{AttemptGuard, AttemptOutcome, Candidate},
};
use http::HeaderMap;
use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Value, json};
use std::{sync::Arc, time::Instant};

struct Context {
    state: AppState,
    id: String,
    trace_id: String,
    external_request: String,
    external_trace: String,
    key: ApiKeyCredential,
    profile_id: Option<String>,
    level: Level,
    endpoint: String,
    model: String,
    ratio: i64,
    started_at: i64,
    request_json: Option<String>,
    affinity: Option<crate::orchestration::affinity::Binding>,
    observation_generation: u64,
}
impl Drop for Context {
    fn drop(&mut self) {
        let db = self.state.db.clone();
        let request = self.id.clone();
        let trace = self.trace_id.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
   let result:Result<(),ApiError>=async {
    let tx=db.begin().await?;
    tx.execute(sql("UPDATE request_facts SET status='cancelled',finished_at=? WHERE id=? AND status='running' AND NOT EXISTS(SELECT 1 FROM execution_facts WHERE request_id=? AND status='running')",vec![db::now().into(),request.clone().into(),request.clone().into()])).await?;
    tx.execute(sql("UPDATE requests SET status=(SELECT status FROM request_facts WHERE id=?),finished_at=? WHERE id=? AND status='running' AND EXISTS(SELECT 1 FROM request_facts WHERE id=? AND status!='running')",vec![request.clone().into(),db::now().into(),request.clone().into(),request.into()])).await?;
    tx.execute(sql("UPDATE traces SET status='finished',finished_at=? WHERE id=? AND status='running' AND NOT EXISTS(SELECT 1 FROM requests WHERE trace_id=? AND status='running')",vec![db::now().into(),trace.clone().into(),trace.into()])).await?;
    tx.commit().await?;Ok(())
   }.await;if result.is_err(){tracing::warn!("request finalization deferred to recovery")}
  });
        }
    }
}
pub struct Request {
    context: Arc<Context>,
}
impl Request {
    pub fn record_event(&self, event: RequestEvent) {
        if self.logs_enabled() {
            self.context
                .state
                .observations
                .record_at(event, self.context.observation_generation)
        }
    }
    pub fn logs_enabled(&self) -> bool {
        self.context.level != Level::Off
    }
    pub async fn complete_local(&self, response: &Value) -> Result<(), ApiError> {
        let ctx = &self.context;
        let tx = ctx.state.db.begin().await?;
        for table in ["request_facts", "requests"] {
            tx.execute(sql(
                format!("UPDATE {table} SET status='succeeded',finished_at=? WHERE id=?"),
                vec![db::now().into(), ctx.id.clone().into()],
            ))
            .await?;
        }
        tx.execute(sql("UPDATE traces SET status='succeeded',finished_at=? WHERE id=? AND NOT EXISTS(SELECT 1 FROM requests WHERE trace_id=? AND status='running')",vec![db::now().into(),ctx.trace_id.clone().into(),ctx.trace_id.clone().into()])).await?;
        tx.execute(sql(
            "UPDATE request_contents SET response_json=? WHERE request_id=?",
            vec![ctx.level.body(response).into(), ctx.id.clone().into()],
        ))
        .await?;
        tx.commit().await?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn begin(
        state: &AppState,
        key: &ApiKeyCredential,
        headers: &HeaderMap,
        request_id: &str,
        trace_id: &str,
        endpoint: &str,
        model: &str,
        payload: &Value,
        affinity: Option<crate::orchestration::affinity::Binding>,
    ) -> Result<Self, ApiError> {
        let level = logging::resolve(&state.db, &key.id).await?;
        let profile_id = state
            .db
            .query_one(sql(
                "SELECT profile_id FROM api_keys WHERE id=?",
                vec![key.id.clone().into()],
            ))
            .await?
            .ok_or(ApiError::Unauthorized)?
            .try_get::<Option<String>>("", "profile_id")?;
        let request = id();
        let now = db::now();
        let group=state.db.query_one(sql("SELECT g.ratio_millionths,g.enabled FROM service_group_keys k JOIN service_groups g ON g.id=k.group_id WHERE k.api_key_id=? AND k.project_id=?",vec![key.id.clone().into(),key.project_id.clone().into()])).await?;
        let ratio = if let Some(row) = group {
            if !row.try_get::<bool>("", "enabled")? {
                return Err(ApiError::Forbidden);
            }
            row.try_get("", "ratio_millionths")?
        } else {
            1_000_000
        };
        let trace = scope_id(&key.project_id, &key.id, trace_id);
        let transaction = state.db.begin().await?;
        transaction.execute(sql("INSERT INTO request_facts(id,project_id,api_key_id,profile_id,user_id,log_level,started_at,ratio_millionths) VALUES(?,?,?,?,?,?,?,?)",vec![request.clone().into(),key.project_id.clone().into(),key.id.clone().into(),profile_id.clone().into(),key.user_id.clone().into(),level.name().into(),now.into(),ratio.into()])).await?;
        let request_json = level.body(payload);
        if level != Level::Off {
            let external_thread = headers
                .get("x-thread-id")
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty() && s.len() <= 128);
            let thread = external_thread.map(|s| scope_id(&key.project_id, &key.id, s));
            if let Some(thread) = &thread {
                transaction.execute(sql("INSERT INTO threads(id,project_id,api_key_id,user_id,external_id,created_at,updated_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET updated_at=excluded.updated_at",vec![thread.clone().into(),key.project_id.clone().into(),key.id.clone().into(),key.user_id.clone().into(),external_thread.into(),now.into(),now.into()])).await?;
            }
            transaction.execute(sql("INSERT INTO traces(id,thread_id,project_id,api_key_id,user_id,external_id,started_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET status='running',finished_at=NULL",vec![trace.clone().into(),thread.into(),key.project_id.clone().into(),key.id.clone().into(),key.user_id.clone().into(),trace_id.into(),now.into()])).await?;
            transaction.execute(sql("INSERT INTO requests(id,trace_id,protocol,endpoint,requested_model,source_ip,status,request_metadata_json,started_at) VALUES(?,?,?,?,?,?,'running',?,?)",vec![request.clone().into(),trace.clone().into(),crate::providers::capability(endpoint).into(),endpoint.into(),model.into(),crate::api::trusted_client_ip(headers).map(|v|v.to_string()).into(),json!({"version":1,"external_id":request_id,"log_level":level.name()}).to_string().into(),now.into()])).await?;
            transaction
                .execute(sql(
                    "INSERT INTO request_contents(request_id,request_json) VALUES(?,?)",
                    vec![request.clone().into(), request_json.clone().into()],
                ))
                .await?;
        }
        transaction.commit().await?;
        Ok(Self {
            context: Arc::new(Context {
                state: state.clone(),
                id: request,
                trace_id: trace,
                external_request: request_id.into(),
                external_trace: trace_id.into(),
                key: key.clone(),
                profile_id,
                level,
                endpoint: endpoint.into(),
                model: model.into(),
                ratio,
                started_at: now,
                request_json,
                affinity,
                observation_generation: state.observations.generation(),
            }),
        })
    }
    pub async fn attempt(
        &self,
        candidate: &Candidate,
        guard: AttemptGuard,
        number: usize,
        tokens: u32,
        payload: &Value,
        billable: bool,
    ) -> Result<Attempt, ApiError> {
        let ctx = &self.context;
        let mut price = pricing::snapshot(&ctx.state.db, candidate, ctx.ratio).await?;
        if !billable {
            price.components.clear();
        }
        let hard=ctx.key.budget_micros.is_some()||ctx.profile_id.is_some()&&ctx.state.db.query_one(sql("SELECT 1 AS present FROM api_key_profiles WHERE id=? AND budget_micros IS NOT NULL",vec![ctx.profile_id.clone().into()])).await?.is_some();
        let bound = if !billable {
            0
        } else if hard {
            price.upper_bound(tokens, payload, &ctx.endpoint)?
        } else {
            price
                .upper_bound(tokens, payload, &ctx.endpoint)
                .unwrap_or(0)
        };
        let execution = id();
        let transaction = ctx.state.db.begin().await?;
        // This first write acquires SQLite's writer lock before reading shared budgets.
        transaction.execute(sql("INSERT INTO execution_facts(id,request_id,provider_id,model_id,credential_id,attempt,price_json,reserved_micros,started_at) VALUES(?,?,?,?,?,?,?,?,?)",vec![execution.clone().into(),ctx.id.clone().into(),candidate.provider_id.clone().into(),candidate.model_id.clone().into(),candidate.credential_id.clone().into(),(number as i64).into(),serde_json::to_string(&price).map_err(|e|ApiError::Internal(e.into()))?.into(),bound.into(),db::now().into()])).await?;
        let allowed=transaction.query_one(sql("SELECT k.id FROM api_keys k WHERE k.id=? AND k.enabled=1 AND (k.budget_micros IS NULL OR k.spent_micros+(SELECT COALESCE(SUM(e.reserved_micros),0) FROM execution_facts e JOIN request_facts r ON r.id=e.request_id WHERE r.api_key_id=k.id AND e.status='running')<=k.budget_micros) AND (k.profile_id IS NULL OR NOT EXISTS(SELECT 1 FROM api_key_profiles p WHERE p.id=k.profile_id AND p.budget_micros IS NOT NULL AND (SELECT COALESCE(SUM(spent_micros),0) FROM api_keys WHERE profile_id=p.id)+(SELECT COALESCE(SUM(e.reserved_micros),0) FROM execution_facts e JOIN request_facts r ON r.id=e.request_id WHERE r.profile_id=p.id AND e.status='running')>p.budget_micros))",vec![ctx.key.id.clone().into()])).await?;
        if allowed.is_none() {
            return Err(ApiError::RateLimited(
                "budget reservation exceeds remaining balance".into(),
            ));
        }
        transaction.execute(sql("UPDATE execution_facts SET config_json=? WHERE id=?",vec![json!({"version":1,"provider_id":candidate.provider_id,"credential_id":candidate.credential_id,"provider_kind":candidate.target.provider_kind,"model":candidate.target.upstream_name,"endpoint":candidate.endpoint}).to_string().into(),execution.clone().into()])).await?;
        transaction
            .execute(sql(
                "UPDATE request_facts SET status='running',finished_at=NULL WHERE id=?",
                vec![ctx.id.clone().into()],
            ))
            .await?;
        if ctx.level != Level::Off {
            transaction
                .execute(sql(
                    "UPDATE requests SET status='running',finished_at=NULL WHERE id=?",
                    vec![ctx.id.clone().into()],
                ))
                .await?;
            transaction
                .execute(sql(
                    "UPDATE traces SET status='running',finished_at=NULL WHERE id=?",
                    vec![ctx.trace_id.clone().into()],
                ))
                .await?;
        }
        if ctx.level != Level::Off {
            transaction.execute(sql("INSERT INTO request_executions(id,request_id,provider_id,credential_id,attempt,model,status,credential_suffix,started_at) SELECT ?,?,?,?,?,?,'running',suffix,? FROM channel_credentials WHERE id=?",vec![execution.clone().into(),ctx.id.clone().into(),candidate.provider_id.clone().into(),candidate.credential_id.clone().into(),(number as i64).into(),candidate.target.upstream_name.clone().into(),db::now().into(),candidate.credential_id.clone().into()])).await?;
        }
        transaction.commit().await?;
        Ok(Attempt {
            context: ctx.clone(),
            id: execution,
            guard: Some(guard),
            price,
            usage: Usage::default(),
            provider: candidate.target.provider_name.clone(),
            provider_id: candidate.provider_id.clone(),
            credential_id: candidate.credential_id.clone(),
            candidate_id: candidate.id(),
            model: candidate.target.upstream_name.clone(),
            started: Instant::now(),
            ttft: None,
            body: None,
            settled: false,
            recover_on_drop: true,
            reserved: bound,
            contacted: false,
            http_status: None,
            response_id: None,
        })
    }
}
fn scope_id(project: &str, key: &str, value: &str) -> String {
    blake3::hash(json!([project, key, value]).to_string().as_bytes())
        .to_hex()
        .to_string()
}

pub struct Attempt {
    context: Arc<Context>,
    id: String,
    guard: Option<AttemptGuard>,
    price: Price,
    pub usage: Usage,
    provider: String,
    provider_id: String,
    credential_id: String,
    candidate_id: String,
    model: String,
    started: Instant,
    ttft: Option<i64>,
    body: Option<String>,
    settled: bool,
    recover_on_drop: bool,
    reserved: i64,
    contacted: bool,
    http_status: Option<u16>,
    response_id: Option<String>,
}
impl Attempt {
    pub async fn rejection(&mut self, status: u16) -> Result<(), ApiError> {
        self.http_status = Some(status);
        self.price.components.clear();
        self.reserved = 0;
        self.usage.reported = true;
        self.context
            .state
            .db
            .execute(sql(
                "UPDATE execution_facts SET reserved_micros=0 WHERE id=? AND status='running'",
                vec![self.id.clone().into()],
            ))
            .await?;
        Ok(())
    }
    pub async fn contacted(&mut self) -> Result<(), ApiError> {
        self.context
            .state
            .db
            .execute(sql(
                "UPDATE execution_facts SET contacted=1 WHERE id=? AND status='running'",
                vec![self.id.clone().into()],
            ))
            .await?;
        self.contacted = true;
        Ok(())
    }
    pub fn response(&mut self, value: &Value) {
        self.capture_response_id(value);
        self.usage.merge(Usage::parse_for(
            value,
            self.context.endpoint == "/v1/messages",
        ));
        self.body = self.context.level.body(value);
    }
    pub fn media(&mut self, endpoint: &str, payload: &Value) {
        let priced = self
            .price
            .components
            .iter()
            .all(|c| matches!(c.kind.as_str(), "flat" | "unit"));
        if let Ok(units) = pricing::media_units(endpoint, payload) {
            self.usage.units = units;
            self.usage.reported |= priced;
        } else if self.price.components.iter().all(|c| c.kind == "flat") {
            self.usage.reported = true;
        }
    }
    pub fn stream_event(&mut self, value: &Value) {
        self.capture_response_id(value);
        self.usage
            .merge_event(value, self.context.endpoint == "/v1/messages");
        if let Some(event) = self.context.level.body(value) {
            let body = self.body.get_or_insert_default();
            if body.len() + event.len() < 1024 * 1024 {
                body.push_str(&event);
                body.push('\n');
            }
        }
    }
    pub fn first_byte(&mut self) {
        self.ttft = Some(self.started.elapsed().as_millis() as i64)
    }
    fn capture_response_id(&mut self, value: &Value) {
        if let Some(id) = value
            .pointer("/response/id")
            .or_else(|| value.pointer("/message/id"))
            .or_else(|| value.get("responseId"))
            .or_else(|| {
                (value.get("usage").is_some()
                    || value.get("choices").is_some()
                    || value.get("status").is_some())
                .then(|| value.get("id"))
                .flatten()
            })
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() <= 512)
        {
            self.response_id = Some(id.to_owned());
        }
    }
    pub async fn finish(&mut self, outcome: AttemptOutcome) -> Result<(), ApiError> {
        let status = match outcome {
            AttemptOutcome::Success => "succeeded",
            AttemptOutcome::UpstreamFailure => "failed",
            AttemptOutcome::LocalFailure => "local_failure",
        };
        self.settle(status).await?;
        if let Some(binding) = &self.context.affinity {
            self.context
                .state
                .orchestrator
                .affinity_rules
                .finish(
                    binding,
                    &self.candidate_id,
                    matches!(outcome, AttemptOutcome::Success),
                )
                .await;
        }
        let health = if !matches!(outcome, AttemptOutcome::LocalFailure) {
            super::runtime::health_for(
                &self.context.state,
                &self.context.key.project_id,
                &self.provider_id,
                Some(&self.credential_id),
                matches!(outcome, AttemptOutcome::Success),
                self.http_status,
                status,
            )
            .await
        } else {
            Ok(())
        };
        // Start recovery timing only once durable settlement and operational
        // state are complete; no await follows the terminal permit transition.
        if let Some(guard) = &mut self.guard {
            guard.finish(outcome)
        }
        health
    }
    async fn settle(&mut self, status: &str) -> Result<(), ApiError> {
        if self.settled {
            return Ok(());
        }
        let ctx = &self.context;
        let (mut cost, mut items) = if self.usage.reported || status == "succeeded" {
            match self.price.calculate(&self.usage) {
                Ok(value) => value,
                Err(_) => {
                    self.usage.reported = false;
                    (0, vec![])
                }
            }
        } else {
            (0, vec![])
        };
        // A lost terminal usage report cannot release a hard-budget reservation. Keep
        // an explicit conservative settlement for cancellation, transport loss or crash.
        if !self.usage.reported && self.contacted && self.reserved > 0 {
            cost = self.reserved;
            items.clear();
        }
        let txn = ctx.state.db.begin().await?;
        let changed = txn
            .execute(sql(
                "UPDATE execution_facts SET status=?,finished_at=? WHERE id=? AND status='running'",
                vec![status.into(), db::now().into(), self.id.clone().into()],
            ))
            .await?
            .rows_affected();
        if changed == 0 {
            txn.rollback().await?;
            self.settled = true;
            return Ok(());
        }
        let usage_id = id();
        let mut kind = if self.usage.reported {
            "reported"
        } else if self.contacted && self.reserved > 0 {
            "conservative"
        } else {
            "unreported"
        };
        if status == "succeeded"
            && let Some(response) = &self.response_id
        {
            let fingerprint = blake3::hash(
                json!([
                    ctx.key.project_id,
                    ctx.key.id,
                    self.provider_id,
                    ctx.endpoint,
                    response
                ])
                .to_string()
                .as_bytes(),
            )
            .to_hex()
            .to_string();
            if txn.execute(sql("INSERT INTO provider_response_settlements(fingerprint,project_id,execution_id) VALUES(?,?,?) ON CONFLICT(fingerprint) DO NOTHING",vec![fingerprint.into(),ctx.key.project_id.clone().into(),self.id.clone().into()])).await?.rows_affected()==0 {cost=0;items.clear();kind="duplicate";}
        }
        let mut uncached = self.usage.clone();
        uncached.cache_read = 0;
        uncached.cache_write = 0;
        uncached.cache_write_1h = 0;
        let savings = if self.usage.reported && kind != "duplicate" {
            self.price
                .calculate(&uncached)?
                .0
                .saturating_sub(cost)
                .max(0)
        } else {
            0
        };
        txn.execute(sql("INSERT INTO usage_logs(id,execution_id,model_id,price_id,input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,request_units,total_cost_micros,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",vec![usage_id.clone().into(),self.id.clone().into(),self.price.model_id.clone().into(),self.price.id.clone().into(),self.usage.input.into(),self.usage.output.into(),self.usage.cache_read.into(),self.usage.cache_write.into(),self.usage.reasoning.into(),self.usage.units.into(),cost.into(),db::now().into()])).await?;
        txn.execute(sql(
            "UPDATE usage_logs SET settlement_kind=?,cache_savings_micros=?,image_input_tokens=?,image_output_tokens=? WHERE id=?",
            vec![kind.into(), savings.into(),self.usage.image_input.into(),self.usage.image_output.into(), usage_id.clone().into()],
        ))
        .await?;
        if kind == "conservative" {
            txn.execute(sql("INSERT INTO usage_cost_items(id,usage_log_id,quantity,subtotal_micros) VALUES(?,?,1,?)",vec![id().into(),usage_id.clone().into(),cost.into()])).await?;
        }
        for item in items {
            txn.execute(sql("INSERT INTO usage_cost_items(id,usage_log_id,price_component_id,quantity,subtotal_micros) VALUES(?,?,?,?,?)",vec![id().into(),usage_id.clone().into(),(!item.component_id.is_empty()).then_some(item.component_id).into(),item.quantity.into(),item.subtotal_micros.into()])).await?;
        }
        if cost > 0 {
            txn.execute(sql(
                "UPDATE api_keys SET spent_micros=spent_micros+? WHERE id=?",
                vec![cost.into(), ctx.key.id.clone().into()],
            ))
            .await?;
        }
        txn.execute(sql(
            "UPDATE request_facts SET status=?,finished_at=? WHERE id=?",
            vec![status.into(), db::now().into(), ctx.id.clone().into()],
        ))
        .await?;
        let latency = self.started.elapsed().as_millis() as i64;
        if ctx.level != Level::Off {
            txn.execute(sql("UPDATE request_executions SET status=?,finished_at=?,latency_ms=?,first_token_at=?,retry_reason=? WHERE id=?",vec![status.into(),db::now().into(),latency.into(),self.ttft.map(|ms|ctx.started_at*1000+ms).into(),(status!="succeeded").then_some(status.to_owned()).into(),self.id.clone().into()])).await?;
            txn.execute(sql(
                "UPDATE requests SET status=?,finished_at=? WHERE id=?",
                vec![status.into(), db::now().into(), ctx.id.clone().into()],
            ))
            .await?;
            txn.execute(sql("UPDATE traces SET status=?,finished_at=? WHERE id=? AND NOT EXISTS(SELECT 1 FROM requests WHERE trace_id=? AND status='running')",vec![status.into(),db::now().into(),ctx.trace_id.clone().into(),ctx.trace_id.clone().into()])).await?;
            txn.execute(sql(
                "UPDATE request_contents SET response_json=? WHERE request_id=?",
                vec![self.body.clone().into(), ctx.id.clone().into()],
            ))
            .await?;
        }
        txn.commit().await?;
        self.settled = true;
        if ctx.level != Level::Off {
            ctx.state.observations.record_at(
                RequestEvent {
                    request_id: ctx.external_request.clone(),
                    trace_id: ctx.external_trace.clone(),
                    started_at: ctx.started_at,
                    finished_at: db::now(),
                    endpoint: ctx.endpoint.clone(),
                    api_key_id: Some(ctx.key.id.clone()),
                    provider: Some(self.provider.clone()),
                    requested_model: Some(ctx.model.clone()),
                    resolved_model: Some(self.model.clone()),
                    status_code: if status == "succeeded" { 200 } else { 502 },
                    error_kind: (status != "succeeded" || !self.usage.reported).then(|| {
                        if !self.usage.reported {
                            "usage_unavailable".into()
                        } else {
                            status.into()
                        }
                    }),
                    latency_ms: latency,
                    ttft_ms: self.ttft,
                    input_tokens: self.usage.input,
                    output_tokens: self.usage.output,
                    cached_tokens: self.usage.cache_read,
                    cost_micros: cost,
                    payload_captured: ctx.request_json.is_some(),
                    request_json: ctx.request_json.clone(),
                    response_json: self.body.clone(),
                },
                ctx.observation_generation,
            );
        }
        Ok(())
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if self.settled || !self.recover_on_drop {
            return;
        }
        let mut cancelled = Self {
            context: self.context.clone(),
            id: self.id.clone(),
            guard: self.guard.take(),
            price: self.price.clone(),
            usage: self.usage.clone(),
            provider: self.provider.clone(),
            provider_id: self.provider_id.clone(),
            credential_id: self.credential_id.clone(),
            candidate_id: self.candidate_id.clone(),
            model: self.model.clone(),
            started: self.started,
            ttft: self.ttft,
            body: self.body.take(),
            settled: false,
            recover_on_drop: false,
            reserved: self.reserved,
            contacted: self.contacted,
            http_status: self.http_status,
            response_id: self.response_id.clone(),
        };
        self.settled = true;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if cancelled.settle("cancelled").await.is_err() {
                    tracing::error!("cancelled execution pending durable recovery");
                }
                cancelled.settled = true;
                drop(cancelled);
            });
        } else {
            cancelled.settled = true;
        }
    }
}
