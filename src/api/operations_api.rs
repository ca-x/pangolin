use super::*;
use crate::operations::{self, backup, id, jobs, logging, pricing, proxy, sql, storage};
use sea_orm::{ConnectionTrait, DatabaseTransaction, TransactionTrait};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::trace_preview;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelRulesInput {
    version: u8,
    strip_prefix: Option<String>,
    auto_trim_prefixes: Option<Vec<String>>,
    lowercase: Option<bool>,
    mappings: Option<std::collections::BTreeMap<String, String>>,
    prefix: Option<String>,
    hide_original: Option<bool>,
    hide_mapped: Option<bool>,
    exclude: Option<Vec<String>>,
    stream: Option<bool>,
    developer_to_system: Option<bool>,
    force_content_array: Option<bool>,
    reasoning_effort: Option<std::collections::BTreeMap<String, String>>,
}

fn validate_model_rules(value: &Value) -> Result<(), ApiError> {
    if value.to_string().len() > 64 * 1024 {
        return Err(ApiError::BadRequest("invalid model rules".into()));
    }
    let rules: ModelRulesInput = serde_json::from_value(value.clone())
        .map_err(|_| ApiError::BadRequest("invalid model rules".into()))?;
    if rules.version != 1 {
        return Err(ApiError::BadRequest("invalid model rules".into()));
    }
    let valid_text = |value: &str| {
        !value.is_empty()
            && value.len() <= 512
            && value.trim() == value
            && !value.chars().any(char::is_control)
    };
    if rules
        .strip_prefix
        .as_deref()
        .is_some_and(|value| !valid_text(value))
        || rules
            .prefix
            .as_deref()
            .is_some_and(|value| !valid_text(value))
    {
        return Err(ApiError::BadRequest("invalid model rules".into()));
    }
    if let Some(prefixes) = &rules.auto_trim_prefixes {
        let mut unique = std::collections::HashSet::new();
        if prefixes.len() > 64
            || prefixes.iter().any(|prefix| {
                !valid_text(prefix)
                    || prefix.starts_with('/')
                    || prefix.ends_with('/')
                    || !unique.insert(prefix)
            })
        {
            return Err(ApiError::BadRequest("invalid model rules".into()));
        }
    }
    if rules.mappings.as_ref().is_some_and(|mappings| {
        mappings.len() > 256
            || mappings
                .iter()
                .any(|(source, target)| !valid_text(source) || !valid_text(target))
    }) {
        return Err(ApiError::BadRequest("invalid model rules".into()));
    }
    if let Some(patterns) = &rules.exclude {
        if patterns.len() > 64
            || patterns
                .iter()
                .any(|pattern| !valid_text(pattern) || regex::Regex::new(pattern).is_err())
        {
            return Err(ApiError::BadRequest("invalid model rules".into()));
        }
    }
    if rules.reasoning_effort.as_ref().is_some_and(|mappings| {
        mappings.len() > 32
            || mappings
                .iter()
                .any(|(source, target)| !valid_text(source) || !valid_text(target))
    }) {
        return Err(ApiError::BadRequest("invalid model rules".into()));
    }
    let _ = (
        rules.lowercase,
        rules.hide_original,
        rules.hide_mapped,
        rules.stream,
        rules.developer_to_system,
        rules.force_content_array,
    );
    Ok(())
}

pub(super) fn router(state: AppState) -> Router<AppState> {
    let instance = Router::new()
        .route("/api/admin/v1/instance/backup", post(instance_export))
        .route(
            "/api/admin/v1/instance/restore/preflight",
            post(instance_preflight),
        )
        .route("/api/admin/v1/instance/restore", post(instance_restore))
        .route(
            "/api/admin/v1/instance/diagnostics/cache",
            get(cache_diagnostics).delete(clear_cache),
        )
        .route(
            "/api/admin/v1/instance/proxy-presets",
            get(proxy_presets).post(save_proxy_preset),
        )
        .route(
            "/api/admin/v1/instance/proxy-presets/{id}",
            delete(delete_proxy_preset),
        )
        .layer(axum::extract::DefaultBodyLimit::max(
            operations::instance_backup::MAX_ARTIFACT_BYTES,
        ))
        .layer(middleware::from_fn_with_state(state, instance_owner));
    let profile_templates = Router::new()
        .route(
            "/api/admin/v1/projects/{project}/profile-templates",
            get(list_profile_templates).post(create_profile_template),
        )
        .route(
            "/api/admin/v1/projects/{project}/profile-templates/import",
            post(import_profile_template),
        )
        .route(
            "/api/admin/v1/projects/{project}/profile-templates/{id}",
            axum::routing::put(update_profile_template).delete(delete_profile_template),
        )
        .route(
            "/api/admin/v1/projects/{project}/profile-templates/{id}/export",
            get(export_profile_template),
        )
        .route(
            "/api/admin/v1/projects/{project}/profile-templates/{id}/apply",
            post(apply_profile_template),
        )
        .layer(axum::extract::DefaultBodyLimit::max(128 * 1024));
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
            "/api/admin/v1/projects/{project}/providers/{provider}/oauth/{flow}/start",
            post(start_provider_oauth),
        )
        .route(
            "/api/admin/v1/projects/{project}/providers/{provider}/oauth/{flow}/complete",
            post(complete_provider_oauth),
        )
        .route(
            "/api/admin/v1/settings/models",
            get(model_settings).put(set_model_settings),
        )
        .route(
            "/api/admin/v1/projects/{project}/operations/{resource}",
            get(list).post(mutate),
        )
        .route(
            "/api/admin/v1/projects/{project}/operations/{resource}/{id}",
            get(detail).delete(remove),
        )
        .route(
            "/api/admin/v1/projects/{project}/operations/storage/{id}/test",
            post(test_storage),
        )
        .route(
            "/api/admin/v1/projects/{project}/traces/{id}/lifecycle",
            post(trace_lifecycle),
        )
        .route(
            "/api/admin/v1/projects/{project}/models/{id}/lifecycle",
            post(model_lifecycle),
        )
        .route(
            "/api/admin/v1/projects/{project}/models/{id}/delete-impact",
            get(model_delete_impact),
        )
        .route(
            "/api/admin/v1/projects/{project}/models/batch",
            post(batch_create_models).layer(axum::extract::DefaultBodyLimit::max(256 * 1024)),
        )
        .route(
            "/api/admin/v1/projects/{project}/models/bulk",
            post(bulk_models).layer(axum::extract::DefaultBodyLimit::max(64 * 1024)),
        )
        .route(
            "/api/admin/v1/projects/{project}/models/unassociated",
            get(unassociated_models),
        )
        .route(
            "/api/admin/v1/projects/{project}/playground/chat",
            post(playground_chat).layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route(
            "/api/admin/v1/projects/{project}/playground/models",
            get(playground_models),
        )
        .route("/api/admin/v1/projects/{project}/analytics", get(analytics))
        .route(
            "/api/admin/v1/projects/{project}/live-requests",
            get(live_requests),
        )
        .route(
            "/api/admin/v1/projects/{project}/settings/orchestration",
            get(orchestration_settings).put(set_orchestration_settings),
        )
        .route(
            "/api/admin/v1/projects/{project}/settings/developers",
            get(developer_settings).put(set_developer_settings),
        )
        .route(
            "/api/admin/v1/projects/{project}/credentials/{id}/recovery-token",
            post(issue_credential_recovery_token),
        )
        .route(
            "/api/admin/v1/projects/{project}/credentials/{id}/recover",
            post(recover_credential),
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
            "/api/admin/v1/projects/{project}/protection-preview",
            post(protection_preview).layer(axum::extract::DefaultBodyLimit::max(32 * 1024)),
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
            "/api/admin/v1/projects/{project}/backup/artifacts",
            get(list_artifacts),
        )
        .route(
            "/api/admin/v1/projects/{project}/backup/artifacts/{artifact}",
            get(get_artifact),
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
        .merge(profile_templates)
        .merge(instance)
}

async fn cache_diagnostics(State(state): State<AppState>) -> Json<operations::diagnostics::Export> {
    Json(operations::diagnostics::export(&state))
}

async fn clear_cache(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<operations::diagnostics::Export>, ApiError> {
    let owner = actor(&state, &headers, None, true).await?;
    Ok(Json(operations::diagnostics::clear(&state, &owner).await?))
}

async fn proxy_presets(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = state
        .db
        .query_all(sql(
            "SELECT id,name,url,enabled,created_at,updated_at FROM proxy_presets ORDER BY name,id LIMIT 100",
            vec![],
        ))
        .await?;
    let mut data = Vec::with_capacity(rows.len());
    for row in rows {
        data.push(json!({
            "id":row.try_get::<String>("","id")?,
            "name":row.try_get::<String>("","name")?,
            "url":row.try_get::<String>("","url")?,
            "enabled":row.try_get::<bool>("","enabled")?,
            "created_at":row.try_get::<i64>("","created_at")?,
            "updated_at":row.try_get::<i64>("","updated_at")?,
        }));
    }
    let total = data.len();
    Ok(Json(json!({"data":data,"total":total})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProxyPresetInput {
    id: Option<String>,
    name: String,
    url: String,
    credentials: Option<Value>,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

async fn save_proxy_preset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ProxyPresetInput>,
) -> Result<Json<Value>, ApiError> {
    let owner = actor(&state, &headers, None, true).await?;
    if input.name.trim().is_empty() || input.name.len() > 128 {
        return Err(ApiError::BadRequest("invalid proxy preset name".into()));
    }
    proxy::validate_url(&input.url)?;
    let preset = input.id.unwrap_or_else(id);
    let envelope = input
        .credentials
        .as_ref()
        .map(|value| proxy::envelope(&state, &preset, value))
        .transpose()?;
    let tx = state.db.begin().await?;
    let exists = tx
        .query_one(sql(
            "SELECT id FROM proxy_presets WHERE id=?",
            vec![preset.clone().into()],
        ))
        .await?
        .is_some();
    if !exists {
        let count = tx
            .query_one(sql("SELECT COUNT(*) AS count FROM proxy_presets", vec![]))
            .await?
            .ok_or(ApiError::NotFound)?
            .try_get::<i64>("", "count")?;
        if count >= 100 {
            return Err(ApiError::BadRequest("proxy preset limit reached".into()));
        }
    }
    let changed = tx.execute(sql(
        "INSERT INTO proxy_presets(id,name,url,secret_envelope,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,url=excluded.url,secret_envelope=COALESCE(excluded.secret_envelope,proxy_presets.secret_envelope),enabled=excluded.enabled,updated_at=excluded.updated_at",
        vec![preset.clone().into(),input.name.into(),input.url.into(),envelope.into(),input.enabled.into(),db::now().into(),db::now().into()],
    )).await?.rows_affected();
    if changed != 1 {
        return Err(ApiError::Conflict("proxy preset changed".into()));
    }
    db::record_audit_event_in(
        &tx,
        owner.user_id.as_deref().ok_or(ApiError::Forbidden)?,
        "proxy_preset.save",
        "proxy_preset",
        &preset,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":preset})))
}

async fn delete_proxy_preset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(preset): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let owner = actor(&state, &headers, None, true).await?;
    let tx = state.db.begin().await?;
    if tx
        .execute(sql(
            "DELETE FROM proxy_presets WHERE id=?",
            vec![preset.clone().into()],
        ))
        .await?
        .rows_affected()
        != 1
    {
        return Err(ApiError::NotFound);
    }
    db::record_audit_event_in(
        &tx,
        owner.user_id.as_deref().ok_or(ApiError::Forbidden)?,
        "proxy_preset.delete",
        "proxy_preset",
        &preset,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":preset})))
}

async fn instance_owner(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, ApiError> {
    // The layer enforces owner scope for every instance route. Individual
    // mutating handlers perform the CSRF-bearing `true` check themselves.
    actor(&state, request.headers(), None, false).await?;
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
    actor_for(
        state,
        headers,
        project,
        if project.is_none() {
            "*"
        } else if write {
            "project:manage"
        } else {
            "project:read"
        },
        write,
    )
    .await
}
/// The same principal resolution with the required permission named explicitly.
///
/// Generic operations require `project:manage`, but a mutation that belongs to a
/// narrower contract must require *that* contract's permission: a custom role holding
/// only `api_key:manage` may change a key through the dedicated route, so the bulk key
/// action has to accept exactly the same authority instead of the generic gate. The
/// browser-mutation guard is not part of the permission question and is applied
/// identically for both.
async fn actor_for(
    state: &AppState,
    headers: &HeaderMap,
    project: Option<&str>,
    permission: &str,
    write: bool,
) -> Result<crate::access::Principal, ApiError> {
    let user = crate::access_api::principal(state, headers).await?;
    crate::access::authorize(&state.db, &user, project, permission)
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
/// The effective policy: supported stored versions tolerate unknown fields, and
/// refused documents become the writable logging-off repair shape. The console
/// reads the policy and writes back what it read, so raw unknown fields would
/// otherwise leave the operator with a form whose only answer is the strict
/// write contract's 400.
async fn log_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<logging::Policy>, ApiError> {
    actor(&state, &headers, None, false).await?;
    let row = state
        .db
        .query_one(sql(
            "SELECT CASE WHEN typeof(value)='text' THEN value END AS value,typeof(value) AS value_type FROM settings WHERE key='request_logging'",
            vec![],
        ))
        .await?;
    Ok(Json(match row {
        Some(row) => match logging::parse_stored_row(&row) {
            Ok(Some(policy)) => policy,
            Ok(None) => logging::Policy::default(),
            Err(problem) => {
                tracing::warn!(
                    problem = problem.code(),
                    "stored request-logging policy is invalid; settings read is using a writable logging-off repair shape"
                );
                logging::Policy::fail_closed()
            }
        },
        None => logging::Policy::default(),
    }))
}
async fn set_log_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(document): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, None, true).await?;
    // Strict at the boundary, tolerant in the record system: the extracted
    // document is validated by the write contract before anything is converted
    // into the runtime policy, so a typo is a 400 that writes nothing rather than
    // a silently different policy that is stored and audited.
    let policy: logging::Policy = logging::PolicyInput::parse(document)?.into();
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
    #[serde(default)]
    cors_allowed_origins: Vec<String>,
    #[serde(default = "default_request_timeout_ms")]
    request_timeout_ms: u64,
}
fn default_request_timeout_ms() -> u64 {
    super::http_policy::DEFAULT_REQUEST_TIMEOUT_MS
}
impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            instance_name: "Pangolin".into(),
            branding_name: "Pangolin / 鲮鲤".into(),
            favicon_url: "/logo.webp".into(),
            onboarding_complete: false,
            cors_allowed_origins: Vec::new(),
            request_timeout_ms: default_request_timeout_ms(),
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
        || !(super::http_policy::HttpPolicy {
            cors_allowed_origins: input.cors_allowed_origins.clone(),
            request_timeout_ms: input.request_timeout_ms,
        })
        .validate()
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderOauthStartInput {
    client_id: String,
    redirect_uri: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderOauthCompleteInput {
    state: String,
    code: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct ProviderOauthSecret {
    verifier: Option<String>,
    device_code: Option<String>,
}

fn oauth_state_digest(state: &str) -> String {
    blake3::hash(state.as_bytes()).to_hex().to_string()
}

fn validate_oauth_redirect(
    state: &AppState,
    headers: &HeaderMap,
    redirect_uri: &str,
) -> Result<(), ApiError> {
    if redirect_uri.len() > 2048 {
        return Err(ApiError::BadRequest("invalid OAuth redirect URI".into()));
    }
    let redirect = reqwest::Url::parse(redirect_uri)
        .map_err(|_| ApiError::BadRequest("invalid OAuth redirect URI".into()))?;
    if !matches!(redirect.scheme(), "http" | "https")
        || redirect.host_str().is_none()
        || !redirect.username().is_empty()
        || redirect.password().is_some()
        || redirect.query().is_some()
        || redirect.fragment().is_some()
    {
        return Err(ApiError::BadRequest("invalid OAuth redirect URI".into()));
    }
    let origin = redirect.origin().ascii_serialization();
    let request_origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .and_then(super::http_policy::canonical_origin);
    let public_origin = state
        .config
        .public_url
        .as_deref()
        .and_then(|value| reqwest::Url::parse(value).ok())
        .and_then(|mut value| {
            value.set_path("");
            value.set_query(None);
            value.set_fragment(None);
            super::http_policy::canonical_origin(value.as_str())
        });
    if request_origin.as_deref() != Some(origin.as_str())
        && public_origin.as_deref() != Some(origin.as_str())
    {
        return Err(ApiError::BadRequest(
            "OAuth redirect origin is not allowed".into(),
        ));
    }
    Ok(())
}

async fn start_provider_oauth(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, provider, flow)): Path<(String, String, String)>,
    Json(input): Json<ProviderOauthStartInput>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    if input.client_id.trim().is_empty()
        || input.client_id.trim() != input.client_id
        || input.client_id.len() > 512
        || input.client_id.chars().any(char::is_control)
    {
        return Err(ApiError::BadRequest("invalid OAuth client ID".into()));
    }
    if state
        .db
        .query_one(sql(
            "SELECT id FROM providers WHERE id=? AND project_id=?",
            vec![provider.clone().into(), project.clone().into()],
        ))
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    let flow = crate::oauth::Flow::parse(&flow)
        .ok_or_else(|| ApiError::BadRequest("unsupported OAuth flow".into()))?;
    let spec = provider_oauth_spec(&state, &provider, flow).await?;
    let (response, secret, expires_in, interval) = if flow.is_device() {
        if input.redirect_uri.is_some() {
            return Err(ApiError::BadRequest(
                "device OAuth does not accept a redirect URI".into(),
            ));
        }
        let start = crate::oauth::start_device(&state.oidc_client, spec, &input.client_id)
            .await
            .map_err(provider_oauth_error)?;
        let secret = ProviderOauthSecret {
            verifier: None,
            device_code: Some(start.device_code.clone()),
        };
        let expires_in = start.expires_in;
        let interval = start.interval;
        (
            serde_json::to_value(&start).unwrap(),
            secret,
            expires_in,
            interval,
        )
    } else {
        let redirect = input
            .redirect_uri
            .as_deref()
            .ok_or_else(|| ApiError::BadRequest("OAuth redirect URI is required".into()))?;
        validate_oauth_redirect(&state, &headers, redirect)?;
        let start = crate::oauth::start_browser(spec, &input.client_id, redirect)
            .map_err(provider_oauth_error)?;
        let secret = ProviderOauthSecret {
            verifier: Some(start.verifier.clone()),
            device_code: None,
        };
        (
            serde_json::to_value(&start).unwrap(),
            secret,
            crate::oauth::MAX_FLOW_LIFETIME_SECS as u64,
            0,
        )
    };
    let state_value = response["state"]
        .as_str()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("OAuth start omitted state")))?;
    let digest = oauth_state_digest(state_value);
    let envelope = state
        .secrets
        .encrypt(&serde_json::to_string(&secret).unwrap())?;
    let now = db::now();
    let tx = state.db.begin().await?;
    tx.execute(sql(
        "DELETE FROM provider_oauth_states WHERE project_id=? AND provider_id=? AND flow=?",
        vec![
            project.clone().into(),
            provider.clone().into(),
            flow.as_str().into(),
        ],
    ))
    .await?;
    tx.execute(sql(
        "INSERT INTO provider_oauth_states(state_digest,project_id,provider_id,flow,client_id,redirect_uri,secret_envelope,interval_seconds,expires_at,created_at) VALUES(?,?,?,?,?,?,?,?,?,?)",
        vec![
            digest.into(),
            project.clone().into(),
            provider.clone().into(),
            flow.as_str().into(),
            input.client_id.into(),
            input.redirect_uri.into(),
            envelope.into(),
            (interval as i64).into(),
            (now + expires_in as i64).into(),
            now.into(),
        ],
    ))
    .await?;
    audit_in(&tx, &user, &project, "provider_oauth.start", &provider).await?;
    tx.commit().await?;
    Ok(Json(response))
}

async fn complete_provider_oauth(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, provider, flow)): Path<(String, String, String)>,
    Json(input): Json<ProviderOauthCompleteInput>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let flow = crate::oauth::Flow::parse(&flow)
        .ok_or_else(|| ApiError::BadRequest("unsupported OAuth flow".into()))?;
    if input.state.len() < 32 || input.state.len() > 256 {
        return Err(ApiError::BadRequest("invalid OAuth state".into()));
    }
    let digest = oauth_state_digest(&input.state);
    let row = state
        .db
        .query_one(sql(
            "SELECT client_id,redirect_uri,secret_envelope,interval_seconds,last_poll_at,expires_at FROM provider_oauth_states WHERE state_digest=? AND project_id=? AND provider_id=? AND flow=?",
            vec![
                digest.clone().into(),
                project.clone().into(),
                provider.clone().into(),
                flow.as_str().into(),
            ],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    let now = db::now();
    let expires_at = row.try_get::<i64>("", "expires_at")?;
    if expires_at <= now {
        let tx = state.db.begin().await?;
        tx.execute(sql(
            "DELETE FROM provider_oauth_states WHERE state_digest=?",
            vec![digest.into()],
        ))
        .await?;
        audit_in(&tx, &user, &project, "provider_oauth.expired", &provider).await?;
        tx.commit().await?;
        return Err(ApiError::BadRequest("OAuth authorization expired".into()));
    }
    let client_id = row.try_get::<String>("", "client_id")?;
    let redirect_uri = row.try_get::<Option<String>>("", "redirect_uri")?;
    let interval = row.try_get::<i64>("", "interval_seconds")?;
    let last_poll = row.try_get::<Option<i64>>("", "last_poll_at")?;
    if flow.is_device() && last_poll.is_some_and(|last| now < last + interval) {
        return Ok(Json(
            json!({"status":"pending","retry_after":last_poll.unwrap() + interval - now}),
        ));
    }
    if flow.is_device() {
        let tx = state.db.begin().await?;
        tx.execute(sql(
            "UPDATE provider_oauth_states SET last_poll_at=? WHERE state_digest=?",
            vec![now.into(), digest.clone().into()],
        ))
        .await?;
        audit_in(&tx, &user, &project, "provider_oauth.poll", &provider).await?;
        tx.commit().await?;
    }
    let secret_document = state
        .secrets
        .decrypt(&row.try_get::<String>("", "secret_envelope")?)?;
    let secret: ProviderOauthSecret =
        serde_json::from_str(&secret_document).map_err(|error| ApiError::Internal(error.into()))?;
    let spec = provider_oauth_spec(&state, &provider, flow).await?;
    let token = if flow.is_device() {
        crate::oauth::poll_device(
            &state.oidc_client,
            spec,
            &client_id,
            secret
                .device_code
                .as_deref()
                .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("device code missing")))?,
        )
        .await
    } else {
        crate::oauth::exchange_browser(
            &state.oidc_client,
            spec,
            &client_id,
            redirect_uri
                .as_deref()
                .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("redirect URI missing")))?,
            input
                .code
                .as_deref()
                .filter(|code| !code.is_empty() && code.len() <= 4096)
                .ok_or_else(|| ApiError::BadRequest("OAuth code is required".into()))?,
            secret
                .verifier
                .as_deref()
                .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("PKCE verifier missing")))?,
        )
        .await
    };
    let token = match token {
        Ok(token) => token.into_value(),
        Err(crate::oauth::Error::Pending) => {
            return Ok(Json(
                json!({"status":"pending","retry_after":interval.max(5)}),
            ));
        }
        Err(crate::oauth::Error::Expired | crate::oauth::Error::Declined) => {
            let tx = state.db.begin().await?;
            tx.execute(sql(
                "DELETE FROM provider_oauth_states WHERE state_digest=?",
                vec![digest.into()],
            ))
            .await?;
            audit_in(&tx, &user, &project, "provider_oauth.failed", &provider).await?;
            tx.commit().await?;
            return Err(ApiError::BadRequest(
                "OAuth authorization did not complete".into(),
            ));
        }
        Err(error) => return Err(provider_oauth_error(error)),
    };
    let credential = operations::id();
    let token_envelope = state.secrets.encrypt(&token.to_string())?;
    let tx = state.db.begin().await?;
    tx.execute(sql(
        "INSERT INTO channel_credentials(id,provider_id,credential_type,secret_envelope,suffix,priority,enabled,settings_json,created_at,updated_at) VALUES(?,?,?,?,'',100,1,?,?,?)",
        vec![
            credential.clone().into(),
            provider.clone().into(),
            format!("oauth_{}", flow.as_str()).into(),
            token_envelope.into(),
            json!({"version":1,"oauth_flow":flow.as_str()}).to_string().into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    tx.execute(sql(
        "DELETE FROM provider_oauth_states WHERE state_digest=?",
        vec![digest.into()],
    ))
    .await?;
    audit_in(
        &tx,
        &user,
        &project,
        "credentials.oauth.create",
        &credential,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"status":"complete","credential_id":credential}),
    ))
}

