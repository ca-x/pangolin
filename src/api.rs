use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use sea_orm::DatabaseConnection;
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, OwnedMutexGuard};
use uuid::Uuid;

mod catalog_api;
pub(crate) mod errors;
pub(crate) mod gateway;
mod operations_api;
mod protocols;

use crate::{
    config::Config,
    crypto::{self, SecretBox},
    db,
    models::{ApiKeyInput, LoginRequest, ModelInput, ProviderInput, SetupRequest, User},
    observability::{ObservationStore, RequestEvent, RequestFilter},
};

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Arc<Config>,
    pub secrets: SecretBox,
    pub observations: ObservationStore,
    pub client: reqwest::Client,
    pub oidc_client: reqwest::Client,
    pub budget_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    pub maintenance: Arc<tokio::sync::RwLock<()>>,
    pub orchestrator: Arc<crate::orchestration::Runtime>,
}

impl AppState {
    async fn lock_budget_key(&self, key_id: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.budget_locks.lock().await;
            Arc::clone(
                locks
                    .entry(key_id.to_owned())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        lock.lock_owned().await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("authentication required")]
    Unauthorized,
    #[error("permission denied")]
    Forbidden,
    #[error("resource not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Upstream(String),
    #[error("{0}")]
    RateLimited(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl ApiError {
    pub(crate) fn public_message(&self) -> String {
        match self {
            Self::Internal(_) => "An internal error occurred".into(),
            _ => self.to_string(),
        }
    }
    /// The only boundary that converts local failures into client-visible data.
    /// Internal causes may contain database rows, credentials or stored payloads.
    pub(crate) fn public_parts(self) -> (StatusCode, &'static str, String) {
        match self {
            Self::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, "invalid_request_error", message)
            }
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                self.to_string(),
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "permission_error", self.to_string()),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found_error", self.to_string()),
            Self::Conflict(message) => (StatusCode::CONFLICT, "conflict_error", message),
            Self::Upstream(message) => (StatusCode::BAD_GATEWAY, "upstream_error", message),
            Self::RateLimited(message) => {
                (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", message)
            }
            Self::Internal(_) => {
                tracing::error!("internal API error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "An internal error occurred".into(),
                )
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind, message) = self.public_parts();
        (
            status,
            Json(json!({ "error": { "type": kind, "message": message } })),
        )
            .into_response()
    }
}

impl From<sea_orm::DbErr> for ApiError {
    fn from(value: sea_orm::DbErr) -> Self {
        Self::Internal(value.into())
    }
}

impl From<crate::orchestration::Error> for ApiError {
    fn from(error: crate::orchestration::Error) -> Self {
        use crate::orchestration::Error;
        match error {
            Error::Forbidden => Self::Forbidden,
            Error::Invalid(message) => Self::BadRequest(message.into()),
            Error::Admission(reason) => Self::RateLimited(reason.into()),
            Error::Configuration => {
                Self::Internal(anyhow::anyhow!("invalid orchestration configuration"))
            }
            Error::Database(error) => Self::from(error),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(protocols::router())
        .merge(catalog_api::router())
        .merge(operations_api::router(state.clone()))
        .merge(crate::access_api::router())
        .route("/api/health/live", get(live))
        .route("/api/health/ready", get(ready))
        .route("/api/v1/bootstrap", get(bootstrap))
        .route("/api/v1/setup", post(setup))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route(
            "/api/admin/v1/providers",
            get(list_providers).post(create_provider),
        )
        .route("/api/admin/v1/providers/{id}", delete(delete_provider))
        .route("/api/admin/v1/models", get(list_models).post(create_model))
        .route("/api/admin/v1/models/{id}", delete(delete_model))
        .route(
            "/api/admin/v1/api-keys",
            get(list_api_keys).post(create_api_key),
        )
        .route("/api/admin/v1/api-keys/{id}", delete(delete_api_key))
        .route(
            "/api/admin/v1/observability/summary",
            get(observation_summary),
        )
        .route(
            "/api/admin/v1/observability/requests",
            get(observation_list),
        )
        .route(
            "/api/admin/v1/observability/requests/{id}",
            get(observation_detail),
        )
        .route("/metrics", get(metrics))
        .route("/v1/models", get(gateway_models))
        .route("/v1/chat/completions", post(gateway_chat))
        .route("/v1/responses", post(gateway_responses))
        .route("/v1/messages", post(gateway_messages))
        .layer(middleware::from_fn(browser_csrf))
        .layer(middleware::from_fn(errors::native_errors))
        .layer(middleware::from_fn(capture_trusted_client_ip))
        .layer(middleware::from_fn_with_state(state.clone(), maintenance))
        .with_state(state)
}

async fn browser_csrf(request: axum::extract::Request, next: Next) -> Response {
    let unsafe_method = !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    );
    let browser_session = request
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|cookies| {
            cookies
                .split(';')
                .any(|cookie| cookie.trim().starts_with("pangolin_session="))
        });
    if request.uri().path().starts_with("/api/admin/v1/")
        && unsafe_method
        && browser_session
        && (request
            .headers()
            .get("x-pangolin-csrf")
            .and_then(|value| value.to_str().ok())
            != Some("1")
            || request
                .headers()
                .get("sec-fetch-site")
                .is_some_and(|value| value == "cross-site"))
    {
        return ApiError::Forbidden.into_response();
    }
    next.run(request).await
}
async fn maintenance(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if path.starts_with("/api/") && !path.starts_with("/api/admin/v1/instance/restore") {
        let guard = state.maintenance.clone().read_owned().await;
        hold_maintenance(next.run(request).await, guard)
    } else {
        next.run(request).await
    }
}
pub(crate) fn hold_maintenance(
    response: Response,
    guard: tokio::sync::OwnedRwLockReadGuard<()>,
) -> Response {
    use futures_util::StreamExt;
    let (parts, body) = response.into_parts();
    let output = async_stream::stream! {let _guard=guard;let mut stream=body.into_data_stream();while let Some(chunk)=stream.next().await{yield chunk;}};
    Response::from_parts(parts, Body::from_stream(output))
}

pub(crate) const TRUSTED_CLIENT_IP_HEADER: &str = "x-pangolin-trusted-client-ip";

async fn capture_trusted_client_ip(mut request: axum::extract::Request, next: Next) -> Response {
    request.headers_mut().remove(TRUSTED_CLIENT_IP_HEADER);
    request.headers_mut().remove("x-pangolin-websocket-session");
    request
        .headers_mut()
        .remove("x-pangolin-websocket-generation");
    if let Some(ConnectInfo(address)) = request.extensions().get::<ConnectInfo<SocketAddr>>()
        && let Ok(value) = HeaderValue::from_str(&address.ip().to_string())
    {
        request
            .headers_mut()
            .insert(TRUSTED_CLIENT_IP_HEADER, value);
    }
    next.run(request).await
}

pub(crate) fn trusted_client_ip(headers: &HeaderMap) -> Option<std::net::IpAddr> {
    headers
        .get(TRUSTED_CLIENT_IP_HEADER)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

async fn live() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn ready(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    db::is_initialized(&state.db).await?;
    Ok(Json(json!({
        "status": "ready",
        "components": {
            "control_database": "ready",
            "observability": if state.observations.is_available() { "ready" } else { "degraded" }
        }
    })))
}

async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let initialized = db::is_initialized(&state.db).await?;
    let user = optional_user(&state, &headers).await?;
    Ok(Json(json!({
        "initialized": initialized,
        "authenticated": user.is_some(),
        "user": user,
        "product": { "name": "Pangolin", "name_zh": "鲮鲤" },
        "public_url": state.config.public_url,
        "capture_payloads": state.config.capture_payloads,
        "observability_available": state.observations.is_available(),
    })))
}

async fn setup(
    State(state): State<AppState>,
    Json(request): Json<SetupRequest>,
) -> Result<Response, ApiError> {
    let user = db::create_initial_admin(&state.db, &request)
        .await
        .map_err(bad_request)?;
    session_response(&state, &user).await
}

async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let user = db::find_user_by_email(&state.db, &request.email)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if !crypto::verify_password(&request.password, &user.password_hash) {
        return Err(ApiError::Unauthorized);
    }
    session_response(&state, &user).await
}

pub(crate) async fn session_response(state: &AppState, user: &User) -> Result<Response, ApiError> {
    let token = db::create_session(&state.db, &user.id).await?;
    let mut cookie =
        format!("pangolin_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000");
    if state.config.session_secure {
        cookie.push_str("; Secure");
    }
    let mut response = Json(json!({ "user": user })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|error| ApiError::Internal(error.into()))?,
    );
    Ok(response)
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = session_token(&headers) {
        db::delete_session(&state.db, token).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("pangolin_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"),
    );
    Ok(response)
}

