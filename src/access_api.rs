use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    access::{self, AccessError, Principal},
    api::{self, ApiError, AppState},
    db, oidc,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/v1/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/api/admin/v1/projects/{project_id}",
            get(get_project)
                .patch(update_project)
                .delete(delete_project),
        )
        .route(
            "/api/admin/v1/projects/{project_id}/members",
            get(list_members).post(upsert_member),
        )
        .route(
            "/api/admin/v1/projects/{project_id}/members/{user_id}",
            delete(remove_member),
        )
        .route(
            "/api/admin/v1/projects/{project_id}/invitations",
            get(list_invitations).post(create_invitation),
        )
        .route(
            "/api/admin/v1/projects/{project_id}/invitations/{invitation_id}",
            delete(delete_invitation),
        )
        .route(
            "/api/admin/v1/projects/{project_id}/roles",
            get(list_roles).post(create_role),
        )
        .route(
            "/api/admin/v1/projects/{project_id}/roles/{role_id}",
            patch(update_role).delete(delete_role),
        )
        .route(
            "/api/admin/v1/projects/{project_id}/api-keys",
            get(list_api_keys).post(create_api_key),
        )
        .route(
            "/api/admin/v1/projects/{project_id}/api-keys/{key_id}",
            patch(update_api_key).delete(delete_api_key),
        )
        .route("/api/admin/v1/users", get(list_users).post(create_user))
        .route(
            "/api/admin/v1/users/{user_id}",
            patch(update_user).delete(delete_user),
        )
        .route(
            "/api/admin/v1/users/{user_id}/role-bindings",
            get(list_role_bindings).post(create_role_binding),
        )
        .route(
            "/api/admin/v1/users/{user_id}/role-bindings/{binding_id}",
            delete(delete_role_binding),
        )
        .route(
            "/api/admin/v1/oidc/providers",
            get(list_oidc_providers).post(create_oidc_provider),
        )
        .route(
            "/api/admin/v1/oidc/providers/{provider_id}",
            patch(update_oidc_provider).delete(delete_oidc_provider),
        )
        .route(
            "/api/admin/v1/oidc/providers/{provider_id}/identities",
            get(list_oidc_identities).post(create_oidc_identity),
        )
        .route(
            "/api/admin/v1/oidc/providers/{provider_id}/identities/{identity_id}",
            delete(delete_oidc_identity),
        )
        .route("/api/v1/invitations/inspect", post(inspect_invitation))
        .route("/api/v1/invitations/accept", post(accept_invitation))
        .route("/api/v1/auth/oidc/{provider_id}/start", get(oidc_start))
        .route("/api/v1/auth/oidc/callback", get(oidc_callback))
}

fn map_access(error: AccessError) -> ApiError {
    match error {
        AccessError::Unauthorized => ApiError::Unauthorized,
        AccessError::Forbidden => ApiError::Forbidden,
        AccessError::NotFound => ApiError::NotFound,
        AccessError::Invalid(message) => ApiError::BadRequest(message),
        AccessError::Conflict(message) => ApiError::Conflict(message),
        AccessError::Internal(error) => ApiError::Internal(error),
    }
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

async fn optional_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<Principal>, ApiError> {
    if let Some(token) = session_token(headers)
        && let Some(user) = db::find_user_by_session(&state.db, token).await?
    {
        return Ok(Some(Principal::session(user.id)));
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
        });
    let Some(token) = token else {
        return Ok(None);
    };
    let Some(key) = db::authenticate_api_key(&state.db, token).await? else {
        return Err(ApiError::Unauthorized);
    };
    let scopes = serde_json::from_str::<Vec<String>>(&key.scopes).unwrap_or_default();
    Ok(Some(Principal::api_key(
        key.id,
        key.project_id,
        key.user_id,
        scopes,
    )))
}

async fn principal(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    optional_principal(state, headers)
        .await?
        .ok_or(ApiError::Unauthorized)
}

async fn list_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<access::ProjectView>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::list_projects(&state.db, &actor)
            .await
            .map_err(map_access)?,
    ))
}

async fn get_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
) -> Result<Json<access::ProjectView>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::get_project(&state.db, &actor, &project_id)
            .await
            .map_err(map_access)?,
    ))
}