fn provider_oauth_error(error: crate::oauth::Error) -> ApiError {
    match error {
        crate::oauth::Error::InvalidConfiguration => {
            ApiError::BadRequest("invalid OAuth configuration".into())
        }
        crate::oauth::Error::Pending => ApiError::Conflict("OAuth authorization is pending".into()),
        crate::oauth::Error::Expired => ApiError::BadRequest("OAuth authorization expired".into()),
        crate::oauth::Error::Declined => {
            ApiError::BadRequest("OAuth authorization was declined".into())
        }
        crate::oauth::Error::ProviderUnavailable => {
            ApiError::Upstream("OAuth provider is unavailable".into())
        }
    }
}

async fn provider_oauth_spec(
    state: &AppState,
    provider: &str,
    flow: crate::oauth::Flow,
) -> Result<crate::oauth::ProviderSpec, ApiError> {
    #[cfg(test)]
    {
        let settings = state
            .db
            .query_one(sql(
                "SELECT settings_json FROM providers WHERE id=?",
                vec![provider.into()],
            ))
            .await?
            .and_then(|row| row.try_get::<String>("", "settings_json").ok())
            .and_then(|document| serde_json::from_str::<Value>(&document).ok());
        if let Some(settings) = settings {
            let token = settings
                .pointer("/oauth_test/token_endpoint")
                .and_then(Value::as_str);
            if let Some(token) = token {
                if flow.is_device() {
                    if let Some(device) = settings
                        .pointer("/oauth_test/device_endpoint")
                        .and_then(Value::as_str)
                    {
                        return Ok(crate::oauth::ProviderSpec::test_device(
                            device.to_owned(),
                            token.to_owned(),
                        ));
                    }
                } else if let Some(authorization) = settings
                    .pointer("/oauth_test/authorization_endpoint")
                    .and_then(Value::as_str)
                {
                    return Ok(crate::oauth::ProviderSpec::test_browser(
                        authorization.to_owned(),
                        token.to_owned(),
                    ));
                }
            }
        }
    }
    let _ = (state, provider);
    Ok(crate::oauth::ProviderSpec::for_flow(flow))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelSettingsInput {
    version: u32,
    fallback_to_channels_on_model_not_found: bool,
    query_all_channel_models: bool,
    default_model_api_include_all: bool,
    auto_reasoning_effort: bool,
    model_blacklist_regex: String,
    hide_unroutable_models_in_list: bool,
}

impl ModelSettingsInput {
    fn parse(document: Value) -> Result<crate::orchestration::ModelSettings, ApiError> {
        let invalid = || {
            ApiError::BadRequest(
                "invalid model settings: version 1 and only the documented fields and types are accepted"
                    .into(),
            )
        };
        if !document.is_object() {
            return Err(invalid());
        }
        let input: Self = serde_json::from_value(document).map_err(|_| invalid())?;
        if input.version != 1
            || crate::orchestration::model_blacklist_regex(&input.model_blacklist_regex).is_err()
        {
            return Err(invalid());
        }
        Ok(crate::orchestration::ModelSettings {
            version: input.version,
            fallback_to_channels_on_model_not_found: input.fallback_to_channels_on_model_not_found,
            query_all_channel_models: input.query_all_channel_models,
            default_model_api_include_all: input.default_model_api_include_all,
            auto_reasoning_effort: input.auto_reasoning_effort,
            model_blacklist_regex: input.model_blacklist_regex,
            hide_unroutable_models_in_list: input.hide_unroutable_models_in_list,
        })
    }
}

async fn model_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::orchestration::ModelSettings>, ApiError> {
    actor(&state, &headers, None, false).await?;
    Ok(Json(crate::orchestration::model_settings(&state.db).await?))
}

async fn set_model_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(document): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, None, true).await?;
    let settings = ModelSettingsInput::parse(document)?;
    let tx = state.db.begin().await?;
    tx.execute(sql(
        "INSERT INTO settings(key,value,updated_at) VALUES('model_settings',?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
        vec![serde_json::to_string(&settings)
            .map_err(|error| ApiError::Internal(error.into()))?
            .into(), db::now().into()],
    ))
    .await?;
    audit_in(&tx, &user, "", "model-settings.update", "model_settings").await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeveloperAssociation {
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    channel_tags: Vec<String>,
    #[serde(default = "default_document")]
    conditions: Value,
    #[serde(default)]
    priority: i32,
    #[serde(default = "default_weight")]
    weight: u32,
}
const fn default_weight() -> u32 {
    1
}
fn default_document() -> Value {
    json!({"version":1})
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeveloperRule {
    developer: String,
    #[serde(default)]
    associations: Vec<DeveloperAssociation>,
    #[serde(default)]
    reasoning_effort: BTreeMap<String, String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeveloperSettings {
    version: u8,
    #[serde(default)]
    rules: Vec<DeveloperRule>,
}
impl Default for DeveloperSettings {
    fn default() -> Self {
        Self {
            version: 1,
            rules: vec![],
        }
    }
}
fn validate_developer_settings(input: &DeveloperSettings) -> Result<(), ApiError> {
    if input.version != 1 || input.rules.len() > 128 {
        return Err(ApiError::BadRequest("invalid developer settings".into()));
    }
    let mut names = std::collections::BTreeSet::new();
    for rule in &input.rules {
        if rule.developer.trim().is_empty()
            || rule.developer.len() > 128
            || !names.insert(rule.developer.as_str())
            || rule.associations.len() > 128
            || rule.reasoning_effort.len() > 32
        {
            return Err(ApiError::BadRequest("invalid developer settings".into()));
        }
        for association in &rule.associations {
            if association.provider_id.is_none() && association.channel_tags.is_empty()
                || association.weight == 0
                || association.weight > 10_000
                || association.channel_tags.len() > 64
            {
                return Err(ApiError::BadRequest("invalid developer association".into()));
            }
            crate::orchestration::policy::validate_conditions(&association.conditions).map_err(
                |_| ApiError::BadRequest("invalid developer association conditions".into()),
            )?;
        }
        if rule
            .reasoning_effort
            .iter()
            .any(|(from, to)| from.is_empty() || to.is_empty() || from.len() > 64 || to.len() > 64)
        {
            return Err(ApiError::BadRequest(
                "invalid reasoning-effort mapping".into(),
            ));
        }
    }
    Ok(())
}
async fn developer_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
) -> Result<Json<DeveloperSettings>, ApiError> {
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
    serde_json::from_value(
        settings
            .get("developer_settings")
            .cloned()
            .unwrap_or_else(|| json!(DeveloperSettings::default())),
    )
    .map(Json)
    .map_err(|_| ApiError::BadRequest("invalid stored developer settings".into()))
}
async fn set_developer_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<DeveloperSettings>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    validate_developer_settings(&input)?;
    let tx = state.db.begin().await?;
    let row = tx
        .query_one(sql(
            "SELECT settings_json FROM projects WHERE id=?",
            vec![project.clone().into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    for provider in input
        .rules
        .iter()
        .flat_map(|rule| rule.associations.iter())
        .filter_map(|association| association.provider_id.as_deref())
    {
        if tx
            .query_one(sql(
                "SELECT 1 AS present FROM providers WHERE id=? AND project_id=?",
                vec![provider.into(), project.clone().into()],
            ))
            .await?
            .is_none()
        {
            return Err(ApiError::NotFound);
        }
    }
    let mut settings: Value = serde_json::from_str(&row.try_get::<String>("", "settings_json")?)
        .map_err(|error| ApiError::Internal(error.into()))?;
    settings["developer_settings"] = json!(input);
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
        "developer-settings.update",
        "developer-settings",
    )
    .await?;
    tx.commit().await?;
    state.orchestrator.reset_derived();
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryTokenInput {
    expires_seconds: Option<i64>,
}
async fn issue_credential_recovery_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, credential)): Path<(String, String)>,
    Json(input): Json<RecoveryTokenInput>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let expires = input.expires_seconds.unwrap_or(600);
    if !(60..=3600).contains(&expires) {
        return Err(ApiError::BadRequest(
            "recovery token expiry must be 60–3600 seconds".into(),
        ));
    }
    let row=state.db.query_one(sql("SELECT c.secret_envelope FROM channel_credentials c JOIN providers p ON p.id=c.provider_id WHERE c.id=? AND p.project_id=?",vec![credential.clone().into(),project.clone().into()])).await?.ok_or(ApiError::NotFound)?;
    let envelope: String = row.try_get("", "secret_envelope")?;
    if state.secrets.decrypt(&envelope).is_ok() {
        return Err(ApiError::Conflict("credential is still recoverable".into()));
    }
    let token = crate::crypto::opaque_token("pcr_");
    let hash = crate::crypto::token_hash(&token);
    let now = db::now();
    let tx = state.db.begin().await?;
    tx.execute(sql("DELETE FROM credential_recovery_tokens WHERE project_id=? AND credential_id=? AND used_at IS NULL",vec![project.clone().into(),credential.clone().into()])).await?;
    tx.execute(sql("INSERT INTO credential_recovery_tokens(token_hash,project_id,credential_id,created_by,expires_at,created_at) VALUES(?,?,?,?,?,?)",vec![hash.into(),project.clone().into(),credential.clone().into(),user.subject_id.clone().into(),(now+expires).into(),now.into()])).await?;
    audit_in(
        &tx,
        &user,
        &project,
        "credential.recovery.issue",
        &credential,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"token":token,"expires_at":now+expires})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoverCredentialInput {
    token: String,
    secret: String,
}
async fn recover_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, credential)): Path<(String, String)>,
    Json(input): Json<RecoverCredentialInput>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    if input.secret.trim().is_empty() || input.secret.len() > 64 * 1024 {
        return Err(ApiError::BadRequest(
            "replacement secret is required".into(),
        ));
    }
    let envelope = state.secrets.encrypt(&input.secret)?;
    let suffix = input
        .secret
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    let now = db::now();
    let tx = state.db.begin().await?;
    let changed=tx.execute(sql("UPDATE credential_recovery_tokens SET used_at=? WHERE token_hash=? AND project_id=? AND credential_id=? AND used_at IS NULL AND expires_at>?",vec![now.into(),crate::crypto::token_hash(&input.token).into(),project.clone().into(),credential.clone().into(),now.into()])).await?.rows_affected();
    if changed != 1 {
        return Err(ApiError::Conflict(
            "recovery token is invalid, expired, or already used".into(),
        ));
    }
    if tx.execute(sql("UPDATE channel_credentials SET secret_envelope=?,suffix=?,updated_at=? WHERE id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![envelope.into(),suffix.into(),now.into(),credential.clone().into(),project.clone().into()])).await?.rows_affected()!=1{return Err(ApiError::NotFound)}
    audit_in(
        &tx,
        &user,
        &project,
        "credential.recovery.replace",
        &credential,
    )
    .await?;
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
    /// The trace list's own lifecycle facet. Only the traces resource reads it.
    lifecycle: Option<String>,
}

/// A resource's stable point-in-time event column. Keeping the SQL spelling in a
/// closed type prevents a request value from ever becoming a SQL identifier.
#[derive(Clone, Copy)]
enum EventTimeColumn {
    Created,
    Started,
    Probed,
    Collected,
}

impl EventTimeColumn {
    const fn sql(self) -> &'static str {
        match self {
            Self::Created => "created_at",
            Self::Started => "started_at",
            Self::Probed => "probed_at",
            Self::Collected => "collected_at",
        }
    }
}

