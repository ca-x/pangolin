use super::*;
use crate::operations::{self, backup, id, jobs, logging, pricing, sql, storage};
use sea_orm::{ConnectionTrait, TransactionTrait};
use serde::{Deserialize, Serialize};

pub(super) fn router(state: AppState) -> Router<AppState> {
    let instance = Router::new()
        .route("/api/admin/v1/instance/backup", post(instance_export))
        .route(
            "/api/admin/v1/instance/restore/preflight",
            post(instance_preflight),
        )
        .route("/api/admin/v1/instance/restore", post(instance_restore))
        .layer(axum::extract::DefaultBodyLimit::max(
            operations::instance_backup::MAX_ARTIFACT_BYTES,
        ))
        .layer(middleware::from_fn_with_state(state, instance_owner));
    Router::new()
        .route(
            "/api/admin/v1/settings/request-logging",
            get(log_policy).put(set_log_policy),
        )
        .route(
            "/api/admin/v1/settings/system",
            get(system_settings).put(set_system_settings),
        )
        .route(
            "/api/admin/v1/projects/{project}/operations/{resource}",
            get(list).post(mutate),
        )
        .route(
            "/api/admin/v1/projects/{project}/operations/{resource}/{id}",
            get(detail).delete(remove),
        )
        .route("/api/admin/v1/projects/{project}/analytics", get(analytics))
        .route(
            "/api/admin/v1/projects/{project}/settings/orchestration",
            get(orchestration_settings).put(set_orchestration_settings),
        )
        .route(
            "/api/admin/v1/projects/{project}/observability/summary",
            get(observation_summary),
        )
        .route(
            "/api/admin/v1/projects/{project}/observability/requests",
            get(observation_list),
        )
        .route(
            "/api/admin/v1/projects/{project}/observability/requests/{id}",
            get(observation_detail),
        )
        .route(
            "/api/admin/v1/projects/{project}/routing-preview",
            post(routing_preview),
        )
        .route(
            "/api/admin/v1/projects/{project}/backup/export",
            post(export),
        )
        .route(
            "/api/admin/v1/projects/{project}/backup/restore",
            post(restore),
        )
        .route(
            "/api/admin/v1/projects/{project}/backup/run",
            post(run_backup),
        )
        .route(
            "/api/admin/v1/projects/{project}/backup/{job}/retry",
            post(retry_backup),
        )
        .layer(axum::extract::DefaultBodyLimit::max(48 * 1024 * 1024))
        .merge(instance)
}
async fn instance_owner(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, ApiError> {
    actor(&state, request.headers(), None, true).await?;
    Ok(next.run(request).await)
}
async fn instance_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<operations::instance_backup::Export>,
) -> Result<Json<operations::instance_backup::Artifact>, ApiError> {
    let actor = actor(&state, &headers, None, true).await?;
    Ok(Json(
        operations::instance_backup::export(&state, &actor, input).await?,
    ))
}
async fn instance_preflight(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<operations::instance_backup::Restore>,
) -> Result<Json<operations::instance_backup::Preflight>, ApiError> {
    let actor = actor(&state, &headers, None, true).await?;
    Ok(Json(
        operations::instance_backup::preflight(&state, &actor, input).await?,
    ))
}
async fn instance_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<operations::instance_backup::Restore>,
) -> Result<Response, ApiError> {
    let actor = actor(&state, &headers, None, true).await?;
    let result = operations::instance_backup::restore(&state, &actor, input).await?;
    Ok((
        [(
            header::SET_COOKIE,
            "pangolin_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        )],
        Json(result),
    )
        .into_response())
}
pub(super) async fn actor(
    state: &AppState,
    headers: &HeaderMap,
    project: Option<&str>,
    write: bool,
) -> Result<crate::access::Principal, ApiError> {
    let user = crate::access_api::principal(state, headers).await?;
    crate::access::authorize(
        &state.db,
        &user,
        project,
        if project.is_none() {
            "*"
        } else if write {
            "project:manage"
        } else {
            "project:read"
        },
    )
    .await
    .map_err(|_| ApiError::Forbidden)?;
    if write && user.kind == crate::access::PrincipalKind::Session {
        // A required non-simple header forces browser cross-origin mutations through
        // CORS preflight; these routes grant no cross-origin CORS permission.
        if headers.get("x-pangolin-csrf").and_then(|v| v.to_str().ok()) != Some("1")
            || headers
                .get("sec-fetch-site")
                .is_some_and(|v| v == "cross-site")
        {
            return Err(ApiError::Forbidden);
        }
    }
    Ok(user)
}
async fn audit_in(
    db: &impl ConnectionTrait,
    user: &crate::access::Principal,
    project: &str,
    action: &str,
    resource: &str,
) -> Result<(), ApiError> {
    operations::audit(db, Some(user), project, action, resource).await
}
async fn log_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, None, false).await?;
    let row = state
        .db
        .query_one(sql(
            "SELECT value FROM settings WHERE key='request_logging'",
            vec![],
        ))
        .await?;
    Ok(Json(if let Some(row) = row {
        serde_json::from_str(&row.try_get::<String>("", "value")?)
            .map_err(|e| ApiError::Internal(e.into()))?
    } else {
        serde_json::to_value(logging::Policy::default()).unwrap()
    }))
}
async fn set_log_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(policy): Json<logging::Policy>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, None, true).await?;
    let tx = state.db.begin().await?;
    logging::set_policy(&tx, &policy).await?;
    audit_in(&tx, &user, "", "logging.update", "request_logging").await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SystemSettings {
    instance_name: String,
    branding_name: String,
    favicon_url: String,
    onboarding_complete: bool,
}
impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            instance_name: "Pangolin".into(),
            branding_name: "Pangolin / 鲮鲤".into(),
            favicon_url: "/logo.webp".into(),
            onboarding_complete: false,
        }
    }
}
async fn system_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SystemSettings>, ApiError> {
    actor(&state, &headers, None, false).await?;
    let row = state
        .db
        .query_one(sql("SELECT value FROM settings WHERE key='system'", vec![]))
        .await?;
    let value = match row {
        Some(row) => serde_json::from_str(&row.try_get::<String>("", "value")?)
            .map_err(|error| ApiError::Internal(error.into()))?,
        None => SystemSettings::default(),
    };
    Ok(Json(value))
}
async fn set_system_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SystemSettings>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, None, true).await?;
    if input.instance_name.trim().is_empty()
        || input.instance_name.len() > 128
        || input.branding_name.trim().is_empty()
        || input.branding_name.len() > 128
        || input.favicon_url.len() > 2048
        || !(input.favicon_url.starts_with('/') || input.favicon_url.starts_with("https://"))
    {
        return Err(ApiError::BadRequest("invalid system settings".into()));
    }
    let tx = state.db.begin().await?;
    tx.execute(sql("INSERT INTO settings(key,value,updated_at) VALUES('system',?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",vec![serde_json::to_string(&input).unwrap().into(),db::now().into()])).await?;
    tx.execute(sql("INSERT INTO settings(key,value,updated_at) VALUES('instance_name',?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",vec![input.instance_name.clone().into(),db::now().into()])).await?;
    audit_in(&tx, &user, "", "system.update", "system").await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize, Default)]
