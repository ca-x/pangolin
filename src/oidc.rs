use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore as _;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
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
    pub issuer_url: String,
    pub client_id: String,
    pub scopes_json: String,
    pub claim_mapping_json: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub secret_configured: bool,
}

#[derive(Debug, Deserialize)]
pub struct OidcProviderInput {
    pub name: String,
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
}

#[derive(Debug, FromQueryResult)]
struct StateRow {
    provider_id: String,
    code_verifier_envelope: String,
    redirect_uri: String,
}

#[derive(Debug)]
pub struct ConsumedState {
    pub provider_id: String,
    pub code_verifier: String,
    pub redirect_uri: String,
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

pub async fn list_providers(
    db: &DatabaseConnection,
    actor: &Principal,
) -> Result<Vec<OidcProviderView>, AccessError> {
    access::authorize(db, actor, None, "oidc:manage").await?;
    Ok(OidcProviderView::find_by_statement(statement(
        "SELECT id,name,issuer_url,client_id,scopes_json,claim_mapping_json,enabled,created_at,updated_at,(client_secret_envelope<>'') AS secret_configured FROM oidc_providers ORDER BY name",
        vec![],
    )).all(db).await?)
}

pub async fn get_provider(
    db: &DatabaseConnection,
    id: &str,
) -> Result<OidcProviderView, AccessError> {
    OidcProviderView::find_by_statement(statement(
        "SELECT id,name,issuer_url,client_id,scopes_json,claim_mapping_json,enabled,created_at,updated_at,(client_secret_envelope<>'') AS secret_configured FROM oidc_providers WHERE id=?",
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
    if input.client_secret.trim().is_empty() {
        return Err(AccessError::Invalid("client secret is required".into()));
    }
    let id = Uuid::new_v4().to_string();
    let timestamp = db::now();
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "INSERT INTO oidc_providers(id,name,issuer_url,client_id,client_secret_envelope,scopes_json,claim_mapping_json,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?)",
        vec![id.clone().into(), input.name.trim().into(), input.issuer_url.trim_end_matches('/').into(), input.client_id.trim().into(), secrets.encrypt(input.client_secret.trim()).map_err(AccessError::Internal)?.into(), serde_json::to_string(&input.scopes).map_err(|e| AccessError::Internal(e.into()))?.into(), input.claim_mapping.to_string().into(), input.enabled.into(), timestamp.into(), timestamp.into()],
    )).await.map_err(|error| AccessError::Conflict(error.to_string()))?;
    write_audit(
        &transaction,
        actor,
        "create",
        "oidc_provider",
        &id,
        json!({"name":input.name,"issuer_url":input.issuer_url}),
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
    let envelope = input
        .client_secret
        .as_deref()
        .filter(|secret| !secret.trim().is_empty())
        .map(|secret| secrets.encrypt(secret.trim()))
        .transpose()
        .map_err(AccessError::Internal)?;
    let transaction = db.begin().await?;
    transaction.execute(statement(
        "UPDATE oidc_providers SET name=?,issuer_url=?,client_id=?,client_secret_envelope=COALESCE(?,client_secret_envelope),scopes_json=?,claim_mapping_json=?,enabled=?,updated_at=? WHERE id=?",
        vec![name.trim().into(), issuer.trim_end_matches('/').into(), client_id.trim().into(), envelope.clone().into(), input.scopes.as_ref().map(serde_json::to_string).transpose().map_err(|e| AccessError::Internal(e.into()))?.unwrap_or(current.scopes_json).into(), input.claim_mapping.clone().map(|value| value.to_string()).unwrap_or(current.claim_mapping_json).into(), input.enabled.unwrap_or(current.enabled).into(), db::now().into(), id.into()],
    )).await.map_err(|error| AccessError::Conflict(error.to_string()))?;
    write_audit(
        &transaction,
        actor,
        "update",
        "oidc_provider",
        id,
        json!({"secret_rotated":envelope.is_some()}),
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
    let state = crypto::opaque_token("os_");
    let mut verifier_bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut verifier_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    db.execute(statement(
        "INSERT INTO oidc_auth_states(state_hash,provider_id,code_verifier_envelope,redirect_uri,expires_at,created_at) VALUES(?,?,?,?,?,?)",
        vec![crypto::token_hash(&state).into(), provider_id.into(), secrets.encrypt(&verifier).map_err(AccessError::Internal)?.into(), redirect_uri.into(), (db::now() + ttl_seconds).into(), db::now().into()],
    )).await?;
    Ok(PkceState {
        state,
        code_challenge,
    })
}

pub async fn consume_pkce_state(
    db: &DatabaseConnection,
    secrets: &SecretBox,
    state: &str,
) -> Result<ConsumedState, AccessError> {
    let row = StateRow::find_by_statement(statement(
        "DELETE FROM oidc_auth_states WHERE state_hash=? AND expires_at>? RETURNING provider_id,code_verifier_envelope,redirect_uri",
        vec![crypto::token_hash(state).into(), db::now().into()],
    )).one(db).await?.ok_or(AccessError::Invalid("OIDC state is invalid, expired, or already used".into()))?;
    Ok(ConsumedState {
        provider_id: row.provider_id,
        code_verifier: secrets
            .decrypt(&row.code_verifier_envelope)
            .map_err(AccessError::Internal)?
            .to_string(),
        redirect_uri: row.redirect_uri,
    })
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
    if claims.sub.trim().is_empty() || claims.email_verified == Some(false) {
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
    }
    if let Some(identity) = IdentityUser::find_by_statement(statement(
        "SELECT user_id FROM oidc_identities WHERE provider_id=? AND subject=?",
        vec![provider_id.into(), claims.sub.clone().into()],
    ))
    .one(db)
    .await?
    {
        let transaction = db.begin().await?;
        transaction.execute(statement("UPDATE oidc_identities SET claims_json=?,last_login_at=? WHERE provider_id=? AND subject=?", vec![serde_json::to_string(claims).map_err(|e| AccessError::Internal(e.into()))?.into(), db::now().into(), provider_id.into(), claims.sub.clone().into()])).await?;
        transaction.commit().await?;
        return access::get_user(db, &identity.user_id).await;
    }
    let jit = mapping.get("jit").and_then(Value::as_bool).unwrap_or(false);
    #[derive(FromQueryResult)]
    struct ExistingUser {
        id: String,
    }
    let existing = ExistingUser::find_by_statement(statement(
        "SELECT id FROM users WHERE email=? COLLATE NOCASE AND enabled=1",
        vec![email.clone().into()],
    ))
    .one(db)
    .await?;
    if existing.is_none() && !jit {
        return Err(AccessError::Forbidden);
    }
    let was_existing = existing.is_some();
    let timestamp = db::now();
    let transaction = db.begin().await?;
    let user_id = if let Some(existing) = existing {
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
    transaction.execute(statement("INSERT INTO oidc_identities(id,provider_id,user_id,subject,claims_json,last_login_at,created_at) VALUES(?,?,?,?,?,?,?)", vec![Uuid::new_v4().to_string().into(), provider_id.into(), user_id.clone().into(), claims.sub.clone().into(), serde_json::to_string(claims).map_err(|e| AccessError::Internal(e.into()))?.into(), timestamp.into(), timestamp.into()])).await.map_err(|error| AccessError::Conflict(error.to_string()))?;

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
    transaction.execute(statement(
        "INSERT INTO project_memberships(id,project_id,user_id,role_id,status,created_at,updated_at) VALUES(?,?,?,?,\'active\',?,?) ON CONFLICT(project_id,user_id) DO UPDATE SET role_id=excluded.role_id,status='active',updated_at=excluded.updated_at",
        vec![Uuid::new_v4().to_string().into(), project_id.into(), user_id.clone().into(), role_id.clone().into(), timestamp.into(), timestamp.into()],
    )).await.map_err(|error| AccessError::Invalid(format!("OIDC role mapping is invalid: {error}")))?;
    write_audit(
        &transaction,
        &Principal::session(user_id.clone()),
        "login",
        "oidc_identity",
        provider_id,
        json!({"project_id":project_id,"role_id":role_id,"jit":!was_existing}),
    )
    .await?;
    transaction.commit().await?;
    access::get_user(db, &user_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SetupRequest;

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
            name: "SSO".into(), issuer_url: "https://id.example.test".into(), client_id: "client".into(), client_secret, scopes: default_scopes(), enabled: true,
            claim_mapping: json!({"version":1,"jit":true,"project_id":db::DEFAULT_PROJECT_ID,"default_role_id":SYSTEM_MEMBER_ROLE_ID,"role_mappings":{"admins":crate::db::SYSTEM_OWNER_ROLE_ID}}),
        }).await.unwrap();
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
        assert_eq!(pkce.code_challenge.len(), 43);
        let consumed = consume_pkce_state(&database, &secrets, &pkce.state)
            .await
            .unwrap();
        assert_eq!(consumed.provider_id, provider.id);
        assert_eq!(consumed.redirect_uri, "https://app.example.test/callback");
        assert!(consumed.code_verifier.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_' | '~')
        }));
        assert_eq!(
            URL_SAFE_NO_PAD.encode(Sha256::digest(consumed.code_verifier.as_bytes())),
            pkce.code_challenge
        );
        assert!(matches!(
            consume_pkce_state(&database, &secrets, &pkce.state).await,
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
            consume_pkce_state(&database, &secrets, &expired.state).await,
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
    }
}