/// The generic list filters only stable event times. Mutable state timestamps,
/// future schedule times and validity intervals are intentionally not events.
fn event_time_column(resource: &str) -> Option<EventTimeColumn> {
    match resource {
        "channels" | "credentials" | "models" | "associations" | "key-profiles" | "prompts"
        | "protection" | "threads" | "usage" | "prices" | "storage" | "jobs" | "webhooks"
        | "webhook-deliveries" | "audit" => Some(EventTimeColumn::Created),
        "traces" | "requests" | "executions" => Some(EventTimeColumn::Started),
        "probes" => Some(EventTimeColumn::Probed),
        "quotas" => Some(EventTimeColumn::Collected),
        _ => None,
    }
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
            "json_object('id',provider_id,'provider_id',provider_id,'endpoint_mappings',json(endpoint_mappings_json),'model_rules',json(model_rules_json),'parameter_overrides',json(parameter_overrides_json),'retry_statuses',json(retry_statuses_json),'auto_disable_policy',json(auto_disable_policy_json),'proxy_url',proxy_url,'proxy_username',proxy_username,'proxy_password_configured',CASE WHEN proxy_secret_envelope IS NULL THEN json('false') ELSE json('true') END,'proxy_reuse_connections',json(CASE WHEN proxy_reuse_connections=1 THEN 'true' ELSE 'false' END),'proxy_preset_id',proxy_preset_id,'model_sync_error',model_sync_error,'model_synced_at',model_synced_at,'model_sync_count',model_sync_count,'updated_at',updated_at)",
        ),
        "models" => (
            "models",
            "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            "json_object('id',id,'provider_id',provider_id,'provider_name',(SELECT name FROM providers WHERE id=models.provider_id),'public_name',public_name,'upstream_name',upstream_name,'capabilities',json(capabilities),'input_price_micros',input_price_micros,'output_price_micros',output_price_micros,'priority',priority,'enabled',enabled,'lifecycle',lifecycle,'disable_developer_settings_inheritance',disable_developer_settings_inheritance,'catalog_metadata_raw',catalog_metadata_json,'created_at',created_at)",
        ),
        "associations" => (
            "model_associations",
            "project_id=?",
            "json_object('id',id,'model_id',model_id,'model_name',(SELECT m.public_name FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=model_associations.model_id AND p.project_id=model_associations.project_id),'provider_id',provider_id,'provider_name',(SELECT name FROM providers WHERE id=model_associations.provider_id AND project_id=model_associations.project_id),'match_type',match_type,'pattern',pattern,'conditions',json(conditions_json),'exclusions',json(exclusions_json),'priority',priority,'weight',weight,'enabled',enabled,'created_at',created_at,'updated_at',updated_at)",
        ),
        "key-profiles" => (
            "api_key_profiles",
            "project_id=?",
            "json_object('id',id,'name',name,'rpm_limit',rpm_limit,'tpm_limit',tpm_limit,'budget_micros',budget_micros,'routing_policy',json(routing_policy_json),'mappings',json(COALESCE((SELECT json_group_array(json_object('source_model',source_model,'target_model',target_model,'priority',priority)) FROM api_key_profile_model_mappings WHERE profile_id=api_key_profiles.id),'[]')),'allowed_models',json(COALESCE((SELECT json_group_array(json_object('pattern',model_pattern,'match_type',match_type)) FROM api_key_profile_allowed_models WHERE profile_id=api_key_profiles.id),'[]')),'created_at',created_at,'updated_at',updated_at)",
        ),
        "prompts" => (
            "prompts",
            "project_id=?",
            "json_object('id',id,'name',name,'role',role,'content',content,'activation',json(activation_json),'order',\"order\",'action',action,'enabled',enabled,'created_at',created_at,'updated_at',updated_at)",
        ),
        "protection" => (
            "prompt_protection_rules",
            "project_id=?",
            "json_object('id',id,'name',name,'description',description,'role_pattern',role_pattern,'content_pattern',content_pattern,'action',action,'replacement',replacement,'scopes',json(scopes_json),'test_mode',test_mode,'enabled',enabled,'state',state,'created_at',created_at,'updated_at',updated_at)",
        ),
        "health" => (
            "channel_health_state",
            "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            "json_object('id',provider_id,'provider_id',provider_id,'consecutive_failures',consecutive_failures,'disabled_until',disabled_until,'backoff_until',backoff_until,'reason',reason,'updated_at',updated_at)",
        ),
        "credential-health" => (
            "credential_health_state",
            "credential_id IN (SELECT c.id FROM channel_credentials c JOIN providers p ON p.id=c.provider_id WHERE p.project_id=?)",
            "json_object('id',credential_id,'credential_id',credential_id,'consecutive_failures',consecutive_failures,'disabled_until',disabled_until,'updated_at',updated_at)",
        ),
        "threads" => (
            "threads",
            "project_id=?",
            "json_object('id',id,'external_id',external_id,'api_key_id',api_key_id,'created_at',created_at)",
        ),
        "traces" => (
            "traces",
            "project_id=?",
            // `request_count` is a count of the record system's own request rows,
            // not of the projection: it stays right while observability is down.
            // `first_user_query` is filled in from the stored bodies of the page
            // by `trace_previews`, which is the only place a body is read.
            // `status` is what the run did and `lifecycle` is where the operator
            // filed it: two different facts, so they are two different columns.
            "json_object('id',id,'thread_id',thread_id,'external_id',external_id,'api_key_id',api_key_id,'status',status,'lifecycle',lifecycle,'started_at',started_at,'finished_at',finished_at,'request_count',(SELECT COUNT(*) FROM requests WHERE trace_id=traces.id),'first_user_query',NULL)",
        ),
        "requests" => (
            "requests",
            "trace_id IN (SELECT id FROM traces WHERE project_id=?)",
            "json_object('id',id,'trace_id',trace_id,'protocol',protocol,'endpoint',endpoint,'model',requested_model,'status',status,'source_ip',source_ip,'started_at',started_at,'finished_at',finished_at,'metadata',json(request_metadata_json))",
        ),
        "executions" => (
            "request_executions",
            "request_id IN (SELECT r.id FROM requests r JOIN traces t ON t.id=r.trace_id WHERE t.project_id=?)",
            "json_object('id',id,'request_id',request_id,'provider_id',provider_id,'provider_name',provider_name,'credential_suffix',credential_suffix,'attempt',attempt,'model',model,'status',status,'retry_reason',retry_reason,'latency_ms',latency_ms,'first_token_at',first_token_at,'http_status',http_status,'error_kind',error_kind)",
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
            "json_object('id',id,'provider_id',provider_id,'model',model,'success',success,'status_code',status_code,'latency_ms',latency_ms,'ttft_ms',ttft_ms,'output_tokens',output_tokens,'probed_at',probed_at,'error',error_code)",
        ),
        "quotas" => (
            "provider_quota_snapshots",
            "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            "json_object('id',id,'provider_id',provider_id,'credential_id',credential_id,'remaining_micros',remaining_micros,'period_start',period_start,'period_end',period_end,'quota',json(quota_json),'collected_at',collected_at)",
        ),
        "storage" => (
            "data_storage_configs",
            "project_id=?",
            "json_object('id',id,'name',name,'kind',kind,'config',json(config_json),'revision',revision,'enabled',enabled)",
        ),
        "schedules" => (
            "operation_schedules",
            "project_id=?",
            "json_object('id',id,'kind',kind,'payload',json(payload_json),'interval_secs',interval_secs,'next_run_at',next_run_at,'enabled',enabled,'revision',revision,'last_error',last_error)",
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
            "json_object('id',id,'name',name,'url',url,'subscriptions',json(subscriptions_json),'timeout_secs',timeout_secs,'proxy_preset_id',proxy_preset_id,'enabled',enabled)",
        ),
        "webhook-deliveries" => (
            "webhook_deliveries",
            "webhook_id IN (SELECT id FROM webhooks WHERE project_id=?)",
            "json_object('id',id,'webhook_id',webhook_id,'event_type',event_type,'attempt',attempt,'status',status,'response_status',response_status,'next_attempt_at',next_attempt_at)",
        ),
        "groups" => (
            "service_groups",
            "project_id=?",
            "json_object('id',id,'name',name,'tier',tier,'ratio_millionths',ratio_millionths,'channels',json(COALESCE((SELECT json_group_array(provider_id) FROM service_group_channels WHERE group_id=service_groups.id),'[]')),'enabled',enabled)",
        ),
        "retention" => (
            "data_retention_policies",
            "project_id=?",
            "json_object('id',id,'resource_type',resource_type,'retention_days',retention_days)",
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
    actor_for(
        &state,
        &headers,
        Some(&project),
        if resource == "key-profiles" {
            "api_key:manage"
        } else {
            "project:read"
        },
        false,
    )
    .await?;
    let (table, scope, projection) = query(&resource)?;
    let query = filter
        .q
        .filter(|value| !value.trim().is_empty() && value.len() <= 128)
        .map(|value| format!("%{}%", value.trim()));
    // The trace lifecycle facet is a predicate on the record system's own column,
    // not a text search: the default view is the one the operator contract names —
    // active and retained — and the archived traces stay reachable by asking for
    // them. Every value is a literal from this list, so nothing a caller sends
    // reaches the statement.
    let lifecycle = match (resource.as_str(), filter.lifecycle.as_deref()) {
        ("traces", None) => Some("lifecycle!='archived'"),
        ("traces", Some("active")) => Some("lifecycle='active'"),
        ("traces", Some("retained")) => Some("lifecycle='retained'"),
        ("traces", Some("archived")) => Some("lifecycle='archived'"),
        ("traces", Some("all")) => None,
        ("traces", Some(_)) => {
            return Err(ApiError::BadRequest(
                "unknown trace lifecycle filter".into(),
            ));
        }
        ("models", None | Some("active")) => Some("lifecycle='active'"),
        ("models", Some("archived")) => Some("lifecycle='archived'"),
        ("models", Some("all")) => None,
        ("models", Some(_)) => {
            return Err(ApiError::BadRequest(
                "unknown model lifecycle filter".into(),
            ));
        }
        _ => None,
    };
    let lifecycle = lifecycle
        .map(|predicate| format!(" AND ({predicate})"))
        .unwrap_or_default();
    // Build the project, resource-state and event-time predicates once. The page
    // and its total clone the same values, so pagination can never describe a
    // different window from the rows it accompanies.
    let mut predicate = format!("{scope}{lifecycle}");
    let mut scoped_values: Vec<sea_orm::Value> = vec![project.clone().into()];
    if let Some(column) = event_time_column(&resource) {
        if let Some(from) = filter.from {
            predicate.push_str(&format!(" AND {}>=?", column.sql()));
            scoped_values.push(from.into());
        }
        if let Some(until) = filter.until {
            predicate.push_str(&format!(" AND {}<?", column.sql()));
            scoped_values.push(until.into());
        }
    }
    let limit = filter.limit.unwrap_or(100).clamp(1, 500);
    let mut row_values = scoped_values.clone();
    row_values.extend([
        query.clone().into(),
        query.clone().into(),
        i64::from(limit).into(),
        i64::from(filter.offset).into(),
    ]);
    let rows=state.db.query_all(sql(format!("SELECT document FROM (SELECT {projection} AS document,rowid AS source_rowid FROM {table} WHERE {predicate}) WHERE (? IS NULL OR document LIKE ?) ORDER BY source_rowid DESC LIMIT ? OFFSET ?"),row_values)).await?;
    let mut total_values = scoped_values;
    total_values.extend([query.clone().into(), query.into()]);
    let total=state.db.query_one(sql(format!("SELECT COUNT(*) AS total FROM (SELECT {projection} AS document FROM {table} WHERE {predicate}) WHERE (? IS NULL OR document LIKE ?)"),total_values)).await?.map(|row|row.try_get::<i64>("","total")).transpose()?.unwrap_or(0);
    let mut data = documents(rows)?;
    // The one projection whose rows carry a derived read: the page's own traces,
    // enriched in a single bounded query rather than one read per row.
    if resource == "traces" {
        trace_previews(&state.db, &mut data).await?;
    }
    if resource == "models" {
        project_model_catalog_cards(&mut data);
    }
    if resource == "credentials" {
        let rows=state.db.query_all(sql("SELECT c.id,c.secret_envelope FROM channel_credentials c JOIN providers p ON p.id=c.provider_id WHERE p.project_id=?",vec![project.clone().into()])).await?;
        let states: HashMap<String, bool> = rows
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<String>("", "id")?,
                    state
                        .secrets
                        .decrypt(&row.try_get::<String>("", "secret_envelope")?)
                        .is_ok(),
                ))
            })
            .collect::<Result<_, sea_orm::DbErr>>()?;
        for credential in &mut data {
            if let Some(object) = credential.as_object_mut() {
                let recoverable = object
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| states.get(id))
                    .copied()
                    .unwrap_or(false);
                object.insert(
                    "state".into(),
                    json!(if recoverable {
                        "ready"
                    } else {
                        "unrecoverable"
                    }),
                );
            }
        }
    }
    Ok(Json(
        json!({"data":data,"total":total,"offset":filter.offset,"limit":limit}),
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

/// Turns the immutable catalog snapshot on a model into the small, typed card
/// projection the console and public model metadata can consume.
///
/// Historical databases may contain `{}`, an older card shape, or hand-edited
/// malformed text. None of those may make the project model list unavailable:
/// every unrecorded or wrongly typed fact stays JSON null. The original valid
/// metadata remains in the response for compatibility, but the console never
/// has to interpret that raw document.
fn project_model_catalog_cards(rows: &mut [Value]) {
    for row in rows {
        let raw = row
            .as_object_mut()
            .and_then(|object| object.remove("catalog_metadata_raw"))
            .and_then(|value| value.as_str().map(str::to_owned));
        let metadata = raw
            .as_deref()
            .and_then(crate::catalog::types::StoredModelMetadata::parse);
        let card = metadata
            .as_ref()
            .and_then(|metadata| metadata.card.as_ref());
        let object = row.as_object_mut().expect("list documents are objects");
        object.insert(
            "catalog_metadata".into(),
            metadata
                .as_ref()
                .map(|metadata| metadata.raw.clone())
                .unwrap_or(Value::Null),
        );
        object.insert(
            "catalog_developer".into(),
            json!(card.and_then(|card| card.developer.as_deref())),
        );
        object.insert(
            "catalog_model_type".into(),
            json!(card.and_then(|card| card.model_type.as_deref())),
        );
        object.insert(
            "catalog_logo_key".into(),
            json!(card.and_then(|card| card.logo_key.as_deref())),
        );
        object.insert(
            "catalog_context_limit_tokens".into(),
            json!(card.and_then(|card| card.limits.context)),
        );
        object.insert(
            "catalog_output_limit_tokens".into(),
            json!(card.and_then(|card| card.limits.output)),
        );
        object.insert(
            "catalog_input_cost".into(),
            json!(card.and_then(|card| card.cost_defaults.input)),
        );
        object.insert(
            "catalog_output_cost".into(),
            json!(card.and_then(|card| card.cost_defaults.output)),
        );
        object.insert(
            "catalog_cost_currency".into(),
            json!(card.and_then(|card| card.cost_defaults.currency.as_deref())),
        );
        object.insert(
            "catalog_cost_unit".into(),
            json!(card.and_then(|card| card.cost_defaults.unit.as_deref())),
        );
    }
}
/// How many requests of one trace are read while looking for the earliest user
/// input, and how large a stored body may be before it is not read at all. Both
/// are hard bounds: a client can put a thousand requests in one trace, and a
/// preview is a triage convenience, not a reason to read a page of megabyte bodies.
/// The size is measured in stored bytes (`length` counts characters of text, so the
/// value is cast), because bytes are what reading one costs.
const PREVIEW_REQUESTS_PER_TRACE: i64 = 8;
const PREVIEW_BODY_BYTES: i64 = 256 * 1024;
/// Fills `first_user_query` on trace rows from the request bodies the logging
/// policy already stored.
///
/// One query for the whole page — the earliest stored bodies of the traces on it,
/// in order, never one read per row — and the extraction itself lives in
/// `trace_preview`, because the protocol shapes are the gateway's own contract.
/// A trace whose stored bodies hold no user text, or whose bodies were never
/// captured, keeps NULL: the console prints `—` for a fact nobody measured.
///
/// The ids bound this read twice over: they are the internal trace ids of rows that
/// were already read under the caller's project scope, and a trace id is the record
/// system's primary key, so naming one here cannot reach a trace that scope denied.
async fn trace_previews(db: &DatabaseConnection, rows: &mut [Value]) -> Result<(), ApiError> {
    let mut values: Vec<sea_orm::Value> = rows
        .iter()
        .filter_map(|row| row["id"].as_str().map(|id| id.to_owned().into()))
        .collect();
    if values.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; values.len()].join(",");
    values.push(PREVIEW_BODY_BYTES.into());
    values.push(PREVIEW_REQUESTS_PER_TRACE.into());
    let candidates = db
        .query_all(sql(
            // "Earliest" is `started_at` and then the arrival order: request rows
            // are stamped with a second, and several requests of one trace share it
            // routinely, so ordering by the primary key there would have picked one
            // at random and made the preview a coin toss.
            format!(
                "SELECT trace_id,request_json FROM (SELECT r.trace_id AS trace_id,c.request_json AS request_json,ROW_NUMBER() OVER (PARTITION BY r.trace_id ORDER BY r.started_at,r.rowid) AS position FROM requests r JOIN request_contents c ON c.request_id=r.id WHERE r.trace_id IN ({placeholders}) AND c.request_json IS NOT NULL AND length(CAST(c.request_json AS BLOB))<=?) WHERE position<=? ORDER BY trace_id,position"
            ),
            values,
        ))
        .await?;
    let mut previews: HashMap<String, String> = HashMap::new();
    for row in candidates {
        let trace: String = row.try_get("", "trace_id")?;
        // The rows arrive in request order per trace, so the first extraction that
        // yields text is the earliest user input of that trace.
        if previews.contains_key(&trace) {
            continue;
        }
        let stored: Option<String> = row.try_get("", "request_json")?;
        if let Some(text) = trace_preview::first_user_query(stored.as_deref()) {
            previews.insert(trace, text);
        }
    }
    for row in rows.iter_mut() {
        let preview = row["id"]
            .as_str()
            .and_then(|id| previews.get(id))
            .map(|text| json!(text))
            .unwrap_or(Value::Null);
        if let Some(object) = row.as_object_mut() {
            object.insert("first_user_query".into(), preview);
        }
    }
    Ok(())
}

/// The four trace lifecycle actions, as one project-scoped mutation.
///
/// The lifecycle is not the execution outcome: `traces.status` says what the run
/// did, `traces.lifecycle` says where the operator filed it. The transition table
/// is the operator contract and nothing else — active may be archived or retained,
/// and each of those is released by its own action — so a repeat or a crossed
/// transition is a typed 409 rather than a silent second status machine.
///
/// The state change and its audit row are one SQLite transaction, and nothing here
/// touches the derived projection or the network.
async fn trace_lifecycle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, id)): Path<(String, String)>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::BadRequest("action is required".into()))?;
    let (audit, required, next) = match action {
        "archive" => ("trace.archive", "active", "archived"),
        "unarchive" => ("trace.unarchive", "archived", "active"),
        "retain" => ("trace.retain", "active", "retained"),
        "unretain" => ("trace.unretain", "retained", "active"),
        _ => {
            return Err(ApiError::BadRequest(
                "unknown trace lifecycle action".into(),
            ));
        }
    };
    let transaction = state.db.begin().await?;
    // Ownership is proven before anything is written, and a trace of another
    // project and a trace that does not exist get the same 404: the answer must not
    // tell a caller which of the two it named.
    let current: String = transaction
        .query_one(sql(
            "SELECT lifecycle FROM traces WHERE project_id=? AND id=?",
            vec![project.clone().into(), id.clone().into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?
        .try_get("", "lifecycle")?;
    if current != required {
        return Err(ApiError::ConflictNamed(
            "invalid_trace_transition",
            format!("trace is {current}; {action} requires {required}"),
        ));
    }
    // The same predicate guards the write, so two accepted actions cannot both
    // land: the loser is the conflict the pre-check would have reported.
    let changed = transaction
        .execute(sql(
            "UPDATE traces SET lifecycle=? WHERE project_id=? AND id=? AND lifecycle=?",
            vec![
                next.into(),
                project.clone().into(),
                id.clone().into(),
                required.into(),
            ],
        ))
        .await?
        .rows_affected();
    if changed != 1 {
        return Err(ApiError::ConflictNamed(
            "invalid_trace_transition",
            format!("trace is no longer {required}"),
        ));
    }
    audit_in(&transaction, &user, &project, audit, &id).await?;
    transaction.commit().await?;
    Ok(Json(json!({"id":id,"lifecycle":next})))
}

#[derive(Deserialize)]
struct ModelLifecycleAction {
    action: String,
}

async fn model_lifecycle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, model)): Path<(String, String)>,
    Json(input): Json<ModelLifecycleAction>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let (from, to, audit) = match input.action.as_str() {
        "archive" => ("active", "archived", "model.archive"),
        "restore" => ("archived", "active", "model.restore"),
        _ => {
            return Err(ApiError::BadRequest(
                "unknown model lifecycle action".into(),
            ));
        }
    };
    let tx = state.db.begin().await?;
    let owned = tx
        .query_one(sql(
            "SELECT m.lifecycle FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=? AND p.project_id=?",
            vec![model.clone().into(), project.clone().into()],
        ))
        .await?;
    let Some(owned) = owned else {
        return Err(ApiError::NotFound);
    };
    let current: String = owned.try_get("", "lifecycle")?;
    if current != from {
        return Err(ApiError::Conflict(format!(
            "model is already {current}; only {from} models may be {}d",
            input.action
        )));
    }
    let changed = tx
        .execute(sql(
            "UPDATE models SET lifecycle=? WHERE id=? AND lifecycle=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",
            vec![to.into(), model.clone().into(), from.into(), project.clone().into()],
        ))
        .await?
        .rows_affected();
    if changed != 1 {
        return Err(ApiError::Conflict("model lifecycle changed; retry".into()));
    }
    audit_in(&tx, &user, &project, audit, &model).await?;
    tx.commit().await?;
    state.orchestrator.reset_derived();
    Ok(Json(json!({"id":model,"lifecycle":to})))
}

