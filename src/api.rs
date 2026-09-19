use std::{collections::HashMap, sync::Arc, time::Instant};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use futures_util::StreamExt as _;
use sea_orm::DatabaseConnection;
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, OwnedMutexGuard};
use uuid::Uuid;

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
    pub budget_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
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
    #[error("resource not found")]
    NotFound,
    #[error("{0}")]
    Upstream(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind, message) = match self {
            Self::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, "invalid_request_error", message)
            }
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                self.to_string(),
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found_error", self.to_string()),
            Self::Upstream(message) => (StatusCode::BAD_GATEWAY, "upstream_error", message),
            Self::Internal(error) => {
                tracing::error!(%error, "internal API error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "An internal error occurred".into(),
                )
            }
        };
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

pub fn router(state: AppState) -> Router {
    Router::new()
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
        .with_state(state)
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

async fn session_response(state: &AppState, user: &User) -> Result<Response, ApiError> {
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
    require_user(&state, &headers).await?;
    Ok(Json(db::list_providers(&state.db).await?))
}

async fn create_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ProviderInput>,
) -> Result<impl IntoResponse, ApiError> {
    let user = require_user(&state, &headers).await?;
    validate_http_url(&input.base_url)?;
    if input.name.trim().is_empty() || input.api_key.trim().is_empty() {
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
    let user = require_user(&state, &headers).await?;
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
    require_user(&state, &headers).await?;
    Ok(Json(db::list_models(&state.db).await?))
}

async fn create_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ModelInput>,
) -> Result<impl IntoResponse, ApiError> {
    let user = require_user(&state, &headers).await?;
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
    let user = require_user(&state, &headers).await?;
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
    require_user(&state, &headers).await?;
    Ok(Json(db::list_api_keys(&state.db).await?))
}

async fn create_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ApiKeyInput>,
) -> Result<impl IntoResponse, ApiError> {
    let user = require_user(&state, &headers).await?;
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let (key, token) = db::create_api_key(&state.db, &input)
        .await
        .map_err(bad_request)?;
    db::record_audit_event(
        &state.db,
        &user.id,
        "create",
        "api_key",
        &key.id,
        json!({"name":key.name,"prefix":key.key_prefix}),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "key": key, "token": token })),
    ))
}

async fn delete_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = require_user(&state, &headers).await?;
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
    require_user(&state, &headers).await?;
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
    require_user(&state, &headers).await?;
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
    require_user(&state, &headers).await?;
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
    Ok(([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body).into_response())
}

async fn gateway_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    gateway_key(&state, &headers).await?;
    let mut seen = std::collections::BTreeSet::new();
    let models = db::list_models(&state.db).await?.into_iter().filter(|model| model.enabled && seen.insert(model.public_name.clone())).map(|model| json!({
        "id": model.public_name, "object": "model", "created": model.created_at, "owned_by": "pangolin"
    })).collect::<Vec<_>>();
    Ok(Json(json!({ "object": "list", "data": models })))
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
    gateway_request(state, headers, body, "/v1/messages").await
}

