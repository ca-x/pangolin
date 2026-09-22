use oauth2::{CsrfToken, PkceCodeChallenge};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    access::{self, AccessError, Principal, SYSTEM_MEMBER_ROLE_ID, UserView},
    crypto::{self, SecretBox},
    db,
};

fn statement(sql: impl Into<String>, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct OidcProviderView {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub button_color: Option<String>,
    pub logo_key: Option<String>,
    pub login_only: bool,
    pub issuer_url: String,
    pub client_id: String,
    #[serde(rename = "scopes")]
    pub scopes_json: String,
    #[serde(rename = "claim_mapping")]
    pub claim_mapping_json: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub secret_configured: bool,
}

#[derive(Debug, Deserialize)]
pub struct OidcProviderInput {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub button_color: Option<String>,
    #[serde(default)]
    pub logo_key: Option<String>,
    #[serde(default)]
    pub login_only: bool,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
    #[serde(default = "default_claim_mapping")]
    pub claim_mapping: Value,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct OidcProviderUpdate {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub button_color: Option<String>,
    pub logo_key: Option<String>,
    pub login_only: Option<bool>,
    pub issuer_url: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub claim_mapping: Option<Value>,
    pub enabled: Option<bool>,
}

fn default_scopes() -> Vec<String> {
    vec!["openid".into(), "profile".into(), "email".into()]
}

fn default_claim_mapping() -> Value {
    json!({"version":1,"jit":true,"email_claim":"email","name_claim":"name","groups_claim":"groups"})
}

fn enabled() -> bool {
    true
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct OidcProviderSecret {
    pub id: String,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret_envelope: String,
    pub scopes_json: String,
    pub claim_mapping_json: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct PkceState {
    pub state: String,
    pub code_challenge: String,
    pub browser_binding: String,
}

#[derive(Debug, FromQueryResult)]
struct StateRow {
    provider_id: String,
    code_verifier_envelope: String,
    redirect_uri: String,
    intent: String,
    link_user_id: Option<String>,
}

#[derive(Debug)]
pub struct ConsumedState {
    pub provider_id: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub link_user_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OidcClaims {
    pub sub: String,
    pub email: String,
    #[serde(default)]
    pub email_verified: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct OidcIdentityInput {
    pub user_id: String,
    pub subject: String,
    #[serde(default)]
    pub claims: Value,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct OidcIdentityView {
    pub id: String,
    pub provider_id: String,
    pub user_id: String,
    pub subject: String,
    pub claims_json: String,
    pub last_login_at: Option<i64>,
    pub created_at: i64,
}

async fn write_audit<C: ConnectionTrait>(
    connection: &C,
    actor: &Principal,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    details: Value,
) -> Result<(), AccessError> {
    connection.execute(statement(
        "INSERT INTO audit_events(id,actor_user_id,action,resource_type,resource_id,details,created_at) VALUES(?,?,?,?,?,?,?)",
        vec![Uuid::new_v4().to_string().into(), actor.user_id.clone().into(), action.into(), resource_type.into(), resource_id.into(), details.to_string().into(), db::now().into()],
    )).await?;
    Ok(())
}

fn validate_provider(name: &str, issuer_url: &str, client_id: &str) -> Result<(), AccessError> {
    if name.trim().is_empty() || client_id.trim().is_empty() {
        return Err(AccessError::Invalid(
            "name and client ID are required".into(),
        ));
    }
    let issuer = reqwest::Url::parse(issuer_url)
        .map_err(|_| AccessError::Invalid("issuer URL is invalid".into()))?;
    if !matches!(issuer.scheme(), "http" | "https") || issuer.host_str().is_none() {
        return Err(AccessError::Invalid(
            "issuer URL must use http or https".into(),
        ));
    }
    if issuer.scheme() != "https"
        && !matches!(issuer.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
    {
        return Err(AccessError::Invalid(
            "issuer URL must use https unless it is loopback".into(),
        ));
    }
    Ok(())
}

const MAX_DISPLAY_NAME_CHARS: usize = 80;
const MAX_LOGO_KEY_CHARS: usize = 128;

fn validate_branding(
    display_name: Option<&str>,
    button_color: Option<&str>,
    logo_key: Option<&str>,
) -> Result<(), AccessError> {
    if let Some(display_name) = display_name {
        let display_name = display_name.trim();
        if display_name.is_empty()
            || display_name.chars().count() > MAX_DISPLAY_NAME_CHARS
            || display_name.chars().any(char::is_control)
        {
            return Err(AccessError::Invalid(
                "display name must contain 1 to 80 printable characters".into(),
            ));
        }
    }
    if let Some(button_color) = button_color.filter(|value| !value.is_empty())
        && (button_color.len() != 7
            || !button_color.starts_with('#')
            || !button_color[1..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(AccessError::Invalid(
            "button color must use #RRGGBB format".into(),
        ));
    }
    if let Some(logo_key) = logo_key.filter(|value| !value.is_empty()) {
        let mut parts = logo_key.split(':');
        let valid_part = |part: &str| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        };
        if logo_key.chars().count() > MAX_LOGO_KEY_CHARS
            || !parts.next().is_some_and(valid_part)
            || !parts.next().is_some_and(valid_part)
            || parts.next().is_some()
        {
            return Err(AccessError::Invalid(
                "logo key must be a bundled catalog key".into(),
            ));
        }
    }
    Ok(())
}

fn optional_trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub async fn list_providers(
    db: &DatabaseConnection,
    actor: &Principal,
) -> Result<Vec<OidcProviderView>, AccessError> {
    access::authorize(db, actor, None, "oidc:manage").await?;
    Ok(OidcProviderView::find_by_statement(statement(
        "SELECT id,name,COALESCE(NULLIF(trim(display_name),''),name) AS display_name,button_color,logo_key,login_only,issuer_url,client_id,scopes_json,claim_mapping_json,enabled,created_at,updated_at,(client_secret_envelope<>'') AS secret_configured FROM oidc_providers ORDER BY name",
        vec![],
    )).all(db).await?)
}

/// Sign-in discovery: the providers a visitor may choose from, without
/// authenticating. Deliberately limited to id and validated display policy,
/// never the internal name, issuer, client id, scopes, secret or admin metadata.
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct PublicOidcProvider {
    pub id: String,
    pub display_name: String,
    pub button_color: Option<String>,
    pub logo_key: Option<String>,
    pub login_only: bool,
}

pub async fn list_enabled_providers(
    db: &DatabaseConnection,
) -> Result<Vec<PublicOidcProvider>, AccessError> {
    Ok(PublicOidcProvider::find_by_statement(statement(
        "SELECT id,COALESCE(NULLIF(trim(display_name),''),name) AS display_name,button_color,logo_key,login_only FROM oidc_providers WHERE enabled=1 ORDER BY name",
        vec![],
    ))
    .all(db)
    .await?)
}

#[derive(FromQueryResult)]
struct PasswordLoginPolicy {
    allowed: bool,
}

/// Local password login stays available unless at least one OIDC provider is
/// enabled and every enabled provider explicitly opts into login-only mode.
pub async fn password_login_allowed(db: &DatabaseConnection) -> Result<bool, AccessError> {
    Ok(PasswordLoginPolicy::find_by_statement(statement(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM oidc_providers WHERE enabled=1) AND NOT EXISTS(SELECT 1 FROM oidc_providers WHERE enabled=1 AND login_only=0) THEN 0 ELSE 1 END AS allowed",
        vec![],
    ))
    .one(db)
    .await?
    .is_none_or(|policy| policy.allowed))
}

pub async fn get_provider(
    db: &DatabaseConnection,
    id: &str,
) -> Result<OidcProviderView, AccessError> {
    OidcProviderView::find_by_statement(statement(
        "SELECT id,name,COALESCE(NULLIF(trim(display_name),''),name) AS display_name,button_color,logo_key,login_only,issuer_url,client_id,scopes_json,claim_mapping_json,enabled,created_at,updated_at,(client_secret_envelope<>'') AS secret_configured FROM oidc_providers WHERE id=?",
        vec![id.into()],
    )).one(db).await?.ok_or(AccessError::NotFound)
}

pub async fn provider_secret(
    db: &DatabaseConnection,
    id: &str,
) -> Result<OidcProviderSecret, AccessError> {
    OidcProviderSecret::find_by_statement(statement(
        "SELECT id,issuer_url,client_id,client_secret_envelope,scopes_json,claim_mapping_json,enabled FROM oidc_providers WHERE id=?",
        vec![id.into()],
    )).one(db).await?.filter(|provider| provider.enabled).ok_or(AccessError::NotFound)
}

pub async fn create_provider(
    db: &DatabaseConnection,
    secrets: &SecretBox,
    actor: &Principal,
    input: &OidcProviderInput,
) -> Result<OidcProviderView, AccessError> {
    access::authorize(db, actor, None, "oidc:manage").await?;
    validate_provider(&input.name, &input.issuer_url, &input.client_id)?;
    validate_branding(
        input.display_name.as_deref(),
        input.button_color.as_deref(),
        input.logo_key.as_deref(),
    )?;
    if input.client_secret.trim().is_empty() {
        return Err(AccessError::Invalid("client secret is required".into()));
    }
    let id = Uuid::new_v4().to_string();
    let timestamp = db::now();
    let display_name = optional_trimmed(input.display_name.as_deref()).unwrap_or_default();
    let button_color = optional_trimmed(input.button_color.as_deref());
    let logo_key = optional_trimmed(input.logo_key.as_deref());
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "INSERT INTO oidc_providers(id,name,display_name,button_color,logo_key,login_only,issuer_url,client_id,client_secret_envelope,scopes_json,claim_mapping_json,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        vec![id.clone().into(), input.name.trim().into(), display_name.into(), button_color.into(), logo_key.into(), input.login_only.into(), input.issuer_url.trim_end_matches('/').into(), input.client_id.trim().into(), secrets.encrypt(input.client_secret.trim()).map_err(AccessError::Internal)?.into(), serde_json::to_string(&input.scopes).map_err(|e| AccessError::Internal(e.into()))?.into(), input.claim_mapping.to_string().into(), input.enabled.into(), timestamp.into(), timestamp.into()],
    )).await.map_err(|error| AccessError::Conflict(error.to_string()))?;
    write_audit(
        &transaction,
        actor,
        "create",
        "oidc_provider",
        &id,
        json!({"name":input.name,"issuer_url":input.issuer_url,"login_only":input.login_only}),
    )
    .await?;
    transaction.commit().await?;
    get_provider(db, &id).await
}

pub async fn update_provider(
    db: &DatabaseConnection,
    secrets: &SecretBox,
    actor: &Principal,
    id: &str,
    input: &OidcProviderUpdate,
) -> Result<OidcProviderView, AccessError> {
    access::authorize(db, actor, None, "oidc:manage").await?;
    let current = get_provider(db, id).await?;
    let name = input.name.as_deref().unwrap_or(&current.name);
    let issuer = input.issuer_url.as_deref().unwrap_or(&current.issuer_url);
    let client_id = input.client_id.as_deref().unwrap_or(&current.client_id);
    validate_provider(name, issuer, client_id)?;
    let display_name = input
        .display_name
        .as_deref()
        .unwrap_or(&current.display_name);
    let button_color = input
        .button_color
        .as_deref()
        .map(|value| optional_trimmed(Some(value)))
        .unwrap_or(current.button_color.clone());
    let logo_key = input
        .logo_key
        .as_deref()
        .map(|value| optional_trimmed(Some(value)))
        .unwrap_or(current.logo_key.clone());
    validate_branding(
        Some(display_name),
        button_color.as_deref(),
        logo_key.as_deref(),
    )?;
    let envelope = input
        .client_secret
        .as_deref()
        .filter(|secret| !secret.trim().is_empty())
        .map(|secret| secrets.encrypt(secret.trim()))
        .transpose()
        .map_err(AccessError::Internal)?;
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "UPDATE oidc_providers SET name=?,display_name=?,button_color=?,logo_key=?,login_only=?,issuer_url=?,client_id=?,client_secret_envelope=COALESCE(?,client_secret_envelope),scopes_json=?,claim_mapping_json=?,enabled=?,updated_at=? WHERE id=?",
        vec![name.trim().into(), display_name.trim().into(), button_color.into(), logo_key.into(), input.login_only.unwrap_or(current.login_only).into(), issuer.trim_end_matches('/').into(), client_id.trim().into(), envelope.clone().into(), input.scopes.as_ref().map(serde_json::to_string).transpose().map_err(|e| AccessError::Internal(e.into()))?.unwrap_or(current.scopes_json).into(), input.claim_mapping.clone().map(|value| value.to_string()).unwrap_or(current.claim_mapping_json).into(), input.enabled.unwrap_or(current.enabled).into(), db::now().into(), id.into()],
    )).await.map_err(|error| AccessError::Conflict(error.to_string()))?;
    write_audit(
        &transaction,
        actor,
        "update",
        "oidc_provider",
        id,
        json!({"secret_rotated":envelope.is_some(),"login_only":input.login_only.unwrap_or(current.login_only)}),
    )
    .await?;
    transaction.commit().await?;
    get_provider(db, id).await
}

pub async fn delete_provider(
    db: &DatabaseConnection,
    actor: &Principal,
    id: &str,
) -> Result<(), AccessError> {
    access::authorize(db, actor, None, "oidc:manage").await?;
    let transaction = db.begin().await?;
    write_audit(
        &transaction,
        actor,
        "delete",
        "oidc_provider",
        id,
        json!({}),
    )
    .await?;
    let result = transaction
        .execute(statement(
            "DELETE FROM oidc_providers WHERE id=?",
            vec![id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn list_identities(
    db: &DatabaseConnection,
    actor: &Principal,
    provider_id: &str,
) -> Result<Vec<OidcIdentityView>, AccessError> {
    access::authorize(db, actor, None, "oidc:manage").await?;
    Ok(OidcIdentityView::find_by_statement(statement("SELECT id,provider_id,user_id,subject,claims_json,last_login_at,created_at FROM oidc_identities WHERE provider_id=? ORDER BY created_at,id", vec![provider_id.into()])).all(db).await?)
}

pub async fn create_identity(
    db: &DatabaseConnection,
    actor: &Principal,
    provider_id: &str,
    input: &OidcIdentityInput,
) -> Result<OidcIdentityView, AccessError> {
    access::authorize(db, actor, None, "oidc:manage").await?;
    if input.subject.trim().is_empty() {
        return Err(AccessError::Invalid("subject is required".into()));
    }
    provider_secret(db, provider_id).await?;
    access::get_user(db, &input.user_id).await?;
    let id = Uuid::new_v4().to_string();
    let transaction = db.begin().await?;
    transaction.execute(statement("INSERT INTO oidc_identities(id,provider_id,user_id,subject,claims_json,created_at) VALUES(?,?,?,?,?,?)", vec![id.clone().into(), provider_id.into(), input.user_id.clone().into(), input.subject.trim().into(), input.claims.to_string().into(), db::now().into()])).await.map_err(|error| AccessError::Conflict(error.to_string()))?;
    write_audit(
        &transaction,
        actor,
        "create",
        "oidc_identity",
        &id,
        json!({"provider_id":provider_id,"user_id":input.user_id,"subject":input.subject}),
    )
    .await?;
    transaction.commit().await?;
    OidcIdentityView::find_by_statement(statement("SELECT id,provider_id,user_id,subject,claims_json,last_login_at,created_at FROM oidc_identities WHERE id=?", vec![id.into()])).one(db).await?.ok_or(AccessError::NotFound)
}

pub async fn delete_identity(
    db: &DatabaseConnection,
    actor: &Principal,
    provider_id: &str,
    identity_id: &str,
) -> Result<(), AccessError> {
    access::authorize(db, actor, None, "oidc:manage").await?;
    let transaction = db.begin().await?;
    write_audit(
        &transaction,
        actor,
        "delete",
        "oidc_identity",
        identity_id,
        json!({"provider_id":provider_id}),
    )
    .await?;
    let result = transaction
        .execute(statement(
            "DELETE FROM oidc_identities WHERE id=? AND provider_id=?",
            vec![identity_id.into(), provider_id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AccessError::NotFound);
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn create_pkce_state(
    db: &DatabaseConnection,
    secrets: &SecretBox,
    provider_id: &str,
    redirect_uri: &str,
    ttl_seconds: i64,
) -> Result<PkceState, AccessError> {
    create_pkce_state_for(db, secrets, provider_id, redirect_uri, None, ttl_seconds).await
}

pub async fn create_link_pkce_state(
    db: &DatabaseConnection,
    secrets: &SecretBox,
    provider_id: &str,
    redirect_uri: &str,
    user_id: &str,
    ttl_seconds: i64,
) -> Result<PkceState, AccessError> {
    let user = access::get_user(db, user_id).await?;
    if !user.enabled {
        return Err(AccessError::Forbidden);
    }
    create_pkce_state_for(
        db,
        secrets,
        provider_id,
        redirect_uri,
        Some(user_id),
        ttl_seconds,
    )
    .await
}

async fn create_pkce_state_for(
    db: &DatabaseConnection,
    secrets: &SecretBox,
    provider_id: &str,
    redirect_uri: &str,
    link_user_id: Option<&str>,
    ttl_seconds: i64,
) -> Result<PkceState, AccessError> {
    if !(60..=900).contains(&ttl_seconds) {
        return Err(AccessError::Invalid(
            "OIDC state lifetime must be between 60 and 900 seconds".into(),
        ));
    }
    let redirect = reqwest::Url::parse(redirect_uri)
        .map_err(|_| AccessError::Invalid("OIDC redirect URI is invalid".into()))?;
    if !matches!(redirect.scheme(), "http" | "https") || redirect.host_str().is_none() {
        return Err(AccessError::Invalid(
            "OIDC redirect URI must use http or https".into(),
        ));
    }
    provider_secret(db, provider_id).await?;
    let state = CsrfToken::new_random().into_secret();
    let browser_binding = CsrfToken::new_random().into_secret();
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let verifier = verifier.into_secret();
    let code_challenge = challenge.as_str().to_owned();
    db.execute(statement(
        "INSERT INTO oidc_auth_states(state_hash,provider_id,code_verifier_envelope,redirect_uri,expires_at,created_at,browser_binding_hash,intent,link_user_id) VALUES(?,?,?,?,?,?,?,?,?)",
        vec![crypto::token_hash(&state).into(), provider_id.into(), secrets.encrypt(&verifier).map_err(AccessError::Internal)?.into(), redirect_uri.into(), (db::now() + ttl_seconds).into(), db::now().into(), crypto::token_hash(&browser_binding).into(), if link_user_id.is_some() { "link" } else { "login" }.into(), link_user_id.into()],
    )).await?;
    Ok(PkceState {
        state,
        code_challenge,
        browser_binding,
    })
}

pub async fn consume_pkce_state(
    db: &DatabaseConnection,
    secrets: &SecretBox,
    state: &str,
    browser_binding: &str,
) -> Result<ConsumedState, AccessError> {
    let row = StateRow::find_by_statement(statement(
        "DELETE FROM oidc_auth_states WHERE state_hash=? AND browser_binding_hash=? AND expires_at>? RETURNING provider_id,code_verifier_envelope,redirect_uri,intent,link_user_id",
        vec![crypto::token_hash(state).into(), crypto::token_hash(browser_binding).into(), db::now().into()],
    )).one(db).await?.ok_or(AccessError::Invalid("OIDC state is invalid, expired, or already used".into()))?;
    let link_user_id = match (row.intent.as_str(), row.link_user_id) {
        ("login", None) => None,
        ("link", Some(user_id)) => Some(user_id),
        _ => {
            return Err(AccessError::Invalid(
                "OIDC state is invalid, expired, or already used".into(),
            ));
        }
    };
    Ok(ConsumedState {
        provider_id: row.provider_id,
        code_verifier: secrets
            .decrypt(&row.code_verifier_envelope)
            .map_err(AccessError::Internal)?
            .to_string(),
        redirect_uri: row.redirect_uri,
        link_user_id,
    })
}

/// Bind the provider subject to the user captured in durable PKCE state. The
/// claims can describe any email, but they never choose the local user.
pub async fn link_identity_to_user(
    db: &DatabaseConnection,
    provider_id: &str,
    user_id: &str,
    claims: &OidcClaims,
) -> Result<OidcIdentityView, AccessError> {
    if claims.sub.trim().is_empty() {
        return Err(AccessError::Invalid("OIDC subject is missing".into()));
    }
    provider_secret(db, provider_id).await?;
    let claims_json =
        serde_json::to_string(claims).map_err(|error| AccessError::Internal(error.into()))?;
    let id = Uuid::new_v4().to_string();
    let timestamp = db::now();
    let transaction = db.begin().await?;
    #[derive(FromQueryResult)]
    struct EnabledUser {
        enabled: bool,
    }
    let user = EnabledUser::find_by_statement(statement(
        "SELECT enabled FROM users WHERE id=?",
        vec![user_id.into()],
    ))
    .one(&transaction)
    .await?
    .ok_or(AccessError::NotFound)?;
    if !user.enabled {
        return Err(AccessError::Forbidden);
    }
    transaction.execute(statement(
        "INSERT INTO oidc_identities(id,provider_id,user_id,subject,claims_json,created_at) VALUES(?,?,?,?,?,?)",
        vec![id.clone().into(), provider_id.into(), user_id.into(), claims.sub.trim().into(), claims_json.into(), timestamp.into()],
    )).await.map_err(|_| AccessError::Conflict("OIDC identity is already linked".into()))?;
    write_audit(
        &transaction,
        &Principal::session(user_id),
        "link",
        "oidc_identity",
        &id,
        json!({"provider_id":provider_id,"user_id":user_id,"subject":claims.sub}),
    )
    .await?;
    transaction.commit().await?;
    OidcIdentityView::find_by_statement(statement(
        "SELECT id,provider_id,user_id,subject,claims_json,last_login_at,created_at FROM oidc_identities WHERE id=?",
        vec![id.into()],
    ))
    .one(db)
    .await?
    .ok_or(AccessError::NotFound)
}

fn mapped_string<'a>(
    claims: &'a OidcClaims,
    mapping: &Value,
    mapping_key: &str,
    fallback: &'a str,
) -> Option<String> {
    let claim_name = mapping
        .get(mapping_key)
        .and_then(Value::as_str)
        .unwrap_or(fallback);
    match claim_name {
        "email" => Some(claims.email.clone()),
        "name" => claims.name.clone(),
        "sub" => Some(claims.sub.clone()),
        other => claims
            .extra
            .get(other)
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn mapped_groups(claims: &OidcClaims, mapping: &Value) -> Vec<String> {
    let claim_name = mapping
        .get("groups_claim")
        .and_then(Value::as_str)
        .unwrap_or("groups");
    if claim_name == "groups" {
        return claims.groups.clone();
    }
    claims
        .extra
        .get(claim_name)
        .and_then(Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub async fn link_or_create_identity(
    db: &DatabaseConnection,
    provider_id: &str,
    claims: &OidcClaims,
) -> Result<UserView, AccessError> {
    if claims.sub.trim().is_empty() {
        return Err(AccessError::Forbidden);
    }
    let provider = provider_secret(db, provider_id).await?;
    let mapping: Value = serde_json::from_str(&provider.claim_mapping_json)
        .map_err(|_| AccessError::Invalid("OIDC claim mapping is invalid".into()))?;
    let email = mapped_string(claims, &mapping, "email_claim", "email")
        .ok_or_else(|| AccessError::Invalid("OIDC email claim is missing".into()))?
        .trim()
        .to_ascii_lowercase();
    if !email.contains('@') {
        return Err(AccessError::Invalid("OIDC email claim is invalid".into()));
    }
    #[derive(FromQueryResult)]
    struct IdentityUser {
        user_id: String,
        enabled: bool,
    }
    let identity = IdentityUser::find_by_statement(statement(
        "SELECT identity.user_id,user.enabled FROM oidc_identities identity JOIN users user ON user.id=identity.user_id WHERE identity.provider_id=? AND identity.subject=?",
        vec![provider_id.into(), claims.sub.clone().into()],
    ))
    .one(db)
    .await?;
    if identity.as_ref().is_some_and(|identity| !identity.enabled) {
        return Err(AccessError::Forbidden);
    }
    let jit = mapping.get("jit").and_then(Value::as_bool).unwrap_or(false);
    #[derive(FromQueryResult)]
    struct ExistingUser {
        id: String,
        enabled: bool,
    }
    let existing = if identity.is_none() {
        let verified_claim = mapping
            .get("email_verified_claim")
            .and_then(Value::as_str)
            .unwrap_or("email_verified");
        let email_verified = if verified_claim == "email_verified" {
            claims.email_verified == Some(true)
        } else {
            claims.extra.get(verified_claim).and_then(Value::as_bool) == Some(true)
        };
        if !email_verified {
            return Err(AccessError::Forbidden);
        }
        let existing = ExistingUser::find_by_statement(statement(
            "SELECT id,enabled FROM users WHERE email=? COLLATE NOCASE",
            vec![email.clone().into()],
        ))
        .one(db)
        .await?;
        if existing.as_ref().is_some_and(|user| !user.enabled) {
            return Err(AccessError::Forbidden);
        }
        if existing.is_none() && !jit {
            return Err(AccessError::Forbidden);
        }
        existing
    } else {
        None
    };
    let was_linked = identity.is_some();
    let was_existing_user = existing.is_some();
    let timestamp = db::now();
    let transaction = db.begin().await?;
    let user_id = if let Some(identity) = identity {
        identity.user_id
    } else if let Some(existing) = existing {
        existing.id
    } else {
        let id = Uuid::new_v4().to_string();
        let unusable_password =
            crypto::hash_password(&crypto::opaque_token("oidc_")).map_err(AccessError::Internal)?;
        let display_name =
            mapped_string(claims, &mapping, "name_claim", "name").unwrap_or_else(|| email.clone());
        transaction.execute(statement("INSERT INTO users(id,email,password_hash,role,language,theme,created_at,display_name,enabled,updated_at) VALUES(?,?,?,'member','zh-CN','system:bronze',?,?,1,?)", vec![id.clone().into(), email.clone().into(), unusable_password.into(), timestamp.into(), display_name.into(), timestamp.into()])).await?;
        id
    };
    let claims_json =
        serde_json::to_string(claims).map_err(|error| AccessError::Internal(error.into()))?;
    if was_linked {
        transaction.execute(statement("UPDATE oidc_identities SET claims_json=?,last_login_at=? WHERE provider_id=? AND subject=?", vec![claims_json.into(), timestamp.into(), provider_id.into(), claims.sub.clone().into()])).await?;
    } else {
        transaction.execute(statement("INSERT INTO oidc_identities(id,provider_id,user_id,subject,claims_json,last_login_at,created_at) VALUES(?,?,?,?,?,?,?)", vec![Uuid::new_v4().to_string().into(), provider_id.into(), user_id.clone().into(), claims.sub.clone().into(), claims_json.into(), timestamp.into(), timestamp.into()])).await.map_err(|error| AccessError::Conflict(error.to_string()))?;
    }

    let project_id = mapping
        .get("project_id")
        .and_then(Value::as_str)
        .unwrap_or(db::DEFAULT_PROJECT_ID);
    let mut role_id = mapping
        .get("default_role_id")
        .and_then(Value::as_str)
        .unwrap_or(SYSTEM_MEMBER_ROLE_ID)
        .to_owned();
    let role_mappings = mapping.get("role_mappings").and_then(Value::as_object);
    let mut groups = mapped_groups(claims, &mapping);
    groups.sort();
    if let Some(role_mappings) = role_mappings {
        for group in groups {
            if let Some(mapped) = role_mappings.get(&group).and_then(Value::as_str) {
                role_id = mapped.to_owned();
                break;
            }
        }
    }
    #[derive(FromQueryResult)]
    struct ProjectOwner {
        owner_user_id: Option<String>,
    }
    let project_owner = ProjectOwner::find_by_statement(statement(
        "SELECT owner_user_id FROM projects WHERE id=?",
        vec![project_id.into()],
    ))
    .one(&transaction)
    .await?
    .ok_or(AccessError::NotFound)?;
    if project_owner.owner_user_id.as_deref() == Some(&user_id) {
        role_id = crate::db::SYSTEM_OWNER_ROLE_ID.into();
    }
    transaction.execute(statement(
        "INSERT INTO project_memberships(id,project_id,user_id,role_id,status,created_at,updated_at) VALUES(?,?,?,?,\'active\',?,?) ON CONFLICT(project_id,user_id) DO UPDATE SET role_id=excluded.role_id,status=CASE WHEN EXISTS (SELECT 1 FROM projects project WHERE project.id=project_memberships.project_id AND project.owner_user_id=project_memberships.user_id) THEN 'active' ELSE project_memberships.status END,updated_at=excluded.updated_at",
        vec![Uuid::new_v4().to_string().into(), project_id.into(), user_id.clone().into(), role_id.clone().into(), timestamp.into(), timestamp.into()],
    )).await.map_err(|error| AccessError::Invalid(format!("OIDC role mapping is invalid: {error}")))?;
    write_audit(
        &transaction,
        &Principal::session(user_id.clone()),
        "login",
        "oidc_identity",
        provider_id,
        json!({"project_id":project_id,"role_id":role_id,"jit":!was_linked && !was_existing_user,"repeat_login":was_linked}),
    )
    .await?;
    transaction.commit().await?;
    access::get_user(db, &user_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SetupRequest;

    #[test]
    fn branding_validation_is_bounded_and_never_accepts_remote_icons() {
        assert!(
            validate_branding(Some("Workforce"), Some("#1A2b3C"), Some("catalog:unknown")).is_ok()
        );
        assert!(validate_branding(None, None, None).is_ok());
        assert!(validate_branding(Some(""), None, None).is_err());
        assert!(validate_branding(Some(&"x".repeat(81)), None, None).is_err());
        assert!(validate_branding(Some("unsafe\nname"), None, None).is_err());
        assert!(validate_branding(None, Some("#fff"), None).is_err());
        assert!(validate_branding(None, Some("#GG0000"), None).is_err());
        assert!(validate_branding(None, None, Some("https://example.test/logo.svg")).is_err());
        assert!(validate_branding(None, None, Some("catalog key")).is_err());
    }

    #[tokio::test]
    async fn public_discovery_exposes_only_safe_login_fields() {
        let database = db::connect("sqlite::memory:").await.unwrap();
        database.execute(statement(
            "INSERT INTO oidc_providers(id,name,issuer_url,client_id,client_secret_envelope,scopes_json,claim_mapping_json,enabled,login_only,display_name,button_color,logo_key,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            vec![
                "provider-id".into(), "Internal name".into(), "https://secret-issuer.example".into(),
                "secret-client-id".into(), "secret-envelope".into(), "[\"openid\"]".into(),
                "{\"version\":1}".into(), true.into(), true.into(), "Workforce SSO".into(),
                "#1A2B3C".into(), "catalog:unknown".into(), 1_i64.into(), 1_i64.into(),
            ],
        )).await.unwrap();

        let value = serde_json::to_value(list_enabled_providers(&database).await.unwrap()).unwrap();
        assert_eq!(
            value,
            json!([{
                "id": "provider-id",
                "display_name": "Workforce SSO",
                "button_color": "#1A2B3C",
                "logo_key": "catalog:unknown",
                "login_only": true
            }])
        );
        let serialized = value.to_string();
        for private in [
            "secret-issuer",
            "secret-client-id",
            "secret-envelope",
            "openid",
            "Internal name",
        ] {
            assert!(
                !serialized.contains(private),
                "public discovery leaked {private}"
            );
        }
    }

    #[tokio::test]
    async fn provider_branding_and_login_policy_mutations_are_audited() {
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
        let actor = Principal::session(owner.id);
        let directory = tempfile::tempdir().unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let created = create_provider(
            &database,
            &secrets,
            &actor,
            &OidcProviderInput {
                name: "workforce".into(),
                display_name: Some("Workforce SSO".into()),
                button_color: Some("#0B5A46".into()),
                logo_key: Some("catalog:unknown".into()),
                login_only: true,
                issuer_url: "https://id.example.test".into(),
                client_id: "client".into(),
                client_secret: "secret".into(),
                scopes: default_scopes(),
                claim_mapping: default_claim_mapping(),
                enabled: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(created.display_name, "Workforce SSO");
        assert_eq!(created.button_color.as_deref(), Some("#0B5A46"));
        assert!(created.login_only);

        let updated = update_provider(
            &database,
            &secrets,
            &actor,
            &created.id,
            &OidcProviderUpdate {
                name: None,
                display_name: Some("Workforce identity".into()),
                button_color: Some(String::new()),
                logo_key: Some("catalog:future".into()),
                login_only: Some(false),
                issuer_url: None,
                client_id: None,
                client_secret: None,
                scopes: None,
                claim_mapping: None,
                enabled: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.display_name, "Workforce identity");
        assert_eq!(updated.button_color, None);
        assert_eq!(updated.logo_key.as_deref(), Some("catalog:future"));
        assert!(!updated.login_only);

        #[derive(FromQueryResult)]
        struct AuditCount {
            count: i64,
        }
        let audits = AuditCount::find_by_statement(statement(
            "SELECT COUNT(*) AS count FROM audit_events WHERE resource_type='oidc_provider' AND resource_id=? AND action IN ('create','update')",
            vec![created.id.into()],
        ))
        .one(&database)
        .await
        .unwrap()
        .unwrap();
        assert_eq!(audits.count, 2);
    }

    #[tokio::test]
    async fn pkce_state_is_encrypted_expiring_one_time_and_jit_maps_roles() {
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
        let actor = Principal::session(owner.id);
        let directory = tempfile::tempdir().unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let client_secret = crypto::opaque_token("test_oidc_");
        let provider = create_provider(&database, &secrets, &actor, &OidcProviderInput {
            name: "SSO".into(), display_name: None, button_color: None, logo_key: None, login_only: false, issuer_url: "https://id.example.test".into(), client_id: "client".into(), client_secret, scopes: default_scopes(), enabled: true,
            claim_mapping: json!({"version":1,"jit":true,"project_id":db::DEFAULT_PROJECT_ID,"default_role_id":SYSTEM_MEMBER_ROLE_ID,"role_mappings":{"admins":crate::db::SYSTEM_OWNER_ROLE_ID}}),
        }).await.unwrap();
        let public = serde_json::to_value(&provider).unwrap();
        assert!(public.get("scopes_json").is_none());
        assert!(public.get("claim_mapping_json").is_none());
        assert_eq!(
            public["scopes"],
            serde_json::to_string(&default_scopes()).unwrap()
        );
        let public_mapping: Value =
            serde_json::from_str(public["claim_mapping"].as_str().unwrap()).unwrap();
        assert_eq!(public_mapping["jit"], true);
        assert!(matches!(
            link_or_create_identity(
                &database,
                &provider.id,
                &OidcClaims {
                    sub: "unverified-subject".into(),
                    email: "owner@example.com".into(),
                    email_verified: None,
                    name: None,
                    groups: vec![],
                    extra: Default::default(),
                }
            )
            .await,
            Err(AccessError::Forbidden)
        ));
        let pkce = create_pkce_state(
            &database,
            &secrets,
            &provider.id,
            "https://app.example.test/callback",
            60,
        )
        .await
        .unwrap();
        assert!(!pkce.state.is_empty());
        assert!(matches!(
            consume_pkce_state(&database, &secrets, &pkce.state, "wrong-browser").await,
            Err(AccessError::Invalid(_))
        ));
        let consumed = consume_pkce_state(&database, &secrets, &pkce.state, &pkce.browser_binding)
            .await
            .unwrap();
        assert_eq!(consumed.provider_id, provider.id);
        assert_eq!(consumed.redirect_uri, "https://app.example.test/callback");
        assert!(consumed.code_verifier.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_' | '~')
        }));
        assert_eq!(pkce.code_challenge.len(), 43);
        assert!(matches!(
            consume_pkce_state(&database, &secrets, &pkce.state, &pkce.browser_binding).await,
            Err(AccessError::Invalid(_))
        ));
        let expired = create_pkce_state(
            &database,
            &secrets,
            &provider.id,
            "https://app.example.test/callback",
            60,
        )
        .await
        .unwrap();
        database
            .execute(statement(
                "UPDATE oidc_auth_states SET expires_at=?",
                vec![(db::now() - 1).into()],
            ))
            .await
            .unwrap();
        assert!(matches!(
            consume_pkce_state(
                &database,
                &secrets,
                &expired.state,
                &expired.browser_binding
            )
            .await,
            Err(AccessError::Invalid(_))
        ));

        let user = link_or_create_identity(
            &database,
            &provider.id,
            &OidcClaims {
                sub: "subject-1".into(),
                email: "jit@example.com".into(),
                email_verified: Some(true),
                name: Some("JIT User".into()),
                groups: vec!["admins".into()],
                extra: Default::default(),
            },
        )
        .await
        .unwrap();
        assert_eq!(user.email, "jit@example.com");
        access::authorize(
            &database,
            &Principal::session(user.id.clone()),
            Some(db::DEFAULT_PROJECT_ID),
            "project:manage",
        )
        .await
        .unwrap();
        let same = link_or_create_identity(
            &database,
            &provider.id,
            &OidcClaims {
                sub: "subject-1".into(),
                email: "changed@example.com".into(),
                email_verified: Some(true),
                name: None,
                groups: vec![],
                extra: Default::default(),
            },
        )
        .await
        .unwrap();
        assert_eq!(same.id, user.id);
        assert_eq!(same.email, "jit@example.com");
        assert!(matches!(
            access::authorize(
                &database,
                &Principal::session(user.id.clone()),
                Some(db::DEFAULT_PROJECT_ID),
                "project:manage"
            )
            .await,
            Err(AccessError::Forbidden)
        ));
        #[derive(FromQueryResult)]
        struct AuditCount {
            count: i64,
        }
        let audits = AuditCount::find_by_statement(statement(
            "SELECT COUNT(*) AS count FROM audit_events WHERE action='login' AND resource_type='oidc_identity' AND actor_user_id=?",
            vec![user.id.into()],
        )).one(&database).await.unwrap().unwrap();
        assert_eq!(audits.count, 2);
    }

    #[tokio::test]
    async fn link_state_captures_the_starting_user_and_is_one_time() {
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
        let actor = Principal::session(owner.id.clone());
        let directory = tempfile::tempdir().unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let provider = create_provider(
            &database,
            &secrets,
            &actor,
            &OidcProviderInput {
                name: "Link SSO".into(),
                display_name: None,
                button_color: None,
                logo_key: None,
                login_only: false,
                issuer_url: "https://id.example.test".into(),
                client_id: "client".into(),
                client_secret: crypto::opaque_token("test_oidc_"),
                scopes: default_scopes(),
                claim_mapping: default_claim_mapping(),
                enabled: true,
            },
        )
        .await
        .unwrap();

        let pkce = create_link_pkce_state(
            &database,
            &secrets,
            &provider.id,
            "https://app.example.test/callback",
            &owner.id,
            60,
        )
        .await
        .unwrap();
        let consumed = consume_pkce_state(&database, &secrets, &pkce.state, &pkce.browser_binding)
            .await
            .unwrap();
        assert_eq!(consumed.link_user_id.as_deref(), Some(owner.id.as_str()));
        assert!(matches!(
            consume_pkce_state(&database, &secrets, &pkce.state, &pkce.browser_binding).await,
            Err(AccessError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn self_service_link_uses_captured_user_and_audits_once() {
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
        let actor = Principal::session(owner.id.clone());
        let directory = tempfile::tempdir().unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let provider = create_provider(
            &database,
            &secrets,
            &actor,
            &OidcProviderInput {
                name: "Link SSO".into(),
                display_name: None,
                button_color: None,
                logo_key: None,
                login_only: false,
                issuer_url: "https://id.example.test".into(),
                client_id: "client".into(),
                client_secret: crypto::opaque_token("test_oidc_"),
                scopes: default_scopes(),
                claim_mapping: default_claim_mapping(),
                enabled: true,
            },
        )
        .await
        .unwrap();

        let linked = link_identity_to_user(
            &database,
            &provider.id,
            &owner.id,
            &OidcClaims {
                sub: "captured-subject".into(),
                email: "someone-else@example.com".into(),
                email_verified: Some(true),
                name: None,
                groups: vec![],
                extra: Default::default(),
            },
        )
        .await
        .unwrap();
        assert_eq!(linked.user_id, owner.id);
        #[derive(FromQueryResult)]
        struct Count {
            count: i64,
        }
        let audits = Count::find_by_statement(statement(
            "SELECT COUNT(*) AS count FROM audit_events WHERE action='link' AND resource_type='oidc_identity' AND actor_user_id=?",
            vec![owner.id.into()],
        ))
        .one(&database)
        .await
        .unwrap()
        .unwrap();
        assert_eq!(audits.count, 1);
        delete_identity(&database, &actor, &provider.id, &linked.id)
            .await
            .unwrap();
        let unlink_audits = Count::find_by_statement(statement(
            "SELECT COUNT(*) AS count FROM audit_events WHERE action='delete' AND resource_type='oidc_identity' AND resource_id=?",
            vec![linked.id.into()],
        ))
        .one(&database)
        .await
        .unwrap()
        .unwrap();
        assert_eq!(unlink_audits.count, 1);
    }

    #[tokio::test]
    async fn self_service_link_conflicts_leave_existing_identities_unchanged() {
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
        let actor = Principal::session(owner.id.clone());
        let other = access::create_user(
            &database,
            &actor,
            &access::UserInput {
                email: "other@example.com".into(),
                password: "another secure password".into(),
                display_name: None,
                language: None,
                enabled: true,
            },
        )
        .await
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let provider = create_provider(
            &database,
            &secrets,
            &actor,
            &OidcProviderInput {
                name: "Link SSO".into(),
                display_name: None,
                button_color: None,
                logo_key: None,
                login_only: false,
                issuer_url: "https://id.example.test".into(),
                client_id: "client".into(),
                client_secret: crypto::opaque_token("test_oidc_"),
                scopes: default_scopes(),
                claim_mapping: default_claim_mapping(),
                enabled: true,
            },
        )
        .await
        .unwrap();
        create_identity(
            &database,
            &actor,
            &provider.id,
            &OidcIdentityInput {
                user_id: other.id,
                subject: "taken-subject".into(),
                claims: json!({}),
            },
        )
        .await
        .unwrap();

        let conflicting_subject = link_identity_to_user(
            &database,
            &provider.id,
            &owner.id,
            &OidcClaims {
                sub: "taken-subject".into(),
                email: "owner@example.com".into(),
                email_verified: Some(true),
                name: None,
                groups: vec![],
                extra: Default::default(),
            },
        )
        .await;
        assert!(matches!(conflicting_subject, Err(AccessError::Conflict(_))));

        create_identity(
            &database,
            &actor,
            &provider.id,
            &OidcIdentityInput {
                user_id: owner.id.clone(),
                subject: "owner-original".into(),
                claims: json!({}),
            },
        )
        .await
        .unwrap();
        let same_user_provider = link_identity_to_user(
            &database,
            &provider.id,
            &owner.id,
            &OidcClaims {
                sub: "owner-replacement".into(),
                email: "owner@example.com".into(),
                email_verified: Some(true),
                name: None,
                groups: vec![],
                extra: Default::default(),
            },
        )
        .await;
        assert!(matches!(same_user_provider, Err(AccessError::Conflict(_))));

        #[derive(FromQueryResult)]
        struct Count {
            count: i64,
        }
        let identities = Count::find_by_statement(statement(
            "SELECT COUNT(*) AS count FROM oidc_identities WHERE provider_id=?",
            vec![provider.id.into()],
        ))
        .one(&database)
        .await
        .unwrap()
        .unwrap();
        let link_audits = Count::find_by_statement(statement(
            "SELECT COUNT(*) AS count FROM audit_events WHERE action='link' AND resource_type='oidc_identity'",
            vec![],
        ))
        .one(&database)
        .await
        .unwrap()
        .unwrap();
        assert_eq!(identities.count, 2);
        assert_eq!(link_audits.count, 0);
    }

    #[tokio::test]
    async fn self_service_link_rolls_back_when_audit_write_fails() {
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
        let actor = Principal::session(owner.id.clone());
        let directory = tempfile::tempdir().unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let provider = create_provider(
            &database,
            &secrets,
            &actor,
            &OidcProviderInput {
                name: "Link SSO".into(),
                display_name: None,
                button_color: None,
                logo_key: None,
                login_only: false,
                issuer_url: "https://id.example.test".into(),
                client_id: "client".into(),
                client_secret: crypto::opaque_token("test_oidc_"),
                scopes: default_scopes(),
                claim_mapping: default_claim_mapping(),
                enabled: true,
            },
        )
        .await
        .unwrap();
        database.execute_unprepared(
            "CREATE TRIGGER fail_link_audit BEFORE INSERT ON audit_events WHEN NEW.action='link' BEGIN SELECT RAISE(ABORT,'forced link audit failure'); END;",
        ).await.unwrap();

        let result = link_identity_to_user(
            &database,
            &provider.id,
            &owner.id,
            &OidcClaims {
                sub: "must-roll-back".into(),
                email: owner.email,
                email_verified: Some(true),
                name: None,
                groups: vec![],
                extra: Default::default(),
            },
        )
        .await;
        assert!(matches!(result, Err(AccessError::Internal(_))));

        #[derive(FromQueryResult)]
        struct Count {
            count: i64,
        }
        let identities = Count::find_by_statement(statement(
            "SELECT COUNT(*) AS count FROM oidc_identities WHERE provider_id=? AND subject='must-roll-back'",
            vec![provider.id.into()],
        ))
        .one(&database)
        .await
        .unwrap()
        .unwrap();
        let audits = Count::find_by_statement(statement(
            "SELECT COUNT(*) AS count FROM audit_events WHERE action='link'",
            vec![],
        ))
        .one(&database)
        .await
        .unwrap()
        .unwrap();
        assert_eq!(identities.count, 0);
        assert_eq!(audits.count, 0);
    }
}