struct Filter {
    #[serde(default)]
    offset: u32,
    limit: Option<u32>,
    from: Option<i64>,
    until: Option<i64>,
    dimension: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    api_key: Option<String>,
    q: Option<String>,
}
fn query(resource: &str) -> Result<(&'static str, &'static str, &'static str), ApiError> {
    // Public projections deliberately omit encrypted secrets and raw job payloads.
    Ok(match resource {
        "channels" => (
            "providers",
            "project_id=?",
            "json_object('id',id,'name',name,'kind',kind,'base_url',base_url,'enabled',enabled,'settings',json(settings_json),'created_at',created_at,'updated_at',updated_at)",
        ),
        "credentials" => (
            "channel_credentials",
            "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            "json_object('id',id,'provider_id',provider_id,'provider_name',(SELECT name FROM providers WHERE id=channel_credentials.provider_id),'credential_type',credential_type,'suffix',suffix,'priority',priority,'enabled',enabled,'settings',json(settings_json),'created_at',created_at,'updated_at',updated_at)",
        ),
        "channel-settings" => (
            "channel_settings",
            "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            "json_object('id',provider_id,'provider_id',provider_id,'endpoint_mappings',json(endpoint_mappings_json),'model_rules',json(model_rules_json),'parameter_overrides',json(parameter_overrides_json),'retry_statuses',json(retry_statuses_json),'auto_disable_policy',json(auto_disable_policy_json),'proxy_settings',json(proxy_settings_json),'updated_at',updated_at)",
        ),
        "models" => (
            "models",
            "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            "json_object('id',id,'provider_id',provider_id,'provider_name',(SELECT name FROM providers WHERE id=models.provider_id),'public_name',public_name,'upstream_name',upstream_name,'capabilities',json(capabilities),'input_price_micros',input_price_micros,'output_price_micros',output_price_micros,'priority',priority,'enabled',enabled,'catalog_metadata',json(catalog_metadata_json),'created_at',created_at)",
        ),
        "associations" => (
            "model_associations",
            "project_id=?",
            "json_object('id',id,'model_id',model_id,'model_name',(SELECT m.public_name FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=model_associations.model_id AND p.project_id=model_associations.project_id),'provider_id',provider_id,'provider_name',(SELECT name FROM providers WHERE id=model_associations.provider_id AND project_id=model_associations.project_id),'match_type',match_type,'pattern',pattern,'conditions',json(conditions_json),'priority',priority,'weight',weight,'enabled',enabled,'created_at',created_at,'updated_at',updated_at)",
        ),
        "key-profiles" => (
            "api_key_profiles",
            "project_id=?",
            "json_object('id',id,'name',name,'rpm_limit',rpm_limit,'tpm_limit',tpm_limit,'budget_micros',budget_micros,'routing_policy',json(routing_policy_json),'mappings',json(COALESCE((SELECT json_group_array(json_object('source_model',source_model,'target_model',target_model,'priority',priority)) FROM api_key_profile_model_mappings WHERE profile_id=api_key_profiles.id),'[]')),'allowed_models',json(COALESCE((SELECT json_group_array(json_object('pattern',model_pattern,'match_type',match_type)) FROM api_key_profile_allowed_models WHERE profile_id=api_key_profiles.id),'[]')),'created_at',created_at,'updated_at',updated_at)",
        ),
        "prompts" => (
            "prompts",
            "project_id=?",
            "json_object('id',id,'name',name,'role',role,'content',content,'activation',json(activation_json),'enabled',enabled,'created_at',created_at,'updated_at',updated_at)",
        ),
        "protection" => (
            "prompt_protection_rules",
            "project_id=?",
            "json_object('id',id,'name',name,'role_pattern',role_pattern,'content_pattern',content_pattern,'action',action,'replacement',replacement,'scopes',json(scopes_json),'test_mode',test_mode,'enabled',enabled,'created_at',created_at,'updated_at',updated_at)",
        ),
        "health" => (
            "channel_health_state",
            "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            "json_object('id',provider_id,'provider_id',provider_id,'consecutive_failures',consecutive_failures,'disabled_until',disabled_until,'backoff_until',backoff_until,'reason',reason,'updated_at',updated_at)",
        ),
        "credential-health" => (
            "credential_health_state",
            "credential_id IN (SELECT c.id FROM channel_credentials c JOIN providers p ON p.id=c.provider_id WHERE p.project_id=?)",
            "json_object('id',credential_id,'consecutive_failures',consecutive_failures,'disabled_until',disabled_until,'updated_at',updated_at)",
        ),
        "threads" => (
            "threads",
            "project_id=?",
            "json_object('id',id,'external_id',external_id,'api_key_id',api_key_id,'created_at',created_at)",
        ),
        "traces" => (
            "traces",
            "project_id=?",
            "json_object('id',id,'thread_id',thread_id,'external_id',external_id,'api_key_id',api_key_id,'status',status,'started_at',started_at,'finished_at',finished_at)",
        ),
        "requests" => (
            "requests",
            "trace_id IN (SELECT id FROM traces WHERE project_id=?)",
            "json_object('id',id,'trace_id',trace_id,'protocol',protocol,'endpoint',endpoint,'model',requested_model,'status',status,'started_at',started_at,'finished_at',finished_at,'metadata',json(request_metadata_json))",
        ),
        "executions" => (
            "request_executions",
            "request_id IN (SELECT r.id FROM requests r JOIN traces t ON t.id=r.trace_id WHERE t.project_id=?)",
            "json_object('id',id,'request_id',request_id,'provider_id',provider_id,'credential_suffix',credential_suffix,'attempt',attempt,'model',model,'status',status,'retry_reason',retry_reason,'latency_ms',latency_ms,'first_token_at',first_token_at)",
        ),
        "usage" => (
            "usage_logs",
            "execution_id IN (SELECT e.id FROM execution_facts e JOIN request_facts r ON r.id=e.request_id WHERE r.project_id=?)",
            "json_object('id',id,'execution_id',execution_id,'input_tokens',input_tokens,'output_tokens',output_tokens,'cache_read_tokens',cache_read_tokens,'cache_write_tokens',cache_write_tokens,'reasoning_tokens',reasoning_tokens,'request_units',request_units,'cost_micros',total_cost_micros,'settlement_kind',settlement_kind,'cache_savings_micros',cache_savings_micros,'price_id',price_id,'created_at',created_at)",
        ),
        "cost-items" => (
            "usage_cost_items",
            "usage_log_id IN (SELECT u.id FROM usage_logs u JOIN execution_facts e ON e.id=u.execution_id JOIN request_facts r ON r.id=e.request_id WHERE r.project_id=?)",
            "json_object('id',id,'usage_id',usage_log_id,'component_id',price_component_id,'quantity',quantity,'subtotal_micros',subtotal_micros)",
        ),
        "prices" => (
            "model_prices",
            "model_id IN (SELECT m.id FROM models m JOIN providers p ON p.id=m.provider_id WHERE p.project_id=?)",
            "json_object('id',id,'model_id',model_id,'model_name',(SELECT public_name FROM models WHERE id=model_prices.model_id),'provider_id',provider_id,'version',version,'valid_from',valid_from,'valid_until',valid_until,'schedule',json(schedule_json))",
        ),
        "probes" => (
            "channel_probes",
            "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            "json_object('id',id,'provider_id',provider_id,'success',success,'status_code',status_code,'latency_ms',latency_ms,'ttft_ms',ttft_ms,'probed_at',probed_at,'error',error_code)",
        ),
        "quotas" => (
            "provider_quota_snapshots",
            "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            "json_object('id',id,'provider_id',provider_id,'credential_id',credential_id,'remaining_micros',remaining_micros,'period_start',period_start,'period_end',period_end,'collected_at',collected_at)",
        ),
        "storage" => (
            "data_storage_configs",
            "project_id=?",
            "json_object('id',id,'name',name,'kind',kind,'config',json(config_json),'revision',revision,'enabled',enabled)",
        ),
        "schedules" => (
            "operation_schedules",
            "project_id=?",
            "json_object('id',id,'kind',kind,'payload',json(payload_json),'interval_secs',interval_secs,'next_run_at',next_run_at,'enabled',enabled,'revision',revision)",
        ),
        "jobs" => (
            "operation_jobs",
            "project_id=?",
            "json_object('id',id,'kind',kind,'status',status,'attempts',attempts,'fence',fence,'due_at',due_at,'created_at',created_at,'error_code',error_code)",
        ),
        "backup-target-results" => (
            "backup_job_targets",
            "job_id IN (SELECT id FROM operation_jobs WHERE project_id=?)",
            "json_object('id',id,'job_id',job_id,'storage_id',storage_id,'revision',revision,'object_key',object_key,'status',status,'error_code',error_code,'byte_size',byte_size)",
        ),
        "webhooks" => (
            "webhooks",
            "project_id=?",
            "json_object('id',id,'name',name,'url',url,'subscriptions',json(subscriptions_json),'enabled',enabled)",
        ),
        "webhook-deliveries" => (
            "webhook_deliveries",
            "webhook_id IN (SELECT id FROM webhooks WHERE project_id=?)",
            "json_object('id',id,'webhook_id',webhook_id,'event_type',event_type,'attempt',attempt,'status',status,'response_status',response_status,'next_attempt_at',next_attempt_at)",
        ),
        "groups" => (
            "service_groups",
            "project_id=?",
            "json_object('id',id,'name',name,'tier',tier,'ratio_millionths',ratio_millionths,'enabled',enabled)",
        ),
        "retention" => (
            "data_retention_policies",
            "project_id=?",
            "json_object('id',id,'resource_type',resource_type,'retention_days',retention_days,'retain_payloads',retain_payloads)",
        ),
        "audit" => (
            "audit_events",
            "json_extract(details,'$.project_id')=?",
            "json_object('id',id,'actor_user_id',actor_user_id,'action',action,'resource_type',resource_type,'resource_id',resource_id,'created_at',created_at)",
        ),
        _ => return Err(ApiError::NotFound),
    })
}
async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, resource)): Path<(String, String)>,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    let (table, scope, projection) = query(&resource)?;
    let query = filter
        .q
        .filter(|value| !value.trim().is_empty() && value.len() <= 128)
        .map(|value| format!("%{}%", value.trim()));
    let rows=state.db.query_all(sql(format!("SELECT document FROM (SELECT {projection} AS document,rowid AS source_rowid FROM {table} WHERE {scope}) WHERE (? IS NULL OR document LIKE ?) ORDER BY source_rowid DESC LIMIT ? OFFSET ?"),vec![project.clone().into(),query.clone().into(),query.clone().into(),i64::from(filter.limit.unwrap_or(100).clamp(1,500)).into(),i64::from(filter.offset).into()])).await?;
    let total=state.db.query_one(sql(format!("SELECT COUNT(*) AS total FROM (SELECT {projection} AS document FROM {table} WHERE {scope}) WHERE (? IS NULL OR document LIKE ?)"),vec![project.into(),query.clone().into(),query.into()])).await?.map(|row|row.try_get::<i64>("","total")).transpose()?.unwrap_or(0);
    Ok(Json(
        json!({"data":documents(rows)?,"total":total,"offset":filter.offset,"limit":filter.limit.unwrap_or(100).clamp(1,500)}),
    ))
}
fn documents(rows: Vec<sea_orm::QueryResult>) -> Result<Vec<Value>, ApiError> {
    rows.into_iter()
        .map(|r| {
            serde_json::from_str(&r.try_get::<String>("", "document")?)
                .map_err(|e| ApiError::Internal(e.into()))
        })
        .collect()
}
async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, resource, id)): Path<(String, String, String)>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    if resource == "trace-detail" {
        let trace=state.db.query_one(sql("SELECT json_object('id',id,'thread_id',thread_id,'external_id',external_id,'status',status,'started_at',started_at,'finished_at',finished_at) AS document FROM traces WHERE project_id=? AND id=?",vec![project.clone().into(),id.clone().into()])).await?.ok_or(ApiError::NotFound)?;
        let trace: Value = serde_json::from_str(&trace.try_get::<String>("", "document")?)
            .map_err(|error| ApiError::Internal(error.into()))?;
        let requests=documents(state.db.query_all(sql("SELECT json_object('id',id,'public_id',COALESCE(json_extract(request_metadata_json,'$.external_id'),id),'protocol',protocol,'endpoint',endpoint,'model',requested_model,'status',status,'started_at',started_at,'finished_at',finished_at) AS document FROM requests WHERE trace_id=? ORDER BY started_at",vec![id.clone().into()])).await?)?;
        let executions=documents(state.db.query_all(sql("SELECT json_object('id',id,'request_id',request_id,'provider_id',provider_id,'attempt',attempt,'model',model,'status',status,'retry_reason',retry_reason,'latency_ms',latency_ms,'started_at',started_at,'finished_at',finished_at) AS document FROM request_executions WHERE request_id IN (SELECT id FROM requests WHERE trace_id=?) ORDER BY started_at",vec![id.into()])).await?)?;
        return Ok(Json(
            json!({"trace":trace,"requests":requests,"executions":executions}),
        ));
    }
    if resource == "content" {
        let row=state.db.query_one(sql("SELECT c.request_json,c.response_json FROM request_contents c JOIN requests r ON r.id=c.request_id JOIN traces t ON t.id=r.trace_id WHERE t.project_id=? AND r.id=?",vec![project.into(),id.into()])).await?.ok_or(ApiError::NotFound)?;
        return Ok(Json(
            json!({"request":row.try_get::<Option<String>>("","request_json")?,"response":row.try_get::<Option<String>>("","response_json")?}),
        ));
    }
    let id_column = match resource.as_str() {
        "health" | "channel-settings" => "provider_id",
        "credential-health" => "credential_id",
        _ => "id",
    };
    let (table, scope, projection) = query(&resource)?;
    let rows = state
        .db
        .query_all(sql(
            format!(
                "SELECT {projection} AS document FROM {table} WHERE ({scope}) AND {id_column}=?"
            ),
            vec![project.into(), id.into()],
        ))
        .await?;
    Ok(Json(
        documents(rows)?
            .into_iter()
            .next()
            .ok_or(ApiError::NotFound)?,
    ))
}
async fn analytics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    let dimension = match filter.dimension.as_deref().unwrap_or("day") {
        "day" => "CAST(r.started_at/86400 AS TEXT)",
        "provider" => "e.provider_id",
        "model" => "e.model_id",
        "api_key" => "r.api_key_id",
        "user" => "r.user_id",
        "project" => "r.project_id",
        _ => return Err(ApiError::BadRequest("unknown analytics dimension".into())),
    };
    let rows=state.db.query_all(sql(format!("SELECT json_object('dimension',{dimension},'requests',COUNT(DISTINCT r.id),'attempts',COUNT(e.id),'errors',SUM(e.status!='succeeded'),'input_tokens',COALESCE(SUM(u.input_tokens),0),'output_tokens',COALESCE(SUM(u.output_tokens),0),'cache_hit_tokens',COALESCE(SUM(u.cache_read_tokens),0),'cache_savings_micros',COALESCE(SUM(u.cache_savings_micros),0),'cost_micros',COALESCE(SUM(u.total_cost_micros),0),'latency_ms',AVG(x.latency_ms),'ttft_ms',AVG(x.first_token_at-x.started_at*1000)) AS document FROM request_facts r JOIN execution_facts e ON e.request_id=r.id LEFT JOIN usage_logs u ON u.execution_id=e.id LEFT JOIN request_executions x ON x.id=e.id WHERE r.project_id=? AND r.started_at>=? AND r.started_at<? AND (? IS NULL OR e.model_id=?) AND (? IS NULL OR e.provider_id=?) AND (? IS NULL OR r.api_key_id=?) GROUP BY {dimension} ORDER BY {dimension} LIMIT 500"),vec![project.into(),filter.from.unwrap_or(db::now()-86400*30).into(),filter.until.unwrap_or(db::now()+1).into(),filter.model.clone().into(),filter.model.into(),filter.provider.clone().into(),filter.provider.into(),filter.api_key.clone().into(),filter.api_key.into()])).await?;
    Ok(Json(
        json!({"data":documents(rows)?,"source":"sqlite","derived_available":state.observations.is_available()}),
    ))
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SessionCompactionSettings {
    enabled: bool,
    threshold_tokens: u64,
    retain_items: usize,
    native: bool,
    summarizer_model: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OrchestrationSettings {
    version: u8,
    affinity_rules: Vec<crate::orchestration::affinity::Rule>,
    session_compaction: SessionCompactionSettings,
}
impl Default for OrchestrationSettings {
    fn default() -> Self {
        Self {
            version: 1,
            affinity_rules: vec![],
            session_compaction: SessionCompactionSettings {
                enabled: false,
                threshold_tokens: 8192,
                retain_items: 16,
                native: true,
                summarizer_model: None,
            },
        }
    }
}
fn validate_orchestration_settings(input: &OrchestrationSettings) -> Result<(), ApiError> {
    if input.version != 1
        || input.affinity_rules.len() > 64
        || input.session_compaction.threshold_tokens < 128
        || input.session_compaction.threshold_tokens > 10_000_000
        || input.session_compaction.retain_items > 128
        || input.session_compaction.enabled
            && !input.session_compaction.native
            && input
                .session_compaction
                .summarizer_model
                .as_deref()
                .is_none_or(str::is_empty)
    {
        return Err(ApiError::BadRequest(
            "invalid orchestration settings".into(),
        ));
    }
    for rule in &input.affinity_rules {
        rule.validate()
            .map_err(|_| ApiError::BadRequest("invalid affinity rule".into()))?;
    }
    Ok(())
}
async fn orchestration_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
) -> Result<Json<OrchestrationSettings>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    let row = state
        .db
        .query_one(sql(
            "SELECT settings_json FROM projects WHERE id=?",
            vec![project.into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    let settings: Value = serde_json::from_str(&row.try_get::<String>("", "settings_json")?)
        .map_err(|error| ApiError::Internal(error.into()))?;
    let default = OrchestrationSettings::default();
    let output = OrchestrationSettings {
        version: 1,
        affinity_rules: serde_json::from_value(
            settings
                .get("affinity_rules")
                .cloned()
                .unwrap_or(json!(default.affinity_rules)),
        )
        .map_err(|_| ApiError::BadRequest("invalid stored affinity settings".into()))?,
        session_compaction: serde_json::from_value(
            settings
                .get("session_compaction")
                .cloned()
                .unwrap_or(json!(default.session_compaction)),
        )
        .map_err(|_| ApiError::BadRequest("invalid stored compaction settings".into()))?,
    };
    Ok(Json(output))
}
async fn set_orchestration_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<OrchestrationSettings>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    validate_orchestration_settings(&input)?;
    let tx = state.db.begin().await?;
    let row = tx
        .query_one(sql(
            "SELECT settings_json FROM projects WHERE id=?",
            vec![project.clone().into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    let mut settings: Value = serde_json::from_str(&row.try_get::<String>("", "settings_json")?)
        .map_err(|error| ApiError::Internal(error.into()))?;
    settings["affinity_rules"] = json!(input.affinity_rules);
    settings["session_compaction"] = json!(input.session_compaction);
    settings["version"] = json!(1);
    tx.execute(sql(
        "UPDATE projects SET settings_json=?,updated_at=? WHERE id=?",
        vec![
            settings.to_string().into(),
            db::now().into(),
            project.clone().into(),
        ],
    ))
    .await?;
    audit_in(
        &tx,
        &user,
        &project,
        "orchestration-settings.update",
        "orchestration-settings",
    )
    .await?;
    tx.commit().await?;
    state.orchestrator.affinity_rules.reset_project(&project);
    Ok(Json(json!({"ok":true})))
}

async fn observation_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
) -> Result<Json<crate::observability::Summary>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    Ok(Json(
        state
            .observations
            .summary_for(project)
            .await
            .map_err(ApiError::Internal)?,
    ))
}

async fn observation_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Query(mut filter): Query<crate::observability::RequestFilter>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    filter.project_id = Some(project);
    let total = state
        .observations
        .count(filter.clone())
        .await
        .map_err(ApiError::Internal)?;
    let data = state
        .observations
        .list(filter.clone())
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(
        json!({"data":data,"total":total,"offset":filter.offset.unwrap_or(0),"limit":filter.limit.unwrap_or(100).clamp(1,500)}),
    ))
}

