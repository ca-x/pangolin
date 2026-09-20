use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use serde::Deserialize;
use serde_json::json;

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
            "/api/admin/v1/projects/{project_id}/permissions",
            get(project_permissions),
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
    named_cookie(headers, "pangolin_session")
}

fn named_cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix(&format!("{name}=")))
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
    let Some(key) =
        db::authenticate_api_key(&state.db, token, api::trusted_client_ip(headers)).await?
    else {
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

pub(crate) async fn principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, ApiError> {
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

async fn project_permissions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<String>>, ApiError> {
    let actor = principal(&state, &headers).await?;
    access::authorize(&state.db, &actor, Some(&project_id), "project:read")
        .await
        .map_err(map_access)?;
    Ok(Json(
        access::effective_scopes(&state.db, &actor, Some(&project_id))
            .await
            .map_err(map_access)?
            .into_iter()
            .collect(),
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
    let user = access::accept_invitation(&state.db, actor.as_ref(), &input)
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
    let imported = input.token_mode == access::ApiKeyTokenMode::ImportExisting;
    let (key, token) = access::create_scoped_api_key(&state.db, &actor, &input)
        .await
        .map_err(map_access)?;
    Ok((
        StatusCode::CREATED,
        Json(if imported {
            json!({"key":key,"mode":"import_existing"})
        } else {
            json!({"key":key,"mode":"generated","token":token})
        }),
    ))
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
) -> Result<Response, ApiError> {
    let provider = oidc::provider_secret(&state.db, &provider_id)
        .await
        .map_err(map_access)?;
    let metadata = discovery(&state.oidc_client, &provider.issuer_url).await?;
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
    let mut response =
        Json(json!({"authorization_url":url.to_string(),"expires_in":600})).into_response();
    let mut cookie = format!(
        "pangolin_oidc_correlation={}; Path=/api/v1/auth/oidc/callback; HttpOnly; SameSite=Lax; Max-Age=600",
        pkce.browser_binding
    );
    if state.config.session_secure || redirect_uri.starts_with("https://") {
        cookie.push_str("; Secure");
    }
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|error| ApiError::Internal(error.into()))?,
    );
    Ok(response)
}

async fn oidc_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OidcCallbackQuery>,
) -> Result<Response, ApiError> {
    let browser_binding = named_cookie(&headers, "pangolin_oidc_correlation")
        .ok_or_else(|| ApiError::BadRequest("OIDC browser correlation is missing".into()))?;
    let consumed =
        oidc::consume_pkce_state(&state.db, &state.secrets, &query.state, browser_binding)
            .await
            .map_err(map_access)?;
    let provider = oidc::provider_secret(&state.db, &consumed.provider_id)
        .await
        .map_err(map_access)?;
    let metadata = discovery(&state.oidc_client, &provider.issuer_url).await?;
    let secret = state
        .secrets
        .decrypt(&provider.client_secret_envelope)
        .map_err(ApiError::Internal)?;
    let token = state
        .oidc_client
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
        .oidc_client
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
    let mut response = api::session_response(&state, &local).await?;
    let mut clear_cookie = "pangolin_oidc_correlation=; Path=/api/v1/auth/oidc/callback; HttpOnly; SameSite=Lax; Max-Age=0".to_owned();
    if state.config.session_secure || consumed.redirect_uri.starts_with("https://") {
        clear_cookie.push_str("; Secure");
    }
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&clear_cookie).map_err(|error| ApiError::Internal(error.into()))?,
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::*;
    use axum::{
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::Request,
        response::Redirect,
    };
    use sea_orm::{DbBackend, FromQueryResult, Statement};
    use tower::ServiceExt as _;

    use crate::{
        access::{InvitationInput, MembershipInput, ScopedApiKeyInput, UserInput},
        config::Config,
        crypto::SecretBox,
        models::SetupRequest,
        observability::ObservationStore,
    };

    async fn test_app() -> (
        Router,
        sea_orm::DatabaseConnection,
        tempfile::TempDir,
        Principal,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let database = db::connect("sqlite::memory:").await.unwrap();
        let owner = db::create_initial_admin(
            &database,
            &SetupRequest {
                email: "owner@example.com".into(),
                password: "a secure password".into(),
                instance_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let observations =
            ObservationStore::open(directory.path().join("access-events.duckdb"), 30).unwrap();
        let state = AppState {
            db: database,
            config: Arc::new(Config {
                bind: "127.0.0.1:0".parse().unwrap(),
                data_dir: directory.path().into(),
                database_url: "sqlite::memory:".into(),
                observation_path: directory.path().join("access-events.duckdb"),
                observation_retention_days: 30,
                public_url: Some("https://pangolin.example.test".into()),
                session_secure: false,
                capture_payloads: false,
                upstream_timeout: Duration::from_secs(30),
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
            budget_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            maintenance: Arc::new(tokio::sync::RwLock::new(())),
            orchestrator: Arc::new(crate::orchestration::Runtime::default()),
        };
        let database = state.db.clone();
        (
            crate::api::router(state),
            database,
            directory,
            Principal::session(owner.id),
        )
    }

    #[tokio::test]
    async fn imported_api_keys_authenticate_by_digest_without_returning_plaintext() {
        let (app, database, _directory, owner) = test_app().await;
        let session = db::create_session(&database, &owner.subject_id)
            .await
            .unwrap();
        let token = "sk-existing_4Fh9pQ2xR7mV6nK3cD8sJ5wL1zB0YtUa";
        let request = || {
            Request::post(format!("/api/admin/v1/projects/{}/api-keys", db::DEFAULT_PROJECT_ID))
            .header(header::COOKIE, format!("pangolin_session={session}"))
            .header("x-pangolin-csrf", "1")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"name":"imported","token_mode":"import_existing","token":token,"key_type":"service","scopes":["gateway:use"]}).to_string()))
            .unwrap()
        };
        let response = app.clone().oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = String::from_utf8(
            to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!body.contains(token));
        assert!(
            db::authenticate_api_key(&database, token, None)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            app.oneshot(request()).await.unwrap().status(),
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn browser_admin_mutations_require_csrf_header() {
        let (app, database, _directory, owner) = test_app().await;
        let session = db::create_session(&database, &owner.subject_id)
            .await
            .unwrap();
        let response = app
            .oneshot(
                Request::post("/api/admin/v1/users")
                    .header(header::COOKIE, format!("pangolin_session={session}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"email":"blocked@example.com","password":"a secure password"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[derive(Clone)]
    struct MockIdp {
        issuer: String,
        claims: Arc<tokio::sync::RwLock<serde_json::Value>>,
        redirect_token: Arc<AtomicBool>,
        redirect_userinfo: Arc<AtomicBool>,
        leak_hits: Arc<AtomicUsize>,
    }

    async fn mock_idp() -> (MockIdp, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let idp = MockIdp {
            issuer: issuer.clone(),
            claims: Arc::new(tokio::sync::RwLock::new(json!({
                "sub":"mock-subject","email":"mock@example.com","email_verified":true,"groups":[]
            }))),
            redirect_token: Arc::new(AtomicBool::new(false)),
            redirect_userinfo: Arc::new(AtomicBool::new(false)),
            leak_hits: Arc::new(AtomicUsize::new(0)),
        };
        let discovery_idp = idp.clone();
        let token_idp = idp.clone();
        let userinfo_idp = idp.clone();
        let leak_idp = idp.clone();
        let router = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(move || {
                    let idp = discovery_idp.clone();
                    async move {
                        Json(json!({
                            "issuer":idp.issuer,
                            "authorization_endpoint":format!("{}/authorize",idp.issuer),
                            "token_endpoint":format!("{}/token",idp.issuer),
                            "userinfo_endpoint":format!("{}/userinfo",idp.issuer)
                        }))
                    }
                }),
            )
            .route(
                "/token",
                post(move |body: axum::body::Bytes| {
                    let idp = token_idp.clone();
                    async move {
                        assert!(String::from_utf8_lossy(&body).contains("code_verifier="));
                        if idp.redirect_token.load(Ordering::SeqCst) {
                            Redirect::temporary(&format!("{}/leak", idp.issuer)).into_response()
                        } else {
                            Json(json!({"access_token":"mock-access"})).into_response()
                        }
                    }
                }),
            )
            .route(
                "/userinfo",
                get(move || {
                    let idp = userinfo_idp.clone();
                    async move {
                        if idp.redirect_userinfo.load(Ordering::SeqCst) {
                            Redirect::temporary(&format!("{}/leak", idp.issuer)).into_response()
                        } else {
                            Json(idp.claims.read().await.clone()).into_response()
                        }
                    }
                }),
            )
            .route(
                "/leak",
                get(move || {
                    let idp = leak_idp.clone();
                    async move {
                        idp.leak_hits.fetch_add(1, Ordering::SeqCst);
                        Json(json!({"leaked":true}))
                    }
                }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (idp, server)
    }

    async fn configured_oidc_provider(
        database: &sea_orm::DatabaseConnection,
        directory: &tempfile::TempDir,
        owner: &Principal,
        issuer: &str,
    ) -> oidc::OidcProviderView {
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        oidc::create_provider(
            database,
            &secrets,
            owner,
            &oidc::OidcProviderInput {
                name: format!("mock-{}", uuid::Uuid::new_v4()),
                issuer_url: issuer.into(),
                client_id: "mock-client".into(),
                client_secret: crate::crypto::opaque_token("mock_"),
                scopes: vec!["openid".into(), "email".into()],
                claim_mapping: json!({
                    "version":1,"jit":true,"project_id":db::DEFAULT_PROJECT_ID,
                    "default_role_id":access::SYSTEM_MEMBER_ROLE_ID,
                    "role_mappings":{"admins":db::SYSTEM_OWNER_ROLE_ID}
                }),
                enabled: true,
            },
        )
        .await
        .unwrap()
    }

    async fn oidc_start_values(app: &Router, provider_id: &str) -> (String, String) {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/api/v1/auth/oidc/{provider_id}/start"))
                    .body(Body::empty())
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
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let url = reqwest::Url::parse(body["authorization_url"].as_str().unwrap()).unwrap();
        let state = url
            .query_pairs()
            .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
            .unwrap();
        (state, cookie)
    }

    async fn oidc_callback_response(app: &Router, state: &str, cookie: &str) -> Response {
        let mut url =
            reqwest::Url::parse("http://pangolin.test/api/v1/auth/oidc/callback").unwrap();
        url.query_pairs_mut()
            .append_pair("state", state)
            .append_pair("code", "mock-code");
        let path_and_query = format!("{}?{}", url.path(), url.query().unwrap());
        app.clone()
            .oneshot(
                Request::get(path_and_query)
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

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

    #[tokio::test]
    async fn api_key_cannot_accept_existing_users_invitation_as_a_session() {
        let (app, database, _directory, owner) = test_app().await;
        let user = access::create_user(
            &database,
            &owner,
            &UserInput {
                email: "member@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        access::upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: user.id.clone(),
                role_id: access::SYSTEM_MEMBER_ROLE_ID.into(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        let (_, api_token) = access::create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "member key".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                user_id: Some(user.id.clone()),
                profile_id: None,
                key_type: "personal".into(),
                scopes: vec!["project:read".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec![],
                denied_ips: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (_, invitation_token) = access::create_invitation(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &InvitationInput {
                email: user.email,
                role_id: access::SYSTEM_MEMBER_ROLE_ID.into(),
                expires_in_seconds: Some(300),
            },
        )
        .await
        .unwrap();
        let response = app
            .oneshot(
                Request::post("/api/v1/invitations/accept")
                    .header(header::AUTHORIZATION, format!("Bearer {api_token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"token":invitation_token}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(response.headers().get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn api_key_owner_cannot_use_interactive_self_service_profile_authority() {
        let (app, database, _directory, owner) = test_app().await;
        let user = access::create_user(
            &database,
            &owner,
            &UserInput {
                email: "profile@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        access::upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: user.id.clone(),
                role_id: access::SYSTEM_MEMBER_ROLE_ID.into(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        let (_, api_token) = access::create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "profile key".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                user_id: Some(user.id.clone()),
                profile_id: None,
                key_type: "personal".into(),
                scopes: vec!["project:read".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec![],
                denied_ips: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let response = app
            .oneshot(
                Request::patch(format!("/api/admin/v1/users/{}", user.id))
                    .header(header::AUTHORIZATION, format!("Bearer {api_token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"display_name":"escaped"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn scoped_api_key_ip_policy_uses_trusted_connect_address() {
        let (app, database, _directory, owner) = test_app().await;
        let (key, api_token) = access::create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "restricted".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                user_id: None,
                profile_id: None,
                key_type: "service".into(),
                scopes: vec!["project:read".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec!["10.0.0.1".into(), "192.0.2.0/24".into()],
                denied_ips: vec!["192.0.2.10".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(key.allowed_ips_json, "[\"10.0.0.1\",\"192.0.2.0/24\"]");
        let mut request = Request::get("/api/admin/v1/projects")
            .header(header::AUTHORIZATION, format!("Bearer {api_token}"))
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "192.0.2.10:4312".parse::<SocketAddr>().unwrap(),
        ));
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let mut allowed_request = Request::get("/api/admin/v1/projects")
            .header(header::AUTHORIZATION, format!("Bearer {api_token}"))
            .body(Body::empty())
            .unwrap();
        allowed_request
            .extensions_mut()
            .insert(ConnectInfo("10.0.0.1:4312".parse::<SocketAddr>().unwrap()));
        assert_eq!(
            app.clone().oneshot(allowed_request).await.unwrap().status(),
            StatusCode::OK
        );
        let spoofed = app
            .oneshot(
                Request::get("/api/admin/v1/projects")
                    .header(header::AUTHORIZATION, format!("Bearer {api_token}"))
                    .header(api::TRUSTED_CLIENT_IP_HEADER, "10.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(spoofed.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn project_manager_cannot_delegate_owner_role_via_membership_or_invitation() {
        let (app, database, _directory, owner) = test_app().await;
        let manager = access::create_user(
            &database,
            &owner,
            &UserInput {
                email: "manager@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        let target = access::create_user(
            &database,
            &owner,
            &UserInput {
                email: "target@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        let manager_role = access::create_role(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &access::RoleInput {
                name: "project manager".into(),
                permissions: vec!["project:manage".into()],
            },
        )
        .await
        .unwrap();
        access::upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: manager.id.clone(),
                role_id: manager_role.id,
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        let session = db::create_session(&database, &manager.id).await.unwrap();
        let member_response = app
            .clone()
            .oneshot(
                Request::post(format!(
                    "/api/admin/v1/projects/{}/members",
                    db::DEFAULT_PROJECT_ID
                ))
                .header(header::COOKIE, format!("pangolin_session={session}"))
                .header("x-pangolin-csrf", "1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"user_id":target.id,"role_id":db::SYSTEM_OWNER_ROLE_ID}).to_string(),
                ))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(member_response.status(), StatusCode::FORBIDDEN);
        let invitation_response = app
            .clone()
            .oneshot(
                Request::post(format!(
                    "/api/admin/v1/projects/{}/invitations",
                    db::DEFAULT_PROJECT_ID
                ))
                .header(header::COOKIE, format!("pangolin_session={session}"))
                .header("x-pangolin-csrf", "1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"email":"new@example.com","role_id":db::SYSTEM_OWNER_ROLE_ID})
                        .to_string(),
                ))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invitation_response.status(), StatusCode::FORBIDDEN);
        let binding_response = app
            .oneshot(
                Request::post(format!(
                    "/api/admin/v1/users/{}/role-bindings",
                    target.id
                ))
                .header(header::COOKIE, format!("pangolin_session={session}"))
                .header("x-pangolin-csrf", "1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"role_id":db::SYSTEM_OWNER_ROLE_ID,"project_id":db::DEFAULT_PROJECT_ID})
                        .to_string(),
                ))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(binding_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn password_disable_and_membership_lifecycle_revoke_sessions_and_owned_keys() {
        let (app, database, _directory, owner) = test_app().await;
        let user = access::create_user(
            &database,
            &owner,
            &UserInput {
                email: "lifecycle@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        access::upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: user.id.clone(),
                role_id: access::SYSTEM_MEMBER_ROLE_ID.into(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        let (_, user_key) = access::create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "user key".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                user_id: Some(user.id.clone()),
                profile_id: None,
                key_type: "user".into(),
                scopes: vec!["gateway:use".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec![],
                denied_ips: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (_, personal_key) = access::create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "personal key".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                user_id: Some(user.id.clone()),
                profile_id: None,
                key_type: "personal".into(),
                scopes: vec!["gateway:use".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec![],
                denied_ips: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let owner_session = db::create_session(&database, &owner.subject_id)
            .await
            .unwrap();
        let old_user_session = db::create_session(&database, &user.id).await.unwrap();
        let reset = app
            .clone()
            .oneshot(
                Request::patch(format!("/api/admin/v1/users/{}", user.id))
                    .header(header::COOKIE, format!("pangolin_session={owner_session}"))
                    .header("x-pangolin-csrf", "1")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"password":"a replacement password"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reset.status(), StatusCode::OK);
        assert!(
            db::find_user_by_session(&database, &old_user_session)
                .await
                .unwrap()
                .is_none()
        );
        let session_before_disable = db::create_session(&database, &user.id).await.unwrap();
        let disabled = app
            .clone()
            .oneshot(
                Request::patch(format!("/api/admin/v1/users/{}", user.id))
                    .header(header::COOKIE, format!("pangolin_session={owner_session}"))
                    .header("x-pangolin-csrf", "1")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"enabled":false}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(disabled.status(), StatusCode::OK);
        assert!(
            db::find_user_by_session(&database, &session_before_disable)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db::authenticate_api_key(&database, &user_key, None)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db::authenticate_api_key(&database, &personal_key, None)
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            access::create_scoped_api_key(
                &database,
                &owner,
                &ScopedApiKeyInput {
                    name: "disabled owner".into(),
                    project_id: db::DEFAULT_PROJECT_ID.into(),
                    user_id: Some(user.id.clone()),
                    profile_id: None,
                    key_type: "user".into(),
                    scopes: vec!["gateway:use".into()],
                    budget_micros: None,
                    expires_at: None,
                    allowed_ips: vec![],
                    denied_ips: vec![],
                    ..Default::default()
                }
            )
            .await,
            Err(AccessError::Invalid(_))
        ));

        let member = access::create_user(
            &database,
            &owner,
            &UserInput {
                email: "membership@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        access::upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: member.id.clone(),
                role_id: access::SYSTEM_MEMBER_ROLE_ID.into(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        let (_, suspended_key) = access::create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "suspended".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                user_id: Some(member.id.clone()),
                profile_id: None,
                key_type: "user".into(),
                scopes: vec!["gateway:use".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec![],
                denied_ips: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let suspended = app.clone().oneshot(Request::post(format!("/api/admin/v1/projects/{}/members", db::DEFAULT_PROJECT_ID)).header(header::COOKIE, format!("pangolin_session={owner_session}")).header("x-pangolin-csrf", "1").header(header::CONTENT_TYPE, "application/json").body(Body::from(json!({"user_id":member.id,"role_id":access::SYSTEM_MEMBER_ROLE_ID,"status":"suspended"}).to_string())).unwrap()).await.unwrap();
        assert_eq!(suspended.status(), StatusCode::OK);
        assert!(
            db::authenticate_api_key(&database, &suspended_key, None)
                .await
                .unwrap()
                .is_none()
        );

        access::upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: member.id.clone(),
                role_id: access::SYSTEM_MEMBER_ROLE_ID.into(),
                status: "active".into(),
            },
        )
        .await
        .unwrap();
        let (removed_record, removed_key) = access::create_scoped_api_key(
            &database,
            &owner,
            &ScopedApiKeyInput {
                name: "removed".into(),
                project_id: db::DEFAULT_PROJECT_ID.into(),
                user_id: Some(member.id.clone()),
                profile_id: None,
                key_type: "personal".into(),
                scopes: vec!["gateway:use".into()],
                budget_micros: None,
                expires_at: None,
                allowed_ips: vec![],
                denied_ips: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let removed = app
            .oneshot(
                Request::delete(format!(
                    "/api/admin/v1/projects/{}/members/{}",
                    db::DEFAULT_PROJECT_ID,
                    member.id
                ))
                .header(header::COOKIE, format!("pangolin_session={owner_session}"))
                .header("x-pangolin-csrf", "1")
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);
        assert!(
            db::authenticate_api_key(&database, &removed_key, None)
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            access::update_scoped_api_key(
                &database,
                &owner,
                db::DEFAULT_PROJECT_ID,
                &removed_record.id,
                &access::ScopedApiKeyUpdate {
                    name: None,
                    enabled: Some(true),
                    expires_at: None,
                    scopes: None,
                    ..Default::default()
                }
            )
            .await,
            Err(AccessError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn owner_transfer_creates_protected_active_owner_membership() {
        let (app, database, _directory, owner) = test_app().await;
        let new_owner = access::create_user(
            &database,
            &owner,
            &UserInput {
                email: "new-owner@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
            },
        )
        .await
        .unwrap();
        let owner_session = db::create_session(&database, &owner.subject_id)
            .await
            .unwrap();
        let transfer = app
            .clone()
            .oneshot(
                Request::patch(format!("/api/admin/v1/projects/{}", db::DEFAULT_PROJECT_ID))
                    .header(header::COOKIE, format!("pangolin_session={owner_session}"))
                    .header("x-pangolin-csrf", "1")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"owner_user_id":new_owner.id}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(transfer.status(), StatusCode::OK);
        access::authorize(
            &database,
            &Principal::session(new_owner.id.clone()),
            Some(db::DEFAULT_PROJECT_ID),
            "project:manage",
        )
        .await
        .unwrap();
        let demotion = app.oneshot(Request::post(format!("/api/admin/v1/projects/{}/members", db::DEFAULT_PROJECT_ID)).header(header::COOKIE, format!("pangolin_session={owner_session}")).header("x-pangolin-csrf", "1").header(header::CONTENT_TYPE, "application/json").body(Body::from(json!({"user_id":new_owner.id,"role_id":access::SYSTEM_MEMBER_ROLE_ID,"status":"suspended"}).to_string())).unwrap()).await.unwrap();
        assert_eq!(demotion.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn oidc_http_flow_binds_state_to_browser_and_rejects_unverified_auto_link() {
        let (idp, server) = mock_idp().await;
        let (app, database, directory, owner) = test_app().await;
        let provider = configured_oidc_provider(&database, &directory, &owner, &idp.issuer).await;
        let (state, cookie) = oidc_start_values(&app, &provider.id).await;
        let wrong_browser =
            oidc_callback_response(&app, &state, "pangolin_oidc_correlation=wrong-browser").await;
        assert_eq!(wrong_browser.status(), StatusCode::BAD_REQUEST);
        let correct_browser = oidc_callback_response(&app, &state, &cookie).await;
        assert_eq!(correct_browser.status(), StatusCode::OK);

        *idp.claims.write().await = json!({
            "sub":"unverified-link","email":"owner@example.com","groups":[]
        });
        let (unverified_state, unverified_cookie) = oidc_start_values(&app, &provider.id).await;
        let unverified = oidc_callback_response(&app, &unverified_state, &unverified_cookie).await;
        assert_eq!(unverified.status(), StatusCode::FORBIDDEN);
        server.abort();
    }

    #[tokio::test]
    async fn oidc_client_rejects_token_redirect_without_leaking_credentials() {
        let (idp, server) = mock_idp().await;
        idp.redirect_token.store(true, Ordering::SeqCst);
        let (app, database, directory, owner) = test_app().await;
        let provider = configured_oidc_provider(&database, &directory, &owner, &idp.issuer).await;
        let (state, cookie) = oidc_start_values(&app, &provider.id).await;
        let response = oidc_callback_response(&app, &state, &cookie).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(idp.leak_hits.load(Ordering::SeqCst), 0);
        idp.redirect_token.store(false, Ordering::SeqCst);
        idp.redirect_userinfo.store(true, Ordering::SeqCst);
        let (userinfo_state, userinfo_cookie) = oidc_start_values(&app, &provider.id).await;
        let userinfo_response =
            oidc_callback_response(&app, &userinfo_state, &userinfo_cookie).await;
        assert_eq!(userinfo_response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(idp.leak_hits.load(Ordering::SeqCst), 0);
        server.abort();
    }

    #[tokio::test]
    async fn repeat_oidc_login_preserves_admin_suspension_while_reapplying_role_mapping() {
        let (idp, server) = mock_idp().await;
        *idp.claims.write().await = json!({
            "sub":"repeat-user","email":"repeat@example.com","email_verified":true,
            "groups":["admins"]
        });
        let (app, database, directory, owner) = test_app().await;
        let provider = configured_oidc_provider(&database, &directory, &owner, &idp.issuer).await;
        let (first_state, first_cookie) = oidc_start_values(&app, &provider.id).await;
        assert_eq!(
            oidc_callback_response(&app, &first_state, &first_cookie)
                .await
                .status(),
            StatusCode::OK
        );
        let repeat_user = db::find_user_by_email(&database, "repeat@example.com")
            .await
            .unwrap()
            .unwrap();
        access::upsert_membership(
            &database,
            &owner,
            db::DEFAULT_PROJECT_ID,
            &MembershipInput {
                user_id: repeat_user.id.clone(),
                role_id: db::SYSTEM_OWNER_ROLE_ID.into(),
                status: "suspended".into(),
            },
        )
        .await
        .unwrap();
        *idp.claims.write().await = json!({
            "sub":"repeat-user","email":"repeat@example.com","groups":[]
        });
        let (second_state, second_cookie) = oidc_start_values(&app, &provider.id).await;
        assert_eq!(
            oidc_callback_response(&app, &second_state, &second_cookie)
                .await
                .status(),
            StatusCode::OK
        );
        #[derive(FromQueryResult)]
        struct MappingResult {
            role_id: String,
            status: String,
            audits: i64,
        }
        let result = MappingResult::find_by_statement(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT membership.role_id,membership.status,(SELECT COUNT(*) FROM audit_events audit WHERE audit.action='login' AND audit.resource_type='oidc_identity' AND audit.actor_user_id=user.id) AS audits FROM users user JOIN project_memberships membership ON membership.user_id=user.id AND membership.project_id='{}' WHERE user.email='repeat@example.com'",
                db::DEFAULT_PROJECT_ID
            ),
        ))
        .one(&database)
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.role_id, access::SYSTEM_MEMBER_ROLE_ID);
        assert_eq!(result.status, "suspended");
        assert_eq!(result.audits, 2);
        assert!(matches!(
            access::authorize(
                &database,
                &Principal::session(repeat_user.id),
                Some(db::DEFAULT_PROJECT_ID),
                "project:read",
            )
            .await,
            Err(AccessError::Forbidden)
        ));
        server.abort();
    }
}
