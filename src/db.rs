use anyhow::{Context, Result, bail};
use sea_orm::{
    ConnectionTrait, Database, DatabaseConnection, DbBackend, FromQueryResult, Statement,
    TransactionTrait,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{crypto, models::*};

pub async fn connect(url: &str) -> Result<DatabaseConnection> {
    let db = Database::connect(url)
        .await
        .context("failed to connect to SQLite")?;
    db.execute_unprepared(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
    )
    .await
    .context("failed to configure SQLite")?;
    migrate(&db).await?;
    Ok(db)
}

async fn migrate(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS instance_state (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            initialized_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            email TEXT NOT NULL UNIQUE COLLATE NOCASE,
            password_hash TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'admin',
            language TEXT NOT NULL DEFAULT 'zh-CN',
            theme TEXT NOT NULL DEFAULT 'system:bronze',
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            token_hash TEXT NOT NULL UNIQUE,
            expires_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(token_hash);
        CREATE INDEX IF NOT EXISTS idx_sessions_expiry ON sessions(expires_at);
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS providers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE COLLATE NOCASE,
            kind TEXT NOT NULL CHECK(kind IN ('openai','openai_compatible','anthropic')),
            base_url TEXT NOT NULL,
            secret_envelope TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS models (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
            public_name TEXT NOT NULL,
            upstream_name TEXT NOT NULL,
            capabilities TEXT NOT NULL DEFAULT '["chat"]',
            input_price_micros INTEGER NOT NULL DEFAULT 0 CHECK(input_price_micros >= 0),
            output_price_micros INTEGER NOT NULL DEFAULT 0 CHECK(output_price_micros >= 0),
            priority INTEGER NOT NULL DEFAULT 100,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            UNIQUE(provider_id, public_name, upstream_name)
        );
        CREATE INDEX IF NOT EXISTS idx_models_public_name ON models(public_name, enabled, priority);
        CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            key_prefix TEXT NOT NULL UNIQUE,
            key_hash TEXT NOT NULL,
            scopes TEXT NOT NULL DEFAULT '["gateway"]',
            budget_micros INTEGER,
            spent_micros INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1,
            last_used_at INTEGER,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_api_keys_prefix ON api_keys(key_prefix);
        CREATE TABLE IF NOT EXISTS audit_events (
            id TEXT PRIMARY KEY,
            actor_user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
            action TEXT NOT NULL,
            resource_type TEXT NOT NULL,
            resource_id TEXT,
            details TEXT NOT NULL DEFAULT '{}',
            created_at INTEGER NOT NULL
        );
        INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(1, unixepoch());
        "#,
    )
    .await
    .context("failed to migrate SQLite schema")?;
    Ok(())
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
        "INSERT INTO users(id,email,password_hash,role,language,theme,created_at) VALUES(?,?,?,?,?,?,?)",
        vec![user.id.clone().into(), user.email.clone().into(), user.password_hash.clone().into(), user.role.clone().into(), user.language.clone().into(), user.theme.clone().into(), user.created_at.into()],
    )).await?;
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
        "SELECT * FROM users WHERE email = ? COLLATE NOCASE",
        vec![email.trim().into()],
    ))
    .one(db)
    .await?)
}