async fn list_providers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    require_user_permission(&state, &headers, "project:read").await?;
    Ok(Json(db::list_providers(&state.db).await?))
}

async fn create_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ProviderInput>,
) -> Result<impl IntoResponse, ApiError> {
    let user = require_user_permission(&state, &headers, "project:manage").await?;
    if !input.base_url.trim().is_empty() {
        validate_http_url(&input.base_url)?;
    }
    if input.name.trim().is_empty() || (input.api_key.trim().is_empty() && input.kind != "ollama") {
        return Err(ApiError::BadRequest("name and API key are required".into()));
    }
    let envelope = state
        .secrets
        .encrypt(input.api_key.trim())
        .map_err(ApiError::Internal)?;
    let provider = db::create_provider(&state.db, &input, envelope)
        .await
        .map_err(bad_request)?;
    db::record_audit_event(
        &state.db,
        &user.id,
        "create",
        "provider",
        &provider.id,
        json!({"name":provider.name,"kind":provider.kind}),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(provider)))
}

async fn delete_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = require_user_permission(&state, &headers, "project:manage").await?;
    if db::delete_provider(&state.db, &id).await? {
        db::record_audit_event(&state.db, &user.id, "delete", "provider", &id, json!({})).await?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

async fn list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    require_user_permission(&state, &headers, "project:read").await?;
    Ok(Json(db::list_models(&state.db).await?))
}