async fn observation_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<crate::observability::RequestEvent>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    state
        .observations
        .get_for(project, id)
        .await
        .map_err(ApiError::Internal)?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[derive(Deserialize)]
struct RoutingPreviewInput {
    api_key_id: String,
    model: String,
    #[serde(default = "preview_endpoint")]
    endpoint: String,
    #[serde(default)]
    body: Value,
}
fn preview_endpoint() -> String {
    "/v1/chat/completions".into()
}
async fn routing_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<RoutingPreviewInput>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    if !crate::providers::ENDPOINTS.contains(&input.endpoint.as_str()) {
        return Err(ApiError::BadRequest("unsupported preview endpoint".into()));
    }
    let key = db::api_key_credential_by_id(&state.db, &project, &input.api_key_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let profile = crate::orchestration::load_profile(&state.db, &key).await?;
    let mut payload = if input.body.is_object() {
        input.body
    } else {
        json!({})
    };
    payload["model"] = Value::String(input.model);
    if payload.get("messages").is_none() && input.endpoint == "/v1/chat/completions" {
        payload["messages"] = json!([{"role":"user","content":"routing preview"}]);
    }
    let plan = crate::orchestration::prepare(
        &state.db,
        &state.orchestrator,
        &key,
        profile,
        payload,
        &HeaderMap::new(),
        &input.endpoint,
    )
    .await?;
    let candidates = plan.candidates.iter().map(|candidate| json!({
        "id":candidate.id(),"provider_id":candidate.provider_id,"model_id":candidate.model_id,
        "upstream_model":candidate.target.upstream_name,"provider":candidate.target.provider_name,
        "priority":candidate.priority,"weight":candidate.weight,"endpoint":candidate.endpoint,
    })).collect::<Vec<_>>();
    Ok(Json(
        json!({"candidates":candidates,"decisions":plan.decisions,"estimated_tokens":plan.estimated_tokens}),
    ))
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 2048)
        .ok_or_else(|| ApiError::BadRequest(format!("{key} is required")))
}
async fn mutate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, resource)): Path<(String, String)>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let resource_id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(id);
    let transaction = state.db.begin().await?;
    match resource.as_str() {
        "channels" => {
            let kind = text(&value, "kind")?;
            if !matches!(
                kind,
                "openai"
                    | "openai_compatible"
                    | "anthropic"
                    | "gemini"
                    | "azure"
                    | "bedrock"
                    | "vertex"
                    | "gcp"
                    | "openrouter"
                    | "deepseek"
                    | "moonshot"
                    | "zhipu"
                    | "doubao"
                    | "xai"
                    | "groq"
                    | "ollama"
                    | "nanogpt"
                    | "jina"
            ) {
                return Err(ApiError::BadRequest("unsupported channel kind".into()));
            }
            let base_url = text(&value, "base_url")?;
            validate_http_url(base_url)?;
            let current_settings = transaction
                .query_one(sql(
                    "SELECT settings_json FROM providers WHERE id=? AND project_id=?",
                    vec![resource_id.clone().into(), project.clone().into()],
                ))
                .await?
                .map(|row| row.try_get::<String>("", "settings_json"))
                .transpose()?;
            let settings = value
                .get("settings")
                .filter(|settings| !settings.is_null())
                .map(Value::to_string)
                .or(current_settings)
                .unwrap_or_else(|| json!({"version":1}).to_string());
            let changed = transaction.execute(sql("INSERT INTO providers(id,name,kind,base_url,enabled,created_at,updated_at,project_id,settings_json) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,kind=excluded.kind,base_url=excluded.base_url,enabled=excluded.enabled,updated_at=excluded.updated_at,settings_json=excluded.settings_json WHERE providers.project_id=excluded.project_id",vec![resource_id.clone().into(),text(&value,"name")?.into(),kind.into(),base_url.into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),db::now().into(),project.clone().into(),settings.into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
            transaction
                .execute(sql(
                    "INSERT OR IGNORE INTO channel_settings(provider_id,updated_at) VALUES(?,?)",
                    vec![resource_id.clone().into(), db::now().into()],
                ))
                .await?;
        }
        "credentials" => {
            let provider = text(&value, "provider_id")?;
            if transaction
                .query_one(sql(
                    "SELECT id FROM providers WHERE id=? AND project_id=?",
                    vec![provider.into(), project.clone().into()],
                ))
                .await?
                .is_none()
            {
                return Err(ApiError::NotFound);
            }
            let secret = value
                .get("secret")
                .and_then(Value::as_str)
                .filter(|secret| !secret.trim().is_empty());
            let existing = transaction.query_one(sql("SELECT id FROM channel_credentials WHERE id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![resource_id.clone().into(),project.clone().into()])).await?.is_some();
            if !existing && secret.is_none() {
                return Err(ApiError::BadRequest(
                    "secret is required for a new credential".into(),
                ));
            }
            let envelope = secret
                .map(|secret| state.secrets.encrypt(secret))
                .transpose()?;
            let suffix = secret.map(|secret| {
                secret
                    .chars()
                    .rev()
                    .take(4)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            });
            let changed = if existing {
                transaction.execute(sql("UPDATE channel_credentials SET provider_id=?,credential_type=?,secret_envelope=COALESCE(?,secret_envelope),suffix=COALESCE(?,suffix),priority=?,enabled=?,settings_json=?,updated_at=? WHERE id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![provider.into(),value["credential_type"].as_str().unwrap_or("api_key").into(),envelope.into(),suffix.into(),value["priority"].as_i64().unwrap_or(100).into(),value["enabled"].as_bool().unwrap_or(true).into(),value.get("settings").cloned().unwrap_or(json!({"version":1})).to_string().into(),db::now().into(),resource_id.clone().into(),project.clone().into()])).await?.rows_affected()
            } else {
                transaction.execute(sql("INSERT INTO channel_credentials(id,provider_id,credential_type,secret_envelope,suffix,priority,enabled,settings_json,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?)",vec![resource_id.clone().into(),provider.into(),value["credential_type"].as_str().unwrap_or("api_key").into(),envelope.into(),suffix.into(),value["priority"].as_i64().unwrap_or(100).into(),value["enabled"].as_bool().unwrap_or(true).into(),value.get("settings").cloned().unwrap_or(json!({"version":1})).to_string().into(),db::now().into(),db::now().into()])).await?.rows_affected()
            };
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
        }
        "channel-settings" => {
            let provider = text(&value, "provider_id")?;
            let current=transaction.query_one(sql("SELECT endpoint_mappings_json,model_rules_json,parameter_overrides_json,retry_statuses_json,auto_disable_policy_json,proxy_settings_json FROM channel_settings WHERE provider_id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![provider.into(),project.clone().into()])).await?.ok_or(ApiError::NotFound)?;
            let document = |key: &str, column: &str| -> Result<String, ApiError> {
                Ok(value
                    .get(key)
                    .filter(|item| !item.is_null())
                    .map(Value::to_string)
                    .unwrap_or(current.try_get::<String>("", column)?))
            };
            let changed = transaction.execute(sql("UPDATE channel_settings SET endpoint_mappings_json=?,model_rules_json=?,parameter_overrides_json=?,retry_statuses_json=?,auto_disable_policy_json=?,proxy_settings_json=?,updated_at=? WHERE provider_id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![document("endpoint_mappings","endpoint_mappings_json")?.into(),document("model_rules","model_rules_json")?.into(),document("parameter_overrides","parameter_overrides_json")?.into(),document("retry_statuses","retry_statuses_json")?.into(),document("auto_disable_policy","auto_disable_policy_json")?.into(),document("proxy_settings","proxy_settings_json")?.into(),db::now().into(),provider.into(),project.clone().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::NotFound);
            }
        }
        "models" => {
            let provider = text(&value, "provider_id")?;
            if transaction
                .query_one(sql(
                    "SELECT id FROM providers WHERE id=? AND project_id=?",
                    vec![provider.into(), project.clone().into()],
                ))
                .await?
                .is_none()
            {
                return Err(ApiError::NotFound);
            }
            let capabilities = value
                .get("capabilities")
                .cloned()
                .unwrap_or(json!(["chat"]));
            if !capabilities.is_array() {
                return Err(ApiError::BadRequest("capabilities must be an array".into()));
            }
            let changed = transaction.execute(sql("INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,input_price_micros,output_price_micros,priority,enabled,created_at,catalog_metadata_json) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET provider_id=excluded.provider_id,public_name=excluded.public_name,upstream_name=excluded.upstream_name,capabilities=excluded.capabilities,input_price_micros=excluded.input_price_micros,output_price_micros=excluded.output_price_micros,priority=excluded.priority,enabled=excluded.enabled,catalog_metadata_json=excluded.catalog_metadata_json WHERE models.provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![resource_id.clone().into(),provider.into(),text(&value,"public_name")?.into(),text(&value,"upstream_name")?.into(),capabilities.to_string().into(),value["input_price_micros"].as_i64().unwrap_or(0).max(0).into(),value["output_price_micros"].as_i64().unwrap_or(0).max(0).into(),value["priority"].as_i64().unwrap_or(100).into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),value.get("catalog_metadata").cloned().unwrap_or(json!({})).to_string().into(),project.clone().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
        }
        "associations" => {
            let match_type = value["match_type"].as_str().unwrap_or("exact");
            if !matches!(match_type, "exact" | "regex" | "tag") {
                return Err(ApiError::BadRequest(
                    "invalid association match type".into(),
                ));
            }
            if match_type == "regex" {
                crate::orchestration::policy::regex(text(&value, "pattern")?)?;
            }
            let model = value
                .get("model_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty());
            let provider = value
                .get("provider_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty());
            if let Some(model)=model&&transaction.query_one(sql("SELECT m.id FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=? AND p.project_id=?",vec![model.into(),project.clone().into()])).await?.is_none(){return Err(ApiError::NotFound)}
            if let Some(provider) = provider
                && transaction
                    .query_one(sql(
                        "SELECT id FROM providers WHERE id=? AND project_id=?",
                        vec![provider.into(), project.clone().into()],
                    ))
                    .await?
                    .is_none()
            {
                return Err(ApiError::NotFound);
            }
            let changed = transaction.execute(sql("INSERT INTO model_associations(id,project_id,model_id,provider_id,match_type,pattern,conditions_json,priority,weight,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET model_id=excluded.model_id,provider_id=excluded.provider_id,match_type=excluded.match_type,pattern=excluded.pattern,conditions_json=excluded.conditions_json,priority=excluded.priority,weight=excluded.weight,enabled=excluded.enabled,updated_at=excluded.updated_at WHERE model_associations.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),model.into(),provider.into(),match_type.into(),text(&value,"pattern")?.into(),value.get("conditions").cloned().unwrap_or(json!({"version":1})).to_string().into(),value["priority"].as_i64().unwrap_or(100).into(),value["weight"].as_i64().unwrap_or(1).clamp(1,10000).into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),db::now().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
        }
        "key-profiles" => {
            let changed = transaction.execute(sql("INSERT INTO api_key_profiles(id,project_id,name,rpm_limit,tpm_limit,budget_micros,routing_policy_json,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,rpm_limit=excluded.rpm_limit,tpm_limit=excluded.tpm_limit,budget_micros=excluded.budget_micros,routing_policy_json=excluded.routing_policy_json,updated_at=excluded.updated_at WHERE api_key_profiles.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),value["rpm_limit"].as_i64().into(),value["tpm_limit"].as_i64().into(),value["budget_micros"].as_i64().into(),value.get("routing_policy").cloned().unwrap_or(json!({"version":1})).to_string().into(),db::now().into(),db::now().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
            if let Some(mappings) = value["mappings"].as_array() {
                transaction
                    .execute(sql(
                        "DELETE FROM api_key_profile_model_mappings WHERE profile_id=?",
                        vec![resource_id.clone().into()],
                    ))
                    .await?;
                for mapping in mappings {
                    transaction.execute(sql("INSERT INTO api_key_profile_model_mappings(id,profile_id,source_model,target_model,priority) VALUES(?,?,?,?,?)",vec![id().into(),resource_id.clone().into(),text(mapping,"source_model")?.into(),text(mapping,"target_model")?.into(),mapping["priority"].as_i64().unwrap_or(100).into()])).await?;
                }
            }
            if let Some(allowed) = value["allowed_models"].as_array() {
                transaction
                    .execute(sql(
                        "DELETE FROM api_key_profile_allowed_models WHERE profile_id=?",
                        vec![resource_id.clone().into()],
                    ))
                    .await?;
                for model in allowed {
                    let kind = model["match_type"].as_str().unwrap_or("exact");
                    if !matches!(kind, "exact" | "regex") {
                        return Err(ApiError::BadRequest(
                            "invalid allowed-model match type".into(),
                        ));
                    }
                    if kind == "regex" {
                        crate::orchestration::policy::regex(text(model, "pattern")?)?;
                    }
                    transaction.execute(sql("INSERT INTO api_key_profile_allowed_models(profile_id,model_pattern,match_type) VALUES(?,?,?)",vec![resource_id.clone().into(),text(model,"pattern")?.into(),kind.into()])).await?;
                }
            }
        }
        "prompts" => {
            let changed = transaction.execute(sql("INSERT INTO prompts(id,project_id,name,role,content,activation_json,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,role=excluded.role,content=excluded.content,activation_json=excluded.activation_json,enabled=excluded.enabled,updated_at=excluded.updated_at WHERE prompts.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),text(&value,"role")?.into(),text(&value,"content")?.into(),value.get("activation").cloned().unwrap_or(json!({"version":1})).to_string().into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),db::now().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
        }
        "protection" => {
            let action = value["action"].as_str().unwrap_or("deny");
            if !matches!(action, "deny" | "redact") {
                return Err(ApiError::BadRequest("invalid protection action".into()));
            }
            crate::orchestration::policy::regex(text(&value, "content_pattern")?)?;
            if let Some(pattern) = value["role_pattern"].as_str() {
                crate::orchestration::policy::regex(pattern)?;
            }
            let changed = transaction.execute(sql("INSERT INTO prompt_protection_rules(id,project_id,name,role_pattern,content_pattern,action,replacement,scopes_json,test_mode,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,role_pattern=excluded.role_pattern,content_pattern=excluded.content_pattern,action=excluded.action,replacement=excluded.replacement,scopes_json=excluded.scopes_json,test_mode=excluded.test_mode,enabled=excluded.enabled,updated_at=excluded.updated_at WHERE prompt_protection_rules.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),value["role_pattern"].as_str().into(),text(&value,"content_pattern")?.into(),action.into(),value["replacement"].as_str().into(),value.get("scopes").cloned().unwrap_or(json!({"version":1})).to_string().into(),value["test_mode"].as_bool().unwrap_or(false).into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),db::now().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
        }
        "bulk-toggle" => {
            let target = text(&value, "resource")?;
            let ids = value["ids"]
                .as_array()
                .filter(|ids| !ids.is_empty() && ids.len() <= 100)
                .ok_or_else(|| ApiError::BadRequest("ids must contain 1–100 items".into()))?;
            let enabled = value["enabled"]
                .as_bool()
                .ok_or_else(|| ApiError::BadRequest("enabled is required".into()))?;
            let (table, scope) = match target {
                "channels" => ("providers", "project_id=?"),
                "models" => (
                    "models",
                    "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                ),
                "credentials" => (
                    "channel_credentials",
                    "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                ),
                _ => return Err(ApiError::BadRequest("unsupported bulk resource".into())),
            };
            for item in ids {
                let item = item
                    .as_str()
                    .ok_or_else(|| ApiError::BadRequest("invalid id".into()))?;
                transaction
                    .execute(sql(
                        format!("UPDATE {table} SET enabled=? WHERE ({scope}) AND id=?"),
                        vec![enabled.into(), project.clone().into(), item.into()],
                    ))
                    .await?;
            }
        }
        "health-policy" => {
            let provider = text(&value, "provider_id")?;
            let policy = &value["policy"];
            if policy["version"] != 1
                || policy["enabled"].as_bool().is_none()
                || !matches!(
                    policy["action"].as_str().unwrap_or("channel"),
                    "channel" | "credential"
                )
                || policy["threshold"]
                    .as_i64()
                    .is_some_and(|v| !(1..=1000).contains(&v))
            {
                return Err(ApiError::BadRequest("invalid health policy".into()));
            }
            if let Some(pattern) = policy["pattern"].as_str() {
                crate::orchestration::policy::regex(pattern)?;
            }
            if let Some(expression) = policy["recovery_cron"].as_str() {
                let expression = if expression.split_whitespace().count() == 5 {
                    format!("0 {expression}")
                } else {
                    expression.into()
                };
                if expression.parse::<cron::Schedule>().is_err() {
                    return Err(ApiError::BadRequest("invalid recovery cron".into()));
                }
            }
            if let Some(zone) = policy["timezone"].as_str()
                && zone.parse::<chrono_tz::Tz>().is_err()
            {
                return Err(ApiError::BadRequest("invalid recovery timezone".into()));
            }
            let changed=transaction.execute(sql("UPDATE channel_settings SET auto_disable_policy_json=?,updated_at=? WHERE provider_id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![policy.to_string().into(),db::now().into(),provider.into(),project.clone().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::NotFound);
            }
        }
        "key-logging" => {
            let key = text(&value, "api_key_id")?;
            let level: logging::Level = serde_json::from_value(value["level"].clone())
                .map_err(|_| ApiError::BadRequest("invalid logging level".into()))?;
            if transaction
                .execute(sql(
                    "UPDATE api_keys SET log_level=? WHERE id=? AND project_id=?",
                    vec![level.name().into(), key.into(), project.clone().into()],
                ))
                .await?
                .rows_affected()
                != 1
            {
                return Err(ApiError::NotFound);
            }
        }
        "storage" => {
            let config: storage::Config =
                serde_json::from_value(value["config"].clone()).map_err(|_| storage::invalid())?;
            storage::validate(&config)?;
            let secret = value
                .get("secret")
                .filter(|secret| !secret.is_null())
                .map(|secret| storage::envelope(&state.secrets, &project, &resource_id, secret))
                .transpose()?;
            let exists = transaction
                .query_one(sql(
                    "SELECT project_id,revision FROM data_storage_configs WHERE id=?",
                    vec![resource_id.clone().into()],
                ))
                .await?;
            if let Some(row) = exists {
                if row.try_get::<Option<String>>("", "project_id")?.as_deref() != Some(&project) {
                    return Err(ApiError::Forbidden);
                }
                if value["revision"].as_i64() != Some(row.try_get("", "revision")?) {
                    return Err(ApiError::Conflict("storage revision changed".into()));
                }
                transaction.execute(sql("UPDATE data_storage_configs SET name=?,config_json=?,kind=?,secret_envelope=COALESCE(?,secret_envelope),enabled=?,revision=revision+1,updated_at=? WHERE id=? AND revision=?",vec![text(&value,"name")?.into(),serde_json::to_string(&config).unwrap().into(),match config{storage::Config::Local{..}=>"local",_=>"s3"}.into(),secret.into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),resource_id.clone().into(),value["revision"].as_i64().into()])).await?;
            } else {
                transaction.execute(sql("INSERT INTO data_storage_configs(id,project_id,name,kind,config_json,secret_envelope,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),match config{storage::Config::Local{..}=>"local",_=>"s3"}.into(),serde_json::to_string(&config).unwrap().into(),secret.into(),db::now().into(),db::now().into()])).await?;
            }
        }
        "prices" => {
            let model = text(&value, "model_id")?;
            if transaction.query_one(sql("SELECT m.id FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=? AND p.project_id=?",vec![model.into(),project.clone().into()])).await?.is_none(){return Err(ApiError::NotFound)}
            let components: Vec<pricing::Component> =
                serde_json::from_value(value["components"].clone())
                    .map_err(|_| ApiError::BadRequest("invalid price components".into()))?;
            if components.is_empty() {
                return Err(ApiError::BadRequest(
                    "at least one price component is required".into(),
                ));
            }
            pricing::Price {
                id: None,
                model_id: model.into(),
                ratio_millionths: 1_000_000,
                components: components.clone(),
            }
            .validate()?;
            let schedule: operations::schedule::Schedule = serde_json::from_value(
                value
                    .get("schedule")
                    .cloned()
                    .unwrap_or(json!({"version":1,"rules":[]})),
            )
            .map_err(|_| ApiError::BadRequest("invalid price schedule".into()))?;
            schedule.validate()?;
            let tx = &transaction;
            tx.execute(sql("INSERT INTO model_prices(id,model_id,version,valid_from,valid_until,created_at,schedule_json) SELECT ?,?,COALESCE(MAX(version),0)+1,?,?,?,? FROM model_prices WHERE model_id=?",vec![resource_id.clone().into(),model.into(),value["valid_from"].as_i64().unwrap_or(db::now()).into(),value["valid_until"].as_i64().into(),db::now().into(),serde_json::to_string(&schedule).unwrap().into(),model.into()])).await?;
            for c in components {
                tx.execute(sql("INSERT INTO model_price_components(id,price_id,kind,unit_size,unit_price_micros,tiers_json) VALUES(?,?,?,?,?,?)",vec![id().into(),resource_id.clone().into(),c.kind.into(),c.unit_size.into(),c.unit_price_micros.into(),json!({"version":1,"tiers":c.tiers,"cache_ttl":c.cache_ttl}).to_string().into()])).await?;
            }
        }
        "groups" => {
            let ratio = if let Some(ratio) = value["ratio_millionths"].as_i64() {
                ratio
            } else if let Some(ratio) = value.get("ratio") {
                operations::runtime::decimal_micros(&ratio.to_string(), 1_000_000)?
            } else {
                1_000_000
            };
            if !(0..=100_000_000).contains(&ratio) {
                return Err(ApiError::BadRequest("invalid integer ratio".into()));
            }
            let tx = &transaction;
            let changed=tx.execute(sql("INSERT INTO service_groups(id,project_id,name,tier,ratio_millionths,enabled) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,tier=excluded.tier,ratio_millionths=excluded.ratio_millionths,enabled=excluded.enabled WHERE service_groups.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),value["tier"].as_str().unwrap_or("standard").into(),ratio.into(),value["enabled"].as_bool().unwrap_or(true).into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
            if let Some(channels) = value["channels"].as_array() {
                tx.execute(sql(
                    "DELETE FROM service_group_channels WHERE group_id=?",
                    vec![resource_id.clone().into()],
                ))
                .await?;
                for channel in channels {
                    let channel = channel.as_str().ok_or(ApiError::NotFound)?;
                    if tx.execute(sql("INSERT INTO service_group_channels(group_id,provider_id) SELECT ?,id FROM providers WHERE id=? AND project_id=?",vec![resource_id.clone().into(),channel.into(),project.clone().into()])).await?.rows_affected()!=1{return Err(ApiError::NotFound)}
                }
            }
            if let Some(keys) = value["api_keys"].as_array() {
                tx.execute(sql(
                    "DELETE FROM service_group_keys WHERE group_id=?",
                    vec![resource_id.clone().into()],
                ))
                .await?;
                for key in keys {
                    tx.execute(sql("INSERT INTO service_group_keys(api_key_id,project_id,group_id) VALUES(?,?,?) ON CONFLICT(api_key_id) DO UPDATE SET group_id=excluded.group_id WHERE service_group_keys.project_id=excluded.project_id",vec![key.as_str().ok_or(ApiError::NotFound)?.into(),project.clone().into(),resource_id.clone().into()])).await?;
                }
            }
        }
        "schedules" => {
            let kind = text(&value, "kind")?;
            if !matches!(
                kind,
                "probe" | "quota" | "automatic_backup" | "backup_retention"
            ) {
                return Err(ApiError::BadRequest("invalid schedule kind".into()));
            }
            let interval = value["interval_secs"].as_i64().unwrap_or(3600);
            if !(30..=31536000).contains(&interval) {
                return Err(ApiError::BadRequest("invalid schedule interval".into()));
            }
            let changed=transaction.execute(sql("INSERT INTO operation_schedules(id,project_id,kind,payload_json,interval_secs,next_run_at,enabled) VALUES(?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET kind=excluded.kind,payload_json=excluded.payload_json,interval_secs=excluded.interval_secs,enabled=excluded.enabled,revision=operation_schedules.revision+1 WHERE operation_schedules.project_id=excluded.project_id AND operation_schedules.revision=?",vec![resource_id.clone().into(),project.clone().into(),kind.into(),value["payload"].to_string().into(),interval.into(),db::now().into(),value["enabled"].as_bool().unwrap_or(true).into(),value["revision"].as_i64().unwrap_or(0).into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Conflict("schedule revision changed".into()));
            }
        }
        "probe" | "quota" => {
            let provider = text(&value, "provider_id")?;
            if transaction
                .query_one(sql(
                    "SELECT id FROM providers WHERE id=? AND project_id=?",
                    vec![provider.into(), project.clone().into()],
                ))
                .await?
                .is_none()
            {
                return Err(ApiError::NotFound);
            }
            let job = jobs::enqueue(
                &transaction,
                Some(&project),
                &resource,
                &format!("manual:{}", id()),
                &value,
                db::now(),
            )
            .await?;
            audit_in(&transaction, &user, &project, "job.enqueue", &job).await?;
            transaction.commit().await?;
            return Ok(Json(json!({"id":job})));
        }
        "retention" => {
            let kind = text(&value, "resource_type")?;
            if !matches!(kind, "requests" | "payloads" | "probes" | "quota") {
                return Err(ApiError::BadRequest("invalid retention resource".into()));
            }
            let days = value["retention_days"].as_i64().unwrap_or(30);
            if !(1..=3650).contains(&days) {
                return Err(ApiError::BadRequest("retention days must be 1–3650".into()));
            }
            transaction.execute(sql("INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,retain_payloads,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(project_id,resource_type) DO UPDATE SET retention_days=excluded.retention_days,retain_payloads=excluded.retain_payloads,updated_at=excluded.updated_at",vec![resource_id.clone().into(),project.clone().into(),kind.into(),days.into(),value["retain_payloads"].as_bool().unwrap_or(false).into(),db::now().into()])).await?;
        }
        "webhooks" => {
            let url = text(&value, "url")?;
            validate_http_url(url)?;
            let parsed = reqwest::Url::parse(url)
                .map_err(|_| ApiError::BadRequest("invalid webhook URL".into()))?;
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(ApiError::BadRequest(
                    "webhook credentials belong in encrypted secret_headers".into(),
                ));
            }
            let secret = if let Some(secret) = value
                .get("secret_headers")
                .filter(|secret| !secret.is_null())
            {
                Some(
                    state.secrets.encrypt(
                        &json!({"project_id":project,"webhook_id":resource_id,"headers":secret})
                            .to_string(),
                    )?,
                )
            } else {
                None
            };
            let changed=transaction.execute(sql("INSERT INTO webhooks(id,project_id,name,url,secret_envelope,headers_json,body_template_json,subscriptions_json,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,url=excluded.url,secret_envelope=COALESCE(excluded.secret_envelope,webhooks.secret_envelope),headers_json=excluded.headers_json,body_template_json=excluded.body_template_json,subscriptions_json=excluded.subscriptions_json,enabled=excluded.enabled,updated_at=excluded.updated_at WHERE webhooks.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),url.into(),secret.into(),json!({"version":1,"headers":value.get("headers").cloned().unwrap_or(json!({}))}).to_string().into(),json!({"version":1,"body":value.get("body").cloned().unwrap_or(json!("$event"))}).to_string().into(),json!({"version":1,"events":value.get("events").cloned().unwrap_or(json!([]))}).to_string().into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),db::now().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
        }
        "webhook-echo" => {
            operations::runtime::notify(
                &transaction,
                &project,
                "test",
                &id(),
                &json!({"echo":true}),
            )
            .await?;
        }
        _ => return Err(ApiError::NotFound),
    }
    audit_in(
        &transaction,
        &user,
        &project,
        &format!("{resource}.save"),
        &resource_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(json!({"id":resource_id})))
}
async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, resource, id)): Path<(String, String, String)>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    if !matches!(
        resource.as_str(),
        "storage"
            | "schedules"
            | "webhooks"
            | "retention"
            | "groups"
            | "channels"
            | "credentials"
            | "models"
            | "associations"
            | "key-profiles"
            | "prompts"
            | "protection"
    ) {
        return Err(ApiError::BadRequest("resource is immutable".into()));
    }
    let (table, scope, _) = query(&resource)?;
    let tx = state.db.begin().await?;
    let changed = tx
        .execute(sql(
            format!("DELETE FROM {table} WHERE ({scope}) AND id=?"),
            vec![project.clone().into(), id.clone().into()],
        ))
        .await?
        .rows_affected();
    if changed != 1 {
        return Err(ApiError::NotFound);
    }
    audit_in(&tx, &user, &project, &format!("{resource}.delete"), &id).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(selection): Json<backup::Selection>,
) -> Result<Json<backup::Artifact>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let artifact = backup::export_as(&state, &project, &selection, Some(&user)).await?;
    Ok(Json(artifact))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restore {
    artifact: backup::Artifact,
    strategy: backup::Conflict,
}
async fn restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<Restore>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let count = backup::restore_as(
        &state,
        &project,
        &input.artifact,
        input.strategy,
        Some(&user),
    )
    .await?;
    Ok(Json(json!({"restored":count})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    targets: Vec<String>,
    resources: Vec<String>,
}
async fn run_backup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<Run>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let tx = state.db.begin().await?;
    let job = backup::enqueue_in(
        &tx,
        &project,
        &input.targets,
        &backup::Selection {
            resources: input.resources,
        },
        &format!("manual:{}", id()),
    )
    .await?;
    audit_in(&tx, &user, &project, "backup.enqueue", &job).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":job})))
}
async fn retry_backup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, job)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let id = backup::retry_as(&state, &project, &job, Some(&user)).await?;
    Ok(Json(json!({"id":id})))
}