pub async fn find_user_by_session(db: &DatabaseConnection, token: &str) -> Result<Option<User>> {
    let hash = crypto::token_hash(token);
    Ok(User::find_by_statement(stmt(
        "SELECT u.* FROM users u JOIN sessions s ON s.user_id=u.id WHERE s.token_hash=? AND s.expires_at>?",
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
    let kind = input.kind.trim();
    if !matches!(kind, "openai" | "openai_compatible" | "anthropic") {
        bail!("unsupported provider kind");
    }
    let id = Uuid::new_v4().to_string();
    let timestamp = now();
    db.execute(stmt(
        "INSERT INTO providers(id,name,kind,base_url,secret_envelope,enabled,created_at,updated_at) VALUES(?,?,?,?,?,1,?,?)",
        vec![id.clone().into(), input.name.trim().to_owned().into(), kind.into(), input.base_url.trim_end_matches('/').to_owned().into(), secret_envelope.into(), timestamp.into(), timestamp.into()],
    )).await?;
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
    let capabilities = serde_json::to_string(input.capabilities.as_deref().unwrap_or(&[
        "chat".to_owned(),
        "responses".to_owned(),
        "messages".to_owned(),
    ]))?;
    db.execute(stmt(
        "INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,input_price_micros,output_price_micros,priority,enabled,created_at) VALUES(?,?,?,?,?,?,?,?,1,?)",
        vec![id.clone().into(), input.provider_id.clone().into(), input.public_name.trim().to_owned().into(), input.upstream_name.trim().to_owned().into(), capabilities.into(), input.input_price_micros.unwrap_or(0).into(), input.output_price_micros.unwrap_or(0).into(), input.priority.unwrap_or(100).into(), now().into()],
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

pub async fn resolve_targets(
    db: &DatabaseConnection,
    public_name: &str,
    endpoint: &str,
) -> Result<Vec<RouteTarget>> {
    let targets = RouteTarget::find_by_statement(stmt(
        "SELECT m.public_name,m.upstream_name,m.capabilities,p.name AS provider_name,p.kind AS provider_kind,p.base_url,p.secret_envelope,m.input_price_micros,m.output_price_micros FROM models m JOIN providers p ON p.id=m.provider_id WHERE m.public_name=? AND m.enabled=1 AND p.enabled=1 ORDER BY m.priority,p.name",
        vec![public_name.into()],
    )).all(db).await?;
    Ok(targets
        .into_iter()
        .filter(|target| {
            let capabilities =
                serde_json::from_str::<Vec<String>>(&target.capabilities).unwrap_or_default();
            match endpoint {
                "/v1/chat/completions" => capabilities.iter().any(|value| value == "chat"),
                "/v1/responses" => {
                    target.provider_kind != "anthropic"
                        && capabilities.iter().any(|value| value == "responses")
                }
                "/v1/messages" => {
                    target.provider_kind == "anthropic"
                        && capabilities
                            .iter()
                            .any(|value| value == "messages" || value == "chat")
                }
                _ => false,
            }
        })
        .collect())
}

pub async fn list_api_keys(db: &DatabaseConnection) -> Result<Vec<ApiKey>> {
    Ok(ApiKey::find_by_statement(stmt("SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,last_used_at,created_at FROM api_keys ORDER BY created_at DESC", vec![])).all(db).await?)
}

pub async fn create_api_key(
    db: &DatabaseConnection,
    input: &ApiKeyInput,
) -> Result<(ApiKey, String)> {
    let prefix_random = crypto::opaque_token("");
    let prefix = prefix_random.chars().take(8).collect::<String>();
    let token = format!("pg_{prefix}_{}", crypto::opaque_token(""));
    let hash = crypto::hash_password(&token)?;
    let id = Uuid::new_v4().to_string();
    db.execute(stmt(
        "INSERT INTO api_keys(id,name,key_prefix,key_hash,scopes,budget_micros,spent_micros,enabled,created_at) VALUES(?,?,?,?,'[\"gateway\"]',?,0,1,?)",
        vec![id.clone().into(), input.name.trim().to_owned().into(), prefix.into(), hash.into(), input.budget_micros.into(), now().into()],
    )).await?;
    let key = ApiKey::find_by_statement(stmt("SELECT id,name,key_prefix,scopes,budget_micros,spent_micros,enabled,last_used_at,created_at FROM api_keys WHERE id=?", vec![id.into()])).one(db).await?.expect("inserted API key exists");
    Ok((key, token))
}

pub async fn delete_api_key(db: &DatabaseConnection, id: &str) -> Result<bool> {
    Ok(db
        .execute(stmt("DELETE FROM api_keys WHERE id=?", vec![id.into()]))
        .await?
        .rows_affected()
        > 0)
}

pub async fn authenticate_api_key(
    db: &DatabaseConnection,
    token: &str,
) -> Result<Option<ApiKeyCredential>> {
    let mut parts = token.splitn(3, '_');
    if parts.next() != Some("pg") {
        return Ok(None);
    }
    let Some(prefix) = parts.next() else {
        return Ok(None);
    };
    if parts.next().is_none() {
        return Ok(None);
    }
    let credential = ApiKeyCredential::find_by_statement(stmt(
        "SELECT id,key_hash,scopes,budget_micros,spent_micros,enabled FROM api_keys WHERE key_prefix=?",
        vec![prefix.into()],
    )).one(db).await?;
    let Some(credential) = credential else {
        return Ok(None);
    };
    if !credential.enabled || !crypto::verify_password(token, &credential.key_hash) {
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

pub async fn add_api_key_spend(db: &DatabaseConnection, id: &str, cost_micros: i64) -> Result<()> {
    if cost_micros <= 0 {
        return Ok(());
    }
    db.execute(stmt(
        "UPDATE api_keys SET spent_micros=spent_micros+? WHERE id=?",
        vec![cost_micros.into(), id.into()],
    ))
    .await?;
    Ok(())
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
        assert!(create_initial_admin(&db, &request).await.is_err());
        let (_, token) = create_api_key(
            &db,
            &ApiKeyInput {
                name: "test".into(),
                budget_micros: None,
            },
        )
        .await
        .unwrap();
        assert!(authenticate_api_key(&db, &token).await.unwrap().is_some());
        assert!(
            authenticate_api_key(&db, "pg_invalid_token")
                .await
                .unwrap()
                .is_none()
        );
    }
}