async fn create_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ModelInput>,
) -> Result<impl IntoResponse, ApiError> {
    let user = require_user_permission(&state, &headers, "project:manage").await?;
    if input.public_name.trim().is_empty() || input.upstream_name.trim().is_empty() {
        return Err(ApiError::BadRequest("model names are required".into()));
    }
    let model = db::create_model(&state.db, &input)
        .await
        .map_err(bad_request)?;
    db::record_audit_event(
        &state.db,
        &user.id,
        "create",
        "model",
        &model.id,
        json!({"public_name":model.public_name,"upstream_name":model.upstream_name}),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(model)))
}

async fn delete_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = require_user_permission(&state, &headers, "project:manage").await?;
    if db::delete_model(&state.db, &id).await? {
        db::record_audit_event(&state.db, &user.id, "delete", "model", &id, json!({})).await?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

async fn list_api_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    require_user_permission(&state, &headers, "project:read").await?;
    Ok(Json(db::list_api_keys(&state.db).await?))
}

async fn create_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ApiKeyInput>,
) -> Result<impl IntoResponse, ApiError> {
    let user = require_user_permission(&state, &headers, "api_key:manage").await?;
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let imported = input.token_mode == crate::models::ApiKeyTokenMode::ImportExisting;
    let (key, token) = db::create_api_key(&state.db, &input)
        .await
        .map_err(bad_request)?;
    db::record_audit_event(
        &state.db,
        &user.id,
        "create",
        "api_key",
        &key.id,
        json!({"name":key.name,"fingerprint":key.key_prefix,"token_mode":if imported {"import_existing"} else {"generated"}}),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(if imported {
            json!({ "key": key, "mode": "import_existing" })
        } else {
            json!({ "key": key, "mode": "generated", "token": token })
        }),
    ))
}

async fn delete_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = require_user_permission(&state, &headers, "api_key:manage").await?;
    if db::delete_api_key(&state.db, &id).await? {
        db::record_audit_event(&state.db, &user.id, "delete", "api_key", &id, json!({})).await?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

async fn observation_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    operations_api::actor(&state, &headers, None, false).await?;
    Ok(Json(
        state
            .observations
            .summary()
            .await
            .map_err(ApiError::Internal)?,
    ))
}

async fn observation_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<RequestFilter>,
) -> Result<impl IntoResponse, ApiError> {
    operations_api::actor(&state, &headers, None, false).await?;
    Ok(Json(
        state
            .observations
            .list(filter)
            .await
            .map_err(ApiError::Internal)?,
    ))
}