async fn gateway_request(
    state: AppState,
    headers: HeaderMap,
    body: axum::body::Bytes,
    endpoint: &'static str,
) -> Result<Response, ApiError> {
    let started = Instant::now();
    let started_at = db::now();
    let request_id = format!("req_{}", Uuid::new_v4().simple());
    let trace_id = Uuid::new_v4().simple().to_string();
    let initial_credential = gateway_key(&state, &headers).await?;
    let mut payload: Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::BadRequest("request body must be valid JSON".into()))?;
    let requested_model = payload
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::BadRequest("model is required".into()))?
        .to_owned();
    let stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if stream && initial_credential.budget_micros.is_some() {
        return Err(ApiError::BadRequest(
            "streaming is disabled for budget-limited API keys because provider-neutral usage settlement is not reliable".into(),
        ));
    }
    let _budget_guard = if initial_credential.budget_micros.is_some() {
        Some(state.lock_budget_key(&initial_credential.id).await)
    } else {
        None
    };
    let credential = if _budget_guard.is_some() {
        gateway_key(&state, &headers).await?
    } else {
        initial_credential
    };
    let targets = db::resolve_targets(&state.db, &requested_model, endpoint).await?;
    if targets.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "no enabled route for model `{requested_model}`"
        )));
    }

    let captured_request = state
        .config
        .capture_payloads
        .then(|| redact_json(payload.clone()).to_string());
    let mut last_error = None;
    let mut last_provider = None;
    let mut last_resolved_model = None;
    for target in targets {
        last_provider = Some(target.provider_name.clone());
        last_resolved_model = Some(target.upstream_name.clone());
        payload["model"] = Value::String(target.upstream_name.clone());
        let secret = state
            .secrets
            .decrypt(&target.secret_envelope)
            .map_err(ApiError::Internal)?;

        if endpoint == "/v1/chat/completions" && target.provider_kind == "anthropic" && !stream {
            match call_anthropic_chat(&state.client, &target, &payload, &secret).await {
                Ok(value) => {
                    let response_bytes = serde_json::to_vec(&value)
                        .map_err(|error| ApiError::Internal(error.into()))?;
                    let event = build_event(
                        &request_id,
                        &trace_id,
                        started_at,
                        started.elapsed().as_millis() as i64,
                        endpoint,
                        &credential.id,
                        &target.provider_name,
                        &requested_model,
                        &target.upstream_name,
                        200,
                        None,
                        &payload,
                        Some(&value),
                        captured_request.clone(),
                        state.config.capture_payloads,
                        target.input_price_micros,
                        target.output_price_micros,
                    );
                    db::add_api_key_spend(&state.db, &credential.id, event.cost_micros).await?;
                    state.observations.record(event);
                    return Ok((
                        StatusCode::OK,
                        [
                            ("content-type", "application/json"),
                            ("x-request-id", request_id.as_str()),
                        ],
                        response_bytes,
                    )
                        .into_response());
                }
                Err(error) => {
                    last_error = Some(error.to_string());
                    continue;
                }
            }
        } else if endpoint == "/v1/chat/completions" && target.provider_kind == "anthropic" {
            return Err(ApiError::BadRequest(
                "streaming OpenAI-to-Anthropic translation is not available in v0.1; use /v1/messages or disable stream".into(),
            ));
        }

        let url = upstream_url(&target.base_url, endpoint);
        let mut request = state
            .client
            .post(url)
            .header(header::CONTENT_TYPE, "application/json");
        request = if target.provider_kind == "anthropic" {
            request.header("x-api-key", secret.as_str()).header(
                "anthropic-version",
                headers
                    .get("anthropic-version")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("2023-06-01"),
            )
        } else {
            request.bearer_auth(secret.as_str())
        };
        let upstream = match request.body(body_from_json(&payload)?).send().await {
            Ok(response) => response,
            Err(error) => {
                last_error = Some(error.to_string());
                continue;
            }
        };
        let status = upstream.status();
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            last_error = Some(format!("{} returned {status}", target.provider_name));
            continue;
        }
        let content_type = upstream
            .headers()
            .get(header::CONTENT_TYPE)
            .cloned()
            .unwrap_or_else(|| HeaderValue::from_static("application/json"));
        if stream {
            let status_code = status.as_u16() as i32;
            let mut upstream_stream = upstream.bytes_stream();
            let mut guard = StreamEventGuard::new(
                state.observations.clone(),
                RequestEvent {
                    request_id: request_id.clone(),
                    trace_id,
                    started_at,
                    finished_at: 0,
                    endpoint: endpoint.into(),
                    api_key_id: Some(credential.id),
                    provider: Some(target.provider_name),
                    requested_model: Some(requested_model),
                    resolved_model: Some(target.upstream_name),
                    status_code,
                    error_kind: None,
                    latency_ms: 0,
                    ttft_ms: None,
                    input_tokens: 0,
                    output_tokens: 0,
                    cached_tokens: 0,
                    cost_micros: 0,
                    payload_captured: state.config.capture_payloads,
                    request_json: captured_request,
                    response_json: None,
                },
                started,
            );
            let stream = async_stream::stream! {
                while let Some(chunk) = upstream_stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            guard.mark_first_byte();
                            yield Ok::<_, std::io::Error>(bytes);
                        }
                        Err(error) => {
                            guard.fail("stream_error");
                            yield Err(std::io::Error::other(error));
                            return;
                        }
                    }
                }
                guard.complete();
            };
            let mut response = Response::new(Body::from_stream(stream));
            *response.status_mut() = status;
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, content_type);
            response.headers_mut().insert(
                "x-request-id",
                HeaderValue::from_str(&request_id).expect("request id is an HTTP header value"),
            );
            return Ok(response);
        }
        let response_bytes = upstream
            .bytes()
            .await
            .map_err(|error| ApiError::Upstream(error.to_string()))?;
        let response_json = serde_json::from_slice::<Value>(&response_bytes).ok();
        let event = build_event(
            &request_id,
            &trace_id,
            started_at,
            started.elapsed().as_millis() as i64,
            endpoint,
            &credential.id,
            &target.provider_name,
            &requested_model,
            &target.upstream_name,
            status.as_u16() as i32,
            (status.as_u16() >= 400).then(|| "upstream_http".into()),
            &payload,
            response_json.as_ref(),
            captured_request.clone(),
            state.config.capture_payloads,
            target.input_price_micros,
            target.output_price_micros,
        );
        db::add_api_key_spend(&state.db, &credential.id, event.cost_micros).await?;
        state.observations.record(event);
        let mut response = Response::new(Body::from(response_bytes));
        *response.status_mut() = status;
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
        response.headers_mut().insert(
            "x-request-id",
            HeaderValue::from_str(&request_id).expect("request id is an HTTP header value"),
        );
        return Ok(response);
    }
    state.observations.record(RequestEvent {
        request_id,
        trace_id,
        started_at,
        finished_at: db::now(),
        endpoint: endpoint.into(),
        api_key_id: Some(credential.id),
        provider: last_provider,
        requested_model: Some(requested_model),
        resolved_model: last_resolved_model,
        status_code: StatusCode::BAD_GATEWAY.as_u16() as i32,
        error_kind: Some("upstream_unavailable".into()),
        latency_ms: started.elapsed().as_millis() as i64,
        ttft_ms: None,
        input_tokens: 0,
        output_tokens: 0,
        cached_tokens: 0,
        cost_micros: 0,
        payload_captured: state.config.capture_payloads,
        request_json: captured_request,
        response_json: None,
    });
    Err(ApiError::Upstream(
        last_error.unwrap_or_else(|| "all upstream targets failed".into()),
    ))
}