async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<access::ProjectInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            access::create_project(&state.db, &actor, &input)
                .await
                .map_err(map_access)?,
        ),
    ))
}

async fn update_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(input): Json<access::ProjectUpdate>,
) -> Result<Json<access::ProjectView>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::update_project(&state.db, &actor, &project_id, &input)
            .await
            .map_err(map_access)?,
    ))
}

async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let actor = principal(&state, &headers).await?;
    access::delete_project(&state.db, &actor, &project_id)
        .await
        .map_err(map_access)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<access::UserView>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::list_users(&state.db, &actor)
            .await
            .map_err(map_access)?,
    ))
}

async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<access::UserInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            access::create_user(&state.db, &actor, &input)
                .await
                .map_err(map_access)?,
        ),
    ))
}

async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(input): Json<access::UserUpdate>,
) -> Result<Json<access::UserView>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::update_user(&state.db, &actor, &user_id, &input)
            .await
            .map_err(map_access)?,
    ))
}

async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let actor = principal(&state, &headers).await?;
    access::delete_user(&state.db, &actor, &user_id)
        .await
        .map_err(map_access)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_role_bindings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<access::RoleBindingView>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::list_role_bindings(&state.db, &actor, &user_id)
            .await
            .map_err(map_access)?,
    ))
}

async fn create_role_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(input): Json<access::RoleBindingInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            access::create_role_binding(&state.db, &actor, &user_id, &input)
                .await
                .map_err(map_access)?,
        ),
    ))
}

async fn delete_role_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((user_id, binding_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let actor = principal(&state, &headers).await?;
    access::delete_role_binding(&state.db, &actor, &user_id, &binding_id)
        .await
        .map_err(map_access)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_roles(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<access::RoleView>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::list_roles(&state.db, &actor, &project_id)
            .await
            .map_err(map_access)?,
    ))
}

async fn create_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(input): Json<access::RoleInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            access::create_role(&state.db, &actor, &project_id, &input)
                .await
                .map_err(map_access)?,
        ),
    ))
}

async fn update_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, role_id)): Path<(String, String)>,
    Json(input): Json<access::RoleInput>,
) -> Result<Json<access::RoleView>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::update_role(&state.db, &actor, &project_id, &role_id, &input)
            .await
            .map_err(map_access)?,
    ))
}

async fn delete_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, role_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let actor = principal(&state, &headers).await?;
    access::delete_role(&state.db, &actor, &project_id, &role_id)
        .await
        .map_err(map_access)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_members(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<access::MembershipView>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::list_memberships(&state.db, &actor, &project_id)
            .await
            .map_err(map_access)?,
    ))
}

async fn upsert_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(input): Json<access::MembershipInput>,
) -> Result<Json<access::MembershipView>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::upsert_membership(&state.db, &actor, &project_id, &input)
            .await
            .map_err(map_access)?,
    ))
}

async fn remove_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let actor = principal(&state, &headers).await?;
    access::remove_membership(&state.db, &actor, &project_id, &user_id)
        .await
        .map_err(map_access)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_invitation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(input): Json<access::InvitationInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = principal(&state, &headers).await?;
    let (invitation, token) = access::create_invitation(&state.db, &actor, &project_id, &input)
        .await
        .map_err(map_access)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"invitation":invitation,"token":token})),
    ))
}

async fn list_invitations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<access::InvitationView>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::list_invitations(&state.db, &actor, &project_id)
            .await
            .map_err(map_access)?,
    ))
}

async fn delete_invitation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, invitation_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let actor = principal(&state, &headers).await?;
    access::delete_invitation(&state.db, &actor, &project_id, &invitation_id)
        .await
        .map_err(map_access)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct InvitationToken {
    token: String,
}

async fn inspect_invitation(
    State(state): State<AppState>,
    Json(input): Json<InvitationToken>,
) -> Result<Json<access::InvitationView>, ApiError> {
    Ok(Json(
        access::inspect_invitation(&state.db, &input.token)
            .await
            .map_err(map_access)?,
    ))
}

async fn accept_invitation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<access::AcceptInvitationInput>,
) -> Result<Response, ApiError> {
    let actor = optional_principal(&state, &headers).await?;
    let user = access::accept_invitation(
        &state.db,
        actor.as_ref().and_then(|actor| actor.user_id.as_deref()),
        &input,
    )
    .await
    .map_err(map_access)?;
    let local = db::find_user_by_email(&state.db, &user.email)
        .await?
        .ok_or(ApiError::NotFound)?;
    api::session_response(&state, &local).await
}

