use std::net::IpAddr;

use anyhow::{Context, Result, bail};
use ipnet::IpNet;
use sea_orm::{
    ConnectionTrait, Database, DatabaseConnection, DbBackend, FromQueryResult, Statement,
    TransactionTrait,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{crypto, models::*};

mod schema;

pub use schema::{DEFAULT_PROJECT_ID, SYSTEM_OWNER_ROLE_ID};

pub async fn connect(url: &str) -> Result<DatabaseConnection> {
    let db = Database::connect(url)
        .await
        .context("failed to connect to SQLite")?;
    db.execute_unprepared(
        "PRAGMA auto_vacuum=INCREMENTAL; PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
    )
    .await
    .context("failed to configure SQLite")?;
    schema::migrate(&db).await?;
    Ok(db)
}

fn stmt(sql: impl Into<String>, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

pub fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

pub async fn is_initialized<C: ConnectionTrait>(db: &C) -> Result<bool> {
    #[derive(FromQueryResult)]
    struct Count {
        count: i64,
    }
    let row = Count::find_by_statement(stmt("SELECT COUNT(*) AS count FROM users", vec![]))
        .one(db)
        .await?;
    Ok(row.is_some_and(|value| value.count > 0))
}

pub async fn create_initial_admin(db: &DatabaseConnection, request: &SetupRequest) -> Result<User> {
    if request.email.trim().is_empty() || !request.email.contains('@') {
        bail!("a valid email is required");
    }
    if request.password.len() < 12 {
        bail!("password must contain at least 12 characters");
    }
    let transaction = db.begin().await?;
    transaction
        .execute(stmt(
            "INSERT INTO instance_state(id,initialized_at) VALUES(1,?)",
            vec![now().into()],
        ))
        .await
        .map_err(|_| anyhow::anyhow!("instance is already initialized"))?;
    let user = User {
        id: Uuid::new_v4().to_string(),
        email: request.email.trim().to_ascii_lowercase(),
        password_hash: crypto::hash_password(&request.password)?,
        role: "admin".into(),
        language: request.language.clone().unwrap_or_else(|| "zh-CN".into()),
        theme: "system:bronze".into(),
        created_at: now(),
    };
    transaction.execute(stmt(
        "INSERT INTO users(id,email,password_hash,role,language,theme,created_at,display_name,enabled,updated_at) VALUES(?,?,?,?,?,?,?,?,1,?)",
        vec![user.id.clone().into(), user.email.clone().into(), user.password_hash.clone().into(), user.role.clone().into(), user.language.clone().into(), user.theme.clone().into(), user.created_at.into(), user.email.clone().into(), user.created_at.into()],
    )).await?;
    transaction
        .execute(stmt(
            "UPDATE projects SET owner_user_id=? WHERE id=? AND owner_user_id IS NULL",
            vec![user.id.clone().into(), DEFAULT_PROJECT_ID.into()],
        ))
        .await?;
    transaction
        .execute(stmt(
            "INSERT OR IGNORE INTO project_memberships(id,project_id,user_id,role_id,status,created_at,updated_at) VALUES(?,?,?,?,\'active\',?,?)",
            vec![
                user.id.clone().into(),
                DEFAULT_PROJECT_ID.into(),
                user.id.clone().into(),
                SYSTEM_OWNER_ROLE_ID.into(),
                user.created_at.into(),
                user.created_at.into(),
            ],
        ))
        .await?;
    transaction
        .execute(stmt(
            "INSERT OR IGNORE INTO user_role_bindings(id,user_id,role_id,project_id,created_at) VALUES(?,?,?,?,?)",
            vec![
                user.id.clone().into(),
                user.id.clone().into(),
                SYSTEM_OWNER_ROLE_ID.into(),
                sea_orm::Value::String(None),
                user.created_at.into(),
            ],
        ))
        .await?;
    if let Some(name) = request
        .instance_name
        .as_ref()
        .filter(|name| !name.trim().is_empty())
    {
        transaction
            .execute(stmt(
                "INSERT OR REPLACE INTO settings(key,value,updated_at) VALUES('instance_name',?,?)",
                vec![name.trim().to_owned().into(), now().into()],
            ))
            .await?;
    }
    transaction.commit().await?;
    Ok(user)
}

pub async fn find_user_by_email(db: &DatabaseConnection, email: &str) -> Result<Option<User>> {
    Ok(User::find_by_statement(stmt(
        "SELECT id,email,password_hash,role,language,theme,created_at FROM users WHERE email = ? COLLATE NOCASE AND enabled=1",
        vec![email.trim().into()],
    ))
    .one(db)
    .await?)
}

pub async fn find_user_by_session(db: &DatabaseConnection, token: &str) -> Result<Option<User>> {
    let hash = crypto::token_hash(token);
    Ok(User::find_by_statement(stmt(
        "SELECT u.id,u.email,u.password_hash,u.role,u.language,u.theme,u.created_at FROM users u JOIN sessions s ON s.user_id=u.id WHERE s.token_hash=? AND s.expires_at>? AND u.enabled=1",
        vec![hash.into(), now().into()],
    )).one(db).await?)
}

pub async fn create_session(db: &DatabaseConnection, user_id: &str) -> Result<String> {
    let token = crypto::opaque_token("ps_");
    db.execute(stmt(
        "INSERT INTO sessions(id,user_id,token_hash,expires_at,created_at) VALUES(?,?,?,?,?)",
        vec![
            Uuid::new_v4().to_string().into(),
            user_id.into(),
            crypto::token_hash(&token).into(),
            (now() + 30 * 24 * 3600).into(),
            now().into(),
        ],
    ))
    .await?;
    Ok(token)
}

pub async fn delete_session(db: &DatabaseConnection, token: &str) -> Result<()> {
    db.execute(stmt(
        "DELETE FROM sessions WHERE token_hash=?",
        vec![crypto::token_hash(token).into()],
    ))
    .await?;
    Ok(())
}

pub async fn list_providers(db: &DatabaseConnection) -> Result<Vec<Provider>> {
    Ok(Provider::find_by_statement(stmt(
        "SELECT id,name,kind,base_url,enabled,created_at,updated_at FROM providers ORDER BY name",
        vec![],
    ))
    .all(db)
    .await?)
}

pub async fn create_provider(
    db: &DatabaseConnection,
    input: &ProviderInput,
    secret_envelope: String,
) -> Result<Provider> {
    let catalog = crate::catalog::repository::effective(db).await?;
    let preset = catalog
        .providers
        .iter()
        .find(|provider| provider.id == input.kind.trim());
    let kind = if crate::providers::KINDS.contains(&input.kind.trim()) {
        input.kind.trim()
    } else {
        preset
            .and_then(|provider| provider.adapter_kind.as_deref())
            .filter(|kind| crate::providers::KINDS.contains(kind))
            .ok_or_else(|| anyhow::anyhow!("provider preset has no implemented adapter"))?
    };
    let base_url = if input.base_url.trim().is_empty() {
        preset
            .and_then(|provider| provider.default_base_url.as_deref())
            .or_else(|| crate::providers::default_base(kind))
            .ok_or_else(|| anyhow::anyhow!("base URL is required for this provider"))?
    } else {
        input.base_url.trim()
    };
    let id = Uuid::new_v4().to_string();
    let timestamp = now();
    let mut catalog_paths = serde_json::Map::new();
    if let Some(preset) = preset {
        for endpoint in &preset.default_endpoints {
            if matches!(
                endpoint.transport,
                crate::catalog::types::Transport::Websocket
            ) {
                bail!("upstream WebSocket presets are not implemented");
            }
            if let Some(canonical) = crate::providers::ENDPOINTS
                .iter()
                .find(|path| crate::providers::capability(path) == endpoint.protocol)
                && !endpoint.path.contains('{')
                && endpoint.path != *canonical
            {
                catalog_paths.insert((*canonical).into(), serde_json::json!(endpoint.path));
            }
        }
    }
    let transaction = db.begin().await?;
    transaction.execute(stmt(
        "INSERT INTO providers(id,name,kind,base_url,enabled,created_at,updated_at,project_id,settings_json) VALUES(?,?,?,?,1,?,?,?,?)",
        vec![id.clone().into(), input.name.trim().to_owned().into(), kind.into(), base_url.trim_end_matches('/').to_owned().into(), timestamp.into(), timestamp.into(), DEFAULT_PROJECT_ID.into(),serde_json::json!({"version":1,"catalog_preset_id":preset.map(|p|&p.id),"catalog_version":catalog.version}).to_string().into()],
    )).await?;
    transaction
        .execute(stmt(
            "INSERT INTO channel_credentials(id,provider_id,credential_type,secret_envelope,suffix,priority,enabled,created_at,updated_at) VALUES(?,?,'api_key',?,'',100,1,?,?)",
            vec![
                id.clone().into(),
                id.clone().into(),
                secret_envelope.into(),
                timestamp.into(),
                timestamp.into(),
            ],
        ))
        .await?;
    transaction
        .execute(stmt(
            "INSERT INTO channel_settings(provider_id,updated_at,endpoint_mappings_json) VALUES(?,?,?)",
            vec![id.clone().into(), timestamp.into(),serde_json::json!({"version":1,"paths":catalog_paths}).to_string().into()],
        ))
        .await?;
    transaction.commit().await?;
    Ok(Provider::find_by_statement(stmt(
        "SELECT id,name,kind,base_url,enabled,created_at,updated_at FROM providers WHERE id=?",
        vec![id.into()],
    ))
    .one(db)
    .await?
    .expect("inserted provider exists"))
}

pub async fn delete_provider(db: &DatabaseConnection, id: &str) -> Result<bool> {
    Ok(db
        .execute(stmt("DELETE FROM providers WHERE id=?", vec![id.into()]))
        .await?
        .rows_affected()
        > 0)
}

pub async fn list_models(db: &DatabaseConnection) -> Result<Vec<Model>> {
    Ok(Model::find_by_statement(stmt(
        "SELECT m.*, p.name AS provider_name FROM models m JOIN providers p ON p.id=m.provider_id ORDER BY m.public_name,m.priority,p.name",
        vec![],
    )).all(db).await?)
}

pub async fn create_model(db: &DatabaseConnection, input: &ModelInput) -> Result<Model> {
    let id = Uuid::new_v4().to_string();
    let catalog = crate::catalog::repository::effective(db).await?;
    let defaults = catalog.models.iter().find(|model| {
        model.upstream_id == input.upstream_name
            || model.id == input.upstream_name
            || model.aliases.contains(&input.upstream_name)
    });
    let default_capabilities = defaults
        .map(|model| model.gateway_capabilities())
        .unwrap_or_else(|| {
            vec![
                "chat".to_owned(),
                "responses".to_owned(),
                "messages".to_owned(),
            ]
        });
    let capabilities = serde_json::to_string(
        input
            .capabilities
            .as_deref()
            .unwrap_or(&default_capabilities),
    )?;
    let price = |value: Option<f64>| {
        value
            .filter(|value| {
                value.is_finite() && *value >= 0.0 && *value <= (i64::MAX as f64 / 1_000_000.0)
            })
            .map(|value| (value * 1_000_000.0).round() as i64)
            .unwrap_or(0)
    };
    let default_prices = defaults.filter(|model| {
        model.cost_defaults.currency.as_deref() == Some("USD")
            && model.cost_defaults.unit.as_deref() == Some("per_million_tokens")
    });
    let metadata =
        serde_json::json!({"catalog_version":catalog.version,"card":defaults}).to_string();
    db.execute(stmt(
        "INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,input_price_micros,output_price_micros,priority,enabled,created_at,catalog_metadata_json) VALUES(?,?,?,?,?,?,?,?,1,?,?)",
        vec![id.clone().into(), input.provider_id.clone().into(), input.public_name.trim().to_owned().into(), input.upstream_name.trim().to_owned().into(), capabilities.into(), input.input_price_micros.unwrap_or_else(||price(default_prices.and_then(|model|model.cost_defaults.input))).into(), input.output_price_micros.unwrap_or_else(||price(default_prices.and_then(|model|model.cost_defaults.output))).into(), input.priority.unwrap_or(100).into(), now().into(),metadata.into()],
    )).await?;
    Ok(Model::find_by_statement(stmt(
        "SELECT m.*,p.name AS provider_name FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.id=?",
        vec![id.into()],
    )).one(db).await?.expect("inserted model exists"))
}

pub async fn delete_model(db: &DatabaseConnection, id: &str) -> Result<bool> {
    Ok(db
        .execute(stmt("DELETE FROM models WHERE id=?", vec![id.into()]))
        .await?
        .rows_affected()
        > 0)
}

pub async fn list_api_keys(db: &DatabaseConnection) -> Result<Vec<ApiKey>> {
    Ok(ApiKey::find_by_statement(stmt("SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,last_used_at,created_at FROM api_keys WHERE project_id=? ORDER BY created_at DESC", vec![DEFAULT_PROJECT_ID.into()])).all(db).await?)
}

pub async fn create_api_key(
    db: &DatabaseConnection,
    input: &ApiKeyInput,
) -> Result<(ApiKey, String)> {
    let token = match input.token_mode {
        ApiKeyTokenMode::Generated => crypto::opaque_token("pg_"),
        ApiKeyTokenMode::ImportExisting => {
            let token = input
                .token
                .as_deref()
                .context("token is required for import")?;
            if !crypto::imported_token_has_entropy(token) {
                bail!("imported token must contain 32–1024 high-entropy, non-whitespace characters")
            }
            token.to_owned()
        }
    };
    let lookup_digest = crypto::token_hash(&token);
    let prefix = lookup_digest[..8].to_owned();
    let hash = crypto::hash_password(&token)?;
    let id = Uuid::new_v4().to_string();
    db.execute(stmt(
        "INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,scopes,budget_micros,spent_micros,enabled,created_at,project_id) VALUES(?,?,?,?,?,'[\"gateway\"]',?,0,1,?,?)",
        vec![id.clone().into(), input.name.trim().to_owned().into(), prefix.into(), hash.into(), lookup_digest.into(), input.budget_micros.into(), now().into(), DEFAULT_PROJECT_ID.into()],
    )).await?;
    let key = ApiKey::find_by_statement(stmt("SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,last_used_at,created_at FROM api_keys WHERE id=?", vec![id.into()])).one(db).await?.expect("inserted API key exists");
    Ok((key, token))
}

pub async fn delete_api_key(db: &DatabaseConnection, id: &str) -> Result<bool> {
    Ok(db
        .execute(stmt(
            "DELETE FROM api_keys WHERE id=? AND project_id=?",
            vec![id.into(), DEFAULT_PROJECT_ID.into()],
        ))
        .await?
        .rows_affected()
        > 0)
}

pub async fn authenticate_api_key(
    db: &DatabaseConnection,
    token: &str,
    client_ip: Option<IpAddr>,
) -> Result<Option<ApiKeyCredential>> {
    let lookup_digest = crypto::token_hash(token);
    let credential = ApiKeyCredential::find_by_statement(stmt(
        "SELECT k.id,k.project_id,k.user_id,k.key_hash,k.scopes,k.budget_micros,k.spent_micros,k.enabled,k.expires_at,k.allowed_ips_json,k.denied_ips_json FROM api_keys k LEFT JOIN users u ON u.id=k.user_id WHERE k.lookup_digest=? AND (k.key_type NOT IN ('user','personal') OR (u.enabled=1 AND EXISTS (SELECT 1 FROM project_memberships membership WHERE membership.project_id=k.project_id AND membership.user_id=k.user_id AND membership.status='active')))",
        vec![lookup_digest.into()],
    )).one(db).await?;
    let Some(credential) = credential else {
        return Ok(None);
    };
    if !credential.enabled
        || credential
            .expires_at
            .is_some_and(|expires| expires <= now())
        || !api_key_ip_allowed(&credential, client_ip)
        || !crypto::verify_password(token, &credential.key_hash)
    {
        return Ok(None);
    }
    if credential
        .budget_micros
        .is_some_and(|budget| credential.spent_micros >= budget)
    {
        return Ok(None);
    }
    db.execute(stmt(
        "UPDATE api_keys SET last_used_at=? WHERE id=?",
        vec![now().into(), credential.id.clone().into()],
    ))
    .await?;
    Ok(Some(credential))
}

pub async fn api_key_credential_by_id(
    db: &DatabaseConnection,
    project_id: &str,
    id: &str,
) -> Result<Option<ApiKeyCredential>> {
    Ok(ApiKeyCredential::find_by_statement(stmt(
        "SELECT k.id,k.project_id,k.user_id,k.key_hash,k.scopes,k.budget_micros,k.spent_micros,k.enabled,k.expires_at,k.allowed_ips_json,k.denied_ips_json FROM api_keys k LEFT JOIN users u ON u.id=k.user_id WHERE k.id=? AND k.project_id=? AND k.enabled=1 AND (k.expires_at IS NULL OR k.expires_at>unixepoch()) AND (k.key_type NOT IN ('user','personal') OR (u.enabled=1 AND EXISTS (SELECT 1 FROM project_memberships membership WHERE membership.project_id=k.project_id AND membership.user_id=k.user_id AND membership.status='active')))",
        vec![id.into(), project_id.into()],
    )).one(db).await?)
}

fn api_key_ip_allowed(credential: &ApiKeyCredential, client_ip: Option<IpAddr>) -> bool {
    let Ok(allowed) = serde_json::from_str::<Vec<String>>(&credential.allowed_ips_json) else {
        return false;
    };
    let Ok(denied) = serde_json::from_str::<Vec<String>>(&credential.denied_ips_json) else {
        return false;
    };
    if allowed.is_empty() && denied.is_empty() {
        return true;
    }
    let Some(client_ip) = client_ip else {
        return false;
    };
    let matches = |rules: &[String]| -> Option<bool> {
        let mut matched = false;
        for rule in rules {
            let network = rule.parse::<IpNet>().or_else(|_| {
                rule.parse::<IpAddr>()
                    .map(IpNet::from)
                    .map_err(|_| "invalid IP policy")
            });
            let Ok(network) = network else { return None };
            matched |= network.contains(&client_ip);
        }
        Some(matched)
    };
    if matches(&denied) != Some(false) {
        return false;
    }
    allowed.is_empty() || matches(&allowed) == Some(true)
}

pub async fn record_audit_event(
    db: &DatabaseConnection,
    actor_user_id: &str,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    details: serde_json::Value,
) -> Result<()> {
    db.execute(stmt(
        "INSERT INTO audit_events(id,actor_user_id,action,resource_type,resource_id,details,created_at) VALUES(?,?,?,?,?,?,?)",
        vec![
            Uuid::new_v4().to_string().into(),
            actor_user_id.into(),
            action.into(),
            resource_type.into(),
            resource_id.into(),
            details.to_string().into(),
            now().into(),
        ],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(FromQueryResult)]
    struct TestCount {
        count: i64,
    }

    #[tokio::test]
    async fn setup_is_one_time_and_api_keys_verify() {
        let db = connect("sqlite::memory:").await.unwrap();
        let request = SetupRequest {
            email: "admin@example.com".into(),
            password: "a secure password".into(),
            instance_name: None,
            language: None,
        };
        create_initial_admin(&db, &request).await.unwrap();
        let owner_count = TestCount::find_by_statement(stmt(
            "SELECT COUNT(*) AS count FROM projects p JOIN project_memberships pm ON pm.project_id=p.id AND pm.user_id=p.owner_user_id WHERE p.id=? AND pm.role_id=?",
            vec![DEFAULT_PROJECT_ID.into(), SYSTEM_OWNER_ROLE_ID.into()],
        ))
        .one(&db)
        .await
        .unwrap()
        .unwrap()
        .count;
        assert_eq!(owner_count, 1);
        assert!(create_initial_admin(&db, &request).await.is_err());
        let (_, token) = create_api_key(
            &db,
            &ApiKeyInput {
                name: "test".into(),
                budget_micros: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let provider = create_provider(
            &db,
            &ProviderInput {
                name: "test provider".into(),
                kind: "openai".into(),
                base_url: "https://example.test".into(),
                api_key: "unused".into(),
            },
            "encrypted-secret".into(),
        )
        .await
        .unwrap();
        let normalized_provider_count = TestCount::find_by_statement(stmt(
            "SELECT COUNT(*) AS count FROM channel_credentials c JOIN channel_settings s ON s.provider_id=c.provider_id WHERE c.provider_id=? AND c.secret_envelope='encrypted-secret'",
            vec![provider.id.into()],
        ))
        .one(&db)
        .await
        .unwrap()
        .unwrap()
        .count;
        assert_eq!(normalized_provider_count, 1);
        assert!(
            authenticate_api_key(&db, &token, None)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            authenticate_api_key(&db, "pg_invalid_token", None)
                .await
                .unwrap()
                .is_none()
        );
    }
}