struct StreamEventGuard {
    store: ObservationStore,
    event: Option<RequestEvent>,
    started: Instant,
}

impl StreamEventGuard {
    fn new(store: ObservationStore, event: RequestEvent, started: Instant) -> Self {
        Self {
            store,
            event: Some(event),
            started,
        }
    }

    fn mark_first_byte(&mut self) {
        if let Some(event) = &mut self.event
            && event.ttft_ms.is_none()
        {
            event.ttft_ms = Some(self.started.elapsed().as_millis() as i64);
        }
    }

    fn complete(&mut self) {
        if let Some(event) = &mut self.event {
            if event.status_code >= 400 {
                event.error_kind = Some("upstream_http".into());
            } else {
                event.error_kind = Some("usage_unavailable".into());
            }
        }
        self.publish();
    }

    fn fail(&mut self, kind: &str) {
        if let Some(event) = &mut self.event {
            event.status_code = StatusCode::BAD_GATEWAY.as_u16() as i32;
            event.error_kind = Some(kind.into());
        }
        self.publish();
    }

    fn publish(&mut self) {
        if let Some(mut event) = self.event.take() {
            event.finished_at = db::now();
            event.latency_ms = self.started.elapsed().as_millis() as i64;
            self.store.record(event);
        }
    }
}