async fn model_delete_impact(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, model)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    let row = state.db.query_one(sql(
        "SELECT m.lifecycle,
          (SELECT COUNT(*) FROM model_prices WHERE model_id=m.id) AS prices,
          (SELECT COUNT(*) FROM model_price_components WHERE price_id IN (SELECT id FROM model_prices WHERE model_id=m.id)) AS price_components,
          (SELECT COUNT(*) FROM usage_logs WHERE model_id=m.id OR price_id IN (SELECT id FROM model_prices WHERE model_id=m.id)) AS usage_history,
          (SELECT COUNT(*) FROM model_associations WHERE model_id=m.id) AS associations,
          (SELECT COUNT(*) FROM execution_facts WHERE model_id=m.id) AS executions
         FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=? AND p.project_id=?",
        vec![model.clone().into(), project.into()],
    )).await?;
    let Some(row) = row else {
        return Err(ApiError::NotFound);
    };
    let prices: i64 = row.try_get("", "prices")?;
    let usage_history: i64 = row.try_get("", "usage_history")?;
    Ok(Json(json!({
        "id": model,
        "lifecycle": row.try_get::<String>("", "lifecycle")?,
        "prices": prices,
        "price_components": row.try_get::<i64>("", "price_components")?,
        "usage_history": usage_history,
        "associations": row.try_get::<i64>("", "associations")?,
        "executions": row.try_get::<i64>("", "executions")?,
        "blocked": prices > 0 || usage_history > 0,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchModelsInput {
    models: Vec<BatchModelInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchModelInput {
    catalog_model_id: String,
    provider_id: String,
    public_name: String,
    upstream_name: String,
}

/// Import catalog-backed models as one audited unit. Catalog resolution happens
/// before the SQLite business transaction, then every project/duplicate check and
/// insert happens on that transaction so a bad later row cannot leave earlier rows.
async fn batch_create_models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<BatchModelsInput>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    if input.models.is_empty() || input.models.len() > 100 {
        return Err(ApiError::BadRequest(
            "models must contain 1–100 rows".into(),
        ));
    }
    let mut resolved = Vec::with_capacity(input.models.len());
    let mut identities = HashSet::with_capacity(input.models.len());
    let catalog = crate::catalog::repository::effective(&state.db).await?;
    for (index, row) in input.models.iter().enumerate() {
        let row_number = index + 1;
        if row.catalog_model_id.is_empty()
            || row.catalog_model_id.len() > 256
            || row.provider_id.is_empty()
            || row.provider_id.len() > 256
            || row.public_name.trim().is_empty()
            || row.public_name.len() > 256
            || row.upstream_name.trim().is_empty()
            || row.upstream_name.len() > 256
        {
            return Err(ApiError::BadRequest(format!(
                "row {row_number}: invalid model identity"
            )));
        }
        let identity = (
            row.provider_id.clone(),
            row.public_name.trim().to_owned(),
            row.upstream_name.trim().to_owned(),
        );
        if !identities.insert(identity) {
            return Err(ApiError::BadRequest(format!(
                "row {row_number}: duplicate model in batch"
            )));
        }
        let defaults = db::catalog_model_defaults_from(&catalog, &row.catalog_model_id)
            .ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "row {row_number}: unknown or stale catalog model card"
                ))
            })?;
        resolved.push(defaults);
    }

    let tx = state.db.begin().await?;
    let mut created_ids = Vec::with_capacity(input.models.len());
    for (index, (row, defaults)) in input.models.iter().zip(&resolved).enumerate() {
        let row_number = index + 1;
        if tx
            .query_one(sql(
                "SELECT id FROM providers WHERE id=? AND project_id=?",
                vec![row.provider_id.clone().into(), project.clone().into()],
            ))
            .await?
            .is_none()
        {
            return Err(ApiError::BadRequest(format!(
                "row {row_number}: channel is not in this project"
            )));
        }
        if tx
            .query_one(sql(
                "SELECT m.id FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.provider_id=? AND m.public_name=? AND m.upstream_name=? AND p.project_id=?",
                vec![
                    row.provider_id.clone().into(),
                    row.public_name.trim().to_owned().into(),
                    row.upstream_name.trim().to_owned().into(),
                    project.clone().into(),
                ],
            ))
            .await?
            .is_some()
        {
            return Err(ApiError::ConflictNamed(
                "duplicate_model",
                format!("row {row_number}: this channel already has that model"),
            ));
        }
        let model = db::create_model_in(
            &tx,
            &ModelInput {
                provider_id: row.provider_id.clone(),
                public_name: row.public_name.trim().to_owned(),
                upstream_name: row.upstream_name.trim().to_owned(),
                capabilities: None,
                input_price_micros: None,
                output_price_micros: None,
                priority: None,
            },
            &project,
            defaults,
        )
        .await?;
        audit_in(&tx, &user, &project, "models.batch_create", &model.id).await?;
        created_ids.push(model.id);
    }
    tx.commit().await?;
    state.orchestrator.reset_derived();
    let created_count = created_ids.len();
    Ok(Json(json!({
        "created_ids": created_ids,
        "created_count": created_count
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BulkModelsInput {
    ids: Vec<String>,
    action: String,
}

/// Apply B30's lifecycle and delete guards to a bounded selection in one
/// transaction. Unknown or foreign ids are echoed only as skipped input; no
/// foreign row is read without the project predicate.
async fn bulk_models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<BulkModelsInput>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    if input.ids.is_empty() || input.ids.len() > 100 {
        return Err(ApiError::BadRequest("ids must contain 1–100 items".into()));
    }
    if !matches!(input.action.as_str(), "archive" | "restore" | "delete") {
        return Err(ApiError::BadRequest("unknown bulk model action".into()));
    }
    let mut seen = HashSet::with_capacity(input.ids.len());
    for model in &input.ids {
        if model.is_empty() || model.len() > 2048 || !seen.insert(model) {
            return Err(ApiError::BadRequest(
                "ids must contain unique non-empty strings".into(),
            ));
        }
    }

    let tx = state.db.begin().await?;
    let mut owned = Vec::with_capacity(input.ids.len());
    let mut skipped_ids = Vec::new();
    for model in &input.ids {
        let row = tx
            .query_one(sql(
                "SELECT m.lifecycle,
                  (SELECT COUNT(*) FROM model_prices WHERE model_id=m.id) AS prices,
                  (SELECT COUNT(*) FROM usage_logs WHERE model_id=m.id OR price_id IN (SELECT id FROM model_prices WHERE model_id=m.id)) AS usage_history
                 FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=? AND p.project_id=?",
                vec![model.clone().into(), project.clone().into()],
            ))
            .await?;
        if let Some(row) = row {
            owned.push((
                model.clone(),
                row.try_get::<String>("", "lifecycle")?,
                row.try_get::<i64>("", "prices")?,
                row.try_get::<i64>("", "usage_history")?,
            ));
        } else {
            skipped_ids.push(model.clone());
        }
    }

    if input.action == "delete" {
        for (_, lifecycle, prices, usage_history) in &owned {
            if lifecycle != "archived" {
                return Err(ApiError::ConflictNamed(
                    "archive_required",
                    "archive every selected model and review its delete impact before deleting"
                        .into(),
                ));
            }
            if *prices > 0 || *usage_history > 0 {
                return Err(ApiError::ConflictNamed(
                    "history_retained",
                    "a selected archived model has immutable price or usage history and cannot be deleted"
                        .into(),
                ));
            }
        }
    }

    let mut changed_ids = Vec::new();
    for (model, lifecycle, _, _) in owned {
        let (changed, audit) = match input.action.as_str() {
            "archive" if lifecycle == "active" => (
                tx.execute(sql(
                    "UPDATE models SET lifecycle='archived' WHERE id=? AND lifecycle='active' AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                    vec![model.clone().into(), project.clone().into()],
                ))
                .await?
                .rows_affected(),
                "model.archive",
            ),
            "restore" if lifecycle == "archived" => (
                tx.execute(sql(
                    "UPDATE models SET lifecycle='active' WHERE id=? AND lifecycle='archived' AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                    vec![model.clone().into(), project.clone().into()],
                ))
                .await?
                .rows_affected(),
                "model.restore",
            ),
            "delete" => (
                tx.execute(sql(
                    "DELETE FROM models WHERE id=? AND lifecycle='archived' AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                    vec![model.clone().into(), project.clone().into()],
                ))
                .await?
                .rows_affected(),
                "models.delete",
            ),
            _ => {
                skipped_ids.push(model);
                continue;
            }
        };
        if changed != 1 {
            return Err(ApiError::Conflict(
                "model lifecycle changed; retry the selection".into(),
            ));
        }
        audit_in(&tx, &user, &project, audit, &model).await?;
        changed_ids.push(model);
    }
    tx.commit().await?;
    state.orchestrator.reset_derived();
    let changed_count = changed_ids.len();
    Ok(Json(json!({
        "action": input.action,
        "changed_ids": changed_ids,
        "changed_count": changed_count,
        "skipped_ids": skipped_ids
    })))
}

/// A structural routing diagnostic: an enabled model is covered when at least
/// one enabled association can select its channel/model and match its public name
/// (or the channel tag used by a tag association). Request-specific conditions do
/// not make a structurally associated model appear unassociated.
async fn unassociated_models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    let models = state
        .db
        .query_all(sql(
            "SELECT m.id,m.public_name,p.id AS provider_id,p.name AS provider_name,p.settings_json
             FROM models m JOIN providers p ON p.id=m.provider_id
             WHERE p.project_id=? AND p.enabled=1 AND m.enabled=1 AND m.lifecycle='active'
             ORDER BY m.public_name,p.name,m.id",
            vec![project.clone().into()],
        ))
        .await?;
    let associations = state
        .db
        .query_all(sql(
            "SELECT model_id,provider_id,match_type,pattern,exclusions_json FROM model_associations WHERE project_id=? AND enabled=1 ORDER BY priority,id",
            vec![project.into()],
        ))
        .await?;
    let mut unassociated = Vec::new();
    for model in models {
        let model_id: String = model.try_get("", "id")?;
        let public_name: String = model.try_get("", "public_name")?;
        let provider_id: String = model.try_get("", "provider_id")?;
        let provider_name: String = model.try_get("", "provider_name")?;
        let settings: Value = serde_json::from_str(&model.try_get::<String>("", "settings_json")?)
            .map_err(|error| ApiError::Internal(error.into()))?;
        let tags: Vec<String> =
            serde_json::from_value(settings.get("tags").cloned().unwrap_or_else(|| json!([])))
                .map_err(|_| ApiError::Internal(anyhow::anyhow!("invalid stored channel tags")))?;
        let mut matched = false;
        for association in &associations {
            if association
                .try_get::<Option<String>>("", "model_id")?
                .as_deref()
                .is_some_and(|id| id != model_id.as_str())
                || association
                    .try_get::<Option<String>>("", "provider_id")?
                    .as_deref()
                    .is_some_and(|id| id != provider_id.as_str())
            {
                continue;
            }
            let pattern: String = association.try_get("", "pattern")?;
            matched = match association.try_get::<String>("", "match_type")?.as_str() {
                "exact" => pattern == public_name,
                "regex" => crate::orchestration::policy::regex(&pattern)
                    .map_err(|_| {
                        ApiError::Internal(anyhow::anyhow!(
                            "invalid stored model association regex"
                        ))
                    })?
                    .is_match(&public_name),
                "tag" => tags.iter().any(|tag| tag == &pattern),
                "channel_tags_regex" => {
                    let regex = crate::orchestration::policy::regex(&pattern).map_err(|_| {
                        ApiError::Internal(anyhow::anyhow!(
                            "invalid stored channel-tag association regex"
                        ))
                    })?;
                    tags.iter().any(|tag| regex.is_match(tag))
                }
                _ => {
                    return Err(ApiError::Internal(anyhow::anyhow!(
                        "invalid stored model association match type"
                    )));
                }
            };
            if matched {
                let exclusions = association.try_get::<String>("", "exclusions_json")?;
                let exclusions = crate::orchestration::policy::AssociationExclusions::parse(
                    &serde_json::from_str(&exclusions)
                        .map_err(|error| ApiError::Internal(error.into()))?,
                )
                .map_err(|_| {
                    ApiError::Internal(anyhow::anyhow!(
                        "invalid stored model association exclusions"
                    ))
                })?;
                matched = !exclusions
                    .excludes(&provider_id, &provider_name, &tags)
                    .map_err(|_| {
                        ApiError::Internal(anyhow::anyhow!(
                            "invalid stored model association exclusion pattern"
                        ))
                    })?;
            }
            if matched {
                break;
            }
        }
        if !matched {
            unassociated.push(json!({
                "id": model_id,
                "public_name": public_name,
                "provider_id": provider_id,
                "provider_name": provider_name
            }));
        }
    }
    let total = unassociated.len();
    let offset = filter.offset as usize;
    let limit = filter.limit.unwrap_or(100).clamp(1, 500) as usize;
    let data = unassociated
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": data,
        "total": total,
        "offset": filter.offset,
        "limit": limit
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaygroundChat {
    api_key_id: String,
    provider_id: Option<String>,
    payload: Value,
}

#[derive(Deserialize)]
struct PlaygroundModels {
    api_key_id: String,
}

async fn playground_models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Query(input): Query<PlaygroundModels>,
) -> Result<Json<Value>, ApiError> {
    let principal = actor_for(&state, &headers, Some(&project), "gateway:use", false).await?;
    if principal.kind != crate::access::PrincipalKind::Session {
        return Err(ApiError::Forbidden);
    }
    let key = db::api_key_credential_for_use_by_id(
        &state.db,
        &project,
        &input.api_key_id,
        crate::api::trusted_client_ip(&headers),
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    let scopes = serde_json::from_str::<Vec<String>>(&key.scopes).unwrap_or_default();
    if !scopes
        .iter()
        .any(|scope| matches!(scope.as_str(), "gateway" | "gateway:use" | "*"))
    {
        return Err(ApiError::Unauthorized);
    }
    let models =
        crate::orchestration::visible_models_for(&state.db, &key, &headers, &["/v1/responses"])
            .await?;
    Ok(Json(json!({
        "object":"list",
        "data":models.into_iter().map(|id|json!({"id":id,"object":"model"})).collect::<Vec<_>>()
    })))
}

/// A console-session entry into the ordinary Responses gateway pipeline. The
/// session authorizes the control-plane action; the named project key still owns
/// every model, IP, budget, admission, routing and accounting decision.
async fn playground_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<PlaygroundChat>,
) -> Result<Response, ApiError> {
    let principal = actor_for(&state, &headers, Some(&project), "gateway:use", true).await?;
    if principal.kind != crate::access::PrincipalKind::Session {
        return Err(ApiError::Forbidden);
    }
    if input.api_key_id.is_empty() || !input.payload.is_object() {
        return Err(ApiError::BadRequest(
            "api_key_id and an object payload are required".into(),
        ));
    }
    crate::api::gateway::execute_admin_input(
        state,
        headers,
        super::protocols::Input::json(input.payload, "/v1/responses"),
        project,
        input.api_key_id,
        input.provider_id,
    )
    .await
}
async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, resource, id)): Path<(String, String, String)>,
) -> Result<Json<Value>, ApiError> {
    actor_for(
        &state,
        &headers,
        Some(&project),
        if resource == "key-profiles" {
            "api_key:manage"
        } else {
            "project:read"
        },
        false,
    )
    .await?;
    if resource == "trace-detail" {
        let trace=state.db.query_one(sql("SELECT json_object('id',id,'thread_id',thread_id,'external_id',external_id,'status',status,'lifecycle',lifecycle,'started_at',started_at,'finished_at',finished_at) AS document FROM traces WHERE project_id=? AND (id=? OR external_id=?)",vec![project.clone().into(),id.clone().into(),id.clone().into()])).await?.ok_or(ApiError::NotFound)?;
        let trace: Value = serde_json::from_str(&trace.try_get::<String>("", "document")?)
            .map_err(|error| ApiError::Internal(error.into()))?;
        // The caller may have passed the client's trace id, so every child query
        // has to use the resolved internal id — otherwise the trace renders with
        // no requests and no executions.
        let trace_id = trace["id"]
            .as_str()
            .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("trace has no id")))?
            .to_owned();
        let requests=documents(state.db.query_all(sql("SELECT json_object('id',id,'public_id',COALESCE(json_extract(request_metadata_json,'$.external_id'),id),'protocol',protocol,'endpoint',endpoint,'model',requested_model,'status',status,'source_ip',source_ip,'started_at',started_at,'finished_at',finished_at) AS document FROM requests WHERE trace_id=? ORDER BY started_at",vec![trace_id.clone().into()])).await?)?;
        let executions=documents(state.db.query_all(sql("SELECT json_object('id',id,'request_id',request_id,'provider_id',provider_id,'provider_name',provider_name,'attempt',attempt,'model',model,'status',status,'retry_reason',retry_reason,'latency_ms',latency_ms,'started_at',started_at,'finished_at',finished_at,'http_status',http_status,'error_kind',error_kind) AS document FROM request_executions WHERE request_id IN (SELECT id FROM requests WHERE trace_id=?) ORDER BY started_at",vec![trace_id.clone().into()])).await?)?;
        // Tokens and money per execution, and the price components each charge was
        // made of. Without these the console can only show a total, which cannot
        // be checked against the price that produced it.
        let usage=documents(state.db.query_all(sql("SELECT json_object('execution_id',u.execution_id,'model_id',u.model_id,'input_tokens',u.input_tokens,'output_tokens',u.output_tokens,'cache_read_tokens',u.cache_read_tokens,'cache_write_tokens',u.cache_write_tokens,'reasoning_tokens',u.reasoning_tokens,'total_cost_micros',u.total_cost_micros,'created_at',u.created_at) AS document FROM usage_logs u JOIN execution_facts e ON e.id=u.execution_id JOIN requests r ON r.id=e.request_id WHERE r.trace_id=? ORDER BY u.created_at",vec![trace_id.clone().into()])).await?)?;
        let cost_items=documents(state.db.query_all(sql("SELECT json_object('execution_id',e.id,'kind',c.kind,'quantity',i.quantity,'unit_price_micros',c.unit_price_micros,'subtotal_micros',i.subtotal_micros) AS document FROM usage_cost_items i JOIN usage_logs u ON u.id=i.usage_log_id JOIN execution_facts e ON e.id=u.execution_id JOIN requests r ON r.id=e.request_id LEFT JOIN model_price_components c ON c.id=i.price_component_id WHERE r.trace_id=? ORDER BY e.id,i.id",vec![trace_id.clone().into()])).await?)?;
        return Ok(Json(
            json!({"trace":trace,"requests":requests,"executions":executions,"usage":usage,"cost_items":cost_items}),
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
    if resource == "requests" {
        // A request's attempts, tokens and per-component cost live in the record
        // system, not in the projection the list is served from. Without them the
        // console can show a request but not explain it.
        // Fail closed for a request that belongs to another project before any
        // of its attempts or costs are read.
        let resolved = state
            .db
            .query_one(sql(
                // The console links by the id the projection exposes as `public_id`:
                // the client's external request id when there is one, the internal id
                // otherwise. Resolving only the primary key made every such link 404.
                "SELECT r.id FROM requests r JOIN traces t ON t.id=r.trace_id WHERE (r.id=? OR COALESCE(json_extract(r.request_metadata_json,'$.external_id'),r.id)=?) AND t.project_id=?",
                vec![id.clone().into(), id.clone().into(), project.clone().into()],
            ))
            .await?
            .ok_or(ApiError::NotFound)?;
        // The pre-check above resolved the identity the console links with — the
        // internal id, or the external one the projection exposes as `public_id` —
        // so the row fetch has to use *that*, not the raw path segment. Binding the
        // path segment here is why those links 404'd even once the pre-check passed.
        let resolved_id: String = resolved.try_get("", "id")?;
        // Every child fact is selected by that same resolved, project-scoped id. The
        // external id is caller-controlled, so binding the raw path segment here both
        // emptied the detail card for a legitimate external-id link and let a colliding
        // external id read another project's attempts, tokens and costs.
        let executions = documents(state.db.query_all(sql("SELECT json_object('id',id,'request_id',request_id,'provider_id',provider_id,'provider_name',provider_name,'attempt',attempt,'model',model,'status',status,'retry_reason',retry_reason,'latency_ms',latency_ms,'started_at',started_at,'finished_at',finished_at,'http_status',http_status,'error_kind',error_kind) AS document FROM request_executions WHERE request_id=? ORDER BY attempt", vec![resolved_id.clone().into()])).await?)?;
        let usage = documents(state.db.query_all(sql("SELECT json_object('execution_id',u.execution_id,'model_id',u.model_id,'input_tokens',u.input_tokens,'output_tokens',u.output_tokens,'cache_read_tokens',u.cache_read_tokens,'cache_write_tokens',u.cache_write_tokens,'reasoning_tokens',u.reasoning_tokens,'total_cost_micros',u.total_cost_micros,'created_at',u.created_at) AS document FROM usage_logs u JOIN execution_facts e ON e.id=u.execution_id WHERE e.request_id=? ORDER BY u.created_at", vec![resolved_id.clone().into()])).await?)?;
        let cost_items = documents(state.db.query_all(sql("SELECT json_object('execution_id',e.id,'kind',c.kind,'quantity',i.quantity,'unit_price_micros',c.unit_price_micros,'subtotal_micros',i.subtotal_micros) AS document FROM usage_cost_items i JOIN usage_logs u ON u.id=i.usage_log_id JOIN execution_facts e ON e.id=u.execution_id LEFT JOIN model_price_components c ON c.id=i.price_component_id WHERE e.request_id=? ORDER BY e.id,i.id", vec![resolved_id.clone().into()])).await?)?;
        let row = state
            .db
            .query_one(sql(
                format!(
                    "SELECT {projection} AS document FROM {table} WHERE ({scope}) AND {id_column}=?"
                ),
                vec![project.into(), resolved_id.into()],
            ))
            .await?
            .ok_or(ApiError::NotFound)?;
        let mut document: Value = serde_json::from_str(&row.try_get::<String>("", "document")?)
            .map_err(|error| ApiError::Internal(error.into()))?;
        if let Some(object) = document.as_object_mut() {
            object.insert("executions".into(), json!(executions));
            object.insert("usage".into(), json!(usage));
            object.insert("cost_items".into(), json!(cost_items));
        }
        return Ok(Json(document));
    }
    let rows = state
        .db
        .query_all(sql(
            format!(
                "SELECT {projection} AS document FROM {table} WHERE ({scope}) AND {id_column}=?"
            ),
            vec![project.into(), id.into()],
        ))
        .await?;
    let mut rows = documents(rows)?;
    // One trace is the smallest possible page: the same derived read, so a detail
    // row and a list row describe the trace the same way.
    if resource == "traces" {
        trace_previews(&state.db, &mut rows).await?;
    }
    Ok(Json(rows.into_iter().next().ok_or(ApiError::NotFound)?))
}
async fn analytics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    if filter
        .from
        .zip(filter.until)
        .is_some_and(|(from, until)| from >= until)
    {
        return Err(ApiError::BadRequest("invalid analytics window".into()));
    }
    // A key-specific analytics link is an object read, not a best-effort string
    // filter. Resolve it inside the route project before reading facts so a
    // foreign id and a nonexistent id are the same opaque 404.
    if let Some(api_key) = filter.api_key.as_deref()
        && state
            .db
            .query_one(sql(
                "SELECT 1 AS present FROM api_keys WHERE id=? AND project_id=?",
                vec![api_key.into(), project.clone().into()],
            ))
            .await?
            .is_none()
    {
        return Err(ApiError::NotFound);
    }
    let dimension = match filter.dimension.as_deref().unwrap_or("day") {
        "day" => "CAST(r.started_at/86400 AS TEXT)",
        "provider" => "e.provider_id",
        "model" => "e.model_id",
        "api_key" => "r.api_key_id",
        "user" => "r.user_id",
        "project" => "r.project_id",
        _ => return Err(ApiError::BadRequest("unknown analytics dimension".into())),
    };
    let rows=state.db.query_all(sql(format!("SELECT json_object('dimension',{dimension},'requests',COUNT(DISTINCT r.id),'attempts',COUNT(e.id),'errors',SUM(e.status!='succeeded'),'usage_measured',CASE WHEN COUNT(u.id)>0 THEN json('true') ELSE json('false') END,'input_tokens',COALESCE(SUM(u.input_tokens),0),'output_tokens',COALESCE(SUM(u.output_tokens),0),'cache_hit_tokens',COALESCE(SUM(u.cache_read_tokens),0),'cache_savings_micros',COALESCE(SUM(u.cache_savings_micros),0),'cost_micros',COALESCE(SUM(u.total_cost_micros),0),'latency_ms',AVG(x.latency_ms),'ttft_ms',AVG(x.first_token_at-x.started_at*1000),'tokens_per_second',AVG(CASE WHEN u.id IS NOT NULL AND x.latency_ms>0 THEN CAST(u.output_tokens AS REAL)*1000.0/x.latency_ms END)) AS document FROM request_facts r JOIN execution_facts e ON e.request_id=r.id LEFT JOIN usage_logs u ON u.execution_id=e.id LEFT JOIN request_executions x ON x.id=e.id WHERE r.project_id=? AND r.started_at>=? AND r.started_at<? AND (? IS NULL OR e.model_id=?) AND (? IS NULL OR e.provider_id=?) AND (? IS NULL OR r.api_key_id=?) GROUP BY {dimension} ORDER BY {dimension} LIMIT 500"),vec![project.into(),filter.from.unwrap_or(db::now()-86400*30).into(),filter.until.unwrap_or(db::now()+1).into(),filter.model.clone().into(),filter.model.into(),filter.provider.clone().into(),filter.provider.into(),filter.api_key.clone().into(),filter.api_key.into()])).await?;
    Ok(Json(
        json!({"data":documents(rows)?,"source":"sqlite","derived_available":state.observations.is_available()}),
    ))
}