async fn observation_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    operations_api::actor(&state, &headers, None, false).await?;
    state
        .observations
        .get(id)
        .await
        .map_err(ApiError::Internal)?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn metrics(State(state): State<AppState>) -> Result<Response, ApiError> {
    let summary = state.observations.summary().await.unwrap_or_default();
    let body = format!(
        "# HELP pangolin_requests_total Requests observed in the last 24 hours\n# TYPE pangolin_requests_total gauge\npangolin_requests_total {}\n# HELP pangolin_errors_total Errors observed in the last 24 hours\n# TYPE pangolin_errors_total gauge\npangolin_errors_total {}\n# HELP pangolin_tokens_total Tokens observed in the last 24 hours\n# TYPE pangolin_tokens_total gauge\npangolin_tokens_total{{direction=\"input\"}} {}\npangolin_tokens_total{{direction=\"output\"}} {}\n# HELP pangolin_observability_available Whether the observation store is available\n# TYPE pangolin_observability_available gauge\npangolin_observability_available {}\n# HELP pangolin_observation_events_dropped_total Observation events dropped since process start\n# TYPE pangolin_observation_events_dropped_total counter\npangolin_observation_events_dropped_total {}\n",
        summary.requests,
        summary.errors,
        summary.input_tokens,
        summary.output_tokens,
        i32::from(state.observations.is_available()),
        state.observations.dropped_events(),
    );
    let body = format!("{body}{}", state.orchestrator.metrics());
    Ok(([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body).into_response())
}

async fn gateway_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let _maintenance = state.maintenance.clone().read_owned().await;
    let credential = gateway_key(&state, &headers).await?;
    let models = crate::orchestration::visible_models(&state.db, &credential, &headers).await?;
    gateway::discovery_response(
        &state,
        &headers,
        &credential,
        json!({"object":"list","data":models}),
        "/v1/models",
    )
    .await
}

async fn gateway_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    gateway_request(state, headers, body, "/v1/chat/completions").await
}

async fn gateway_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    gateway_request(state, headers, body, "/v1/responses").await
}

async fn gateway_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    protocols::protocol_error(
        gateway_request(state, headers, body, "/v1/messages").await,
        false,
    )
}

async fn gateway_request(
    state: AppState,
    headers: HeaderMap,
    body: axum::body::Bytes,
    endpoint: &'static str,
) -> Result<Response, ApiError> {
    gateway::execute(state, headers, body, endpoint).await
}

fn push_anthropic_message(messages: &mut Vec<Value>, role: &str, mut blocks: Vec<Value>) {
    if let Some(last) = messages.last_mut()
        && last.get("role").and_then(Value::as_str) == Some(role)
        && let Some(content) = last.get_mut("content").and_then(Value::as_array_mut)
    {
        content.append(&mut blocks);
        return;
    }
    messages.push(json!({ "role": role, "content": blocks }));
}