async fn create_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(mut input): Json<access::ScopedApiKeyInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = principal(&state, &headers).await?;
    if !input.project_id.is_empty() && input.project_id != project_id {
        return Err(ApiError::BadRequest(
            "path and body project IDs must match".into(),
        ));
    }
    input.project_id = project_id;
    let (key, token) = access::create_scoped_api_key(&state.db, &actor, &input)
        .await
        .map_err(map_access)?;
    Ok((StatusCode::CREATED, Json(json!({"key":key,"token":token}))))
}

async fn list_api_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<access::ScopedApiKeyView>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::list_scoped_api_keys(&state.db, &actor, &project_id)
            .await
            .map_err(map_access)?,
    ))
}

async fn update_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, key_id)): Path<(String, String)>,
    Json(input): Json<access::ScopedApiKeyUpdate>,
) -> Result<Json<access::ScopedApiKeyView>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        access::update_scoped_api_key(&state.db, &actor, &project_id, &key_id, &input)
            .await
            .map_err(map_access)?,
    ))
}

async fn delete_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, key_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let actor = principal(&state, &headers).await?;
    access::delete_scoped_api_key(&state.db, &actor, &project_id, &key_id)
        .await
        .map_err(map_access)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_oidc_providers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<oidc::OidcProviderView>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        oidc::list_providers(&state.db, &actor)
            .await
            .map_err(map_access)?,
    ))
}

async fn create_oidc_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<oidc::OidcProviderInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            oidc::create_provider(&state.db, &state.secrets, &actor, &input)
                .await
                .map_err(map_access)?,
        ),
    ))
}

async fn update_oidc_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
    Json(input): Json<oidc::OidcProviderUpdate>,
) -> Result<Json<oidc::OidcProviderView>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        oidc::update_provider(&state.db, &state.secrets, &actor, &provider_id, &input)
            .await
            .map_err(map_access)?,
    ))
}

async fn delete_oidc_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let actor = principal(&state, &headers).await?;
    oidc::delete_provider(&state.db, &actor, &provider_id)
        .await
        .map_err(map_access)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_oidc_identities(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
) -> Result<Json<Vec<oidc::OidcIdentityView>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok(Json(
        oidc::list_identities(&state.db, &actor, &provider_id)
            .await
            .map_err(map_access)?,
    ))
}

async fn create_oidc_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
    Json(input): Json<oidc::OidcIdentityInput>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = principal(&state, &headers).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            oidc::create_identity(&state.db, &actor, &provider_id, &input)
                .await
                .map_err(map_access)?,
        ),
    ))
}

async fn delete_oidc_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider_id, identity_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let actor = principal(&state, &headers).await?;
    oidc::delete_identity(&state.db, &actor, &provider_id, &identity_id)
        .await
        .map_err(map_access)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct OidcCallbackQuery {
    state: String,
    code: String,
}
#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

async fn discovery(client: &reqwest::Client, issuer: &str) -> Result<Discovery, ApiError> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| ApiError::Upstream(error.to_string()))?;
    if !response.status().is_success() {
        return Err(ApiError::Upstream(format!(
            "OIDC discovery returned {}",
            response.status()
        )));
    }
    let metadata: Discovery = response
        .json()
        .await
        .map_err(|error| ApiError::Upstream(error.to_string()))?;
    if metadata.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
        return Err(ApiError::Upstream(
            "OIDC discovery issuer does not match the configured issuer".into(),
        ));
    }
    for endpoint in [
        &metadata.authorization_endpoint,
        &metadata.token_endpoint,
        &metadata.userinfo_endpoint,
    ] {
        let endpoint = reqwest::Url::parse(endpoint).map_err(|_| {
            ApiError::Upstream("OIDC discovery returned an invalid endpoint".into())
        })?;
        if endpoint.scheme() != "https"
            && !matches!(endpoint.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
        {
            return Err(ApiError::Upstream(
                "OIDC endpoints must use https unless they are loopback".into(),
            ));
        }
    }
    Ok(metadata)
}