async fn live_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    let row = state
        .db
        .query_one(sql(
            "SELECT CASE WHEN typeof(value)='text' THEN value END AS value,typeof(value) AS value_type FROM settings WHERE key='request_logging'",
            vec![],
        ))
        .await?;
    let policy = match row {
        Some(row) => match logging::parse_stored_row(&row) {
            Ok(Some(policy)) => policy,
            Ok(None) => logging::Policy::default(),
            Err(problem) => {
                tracing::warn!(
                    problem = problem.code(),
                    "stored request-logging policy is invalid; live preview remains disabled"
                );
                logging::Policy::fail_closed()
            }
        },
        None => logging::Policy::default(),
    };
    if !policy.live_preview_enabled {
        return Ok(Json(json!({"enabled":false,"data":[]})));
    }
    Ok(Json(json!({
        "enabled": true,
        "data": state.orchestrator.live_requests(&project),
    })))
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
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct SemanticMemorySettings {
    enabled: bool,
    max_candidates: usize,
    rerank: bool,
}
impl Default for SemanticMemorySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            max_candidates: 4,
            rerank: false,
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OrchestrationSettings {
    version: u8,
    affinity_rules: Vec<crate::orchestration::affinity::Rule>,
    session_compaction: SessionCompactionSettings,
    #[serde(default)]
    semantic_memory: SemanticMemorySettings,
    /// The project's default routing conditions. The orchestrator already reads
    /// this; until now no route wrote it. `None` means the request did not mention
    /// it, and the stored policy must be left alone: a form that does not carry
    /// this field would otherwise reset the project's routing on every save.
    #[serde(default)]
    routing: Option<Value>,
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
            semantic_memory: SemanticMemorySettings::default(),
            routing: None,
        }
    }
}
fn validate_orchestration_settings(input: &OrchestrationSettings) -> Result<(), ApiError> {
    if input.version != 1
        || input.affinity_rules.len() > 64
        || input.session_compaction.threshold_tokens < 128
        || input.session_compaction.threshold_tokens > 10_000_000
        || input.session_compaction.retain_items > 128
        || input.semantic_memory.enabled
            && !(1..=16).contains(&input.semantic_memory.max_candidates)
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
    if let Some(routing) = &input.routing {
        crate::orchestration::policy::validate_conditions(routing)
            .map_err(|_| ApiError::BadRequest("invalid default routing conditions".into()))?;
    }
    {}
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
        semantic_memory: serde_json::from_value(
            settings
                .get("semantic_memory")
                .cloned()
                .unwrap_or(json!(default.semantic_memory)),
        )
        .map_err(|_| ApiError::BadRequest("invalid stored semantic memory settings".into()))?,
        routing: settings.get("routing").cloned(),
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
    settings["semantic_memory"] = json!(input.semantic_memory);
    if let Some(routing) = &input.routing {
        settings["routing"] = routing.clone();
    }
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
    Query(filter): Query<crate::observability::SummaryFilter>,
) -> Result<Json<crate::observability::Summary>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    reject_inverted_window(&filter)?;
    Ok(Json(
        state
            .observations
            .summary_for(project, filter)
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
    filter.validate_facets().map_err(ApiError::BadRequest)?;
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

const PROTECTION_PREVIEW_SAMPLE_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
struct ProtectionPreviewInput {
    text: String,
}

async fn protection_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<ProtectionPreviewInput>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    if input.text.len() > PROTECTION_PREVIEW_SAMPLE_BYTES {
        return Err(ApiError::BadRequest(
            "protection preview text exceeds 16 KiB".into(),
        ));
    }
    let rules = crate::orchestration::preview_protection(&state.db, &project, &input.text).await?;
    Ok(Json(json!({ "rules": rules })))
}

const PROFILE_TEMPLATE_DOCUMENT_BYTES: usize = 64 * 1024;
const PROFILE_TEMPLATE_ITEMS: usize = 128;

fn default_profile_routing() -> Value {
    json!({"version":1})
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileMappingDocument {
    source_model: String,
    target_model: String,
    #[serde(default = "default_profile_mapping_priority")]
    priority: i64,
}

const fn default_profile_mapping_priority() -> i64 {
    100
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileAllowedModelDocument {
    pattern: String,
    #[serde(default = "default_profile_match_type")]
    match_type: String,
}

fn default_profile_match_type() -> String {
    "exact".into()
}

/// The complete policy portion of a profile. It deliberately excludes identity
/// and name, so an exported document cannot select a project or silently rename
/// the target it is applied to.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileTemplateDocument {
    version: u32,
    #[serde(default)]
    rpm_limit: Option<i64>,
    #[serde(default)]
    tpm_limit: Option<i64>,
    #[serde(default)]
    budget_micros: Option<i64>,
    #[serde(default = "default_profile_routing")]
    routing_policy: Value,
    #[serde(default)]
    mappings: Vec<ProfileMappingDocument>,
    #[serde(default)]
    allowed_models: Vec<ProfileAllowedModelDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileWriteDocument {
    id: Option<String>,
    name: String,
    #[serde(default)]
    rpm_limit: Option<i64>,
    #[serde(default)]
    tpm_limit: Option<i64>,
    #[serde(default)]
    budget_micros: Option<i64>,
    #[serde(default = "default_profile_routing")]
    routing_policy: Value,
    #[serde(default)]
    mappings: Vec<ProfileMappingDocument>,
    #[serde(default)]
    allowed_models: Vec<ProfileAllowedModelDocument>,
}

fn validate_profile_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(ApiError::BadRequest(
            "profile and template names must contain 1–128 bytes".into(),
        ));
    }
    Ok(name.into())
}

fn validate_profile_document(document: &ProfileTemplateDocument) -> Result<(), ApiError> {
    if document.version != 1
        || document.rpm_limit.is_some_and(|value| value < 0)
        || document.tpm_limit.is_some_and(|value| value < 0)
        || document.budget_micros.is_some_and(|value| value < 0)
        || document.mappings.len() > PROFILE_TEMPLATE_ITEMS
        || document.allowed_models.len() > PROFILE_TEMPLATE_ITEMS
    {
        return Err(ApiError::BadRequest("invalid profile document".into()));
    }
    crate::orchestration::policy::Routing::parse(&document.routing_policy.to_string())
        .map_err(|_| ApiError::BadRequest("invalid profile routing policy".into()))?;
    let mut sources = std::collections::BTreeSet::new();
    for mapping in &document.mappings {
        if mapping.source_model.is_empty()
            || mapping.source_model.len() > 256
            || mapping.target_model.is_empty()
            || mapping.target_model.len() > 256
            || !sources.insert(mapping.source_model.as_str())
        {
            return Err(ApiError::BadRequest(
                "invalid or duplicate model mapping".into(),
            ));
        }
    }
    let mut allowed = std::collections::BTreeSet::new();
    for model in &document.allowed_models {
        if model.pattern.is_empty()
            || model.pattern.len() > 512
            || !matches!(model.match_type.as_str(), "exact" | "regex")
            || !allowed.insert((model.pattern.as_str(), model.match_type.as_str()))
        {
            return Err(ApiError::BadRequest(
                "invalid or duplicate allowed model".into(),
            ));
        }
        if model.match_type == "regex" {
            crate::orchestration::policy::regex(&model.pattern)
                .map_err(|_| ApiError::BadRequest("invalid allowed-model regex".into()))?;
        }
    }
    if serde_json::to_vec(document)
        .map_err(|error| ApiError::Internal(error.into()))?
        .len()
        > PROFILE_TEMPLATE_DOCUMENT_BYTES
    {
        return Err(ApiError::BadRequest(
            "profile document exceeds 65536 bytes".into(),
        ));
    }
    Ok(())
}

/// Stored documents are allowed to carry fields written by a newer Pangolin.
/// Only the known policy fields are projected, then the same validator used for
/// current writes decides whether applying them is safe.
fn parse_stored_profile_document(raw: &str) -> Result<ProfileTemplateDocument, ApiError> {
    let mut value: Value = serde_json::from_str(raw)
        .map_err(|_| ApiError::BadRequest("stored profile template is malformed".into()))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| ApiError::BadRequest("stored profile template is malformed".into()))?;
    object.retain(|key, _| {
        matches!(
            key.as_str(),
            "version"
                | "rpm_limit"
                | "tpm_limit"
                | "budget_micros"
                | "routing_policy"
                | "mappings"
                | "allowed_models"
        )
    });
    let document: ProfileTemplateDocument = serde_json::from_value(value)
        .map_err(|_| ApiError::BadRequest("stored profile template is malformed".into()))?;
    validate_profile_document(&document)?;
    Ok(document)
}