async fn call_anthropic_chat(
    client: &reqwest::Client,
    target: &crate::models::RouteTarget,
    payload: &Value,
    secret: &str,
    headers: &HeaderMap,
    endpoint: &str,
    mut accounting: Option<&mut crate::operations::lifecycle::Attempt>,
) -> anyhow::Result<Value> {
    crate::providers::ensure_fields(
        payload,
        &[
            "model",
            "messages",
            "max_tokens",
            "max_completion_tokens",
            "max_output_tokens",
            "temperature",
            "top_p",
            "stop",
            "tools",
            "tool_choice",
            "stream",
            "n",
        ],
    )?;
    let object = payload
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("chat request must be an object"))?;
    let source_messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("messages are required"))?;
    let mut system = Vec::<String>::new();
    let mut messages = Vec::new();
    for message in source_messages {
        crate::providers::ensure_fields(
            message,
            &["role", "content", "tool_calls", "tool_call_id"],
        )?;
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("every message requires a role"))?;
        if (role != "assistant" && message.get("tool_calls").is_some())
            || (role != "tool" && message.get("tool_call_id").is_some())
        {
            anyhow::bail!("tool fields do not match the message role");
        }
        match role {
            "system" => {
                let content = message
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Anthropic bridge supports text system messages only")
                    })?;
                system.push(content.to_owned());
            }
            "tool" => {
                let tool_use_id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("tool messages require tool_call_id"))?;
                let content = message
                    .get("content")
                    .cloned()
                    .unwrap_or(Value::String(String::new()));
                push_anthropic_message(
                    &mut messages,
                    "user",
                    vec![json!({
                        "type": "tool_result", "tool_use_id": tool_use_id, "content": content
                    })],
                );
            }
            "user" | "assistant" => {
                let mut blocks = Vec::new();
                match message.get("content") {
                    Some(Value::String(text)) if !text.is_empty() => {
                        blocks.push(json!({"type":"text","text":text}))
                    }
                    Some(Value::Array(parts)) => {
                        for part in parts {
                            crate::providers::ensure_fields(part, &["type", "text"])?;
                            if part.get("type").and_then(Value::as_str) != Some("text") {
                                anyhow::bail!(
                                    "Anthropic bridge currently supports text content parts only"
                                );
                            }
                            blocks.push(json!({"type":"text","text":part.get("text").and_then(Value::as_str).unwrap_or("")}));
                        }
                    }
                    Some(Value::Null) | None => {}
                    _ => anyhow::bail!("message content must be text or text content parts"),
                }
                if role == "assistant"
                    && let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array)
                {
                    for call in tool_calls {
                        crate::providers::ensure_fields(call, &["id", "type", "function"])?;
                        if call["type"] != "function" {
                            anyhow::bail!("only function tool calls are supported");
                        }
                        let function = call
                            .get("function")
                            .ok_or_else(|| anyhow::anyhow!("tool call requires function"))?;
                        crate::providers::ensure_fields(function, &["name", "arguments"])?;
                        let arguments = function
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let input: Value = serde_json::from_str(arguments).map_err(|_| {
                            anyhow::anyhow!("tool call arguments must be valid JSON")
                        })?;
                        blocks.push(json!({
                            "type":"tool_use",
                            "id": call.get("id").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("tool call requires id"))?,
                            "name": function.get("name").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("tool call requires function name"))?,
                            "input": input
                        }));
                    }
                }
                if blocks.is_empty() {
                    anyhow::bail!("message has no supported content");
                }
                push_anthropic_message(&mut messages, role, blocks);
            }
            _ => anyhow::bail!("unsupported OpenAI message role `{role}`"),
        }
    }
    let mut request = Map::new();
    request.insert("model".into(), Value::String(target.upstream_name.clone()));
    request.insert("messages".into(), Value::Array(messages));
    request.insert(
        "max_tokens".into(),
        Value::from(crate::orchestration::output_limit(payload)?),
    );
    if !system.is_empty() {
        request.insert("system".into(), Value::String(system.join("\n\n")));
    }
    for key in ["temperature", "top_p"] {
        if let Some(value) = object.get(key) {
            request.insert(key.into(), value.clone());
        }
    }
    if let Some(stop) = object.get("stop") {
        let sequences = match stop {
            Value::String(value) => Value::Array(vec![Value::String(value.clone())]),
            Value::Array(values) => Value::Array(values.clone()),
            _ => anyhow::bail!("stop must be a string or string array"),
        };
        request.insert("stop_sequences".into(), sequences);
    }
    if let Some(tools) = object.get("tools").and_then(Value::as_array) {
        let mut converted = Vec::with_capacity(tools.len());
        for tool in tools {
            crate::providers::ensure_fields(tool, &["type", "function"])?;
            if tool["type"] != "function" {
                anyhow::bail!("only function tools are supported");
            }
            let function = tool
                .get("function")
                .ok_or_else(|| anyhow::anyhow!("tool requires function"))?;
            crate::providers::ensure_fields(function, &["name", "description", "parameters"])?;
            converted.push(json!({
                "name": function.get("name").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("tool requires function name"))?,
                "description": function.get("description").cloned().unwrap_or(Value::Null),
                "input_schema": function.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}})),
            }));
        }
        request.insert("tools".into(), Value::Array(converted));
    }
    if let Some(choice) = object.get("tool_choice") {
        if choice.is_object() {
            crate::providers::ensure_fields(choice, &["type", "function"])?;
            crate::providers::ensure_fields(&choice["function"], &["name"])?;
            if choice["type"] != "function" || !choice["function"]["name"].is_string() {
                anyhow::bail!("unsupported tool_choice");
            }
        }
        let disable_tools = choice.as_str() == Some("none");
        let converted = match choice {
            Value::String(value) if value == "auto" => Some(json!({"type":"auto"})),
            Value::String(value) if value == "required" => Some(json!({"type":"any"})),
            Value::String(value) if value == "none" => None,
            Value::Object(value) => value
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .map(|name| json!({"type":"tool","name":name})),
            _ => anyhow::bail!("unsupported tool_choice"),
        };
        if disable_tools {
            request.remove("tools");
        } else if let Some(converted) = converted {
            request.insert("tool_choice".into(), converted);
        }
    }
    let mut checked_payload = payload.clone();
    for name in ["max_completion_tokens", "max_output_tokens", "n"] {
        checked_payload.as_object_mut().unwrap().remove(name);
    }
    checked_payload["max_tokens"] = json!(crate::orchestration::output_limit(payload)?);
    if let Some(value) =
        crate::providers::anthropic_text_request(&target.upstream_name, &checked_payload)
    {
        request = value.as_object().expect("LiteLLM emits an object").clone();
    }
    let request = client
        .post(upstream_url(&target.base_url, endpoint))
        .headers(headers.clone())
        .header("x-api-key", secret)
        .header("anthropic-version", "2023-06-01")
        .json(&request)
        .build()?;
    if let Some(accounting) = accounting.as_deref_mut() {
        accounting.contacted().await?;
    }
    let upstream = client.execute(request).await?;
    let status = upstream.status();
    let bytes = gateway::read_body(upstream).await?;
    let body: Value = serde_json::from_slice(&bytes)
        .map_err(|_| std::io::Error::other("upstream JSON response is invalid"))?;
    if status.is_success()
        && let Some(accounting) = accounting
    {
        accounting
            .usage
            .merge(crate::operations::pricing::Usage::parse_for(&body, true));
    }
    if !status.is_success() {
        return Err(AnthropicHttpError {
            status,
            body: body.to_string(),
        }
        .into());
    }
    let content = body
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if content.iter().any(|block| {
        !matches!(
            block.get("type").and_then(Value::as_str),
            Some("text" | "tool_use")
        )
    }) {
        anyhow::bail!("unsupported Anthropic response content block");
    }
    for block in &content {
        crate::providers::ensure_fields(
            block,
            if block["type"] == "text" {
                &["type", "text"]
            } else {
                &["type", "id", "name", "input"]
            },
        )?;
    }
    let text = content
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("");
    let tool_calls = content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(|block| {
            json!({
                "id": block.get("id"),
                "type": "function",
                "function": {
                    "name": block.get("name"),
                    "arguments": block.get("input").cloned().unwrap_or_else(|| json!({})).to_string()
                }
            })
        })
        .collect::<Vec<_>>();
    let mut message = json!({ "role": "assistant", "content": text });
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let canonical_usage = crate::operations::pricing::Usage::parse_for(&body, true);
    let input_tokens = canonical_usage.input;
    let output_tokens = canonical_usage.output;
    Ok(json!({
        "id": body.get("id").cloned().unwrap_or_else(|| Value::String(format!("chatcmpl_{}", Uuid::new_v4().simple()))),
        "object": "chat.completion",
        "created": db::now(),
        "model": target.public_name,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": match body.get("stop_reason").and_then(Value::as_str) {
                Some("max_tokens") => "length",
                Some("tool_use") => "tool_calls",
                _ => "stop",
            }
        }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens,
            "prompt_tokens_details":{"cached_tokens":body.pointer("/usage/cache_read_input_tokens").and_then(Value::as_i64).unwrap_or(0),"cache_creation_tokens":body.pointer("/usage/cache_creation_input_tokens").and_then(Value::as_i64).unwrap_or(0)}
        }
    }))
}