impl Drop for StreamEventGuard {
    fn drop(&mut self) {
        if let Some(event) = &mut self.event {
            event.status_code = 499;
            event.error_kind = Some("client_cancelled".into());
        }
        self.publish();
    }
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
) -> anyhow::Result<Value> {
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
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("every message requires a role"))?;
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
                        let function = call
                            .get("function")
                            .ok_or_else(|| anyhow::anyhow!("tool call requires function"))?;
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
        object
            .get("max_tokens")
            .cloned()
            .unwrap_or(Value::from(4096)),
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
            let function = tool
                .get("function")
                .ok_or_else(|| anyhow::anyhow!("tool requires function"))?;
            converted.push(json!({
                "name": function.get("name").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("tool requires function name"))?,
                "description": function.get("description").cloned().unwrap_or(Value::Null),
                "input_schema": function.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}})),
            }));
        }
        request.insert("tools".into(), Value::Array(converted));
    }
    if let Some(choice) = object.get("tool_choice") {
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
    let upstream = client
        .post(upstream_url(&target.base_url, "/v1/messages"))
        .header("x-api-key", secret)
        .header("anthropic-version", "2023-06-01")
        .json(&request)
        .send()
        .await?;
    let status = upstream.status();
    let body: Value = upstream.json().await?;
    if !status.is_success() {
        anyhow::bail!("Anthropic returned {status}: {body}");
    }
    let content = body
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
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
    let input_tokens = body
        .pointer("/usage/input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = body
        .pointer("/usage/output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
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
            "total_tokens": input_tokens + output_tokens
        }
    }))
}

#[allow(clippy::too_many_arguments)]
fn build_event(
    request_id: &str,
    trace_id: &str,
    started_at: i64,
    latency_ms: i64,
    endpoint: &str,
    api_key_id: &str,
    provider: &str,
    requested_model: &str,
    resolved_model: &str,
    status_code: i32,
    error_kind: Option<String>,
    request: &Value,
    response: Option<&Value>,
    captured_request: Option<String>,
    capture: bool,
    input_price: i64,
    output_price: i64,
) -> RequestEvent {
    let (input_tokens, output_tokens, cached_tokens) = usage(response);
    let cost_micros = input_tokens
        .saturating_mul(input_price)
        .saturating_add(output_tokens.saturating_mul(output_price))
        / 1_000_000;
    RequestEvent {
        request_id: request_id.into(),
        trace_id: trace_id.into(),
        started_at,
        finished_at: db::now(),
        endpoint: endpoint.into(),
        api_key_id: Some(api_key_id.into()),
        provider: Some(provider.into()),
        requested_model: Some(requested_model.into()),
        resolved_model: Some(resolved_model.into()),
        status_code,
        error_kind,
        latency_ms,
        ttft_ms: None,
        input_tokens,
        output_tokens,
        cached_tokens,
        cost_micros,
        payload_captured: capture,
        request_json: captured_request
            .or_else(|| capture.then(|| redact_json(request.clone()).to_string())),
        response_json: capture
            .then(|| response.map(|value| redact_json(value.clone()).to_string()))
            .flatten(),
    }
}

fn usage(response: Option<&Value>) -> (i64, i64, i64) {
    let usage = response.and_then(|value| value.get("usage"));
    let input = usage
        .and_then(|value| {
            value
                .get("prompt_tokens")
                .or_else(|| value.get("input_tokens"))
        })
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output = usage
        .and_then(|value| {
            value
                .get("completion_tokens")
                .or_else(|| value.get("output_tokens"))
        })
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cached = usage
        .and_then(|value| {
            value
                .pointer("/prompt_tokens_details/cached_tokens")
                .or_else(|| value.get("cache_read_input_tokens"))
        })
        .and_then(Value::as_i64)
        .unwrap_or(0);
    (input, output, cached)
}

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

fn body_from_json(value: &Value) -> Result<Vec<u8>, ApiError> {
    serde_json::to_vec(value).map_err(|error| ApiError::Internal(error.into()))
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
    let credential = db::authenticate_api_key(&state.db, token)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if !credential.scopes.contains("gateway") {
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
            budget_locks: Arc::new(Mutex::new(HashMap::new())),
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
        observations.flush().await;
        assert_eq!(observations.summary().await.unwrap().requests, 1);
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
            capabilities: "[\"chat\"]".into(),
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