async fn load_profile_document(
    db: &impl ConnectionTrait,
    project: &str,
    profile_id: &str,
) -> Result<(String, ProfileTemplateDocument), ApiError> {
    let profile = db
        .query_one(sql(
            "SELECT name,rpm_limit,tpm_limit,budget_micros,routing_policy_json FROM api_key_profiles WHERE id=? AND project_id=?",
            vec![profile_id.into(), project.into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    let mappings = db
        .query_all(sql(
            "SELECT source_model,target_model,priority FROM api_key_profile_model_mappings WHERE profile_id=? ORDER BY priority,id",
            vec![profile_id.into()],
        ))
        .await?
        .into_iter()
        .map(|row| {
            Ok(ProfileMappingDocument {
                source_model: row.try_get("", "source_model")?,
                target_model: row.try_get("", "target_model")?,
                priority: row.try_get("", "priority")?,
            })
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()?;
    let allowed_models = db
        .query_all(sql(
            "SELECT model_pattern,match_type FROM api_key_profile_allowed_models WHERE profile_id=? ORDER BY model_pattern,match_type",
            vec![profile_id.into()],
        ))
        .await?
        .into_iter()
        .map(|row| {
            Ok(ProfileAllowedModelDocument {
                pattern: row.try_get("", "model_pattern")?,
                match_type: row.try_get("", "match_type")?,
            })
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()?;
    let routing: String = profile.try_get("", "routing_policy_json")?;
    let document = ProfileTemplateDocument {
        version: 1,
        rpm_limit: profile.try_get("", "rpm_limit")?,
        tpm_limit: profile.try_get("", "tpm_limit")?,
        budget_micros: profile.try_get("", "budget_micros")?,
        routing_policy: serde_json::from_str(&routing)
            .map_err(|error| ApiError::Internal(error.into()))?,
        mappings,
        allowed_models,
    };
    validate_profile_document(&document)?;
    Ok((profile.try_get("", "name")?, document))
}

async fn write_profile_document(
    transaction: &DatabaseTransaction,
    project: &str,
    profile_id: &str,
    name: &str,
    document: &ProfileTemplateDocument,
) -> Result<(), ApiError> {
    let name = validate_profile_name(name)?;
    validate_profile_document(document)?;
    let changed = transaction.execute(sql("INSERT INTO api_key_profiles(id,project_id,name,rpm_limit,tpm_limit,budget_micros,routing_policy_json,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,rpm_limit=excluded.rpm_limit,tpm_limit=excluded.tpm_limit,budget_micros=excluded.budget_micros,routing_policy_json=excluded.routing_policy_json,updated_at=excluded.updated_at WHERE api_key_profiles.project_id=excluded.project_id",vec![profile_id.into(),project.into(),name.into(),document.rpm_limit.into(),document.tpm_limit.into(),document.budget_micros.into(),document.routing_policy.to_string().into(),db::now().into(),db::now().into()])).await?.rows_affected();
    if changed != 1 {
        return Err(ApiError::NotFound);
    }
    transaction
        .execute(sql(
            "DELETE FROM api_key_profile_model_mappings WHERE profile_id=?",
            vec![profile_id.into()],
        ))
        .await?;
    for mapping in &document.mappings {
        transaction.execute(sql("INSERT INTO api_key_profile_model_mappings(id,profile_id,source_model,target_model,priority) VALUES(?,?,?,?,?)",vec![id().into(),profile_id.into(),mapping.source_model.clone().into(),mapping.target_model.clone().into(),mapping.priority.into()])).await?;
    }
    transaction
        .execute(sql(
            "DELETE FROM api_key_profile_allowed_models WHERE profile_id=?",
            vec![profile_id.into()],
        ))
        .await?;
    for model in &document.allowed_models {
        transaction.execute(sql("INSERT INTO api_key_profile_allowed_models(profile_id,model_pattern,match_type) VALUES(?,?,?)",vec![profile_id.into(),model.pattern.clone().into(),model.match_type.clone().into()])).await?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileTemplateSave {
    name: String,
    #[serde(default)]
    source_profile_id: Option<String>,
    #[serde(default)]
    document: Option<ProfileTemplateDocument>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileTemplateTransfer {
    version: u32,
    name: String,
    profile: ProfileTemplateDocument,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProfileTemplateConflict {
    Fail,
    Overwrite,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileTemplateImport {
    document: ProfileTemplateTransfer,
    conflict: ProfileTemplateConflict,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileTemplateApply {
    #[serde(default)]
    target_profile_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

async fn template_document_for_save(
    transaction: &DatabaseTransaction,
    project: &str,
    input: ProfileTemplateSave,
) -> Result<(String, ProfileTemplateDocument), ApiError> {
    let name = validate_profile_name(&input.name)?;
    let document = match (input.source_profile_id, input.document) {
        (Some(profile_id), None) => {
            load_profile_document(transaction, project, &profile_id)
                .await?
                .1
        }
        (None, Some(document)) => document,
        _ => {
            return Err(ApiError::BadRequest(
                "provide exactly one source_profile_id or document".into(),
            ));
        }
    };
    validate_profile_document(&document)?;
    Ok((name, document))
}

async fn list_profile_templates(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>, ApiError> {
    actor_for(&state, &headers, Some(&project), "api_key:manage", false).await?;
    let limit = filter.limit.unwrap_or(100).clamp(1, 500);
    let rows = state.db.query_all(sql("SELECT id,name,document_json,created_at,updated_at FROM api_key_profile_templates WHERE project_id=? ORDER BY updated_at DESC,id LIMIT ? OFFSET ?",vec![project.clone().into(),i64::from(limit).into(),i64::from(filter.offset).into()])).await?;
    let total = state
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS total FROM api_key_profile_templates WHERE project_id=?",
            vec![project.into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "total"))
        .transpose()?
        .unwrap_or(0);
    let mut data = Vec::with_capacity(rows.len());
    for row in rows {
        let raw: String = row.try_get("", "document_json")?;
        data.push(json!({
            "id": row.try_get::<String>("", "id")?,
            "name": row.try_get::<String>("", "name")?,
            "profile": parse_stored_profile_document(&raw)?,
            "created_at": row.try_get::<i64>("", "created_at")?,
            "updated_at": row.try_get::<i64>("", "updated_at")?,
        }));
    }
    Ok(Json(
        json!({"total":total,"offset":filter.offset,"limit":limit,"data":data}),
    ))
}

async fn create_profile_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<ProfileTemplateSave>,
) -> Result<Json<Value>, ApiError> {
    let user = actor_for(&state, &headers, Some(&project), "api_key:manage", true).await?;
    let transaction = state.db.begin().await?;
    let (name, document) = template_document_for_save(&transaction, &project, input).await?;
    if transaction
        .query_one(sql(
            "SELECT id FROM api_key_profile_templates WHERE project_id=? AND name=? COLLATE NOCASE",
            vec![project.clone().into(), name.clone().into()],
        ))
        .await?
        .is_some()
    {
        return Err(ApiError::ConflictNamed(
            "template_name_conflict",
            "a template with this name already exists".into(),
        ));
    }
    let template_id = id();
    transaction.execute(sql("INSERT INTO api_key_profile_templates(id,project_id,name,document_json,created_at,updated_at) VALUES(?,?,?,?,?,?)",vec![template_id.clone().into(),project.clone().into(),name.into(),serde_json::to_string(&document).map_err(|error|ApiError::Internal(error.into()))?.into(),db::now().into(),db::now().into()])).await?;
    audit_in(
        &transaction,
        &user,
        &project,
        "profile-template.create",
        &template_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(json!({"id":template_id})))
}

async fn update_profile_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, template_id)): Path<(String, String)>,
    Json(input): Json<ProfileTemplateSave>,
) -> Result<Json<Value>, ApiError> {
    let user = actor_for(&state, &headers, Some(&project), "api_key:manage", true).await?;
    let transaction = state.db.begin().await?;
    if transaction
        .query_one(sql(
            "SELECT id FROM api_key_profile_templates WHERE id=? AND project_id=?",
            vec![template_id.clone().into(), project.clone().into()],
        ))
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    let (name, document) = template_document_for_save(&transaction, &project, input).await?;
    if transaction.query_one(sql("SELECT id FROM api_key_profile_templates WHERE project_id=? AND name=? COLLATE NOCASE AND id<>?",vec![project.clone().into(),name.clone().into(),template_id.clone().into()])).await?.is_some() {
        return Err(ApiError::ConflictNamed("template_name_conflict", "a template with this name already exists".into()));
    }
    transaction.execute(sql("UPDATE api_key_profile_templates SET name=?,document_json=?,updated_at=? WHERE id=? AND project_id=?",vec![name.into(),serde_json::to_string(&document).map_err(|error|ApiError::Internal(error.into()))?.into(),db::now().into(),template_id.clone().into(),project.clone().into()])).await?;
    audit_in(
        &transaction,
        &user,
        &project,
        "profile-template.update",
        &template_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(json!({"id":template_id})))
}

async fn delete_profile_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, template_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let user = actor_for(&state, &headers, Some(&project), "api_key:manage", true).await?;
    let transaction = state.db.begin().await?;
    let changed = transaction
        .execute(sql(
            "DELETE FROM api_key_profile_templates WHERE id=? AND project_id=?",
            vec![template_id.clone().into(), project.clone().into()],
        ))
        .await?
        .rows_affected();
    if changed != 1 {
        return Err(ApiError::NotFound);
    }
    audit_in(
        &transaction,
        &user,
        &project,
        "profile-template.delete",
        &template_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(json!({"ok":true})))
}

async fn export_profile_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, template_id)): Path<(String, String)>,
) -> Result<Json<ProfileTemplateTransfer>, ApiError> {
    actor_for(&state, &headers, Some(&project), "api_key:manage", false).await?;
    let row = state
        .db
        .query_one(sql(
            "SELECT name,document_json FROM api_key_profile_templates WHERE id=? AND project_id=?",
            vec![template_id.into(), project.into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    let raw: String = row.try_get("", "document_json")?;
    Ok(Json(ProfileTemplateTransfer {
        version: 1,
        name: row.try_get("", "name")?,
        profile: parse_stored_profile_document(&raw)?,
    }))
}

async fn import_profile_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<ProfileTemplateImport>,
) -> Result<Json<Value>, ApiError> {
    let user = actor_for(&state, &headers, Some(&project), "api_key:manage", true).await?;
    if input.document.version != 1 {
        return Err(ApiError::BadRequest(
            "unsupported template document version".into(),
        ));
    }
    let name = validate_profile_name(&input.document.name)?;
    validate_profile_document(&input.document.profile)?;
    let transaction = state.db.begin().await?;
    let existing = transaction
        .query_one(sql(
            "SELECT id FROM api_key_profile_templates WHERE project_id=? AND name=? COLLATE NOCASE",
            vec![project.clone().into(), name.clone().into()],
        ))
        .await?;
    let template_id = match (existing, input.conflict) {
        (Some(_), ProfileTemplateConflict::Fail) => {
            return Err(ApiError::ConflictNamed(
                "template_name_conflict",
                "a template with this name already exists".into(),
            ));
        }
        (Some(row), ProfileTemplateConflict::Overwrite) => {
            let template_id: String = row.try_get("", "id")?;
            transaction.execute(sql("UPDATE api_key_profile_templates SET document_json=?,updated_at=? WHERE id=? AND project_id=?",vec![serde_json::to_string(&input.document.profile).map_err(|error|ApiError::Internal(error.into()))?.into(),db::now().into(),template_id.clone().into(),project.clone().into()])).await?;
            template_id
        }
        (None, _) => {
            let template_id = id();
            transaction.execute(sql("INSERT INTO api_key_profile_templates(id,project_id,name,document_json,created_at,updated_at) VALUES(?,?,?,?,?,?)",vec![template_id.clone().into(),project.clone().into(),name.into(),serde_json::to_string(&input.document.profile).map_err(|error|ApiError::Internal(error.into()))?.into(),db::now().into(),db::now().into()])).await?;
            template_id
        }
    };
    audit_in(
        &transaction,
        &user,
        &project,
        "profile-template.import",
        &template_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(json!({"id":template_id})))
}

async fn apply_profile_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, template_id)): Path<(String, String)>,
    Json(input): Json<ProfileTemplateApply>,
) -> Result<Json<Value>, ApiError> {
    let user = actor_for(&state, &headers, Some(&project), "api_key:manage", true).await?;
    let transaction = state.db.begin().await?;
    let row = transaction
        .query_one(sql(
            "SELECT document_json FROM api_key_profile_templates WHERE id=? AND project_id=?",
            vec![template_id.clone().into(), project.clone().into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    let raw: String = row.try_get("", "document_json")?;
    let document = parse_stored_profile_document(&raw)?;
    let (profile_id, name) = match (input.target_profile_id, input.name) {
        (Some(_), Some(_)) => {
            return Err(ApiError::BadRequest(
                "name is only valid when creating a profile".into(),
            ));
        }
        (Some(profile_id), None) => {
            let (current_name, _) =
                load_profile_document(&transaction, &project, &profile_id).await?;
            (profile_id, current_name)
        }
        (None, name) => (id(), validate_profile_name(name.as_deref().unwrap_or(""))?),
    };
    write_profile_document(&transaction, &project, &profile_id, &name, &document).await?;
    audit_in(
        &transaction,
        &user,
        &project,
        "profile-template.apply",
        &profile_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(json!({"id":profile_id})))
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 2048)
        .ok_or_else(|| ApiError::BadRequest(format!("{key} is required")))
}
/// The console's bulk key action. Everything that decides the outcome — the
/// `api_key:manage` authorization, the owner-membership rule for owned keys, the
/// transaction and the audit rows — lives in the access layer, so this only checks
/// the request shape and maps the domain error. That is what keeps bulk key state
/// from being a second, weaker copy of the single-key contract.
async fn bulk_toggle_api_keys(
    state: &AppState,
    user: &crate::access::Principal,
    project: &str,
    value: &Value,
) -> Result<Json<Value>, ApiError> {
    let ids = value["ids"]
        .as_array()
        .filter(|ids| !ids.is_empty() && ids.len() <= 100)
        .ok_or_else(|| ApiError::BadRequest("ids must contain 1–100 items".into()))?;
    let enabled = value["enabled"]
        .as_bool()
        .ok_or_else(|| ApiError::BadRequest("enabled is required".into()))?;
    let ids = ids
        .iter()
        .map(|key| {
            key.as_str()
                .map(str::to_owned)
                .ok_or_else(|| ApiError::BadRequest("invalid id".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let updated =
        crate::access::set_scoped_api_keys_enabled(&state.db, user, project, &ids, enabled)
            .await
            .map_err(crate::api::errors::access)?;
    Ok(Json(json!({"updated":updated})))
}
async fn channel_dependency_preview(
    db: &impl ConnectionTrait,
    project: &str,
    ids: &[String],
) -> Result<Vec<Value>, ApiError> {
    let mut output = Vec::new();
    for id in ids {
        if let Some(row)=db.query_one(sql("SELECT p.id,p.name,(SELECT COUNT(*) FROM models WHERE provider_id=p.id) AS models,(SELECT COUNT(*) FROM channel_credentials WHERE provider_id=p.id) AS credentials FROM providers p WHERE p.id=? AND p.project_id=?",vec![id.clone().into(),project.into()])).await?{
            let models:i64=row.try_get("","models")?;
            let credentials:i64=row.try_get("","credentials")?;
            output.push(json!({"id":row.try_get::<String>("","id")?,"name":row.try_get::<String>("","name")?,"models":models,"credentials":credentials,"blocked":models+credentials>0}));
        }
    }
    Ok(output)
}
async fn mutate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, resource)): Path<(String, String)>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let mut resource_id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(id);
    // API-key lifecycle is not a table toggle. It needs the key contract's own
    // `api_key:manage` authorization — a role holding only that permission can change
    // a key through the dedicated route, so it must be able to do the same here — and
    // the owner-membership rule for owned keys, so it is an access-layer operation
    // rather than an UPDATE in this match. It runs before the generic transaction so
    // the two never hold separate SQLite connections at the same time.
    let key_bulk = resource == "bulk-toggle" && value["resource"].as_str() == Some("keys");
    let user = actor_for(
        &state,
        &headers,
        Some(&project),
        if key_bulk || resource == "key-profiles" {
            "api_key:manage"
        } else {
            "project:manage"
        },
        true,
    )
    .await?;
    if key_bulk {
        return bulk_toggle_api_keys(&state, &user, &project, &value).await;
    }
    // Catalog reads happen before the business transaction. An edit is scoped
    // through the model's provider, and never re-resolves a submitted card id.
    let existing_model = if resource == "models" {
        match value.get("id").and_then(Value::as_str) {
            Some(model_id) => state
                .db
                .query_one(sql(
                    "SELECT m.id FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=? AND p.project_id=?",
                    vec![model_id.into(), project.clone().into()],
                ))
                .await?
                .is_some(),
            None => false,
        }
    } else {
        false
    };
    let catalog_defaults = if resource == "models" && !existing_model {
        match value
            .get("catalog_model_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            Some(card_id) => Some(
                db::catalog_model_defaults(&state.db, card_id)
                    .await?
                    .ok_or_else(|| {
                        ApiError::BadRequest("unknown or stale catalog model card".into())
                    })?,
            ),
            None => None,
        }
    } else {
        None
    };
    let transaction = state.db.begin().await?;
    match resource.as_str() {
        "channel-preview" => {
            let action = text(&value, "action")?;
            let ids = value["ids"]
                .as_array()
                .ok_or_else(|| ApiError::BadRequest("ids are required".into()))?
                .iter()
                .map(|id| {
                    id.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| ApiError::BadRequest("invalid id".into()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if ids.is_empty()
                || ids.len() > 100
                || !matches!(action, "clone" | "merge" | "bulk_delete")
            {
                return Err(ApiError::BadRequest("invalid channel preview".into()));
            }
            let dependencies = channel_dependency_preview(&transaction, &project, &ids).await?;
            if action == "merge" && ids.len() == 2 {
                for channel in &ids {
                    if transaction
                        .query_one(sql(
                            "SELECT 1 AS present FROM providers WHERE id=? AND project_id=?",
                            vec![channel.clone().into(), project.clone().into()],
                        ))
                        .await?
                        .is_none()
                    {
                        return Err(ApiError::NotFound);
                    }
                }
                let collisions=transaction.query_all(sql("SELECT s.public_name AS name FROM models s JOIN providers sp ON sp.id=s.provider_id JOIN models t ON t.public_name=s.public_name JOIN providers tp ON tp.id=t.provider_id WHERE s.provider_id=? AND t.provider_id=? AND sp.project_id=? AND tp.project_id=? ORDER BY s.public_name",vec![ids[0].clone().into(),ids[1].clone().into(),project.clone().into(),project.clone().into()])).await?.into_iter().map(|row|row.try_get::<String>("","name")).collect::<Result<Vec<_>,_>>()?;
                return Ok(Json(
                    json!({"action":action,"dependencies":dependencies,"collisions":collisions,"credential_secrets_copied":false}),
                ));
            }
            return Ok(Json(
                json!({"action":action,"dependencies":dependencies,"credential_secrets_copied":false}),
            ));
        }
        "channel-clone" => {
            let source = text(&value, "source_id")?;
            let name = text(&value, "name")?;
            let row=transaction.query_one(sql("SELECT kind,base_url,enabled,settings_json FROM providers WHERE id=? AND project_id=?",vec![source.into(),project.clone().into()])).await?.ok_or(ApiError::NotFound)?;
            let changed=transaction.execute(sql("INSERT INTO providers(id,name,kind,base_url,enabled,created_at,updated_at,project_id,settings_json) VALUES(?,?,?,?,?,?,?,?,?)",vec![resource_id.clone().into(),name.into(),row.try_get::<String>("","kind")?.into(),row.try_get::<String>("","base_url")?.into(),row.try_get::<bool>("","enabled")?.into(),db::now().into(),db::now().into(),project.clone().into(),row.try_get::<String>("","settings_json")?.into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::ConflictNamed(
                    "duplicate_channel",
                    "a channel with that name already exists".into(),
                ));
            }
            transaction.execute(sql("INSERT INTO channel_settings(provider_id,endpoint_mappings_json,model_rules_json,parameter_overrides_json,retry_statuses_json,auto_disable_policy_json,updated_at) SELECT ?,endpoint_mappings_json,model_rules_json,parameter_overrides_json,retry_statuses_json,auto_disable_policy_json,? FROM channel_settings WHERE provider_id=?",vec![resource_id.clone().into(),db::now().into(),source.into()])).await?;
            let models=transaction.query_all(sql("SELECT public_name,upstream_name,capabilities,input_price_micros,output_price_micros,priority,enabled,catalog_metadata_json,disable_developer_settings_inheritance FROM models WHERE provider_id=? AND lifecycle='active' ORDER BY id",vec![source.into()])).await?;
            for model in models {
                transaction.execute(sql("INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,input_price_micros,output_price_micros,priority,enabled,created_at,catalog_metadata_json,disable_developer_settings_inheritance) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",vec![id().into(),resource_id.clone().into(),model.try_get::<String>("","public_name")?.into(),model.try_get::<String>("","upstream_name")?.into(),model.try_get::<String>("","capabilities")?.into(),model.try_get::<i64>("","input_price_micros")?.into(),model.try_get::<i64>("","output_price_micros")?.into(),model.try_get::<i64>("","priority")?.into(),model.try_get::<bool>("","enabled")?.into(),db::now().into(),model.try_get::<String>("","catalog_metadata_json")?.into(),model.try_get::<bool>("","disable_developer_settings_inheritance")?.into()])).await?;
            }
        }
        "channel-merge" => {
            let source = text(&value, "source_id")?.to_owned();
            let target = text(&value, "target_id")?.to_owned();
            if source == target {
                return Err(ApiError::BadRequest(
                    "source and target channels must differ".into(),
                ));
            }
            for channel in [&source, &target] {
                if transaction
                    .query_one(sql(
                        "SELECT 1 AS present FROM providers WHERE id=? AND project_id=?",
                        vec![channel.as_str().into(), project.clone().into()],
                    ))
                    .await?
                    .is_none()
                {
                    return Err(ApiError::NotFound);
                }
            }
            let collisions=transaction.query_one(sql("SELECT COUNT(*) AS n FROM models s JOIN models t ON t.public_name=s.public_name WHERE s.provider_id=? AND t.provider_id=?",vec![source.clone().into(),target.clone().into()])).await?.and_then(|row|row.try_get::<i64>("","n").ok()).unwrap_or(0);
            if collisions > 0 {
                return Err(ApiError::ConflictNamed(
                    "duplicate_model",
                    "the source and target channels contain models with the same public name"
                        .into(),
                ));
            }
            transaction
                .execute(sql(
                    "UPDATE models SET provider_id=? WHERE provider_id=?",
                    vec![target.clone().into(), source.into()],
                ))
                .await?;
            resource_id = target;
        }
        "bulk-delete" => {
            if text(&value, "resource")? != "channels" {
                return Err(ApiError::BadRequest("unsupported bulk resource".into()));
            }
            let ids = value["ids"]
                .as_array()
                .filter(|ids| !ids.is_empty() && ids.len() <= 100)
                .ok_or_else(|| ApiError::BadRequest("ids must contain 1–100 items".into()))?
                .iter()
                .map(|id| {
                    id.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| ApiError::BadRequest("invalid id".into()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let dependencies = channel_dependency_preview(&transaction, &project, &ids).await?;
            if dependencies.iter().any(|row| row["blocked"] == true) {
                return Err(ApiError::ConflictNamed(
                    "resource_in_use",
                    "one or more channels still have models or credentials".into(),
                ));
            }
            let mut deleted = 0u64;
            for channel in dependencies.iter().filter_map(|row| row["id"].as_str()) {
                deleted += transaction
                    .execute(sql(
                        "DELETE FROM providers WHERE id=? AND project_id=?",
                        vec![channel.into(), project.clone().into()],
                    ))
                    .await?
                    .rows_affected();
                audit_in(&transaction, &user, &project, "channels.delete", channel).await?;
            }
            transaction.commit().await?;
            return Ok(Json(json!({"deleted":deleted})));
        }
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
            let current=transaction.query_one(sql("SELECT endpoint_mappings_json,model_rules_json,parameter_overrides_json,retry_statuses_json,auto_disable_policy_json,proxy_url,proxy_username,proxy_secret_envelope,proxy_reuse_connections,proxy_preset_id FROM channel_settings WHERE provider_id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![provider.into(),project.clone().into()])).await?.ok_or(ApiError::NotFound)?;
            let document = |key: &str, column: &str| -> Result<String, ApiError> {
                Ok(value
                    .get(key)
                    .filter(|item| !item.is_null())
                    .map(Value::to_string)
                    .unwrap_or(current.try_get::<String>("", column)?))
            };
            if let Some(model_rules) = value.get("model_rules").filter(|item| !item.is_null()) {
                validate_model_rules(model_rules)?;
            }
            let proxy_url = match value.get("proxy_url") {
                Some(Value::String(url)) if !url.trim().is_empty() => {
                    let url = url.trim().to_owned();
                    validate_proxy_url(&url)?;
                    Some(url)
                }
                Some(Value::String(_)) | Some(Value::Null) => None,
                Some(_) => return Err(ApiError::BadRequest("proxy URL must be a string".into())),
                None => current.try_get("", "proxy_url")?,
            };
            let proxy_username = match value.get("proxy_username") {
                Some(Value::String(username)) if !username.trim().is_empty() => {
                    if username.len() > 256 || username.chars().any(char::is_control) {
                        return Err(ApiError::BadRequest("invalid proxy username".into()));
                    }
                    Some(username.trim().to_owned())
                }
                Some(Value::String(_)) | Some(Value::Null) => None,
                Some(_) => {
                    return Err(ApiError::BadRequest(
                        "proxy username must be a string".into(),
                    ));
                }
                None => current.try_get("", "proxy_username")?,
            };
            let supplied_password = value
                .get("proxy_password")
                .and_then(Value::as_str)
                .filter(|password| !password.is_empty());
            if supplied_password.is_some_and(|password| {
                password.len() > 4096 || password.chars().any(char::is_control)
            }) {
                return Err(ApiError::BadRequest("invalid proxy password".into()));
            }
            let proxy_secret_envelope = if proxy_url.is_none() {
                None
            } else if let Some(password) = supplied_password {
                Some(state.secrets.encrypt(password)?)
            } else {
                current.try_get("", "proxy_secret_envelope")?
            };
            if proxy_secret_envelope.is_some() && proxy_username.is_none() {
                return Err(ApiError::BadRequest(
                    "proxy username is required when a proxy password is configured".into(),
                ));
            }
            let proxy_reuse_connections = value
                .get("proxy_reuse_connections")
                .and_then(Value::as_bool)
                .unwrap_or(current.try_get("", "proxy_reuse_connections")?);
            let proxy_preset = value
                .get("proxy_preset_id")
                .map(|item| item.as_str().map(str::to_owned))
                .unwrap_or(current.try_get::<Option<String>>("", "proxy_preset_id")?);
            if let Some(preset) = &proxy_preset
                && transaction
                    .query_one(sql(
                        "SELECT id FROM proxy_presets WHERE id=? AND enabled=1",
                        vec![preset.into()],
                    ))
                    .await?
                    .is_none()
            {
                return Err(ApiError::BadRequest("unknown proxy preset".into()));
            }
            let changed = transaction.execute(sql("UPDATE channel_settings SET endpoint_mappings_json=?,model_rules_json=?,parameter_overrides_json=?,retry_statuses_json=?,auto_disable_policy_json=?,proxy_url=?,proxy_username=?,proxy_secret_envelope=?,proxy_reuse_connections=?,proxy_preset_id=?,updated_at=? WHERE provider_id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![document("endpoint_mappings","endpoint_mappings_json")?.into(),document("model_rules","model_rules_json")?.into(),document("parameter_overrides","parameter_overrides_json")?.into(),document("retry_statuses","retry_statuses_json")?.into(),document("auto_disable_policy","auto_disable_policy_json")?.into(),proxy_url.into(),proxy_username.into(),proxy_secret_envelope.into(),proxy_reuse_connections.into(),proxy_preset.into(),db::now().into(),provider.into(),project.clone().into()])).await?.rows_affected();
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
            let current_metadata = transaction
                .query_one(sql(
                    "SELECT catalog_metadata_json FROM models WHERE id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                    vec![resource_id.clone().into(), project.clone().into()],
                ))
                .await?
                .map(|row| row.try_get::<String>("", "catalog_metadata_json"))
                .transpose()?;
            let capabilities = catalog_defaults
                .as_ref()
                .map(|defaults| json!(defaults.capabilities))
                .unwrap_or_else(|| {
                    value
                        .get("capabilities")
                        .cloned()
                        .unwrap_or(json!(["chat"]))
                });
            if !capabilities.is_array() {
                return Err(ApiError::BadRequest("capabilities must be an array".into()));
            }
            let input_price = catalog_defaults
                .as_ref()
                .map(|defaults| defaults.input_price_micros)
                .unwrap_or_else(|| value["input_price_micros"].as_i64().unwrap_or(0).max(0));
            let output_price = catalog_defaults
                .as_ref()
                .map(|defaults| defaults.output_price_micros)
                .unwrap_or_else(|| value["output_price_micros"].as_i64().unwrap_or(0).max(0));
            let catalog_metadata = catalog_defaults
                .as_ref()
                .map(|defaults| defaults.metadata.clone())
                .or(current_metadata)
                .unwrap_or_else(|| "{}".into());
            // UNIQUE(provider_id, public_name, upstream_name) would otherwise
            // surface as an opaque 500 with the constraint name only in the log.
            let duplicate = transaction
                .query_one(sql(
                    "SELECT id FROM models WHERE provider_id=? AND public_name=? AND upstream_name=? AND id NOT IN (?)",
                    vec![
                        provider.into(),
                        text(&value, "public_name")?.into(),
                        text(&value, "upstream_name")?.into(),
                        resource_id.clone().into(),
                    ],
                ))
                .await?;
            if duplicate.is_some() {
                return Err(ApiError::ConflictNamed(
                    "duplicate_model",
                    "this channel already has a model with that name".into(),
                ));
            }
            let changed = transaction.execute(sql("INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,input_price_micros,output_price_micros,priority,enabled,created_at,catalog_metadata_json,disable_developer_settings_inheritance) VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET provider_id=excluded.provider_id,public_name=excluded.public_name,upstream_name=excluded.upstream_name,capabilities=excluded.capabilities,input_price_micros=excluded.input_price_micros,output_price_micros=excluded.output_price_micros,priority=excluded.priority,enabled=excluded.enabled,catalog_metadata_json=excluded.catalog_metadata_json,disable_developer_settings_inheritance=excluded.disable_developer_settings_inheritance WHERE models.provider_id IN (SELECT id FROM providers WHERE project_id=?)",vec![resource_id.clone().into(),provider.into(),text(&value,"public_name")?.into(),text(&value,"upstream_name")?.into(),capabilities.to_string().into(),input_price.into(),output_price.into(),value["priority"].as_i64().unwrap_or(100).into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),catalog_metadata.into(),value["disable_developer_settings_inheritance"].as_bool().unwrap_or(false).into(),project.clone().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
        }
        "associations" => {
            let match_type = value["match_type"].as_str().unwrap_or("exact");
            if !matches!(match_type, "exact" | "regex" | "tag" | "channel_tags_regex") {
                return Err(ApiError::BadRequest(
                    "invalid association match type".into(),
                ));
            }
            if matches!(match_type, "regex" | "channel_tags_regex") {
                crate::orchestration::policy::regex(text(&value, "pattern")?)?;
            }
            // Validate the condition tree with the evaluator that will run it.
            // A malformed condition used to be stored happily and then abort
            // candidate generation for the whole project, surfacing as a generic
            // 500 on every request with nothing naming the offending rule.
            let conditions = value
                .get("conditions")
                .cloned()
                .unwrap_or_else(|| json!({"version":1}));
            crate::orchestration::policy::validate_conditions(&conditions).map_err(|_| {
                ApiError::BadRequest(
                    "invalid conditions: only version, all, any, field, op and value are allowed; field must be a JSON pointer; value must match the operator".into(),
                )
            })?;
            let exclusions = value
                .get("exclusions")
                .cloned()
                .unwrap_or_else(|| json!({"version":1}));
            crate::orchestration::policy::AssociationExclusions::parse(&exclusions).map_err(
                |_| {
                    ApiError::BadRequest(
                        "invalid exclusions: use version 1 arrays for channel_name_patterns, channel_ids and channel_tags; name patterns must be valid regular expressions".into(),
                    )
                },
            )?;
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
            let changed = transaction.execute(sql("INSERT INTO model_associations(id,project_id,model_id,provider_id,match_type,pattern,conditions_json,exclusions_json,priority,weight,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET model_id=excluded.model_id,provider_id=excluded.provider_id,match_type=excluded.match_type,pattern=excluded.pattern,conditions_json=excluded.conditions_json,exclusions_json=excluded.exclusions_json,priority=excluded.priority,weight=excluded.weight,enabled=excluded.enabled,updated_at=excluded.updated_at WHERE model_associations.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),model.into(),provider.into(),match_type.into(),text(&value,"pattern")?.into(),conditions.to_string().into(),exclusions.to_string().into(),value["priority"].as_i64().unwrap_or(100).into(),value["weight"].as_i64().unwrap_or(1).clamp(1,10000).into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),db::now().into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Forbidden);
            }
        }
        "key-profiles" => {
            let input: ProfileWriteDocument =
                serde_json::from_value(value.clone()).map_err(|error| {
                    ApiError::BadRequest(format!("invalid profile document: {error}"))
                })?;
            let _submitted_id = input.id;
            let document = ProfileTemplateDocument {
                version: 1,
                rpm_limit: input.rpm_limit,
                tpm_limit: input.tpm_limit,
                budget_micros: input.budget_micros,
                routing_policy: input.routing_policy,
                mappings: input.mappings,
                allowed_models: input.allowed_models,
            };
            write_profile_document(&transaction, &project, &resource_id, &input.name, &document)
                .await?;
        }
        "prompts" => {
            // A malformed activation used to be stored happily and then abort
            // prompt injection for the whole project on every gateway request,
            // surfacing as a generic 500 with nothing naming the offending rule.
            // Validate it where it is written, like the association conditions.
            let activation = match value.get("activation") {
                // A cleared JSON box in the console serialises as null.
                None | Some(Value::Null) => json!({"version":1}),
                Some(document) => document.clone(),
            };
            crate::orchestration::policy::validate_conditions(&activation)
                .map_err(|_| ApiError::BadRequest("invalid prompt activation conditions".into()))?;
            let role = text(&value, "role")?;
            if !matches!(role, "system" | "user" | "assistant") {
                return Err(ApiError::BadRequest(
                    "prompt role must be system, user or assistant".into(),
                ));
            }
            let current = transaction
                .query_one(sql(
                    "SELECT enabled,\"order\",action FROM prompts WHERE id=? AND project_id=?",
                    vec![resource_id.clone().into(), project.clone().into()],
                ))
                .await?;
            let action = match value.get("action") {
                None => current
                    .as_ref()
                    .map(|row| row.try_get::<String>("", "action"))
                    .transpose()?
                    .unwrap_or_else(|| "prepend".into()),
                Some(Value::String(action)) if matches!(action.as_str(), "prepend" | "append") => {
                    action.clone()
                }
                Some(_) => {
                    return Err(ApiError::BadRequest(
                        "prompt action must be prepend or append".into(),
                    ));
                }
            };
            let order = match value.get("order") {
                None => current
                    .as_ref()
                    .map(|row| row.try_get::<i64>("", "order"))
                    .transpose()?
                    .unwrap_or(0),
                Some(order) => order.as_i64().ok_or_else(|| {
                    ApiError::BadRequest("prompt order must be an integer".into())
                })?,
            };
            let enabled = match value.get("enabled") {
                None => current
                    .as_ref()
                    .map(|row| row.try_get::<bool>("", "enabled"))
                    .transpose()?
                    .unwrap_or(false),
                Some(enabled) => enabled.as_bool().ok_or_else(|| {
                    ApiError::BadRequest("prompt enabled must be a boolean".into())
                })?,
            };
            let existing = transaction
                .query_one(sql(
                    "SELECT id FROM prompts WHERE project_id=? AND name=? AND id NOT IN (?)",
                    vec![
                        project.clone().into(),
                        text(&value, "name")?.into(),
                        resource_id.clone().into(),
                    ],
                ))
                .await?;
            if existing.is_some() {
                // UNIQUE(project_id,name) would otherwise surface as an opaque 500.
                return Err(ApiError::Conflict(
                    "a prompt with that name already exists in this project".into(),
                ));
            }
            let changed = transaction.execute(sql("INSERT INTO prompts(id,project_id,name,role,content,activation_json,enabled,\"order\",action,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,role=excluded.role,content=excluded.content,activation_json=excluded.activation_json,enabled=excluded.enabled,\"order\"=excluded.\"order\",action=excluded.action,updated_at=excluded.updated_at WHERE prompts.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),role.into(),text(&value,"content")?.into(),activation.to_string().into(),enabled.into(),order.into(),action.into(),db::now().into(),db::now().into()])).await?.rows_affected();
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
            // Same defect as prompts: an unvalidated scope document is only
            // parsed per request, so a bad one 500s the whole project. A cleared
            // JSON box serialises as null and must become the default document —
            // persisting the literal `null` is exactly what breaks the project.
            let scopes = match value.get("scopes") {
                None | Some(Value::Null) => json!({"version":1}),
                Some(document) => document.clone(),
            };
            crate::orchestration::policy::validate_conditions(&scopes)
                .map_err(|_| ApiError::BadRequest("invalid protection rule scopes".into()))?;
            let current = transaction
                .query_one(sql(
                    "SELECT description,state FROM prompt_protection_rules WHERE id=? AND project_id=?",
                    vec![resource_id.clone().into(), project.clone().into()],
                ))
                .await?;
            let description = match value.get("description") {
                None => current
                    .as_ref()
                    .map(|row| row.try_get::<String>("", "description"))
                    .transpose()?
                    .unwrap_or_default(),
                Some(Value::Null) => String::new(),
                Some(Value::String(description)) if description.len() <= 2048 => {
                    description.clone()
                }
                Some(_) => {
                    return Err(ApiError::BadRequest(
                        "protection description must be text up to 2048 bytes".into(),
                    ));
                }
            };
            let state_value = match value.get("state") {
                None => current
                    .as_ref()
                    .map(|row| row.try_get::<String>("", "state"))
                    .transpose()?
                    .unwrap_or_else(|| "active".into()),
                Some(Value::String(state)) if matches!(state.as_str(), "active" | "archived") => {
                    state.clone()
                }
                Some(_) => {
                    return Err(ApiError::BadRequest(
                        "protection state must be active or archived".into(),
                    ));
                }
            };
            let changed = transaction.execute(sql("INSERT INTO prompt_protection_rules(id,project_id,name,description,role_pattern,content_pattern,action,replacement,scopes_json,test_mode,enabled,state,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,role_pattern=excluded.role_pattern,content_pattern=excluded.content_pattern,action=excluded.action,replacement=excluded.replacement,scopes_json=excluded.scopes_json,test_mode=excluded.test_mode,enabled=excluded.enabled,state=excluded.state,updated_at=excluded.updated_at WHERE prompt_protection_rules.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),description.into(),value["role_pattern"].as_str().into(),text(&value,"content_pattern")?.into(),action.into(),value["replacement"].as_str().into(),scopes.to_string().into(),value["test_mode"].as_bool().unwrap_or(false).into(),value["enabled"].as_bool().unwrap_or(true).into(),state_value.into(),db::now().into(),db::now().into()])).await?.rows_affected();
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
                    "provider_id IN (SELECT id FROM providers WHERE project_id=?) AND lifecycle='active'",
                ),
                "credentials" => (
                    "channel_credentials",
                    "provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                ),
                // Prompts and protection rules are project-scoped and carry an
                // `enabled` flag, so they belong to the same lifecycle as the
                // other bulk resources instead of being refused.
                "prompts" => ("prompts", "project_id=?"),
                // API keys carry an `enabled` flag and a `project_id`, but they are
                // not handled here: their lifecycle belongs to the access layer (see
                // `bulk_toggle_api_keys`), which is where the `api_key:manage`
                // contract and the owner-membership rule live.
                "protection" => ("prompt_protection_rules", "project_id=?"),
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
                .map(|secret| {
                    storage::validate_secret(&config, secret)?;
                    storage::envelope(&state.secrets, &project, &resource_id, secret)
                })
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
                transaction.execute(sql("UPDATE data_storage_configs SET name=?,config_json=?,kind=?,secret_envelope=COALESCE(?,secret_envelope),enabled=?,revision=revision+1,updated_at=? WHERE id=? AND revision=?",vec![text(&value,"name")?.into(),serde_json::to_string(&config).unwrap().into(),storage_kind(&config).into(),secret.into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),resource_id.clone().into(),value["revision"].as_i64().into()])).await?;
            } else {
                transaction.execute(sql("INSERT INTO data_storage_configs(id,project_id,name,kind,config_json,secret_envelope,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),storage_kind(&config).into(),serde_json::to_string(&config).unwrap().into(),secret.into(),db::now().into(),db::now().into()])).await?;
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
                "probe" | "quota" | "model_sync" | "automatic_backup" | "backup_retention"
            ) {
                return Err(ApiError::BadRequest("invalid schedule kind".into()));
            }
            let interval = value["interval_secs"].as_i64().unwrap_or(3600);
            if !(30..=31536000).contains(&interval) {
                return Err(ApiError::BadRequest("invalid schedule interval".into()));
            }
            if kind == "model_sync" {
                let provider = value
                    .pointer("/payload/provider_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ApiError::BadRequest("model sync requires a channel".into()))?;
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
            }
            let changed=transaction.execute(sql("INSERT INTO operation_schedules(id,project_id,kind,payload_json,interval_secs,next_run_at,enabled) VALUES(?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET kind=excluded.kind,payload_json=excluded.payload_json,interval_secs=excluded.interval_secs,enabled=excluded.enabled,revision=operation_schedules.revision+1 WHERE operation_schedules.project_id=excluded.project_id AND operation_schedules.revision=?",vec![resource_id.clone().into(),project.clone().into(),kind.into(),value["payload"].to_string().into(),interval.into(),db::now().into(),value["enabled"].as_bool().unwrap_or(true).into(),value["revision"].as_i64().unwrap_or(0).into()])).await?.rows_affected();
            if changed != 1 {
                return Err(ApiError::Conflict("schedule revision changed".into()));
            }
        }
        "probe" | "quota" | "model-sync" => {
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
            if resource == "probe" {
                let model = text(&value, "model_id")?;
                if transaction
                    .query_one(sql(
                        "SELECT m.id FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=? AND m.provider_id=? AND p.project_id=? AND p.enabled=1 AND m.enabled=1 AND m.lifecycle='active'",
                        vec![model.into(), provider.into(), project.clone().into()],
                    ))
                    .await?
                    .is_none()
                {
                    return Err(ApiError::BadRequest(
                        "model is not enabled for this channel".into(),
                    ));
                }
            }
            let job_kind = if resource == "model-sync" {
                "model_sync"
            } else {
                resource.as_str()
            };
            let job = jobs::enqueue(
                &transaction,
                Some(&project),
                job_kind,
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
            transaction.execute(sql("INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES(?,?,?,?,?) ON CONFLICT(project_id,resource_type) DO UPDATE SET retention_days=excluded.retention_days,updated_at=excluded.updated_at",vec![resource_id.clone().into(),project.clone().into(),kind.into(),days.into(),db::now().into()])).await?;
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
            let timeout = value["timeout_secs"].as_i64().unwrap_or(20);
            if !(1..=300).contains(&timeout) {
                return Err(ApiError::BadRequest(
                    "webhook timeout must be 1–300 seconds".into(),
                ));
            }
            let proxy_preset = if value
                .as_object()
                .is_some_and(|object| object.contains_key("proxy_preset_id"))
            {
                value
                    .get("proxy_preset_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            } else {
                transaction
                    .query_one(sql(
                        "SELECT proxy_preset_id FROM webhooks WHERE id=? AND project_id=?",
                        vec![resource_id.clone().into(), project.clone().into()],
                    ))
                    .await?
                    .map(|row| row.try_get::<Option<String>>("", "proxy_preset_id"))
                    .transpose()?
                    .flatten()
            };
            if let Some(preset) = &proxy_preset
                && transaction
                    .query_one(sql(
                        "SELECT id FROM proxy_presets WHERE id=? AND enabled=1",
                        vec![preset.into()],
                    ))
                    .await?
                    .is_none()
            {
                return Err(ApiError::BadRequest("unknown proxy preset".into()));
            }
            let changed=transaction.execute(sql("INSERT INTO webhooks(id,project_id,name,url,secret_envelope,headers_json,body_template_json,subscriptions_json,timeout_secs,proxy_preset_id,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,url=excluded.url,secret_envelope=COALESCE(excluded.secret_envelope,webhooks.secret_envelope),headers_json=excluded.headers_json,body_template_json=excluded.body_template_json,subscriptions_json=excluded.subscriptions_json,timeout_secs=excluded.timeout_secs,proxy_preset_id=excluded.proxy_preset_id,enabled=excluded.enabled,updated_at=excluded.updated_at WHERE webhooks.project_id=excluded.project_id",vec![resource_id.clone().into(),project.clone().into(),text(&value,"name")?.into(),url.into(),secret.into(),json!({"version":1,"headers":value.get("headers").cloned().unwrap_or(json!({}))}).to_string().into(),json!({"version":1,"body":value.get("body").cloned().unwrap_or(json!("$event"))}).to_string().into(),json!({"version":1,"events":value.get("events").cloned().unwrap_or(json!([]))}).to_string().into(),timeout.into(),proxy_preset.into(),value["enabled"].as_bool().unwrap_or(true).into(),db::now().into(),db::now().into()])).await?.rows_affected();
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
fn validate_proxy_url(value: &str) -> Result<(), ApiError> {
    if value.len() > 2048 {
        return Err(ApiError::BadRequest("proxy URL is too long".into()));
    }
    let url = reqwest::Url::parse(value)
        .map_err(|_| ApiError::BadRequest("proxy URL is invalid".into()))?;
    if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ApiError::BadRequest(
            "proxy URL must use http, https, socks5 or socks5h without embedded credentials".into(),
        ));
    }
    Ok(())
}

fn storage_kind(config: &storage::Config) -> &'static str {
    match config {
        storage::Config::Local { .. } => "local",
        storage::Config::S3 { .. } => "s3",
        storage::Config::Gcs { .. } => "gcs",
        storage::Config::Webdav { .. } => "webdav",
    }
}

async fn test_storage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, storage_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let row = state
        .db
        .query_one(sql(
            "SELECT config_json,secret_envelope FROM data_storage_configs WHERE id=? AND project_id=? AND enabled=1",
            vec![storage_id.clone().into(), project.clone().into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    let config: storage::Config = serde_json::from_str(&row.try_get::<String>("", "config_json")?)
        .map_err(|_| storage::invalid())?;
    let secret: Option<String> = row.try_get("", "secret_envelope")?;
    // The network check intentionally runs without a SQLite business transaction.
    storage::test_connection(&state, &project, &storage_id, &config, secret.as_deref()).await?;
    let tx = state.db.begin().await?;
    audit_in(&tx, &user, &project, "storage.test", &storage_id).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, resource, id)): Path<(String, String, String)>,
) -> Result<Json<Value>, ApiError> {
    let user = actor_for(
        &state,
        &headers,
        Some(&project),
        if resource == "key-profiles" {
            "api_key:manage"
        } else {
            "project:manage"
        },
        true,
    )
    .await?;
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
    // Ownership first, then dependencies. The dependency checks below are not
    // project-scoped, so running them first let a project-A manager tell a foreign
    // channel or model that is still in use (409) from a foreign or absent id that
    // is not (404): an oracle over another project's resources. A resource this
    // project does not own is simply not here, and it is answered the same way an id
    // that does not exist is — before anything else is read.
    if tx
        .query_one(sql(
            format!("SELECT id FROM {table} WHERE ({scope}) AND id=?"),
            vec![project.clone().into(), id.clone().into()],
        ))
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    if resource == "channels" {
        // Deleting a channel cascades into its models, credentials, settings and
        // price history. With history present the immutability trigger aborts and
        // the operator sees an opaque 500; without it, the models and credentials
        // vanish silently. Neither is acceptable, so refuse and say what is at
        // stake.
        let attached = tx
            .query_one(sql(
                "SELECT (SELECT COUNT(*) FROM models WHERE provider_id=?) + (SELECT COUNT(*) FROM channel_credentials WHERE provider_id=?) AS n",
                vec![id.clone().into(), id.clone().into()],
            ))
            .await?
            .and_then(|row| row.try_get::<i64>("", "n").ok())
            .unwrap_or(0);
        if attached > 0 {
            return Err(ApiError::ConflictNamed(
                "resource_in_use",
                "this channel still has models or credentials; remove those first so nothing is deleted silently"
                    .into(),
            ));
        }
    }
    if resource == "models" {
        let lifecycle = tx
            .query_one(sql(
                "SELECT lifecycle FROM models WHERE id=? AND provider_id IN (SELECT id FROM providers WHERE project_id=?)",
                vec![id.clone().into(), project.clone().into()],
            ))
            .await?
            .and_then(|row| row.try_get::<String>("", "lifecycle").ok())
            .ok_or(ApiError::NotFound)?;
        if lifecycle != "archived" {
            return Err(ApiError::ConflictNamed(
                "archive_required",
                "archive the model and review its delete impact before deleting it".into(),
            ));
        }
        // Deleting a model cascades into immutable price history, where a trigger
        // aborts the statement — which surfaced as an opaque 500 with nothing
        // naming the cause. Refuse with the reason instead.
        let referenced = tx
            .query_one(sql(
                "SELECT (SELECT COUNT(*) FROM model_prices WHERE model_id=?) + (SELECT COUNT(*) FROM usage_logs WHERE model_id=?) AS n",
                vec![id.clone().into(), id.clone().into()],
            ))
            .await?
            .and_then(|row| row.try_get::<i64>("", "n").ok())
            .unwrap_or(0);
        if referenced > 0 {
            return Err(ApiError::ConflictNamed(
                "history_retained",
                "this archived model has immutable price or usage history and cannot be deleted"
                    .into(),
            ));
        }
    }
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
    if resource == "models" {
        state.orchestrator.reset_derived();
    }
    Ok(Json(json!({"ok":true})))
}
/// What artifacts this project has, without their payloads. The envelope is the
/// encrypted database image and is only returned by the single-artifact route.
async fn list_artifacts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
) -> Result<Json<Value>, ApiError> {
    actor(&state, &headers, Some(&project), false).await?;
    let rows = state
        .db
        .query_all(sql(
            "SELECT id,digest,manifest_json,created_at FROM backup_artifacts WHERE project_id=? ORDER BY created_at DESC LIMIT 200",
            vec![project.into()],
        ))
        .await?;
    let artifacts: Vec<Value> = rows
        .iter()
        .filter_map(|row| {
            Some(json!({
                "id": row.try_get::<String>("", "id").ok()?,
                "digest": row.try_get::<String>("", "digest").ok()?,
                "manifest": serde_json::from_str::<Value>(
                    &row.try_get::<String>("", "manifest_json").ok()?
                ).ok()?,
                "created_at": row.try_get::<i64>("", "created_at").ok()?,
            }))
        })
        .collect();
    Ok(Json(json!({"artifacts": artifacts})))
}

/// Re-download an artifact created earlier, including its encrypted payload.
async fn get_artifact(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, artifact)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let row = state
        .db
        .query_one(sql(
            "SELECT id,digest,manifest_json,envelope,created_at FROM backup_artifacts WHERE id=? AND project_id=?",
            vec![artifact.clone().into(), project.clone().into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    // Downloading a copy of the encrypted database is worth an audit trail.
    state
        .db
        .execute(sql(
            "INSERT INTO audit_events(id,action,resource_type,resource_id,details,created_at) VALUES(?,'backup.artifact.read','backup_artifact',?,?,?)",
            vec![
                crate::operations::id().into(),
                artifact.clone().into(),
                json!({"principal_id": user.subject_id, "digest": row.try_get::<String>("", "digest").unwrap_or_default()}).to_string().into(),
                db::now().into(),
            ],
        ))
        .await?;
    // The console restores from the document this route returns, and its parser
    // requires `version`, `project_id` and `resources` alongside the envelope. The
    // five-field shape this used to send made the console reject its own artifact
    // ("该文件不是可恢复的备份产物。"), so the export path and this route now build
    // the document the same way.
    let artifact = crate::operations::backup::read_artifact(&state.db, &artifact, &project)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(
        serde_json::to_value(artifact).map_err(|error| ApiError::Internal(error.into()))?,
    ))
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
    #[serde(default)]
    strategy: backup::Conflict,
    #[serde(default)]
    strategies: std::collections::BTreeMap<String, backup::Conflict>,
}
async fn restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(input): Json<Restore>,
) -> Result<Json<Value>, ApiError> {
    let user = actor(&state, &headers, Some(&project), true).await?;
    let count = backup::restore_with_strategies_as(
        &state,
        &project,
        &input.artifact,
        &backup::RestoreStrategies {
            default: input.strategy,
            resources: input.strategies,
        },
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