#[derive(Debug, thiserror::Error)]
#[error("Anthropic request failed with HTTP {status}")]
struct AnthropicHttpError {
    status: StatusCode,
    body: String,
}

#[cfg(test)]
fn redact_json(mut value: Value) -> Value {
    match &mut value {
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "api_key" | "authorization" | "password" | "secret" | "token"
                ) {
                    *child = Value::String("[REDACTED]".into());
                } else {
                    *child = redact_json(child.take());
                }
            }
        }
        Value::Array(array) => {
            for child in array {
                *child = redact_json(child.take());
            }
        }
        _ => {}
    }
    value
}

fn upstream_url(base: &str, endpoint: &str) -> String {
    let base = base.trim_end_matches('/');
    let suffix = endpoint.strip_prefix("/v1").unwrap_or(endpoint);
    if base.ends_with("/v1") {
        format!("{base}{suffix}")
    } else {
        format!("{base}{endpoint}")
    }
}

fn validate_http_url(value: &str) -> Result<(), ApiError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| ApiError::BadRequest("base URL is invalid".into()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::BadRequest(
            "base URL must use http or https".into(),
        ));
    }
    Ok(())
}

async fn require_user(state: &AppState, headers: &HeaderMap) -> Result<User, ApiError> {
    optional_user(state, headers)
        .await?
        .ok_or(ApiError::Unauthorized)
}