fn oidc_redirect_uri(public_url: Option<&str>) -> Result<String, ApiError> {
    let public_url = public_url.ok_or_else(|| {
        ApiError::BadRequest("PANGOLIN_PUBLIC_URL is required for OIDC login".into())
    })?;
    let parsed = reqwest::Url::parse(public_url)
        .map_err(|_| ApiError::BadRequest("PANGOLIN_PUBLIC_URL is invalid".into()))?;
    if parsed.scheme() != "https"
        && !matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
    {
        return Err(ApiError::BadRequest(
            "PANGOLIN_PUBLIC_URL must use https unless it is loopback".into(),
        ));
    }
    Ok(format!(
        "{}/api/v1/auth/oidc/callback",
        public_url.trim_end_matches('/')
    ))
}

async fn oidc_start(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let provider = oidc::provider_secret(&state.db, &provider_id)
        .await
        .map_err(map_access)?;
    let metadata = discovery(&state.client, &provider.issuer_url).await?;
    let redirect_uri = oidc_redirect_uri(state.config.public_url.as_deref())?;
    let pkce = oidc::create_pkce_state(&state.db, &state.secrets, &provider_id, &redirect_uri, 600)
        .await
        .map_err(map_access)?;
    let mut url = reqwest::Url::parse(&metadata.authorization_endpoint)
        .map_err(|_| ApiError::Upstream("OIDC authorization endpoint is invalid".into()))?;
    let scopes = serde_json::from_str::<Vec<String>>(&provider.scopes_json)
        .unwrap_or_else(|_| vec!["openid".into()]);
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &provider.client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", &scopes.join(" "))
        .append_pair("state", &pkce.state)
        .append_pair("code_challenge", &pkce.code_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(Json(
        json!({"authorization_url":url.to_string(),"expires_in":600}),
    ))
}

async fn oidc_callback(
    State(state): State<AppState>,
    Query(query): Query<OidcCallbackQuery>,
) -> Result<Response, ApiError> {
    let consumed = oidc::consume_pkce_state(&state.db, &state.secrets, &query.state)
        .await
        .map_err(map_access)?;
    let provider = oidc::provider_secret(&state.db, &consumed.provider_id)
        .await
        .map_err(map_access)?;
    let metadata = discovery(&state.client, &provider.issuer_url).await?;
    let secret = state
        .secrets
        .decrypt(&provider.client_secret_envelope)
        .map_err(ApiError::Internal)?;
    let token = state
        .client
        .post(metadata.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", query.code.as_str()),
            ("redirect_uri", consumed.redirect_uri.as_str()),
            ("client_id", provider.client_id.as_str()),
            ("client_secret", secret.as_str()),
            ("code_verifier", consumed.code_verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|error| ApiError::Upstream(error.to_string()))?;
    if !token.status().is_success() {
        return Err(ApiError::Unauthorized);
    }
    let token: TokenResponse = token
        .json()
        .await
        .map_err(|error| ApiError::Upstream(error.to_string()))?;
    let claims = state
        .client
        .get(metadata.userinfo_endpoint)
        .bearer_auth(token.access_token)
        .send()
        .await
        .map_err(|error| ApiError::Upstream(error.to_string()))?;
    if !claims.status().is_success() {
        return Err(ApiError::Unauthorized);
    }
    let claims: oidc::OidcClaims = claims
        .json()
        .await
        .map_err(|error| ApiError::Upstream(error.to_string()))?;
    let user = oidc::link_or_create_identity(&state.db, &provider.id, &claims)
        .await
        .map_err(map_access)?;
    let local = db::find_user_by_email(&state.db, &user.email)
        .await?
        .ok_or(ApiError::NotFound)?;
    api::session_response(&state, &local).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oidc_callback_uses_only_safe_configured_public_url() {
        assert!(oidc_redirect_uri(None).is_err());
        assert!(oidc_redirect_uri(Some("http://public.example.test")).is_err());
        assert_eq!(
            oidc_redirect_uri(Some("https://pangolin.example.test/base/")).unwrap(),
            "https://pangolin.example.test/base/api/v1/auth/oidc/callback"
        );
        assert_eq!(
            oidc_redirect_uri(Some("http://127.0.0.1:8080")).unwrap(),
            "http://127.0.0.1:8080/api/v1/auth/oidc/callback"
        );
    }
}