async fn require_user_permission(
    state: &AppState,
    headers: &HeaderMap,
    permission: &str,
) -> Result<User, ApiError> {
    let user = require_user(state, headers).await?;
    crate::access::authorize(
        &state.db,
        &crate::access::Principal::session(user.id.clone()),
        Some(db::DEFAULT_PROJECT_ID),
        permission,
    )
    .await
    .map_err(|error| match error {
        crate::access::AccessError::Forbidden => ApiError::Forbidden,
        crate::access::AccessError::Unauthorized => ApiError::Unauthorized,
        crate::access::AccessError::NotFound => ApiError::NotFound,
        crate::access::AccessError::Invalid(message) => ApiError::BadRequest(message),
        crate::access::AccessError::Conflict(message) => ApiError::Conflict(message),
        crate::access::AccessError::Internal(error) => ApiError::Internal(error),
    })?;
    Ok(user)
}

async fn optional_user(state: &AppState, headers: &HeaderMap) -> Result<Option<User>, ApiError> {
    let Some(token) = session_token(headers) else {
        return Ok(None);
    };
    Ok(db::find_user_by_session(&state.db, token).await?)
}

fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix("pangolin_session="))
}

async fn gateway_key(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<crate::models::ApiKeyCredential, ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
        })
        .ok_or(ApiError::Unauthorized)?;
    let credential = db::authenticate_api_key(&state.db, token, trusted_client_ip(headers))
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let scopes = serde_json::from_str::<Vec<String>>(&credential.scopes).unwrap_or_default();
    if !scopes
        .iter()
        .any(|scope| matches!(scope.as_str(), "gateway" | "gateway:use" | "*"))
    {
        return Err(ApiError::Unauthorized);
    }
    Ok(credential)
}

fn bad_request(error: impl std::fmt::Display) -> ApiError {
    ApiError::BadRequest(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request};
    use tower::ServiceExt as _;

    #[test]
    fn joins_upstream_paths_and_redacts_nested_secrets() {
        assert_eq!(
            upstream_url("https://api.openai.com/v1", "/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            upstream_url("http://localhost:4000", "/v1/responses"),
            "http://localhost:4000/v1/responses"
        );
        let redacted = redact_json(json!({"password":"secret","nested":{"api_key":"sk-test"}}));
        assert_eq!(redacted["password"], "[REDACTED]");
        assert_eq!(redacted["nested"]["api_key"], "[REDACTED]");
    }

    #[tokio::test]
    async fn local_password_login_remains_compatible() {
        let directory = tempfile::tempdir().unwrap();
        let database = db::connect("sqlite::memory:").await.unwrap();
        db::create_initial_admin(
            &database,
            &SetupRequest {
                email: "admin@example.com".into(),
                password: "a secure password".into(),
                instance_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let observations =
            ObservationStore::open(directory.path().join("login-events.duckdb"), 30).unwrap();
        let app = router(AppState {
            db: database,
            config: Arc::new(Config {
                bind: "127.0.0.1:0".parse().unwrap(),
                data_dir: directory.path().into(),
                database_url: "sqlite::memory:".into(),
                observation_path: directory.path().join("login-events.duckdb"),
                observation_retention_days: 30,
                public_url: None,
                session_secure: false,
                capture_payloads: false,
                upstream_timeout: std::time::Duration::from_secs(30),
                admin_email: None,
                admin_password: None,
                master_key: None,
            }),
            secrets,
            observations,
            client: reqwest::Client::new(),
            oidc_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            budget_locks: Arc::new(Mutex::new(HashMap::new())),
            maintenance: Arc::new(tokio::sync::RwLock::new(())),
            orchestrator: Arc::new(crate::orchestration::Runtime::default()),
        });
        let response = app
            .oneshot(
                Request::post("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"email":"ADMIN@example.com","password":"a secure password"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cookie.starts_with("pangolin_session=ps_"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
    }

    #[tokio::test]
    async fn proxies_an_openai_request_and_records_it() {
        let mock = Router::new().route(
            "/v1/chat/completions",
            post(|Json(payload): Json<Value>| async move {
                assert_eq!(payload["model"], "upstream-model");
                Json(json!({
                    "id": "chatcmpl-test",
                    "object": "chat.completion",
                    "model": "upstream-model",
                    "choices": [{"index":0,"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],
                    "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });

        let directory = tempfile::tempdir().unwrap();
        let database = db::connect("sqlite::memory:").await.unwrap();
        db::create_initial_admin(
            &database,
            &SetupRequest {
                email: "admin@example.com".into(),
                password: "a secure password".into(),
                instance_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let provider = db::create_provider(
            &database,
            &ProviderInput {
                name: "mock".into(),
                kind: "openai_compatible".into(),
                base_url: format!("http://{address}/v1"),
                api_key: "unused".into(),
            },
            secrets.encrypt("upstream-secret").unwrap(),
        )
        .await
        .unwrap();
        db::create_model(
            &database,
            &ModelInput {
                provider_id: provider.id,
                public_name: "fast".into(),
                upstream_name: "upstream-model".into(),
                capabilities: None,
                input_price_micros: Some(1_000_000),
                output_price_micros: Some(2_000_000),
                priority: None,
            },
        )
        .await
        .unwrap();
        let (_, token) = db::create_api_key(
            &database,
            &ApiKeyInput {
                name: "test".into(),
                budget_micros: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let observations =
            ObservationStore::open(directory.path().join("events.duckdb"), 30).unwrap();
        let app = router(AppState {
            db: database,
            config: Arc::new(Config {
                bind: "127.0.0.1:0".parse().unwrap(),
                data_dir: directory.path().into(),
                database_url: "sqlite::memory:".into(),
                observation_path: directory.path().join("events.duckdb"),
                observation_retention_days: 30,
                public_url: None,
                session_secure: false,
                capture_payloads: false,
                upstream_timeout: std::time::Duration::from_secs(30),
                admin_email: None,
                admin_password: None,
                master_key: None,
            }),
            secrets,
            observations: observations.clone(),
            client: reqwest::Client::new(),
            oidc_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            budget_locks: Arc::new(Mutex::new(HashMap::new())),
            maintenance: Arc::new(tokio::sync::RwLock::new(())),
            orchestrator: Arc::new(crate::orchestration::Runtime::default()),
        });
        let response = app
            .clone()
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"model":"fast","messages":[{"role":"user","content":"hi"}]})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["choices"][0]["message"]["content"], "hello");
        let models_response = app
            .oneshot(
                Request::get("/v1/models")
                    .header("x-api-key", &token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(models_response.status(), StatusCode::OK);
        assert!(models_response.headers().contains_key("x-trace-id"));
        observations.flush().await;
        assert_eq!(observations.summary().await.unwrap().requests, 2);
        server.abort();
    }

    #[tokio::test]
    async fn anthropic_bridge_maps_system_stops_and_tool_round_trip() {
        let mock = Router::new().route(
            "/v1/messages",
            post(|Json(payload): Json<Value>| async move {
                assert_eq!(payload["system"], "first\n\nsecond");
                assert_eq!(payload["stop_sequences"], json!(["done"]));
                assert_eq!(payload["messages"][1]["content"][0]["type"], "tool_use");
                assert_eq!(payload["messages"][2]["content"][0]["type"], "tool_result");
                Json(json!({
                    "id": "msg_test",
                    "content": [{"type":"tool_use","id":"call_2","name":"next","input":{"ok":true}}],
                    "stop_reason": "tool_use",
                    "usage": {"input_tokens": 20, "output_tokens": 7}
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
        let target = crate::models::RouteTarget {
            public_name: "assistant".into(),
            upstream_name: "claude-test".into(),
            provider_name: "Anthropic".into(),
            provider_kind: "anthropic".into(),
            base_url: format!("http://{address}/v1"),
            secret_envelope: String::new(),
            input_price_micros: 0,
            output_price_micros: 0,
        };
        let response = call_anthropic_chat(
            &reqwest::Client::new(),
            &target,
            &json!({
                "messages": [
                    {"role":"system","content":"first"},
                    {"role":"system","content":"second"},
                    {"role":"user","content":"run tool"},
                    {"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"q\":\"x\"}"}}]},
                    {"role":"tool","tool_call_id":"call_1","content":"result"}
                ],
                "stop": ["done"],
                "tools": [{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]
            }),
            "test-key",
            &HeaderMap::new(),
            "/v1/messages",
            None,
        )
        .await
        .unwrap();
        assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            response["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "next"
        );
        server.abort();
    }
}
