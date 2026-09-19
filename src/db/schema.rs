use anyhow::{Context, Result};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, TransactionTrait,
};

pub const DEFAULT_PROJECT_ID: &str = "00000000-0000-0000-0000-000000000001";
pub const SYSTEM_OWNER_ROLE_ID: &str = "00000000-0000-0000-0000-000000000010";

const MIGRATION_LEDGER: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);
"#;

const EXPERIMENTAL_V1_RESET: &str = r#"
DROP TABLE IF EXISTS audit_events;
DROP TABLE IF EXISTS models;
DROP TABLE IF EXISTS providers;
DROP TABLE IF EXISTS api_keys;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS settings;
DROP TABLE IF EXISTS instance_state;
DROP TABLE IF EXISTS users;
DELETE FROM schema_migrations;
"#;

const BASE_SCHEMA: &str = r#"
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
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id),
    settings_json TEXT NOT NULL DEFAULT '{"version":1}'
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
    created_at INTEGER NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id),
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    profile_id TEXT REFERENCES api_key_profiles(id) ON DELETE SET NULL,
    key_type TEXT NOT NULL DEFAULT 'service' CHECK(key_type IN ('user','service','personal','no_auth')),
    expires_at INTEGER,
    allowed_ips_json TEXT NOT NULL DEFAULT '[]',
    denied_ips_json TEXT NOT NULL DEFAULT '[]'
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
"#;

const V2_SCHEMA: &str = r#"
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE COLLATE NOCASE,
    owner_user_id TEXT REFERENCES users(id) ON DELETE RESTRICT,
    is_default INTEGER NOT NULL DEFAULT 0 CHECK(is_default IN (0,1)),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    settings_json TEXT NOT NULL DEFAULT '{"version":1}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_projects_one_default ON projects(is_default) WHERE is_default = 1;
CREATE INDEX idx_projects_owner ON projects(owner_user_id);

CREATE TABLE permissions (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    level TEXT NOT NULL CHECK(level IN ('system','project')),
    description TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);
CREATE TABLE roles (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    scope TEXT NOT NULL CHECK(scope IN ('system','project')),
    is_system INTEGER NOT NULL DEFAULT 0 CHECK(is_system IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_roles_system_name ON roles(name) WHERE project_id IS NULL;
CREATE UNIQUE INDEX idx_roles_project_name ON roles(project_id,name) WHERE project_id IS NOT NULL;
CREATE INDEX idx_roles_project ON roles(project_id);
CREATE TABLE role_permissions (
    role_id TEXT NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id TEXT NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(role_id,permission_id)
);
CREATE INDEX idx_role_permissions_permission ON role_permissions(permission_id);
CREATE TABLE project_memberships (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id TEXT NOT NULL REFERENCES roles(id) ON DELETE RESTRICT,
    status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','suspended')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id,user_id)
);
CREATE INDEX idx_project_memberships_user ON project_memberships(user_id);
CREATE INDEX idx_project_memberships_role ON project_memberships(role_id);
CREATE TABLE project_invitations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    email TEXT NOT NULL COLLATE NOCASE,
    role_id TEXT NOT NULL REFERENCES roles(id) ON DELETE RESTRICT,
    invited_by_user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at INTEGER NOT NULL,
    accepted_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_project_invitations_project_email ON project_invitations(project_id,email);
CREATE INDEX idx_project_invitations_role ON project_invitations(role_id);
CREATE INDEX idx_project_invitations_inviter ON project_invitations(invited_by_user_id);
CREATE TABLE user_role_bindings (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id TEXT NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    UNIQUE(user_id,role_id,project_id)
);
CREATE INDEX idx_user_role_bindings_role ON user_role_bindings(role_id);
CREATE INDEX idx_user_role_bindings_project ON user_role_bindings(project_id);

CREATE TABLE oidc_providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    issuer_url TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_secret_envelope TEXT NOT NULL,
    scopes_json TEXT NOT NULL DEFAULT '["openid","profile","email"]',
    claim_mapping_json TEXT NOT NULL DEFAULT '{"version":1}',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE oidc_identities (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES oidc_providers(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subject TEXT NOT NULL,
    claims_json TEXT NOT NULL DEFAULT '{"version":1}',
    last_login_at INTEGER,
    created_at INTEGER NOT NULL,
    UNIQUE(provider_id,subject),
    UNIQUE(provider_id,user_id)
);
CREATE INDEX idx_oidc_identities_user ON oidc_identities(user_id);

CREATE TABLE api_key_profiles (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    rpm_limit INTEGER CHECK(rpm_limit IS NULL OR rpm_limit >= 0),
    tpm_limit INTEGER CHECK(tpm_limit IS NULL OR tpm_limit >= 0),
    budget_micros INTEGER CHECK(budget_micros IS NULL OR budget_micros >= 0),
    routing_policy_json TEXT NOT NULL DEFAULT '{"version":1}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id,name)
);
CREATE TABLE api_key_profile_model_mappings (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES api_key_profiles(id) ON DELETE CASCADE,
    source_model TEXT NOT NULL,
    target_model TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 100,
    UNIQUE(profile_id,source_model)
);
CREATE INDEX idx_api_key_profile_mappings_profile ON api_key_profile_model_mappings(profile_id,priority);
CREATE TABLE api_key_profile_allowed_models (
    profile_id TEXT NOT NULL REFERENCES api_key_profiles(id) ON DELETE CASCADE,
    model_pattern TEXT NOT NULL,
    match_type TEXT NOT NULL DEFAULT 'exact' CHECK(match_type IN ('exact','regex')),
    PRIMARY KEY(profile_id,model_pattern,match_type)
);

CREATE INDEX idx_providers_project ON providers(project_id);
CREATE INDEX idx_sessions_user ON sessions(user_id);
CREATE INDEX idx_audit_events_actor ON audit_events(actor_user_id);
CREATE INDEX idx_api_keys_project ON api_keys(project_id);
CREATE INDEX idx_api_keys_user ON api_keys(user_id);
CREATE INDEX idx_api_keys_profile ON api_keys(profile_id);

CREATE TABLE channel_credentials (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    credential_type TEXT NOT NULL DEFAULT 'api_key',
    secret_envelope TEXT NOT NULL,
    suffix TEXT NOT NULL DEFAULT '',
    priority INTEGER NOT NULL DEFAULT 100,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    settings_json TEXT NOT NULL DEFAULT '{"version":1}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX idx_channel_credentials_provider ON channel_credentials(provider_id,enabled,priority);
CREATE TABLE channel_settings (
    provider_id TEXT PRIMARY KEY REFERENCES providers(id) ON DELETE CASCADE,
    endpoint_mappings_json TEXT NOT NULL DEFAULT '{"version":1}',
    model_rules_json TEXT NOT NULL DEFAULT '{"version":1}',
    parameter_overrides_json TEXT NOT NULL DEFAULT '{"version":1}',
    retry_statuses_json TEXT NOT NULL DEFAULT '{"version":1,"statuses":[408,409,429,500,502,503,504]}',
    auto_disable_policy_json TEXT NOT NULL DEFAULT '{"version":1,"enabled":false}',
    proxy_settings_json TEXT NOT NULL DEFAULT '{"version":1}',
    updated_at INTEGER NOT NULL
);

CREATE TABLE model_associations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    model_id TEXT REFERENCES models(id) ON DELETE CASCADE,
    provider_id TEXT REFERENCES providers(id) ON DELETE CASCADE,
    match_type TEXT NOT NULL CHECK(match_type IN ('exact','regex','tag')),
    pattern TEXT NOT NULL,
    conditions_json TEXT NOT NULL DEFAULT '{"version":1}',
    priority INTEGER NOT NULL DEFAULT 100,
    weight INTEGER NOT NULL DEFAULT 1 CHECK(weight > 0),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX idx_model_associations_project ON model_associations(project_id,enabled,priority);
CREATE INDEX idx_model_associations_model ON model_associations(model_id);
CREATE INDEX idx_model_associations_provider ON model_associations(provider_id);
CREATE TABLE model_prices (
    id TEXT PRIMARY KEY,
    model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    provider_id TEXT REFERENCES providers(id) ON DELETE CASCADE,
    version INTEGER NOT NULL CHECK(version > 0),
    currency TEXT NOT NULL DEFAULT 'USD',
    valid_from INTEGER NOT NULL,
    valid_until INTEGER,
    schedule_json TEXT NOT NULL DEFAULT '{"version":1}',
    created_at INTEGER NOT NULL,
    UNIQUE(model_id,provider_id,version)
);
CREATE INDEX idx_model_prices_model_schedule ON model_prices(model_id,valid_from,valid_until);
CREATE INDEX idx_model_prices_provider ON model_prices(provider_id);
CREATE TABLE model_price_components (
    id TEXT PRIMARY KEY,
    price_id TEXT NOT NULL REFERENCES model_prices(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('input','output','cache_read','cache_write','reasoning','flat','unit')),
    unit_size INTEGER NOT NULL DEFAULT 1 CHECK(unit_size > 0),
    unit_price_micros INTEGER NOT NULL CHECK(unit_price_micros >= 0),
    tiers_json TEXT NOT NULL DEFAULT '{"version":1,"tiers":[]}'
);
CREATE INDEX idx_model_price_components_price ON model_price_components(price_id);

CREATE TABLE prompts (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    activation_json TEXT NOT NULL DEFAULT '{"version":1}',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id,name)
);
CREATE TABLE prompt_protection_rules (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    role_pattern TEXT,
    content_pattern TEXT NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('deny','redact')),
    replacement TEXT,
    scopes_json TEXT NOT NULL DEFAULT '{"version":1}',
    test_mode INTEGER NOT NULL DEFAULT 0 CHECK(test_mode IN (0,1)),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX idx_prompt_protection_rules_project ON prompt_protection_rules(project_id,enabled);

CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    api_key_id TEXT REFERENCES api_keys(id) ON DELETE SET NULL,
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    external_id TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{"version":1}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id,external_id)
);
CREATE INDEX idx_threads_api_key ON threads(api_key_id);
CREATE INDEX idx_threads_user ON threads(user_id);
CREATE TABLE traces (
    id TEXT PRIMARY KEY,
    thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    api_key_id TEXT REFERENCES api_keys(id) ON DELETE SET NULL,
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    source TEXT NOT NULL DEFAULT 'api',
    status TEXT NOT NULL DEFAULT 'running',
    started_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX idx_traces_thread ON traces(thread_id);
CREATE INDEX idx_traces_project_started ON traces(project_id,started_at);
CREATE INDEX idx_traces_api_key ON traces(api_key_id);
CREATE INDEX idx_traces_user ON traces(user_id);
CREATE TABLE requests (
    id TEXT PRIMARY KEY,
    trace_id TEXT NOT NULL REFERENCES traces(id) ON DELETE CASCADE,
    parent_request_id TEXT REFERENCES requests(id) ON DELETE SET NULL,
    protocol TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    requested_model TEXT,
    source_ip TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    request_metadata_json TEXT NOT NULL DEFAULT '{"version":1}',
    response_metadata_json TEXT NOT NULL DEFAULT '{"version":1}',
    started_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX idx_requests_trace ON requests(trace_id,started_at);
CREATE INDEX idx_requests_parent ON requests(parent_request_id);
CREATE TABLE request_executions (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    provider_id TEXT REFERENCES providers(id) ON DELETE SET NULL,
    credential_id TEXT REFERENCES channel_credentials(id) ON DELETE SET NULL,
    attempt INTEGER NOT NULL CHECK(attempt > 0),
    model TEXT,
    status TEXT NOT NULL,
    retry_reason TEXT,
    credential_suffix TEXT,
    started_at INTEGER NOT NULL,
    first_token_at INTEGER,
    finished_at INTEGER,
    latency_ms INTEGER,
    UNIQUE(request_id,attempt)
);
CREATE INDEX idx_request_executions_provider ON request_executions(provider_id,started_at);
CREATE INDEX idx_request_executions_credential ON request_executions(credential_id,started_at);
CREATE TABLE usage_logs (
    id TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL UNIQUE REFERENCES request_executions(id) ON DELETE CASCADE,
    model_id TEXT REFERENCES models(id) ON DELETE SET NULL,
    price_id TEXT REFERENCES model_prices(id) ON DELETE SET NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0 CHECK(input_tokens >= 0),
    output_tokens INTEGER NOT NULL DEFAULT 0 CHECK(output_tokens >= 0),
    cache_read_tokens INTEGER NOT NULL DEFAULT 0 CHECK(cache_read_tokens >= 0),
    cache_write_tokens INTEGER NOT NULL DEFAULT 0 CHECK(cache_write_tokens >= 0),
    reasoning_tokens INTEGER NOT NULL DEFAULT 0 CHECK(reasoning_tokens >= 0),
    request_units INTEGER NOT NULL DEFAULT 0 CHECK(request_units >= 0),
    total_cost_micros INTEGER NOT NULL DEFAULT 0 CHECK(total_cost_micros >= 0),
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_usage_logs_model ON usage_logs(model_id,created_at);
CREATE INDEX idx_usage_logs_price ON usage_logs(price_id);
CREATE TABLE usage_cost_items (
    id TEXT PRIMARY KEY,
    usage_log_id TEXT NOT NULL REFERENCES usage_logs(id) ON DELETE CASCADE,
    price_component_id TEXT REFERENCES model_price_components(id) ON DELETE SET NULL,
    quantity INTEGER NOT NULL CHECK(quantity >= 0),
    subtotal_micros INTEGER NOT NULL CHECK(subtotal_micros >= 0)
);
CREATE INDEX idx_usage_cost_items_usage ON usage_cost_items(usage_log_id);
CREATE INDEX idx_usage_cost_items_component ON usage_cost_items(price_component_id);

CREATE TABLE provider_quota_snapshots (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    credential_id TEXT REFERENCES channel_credentials(id) ON DELETE CASCADE,
    period_start INTEGER,
    period_end INTEGER,
    remaining_micros INTEGER,
    quota_json TEXT NOT NULL DEFAULT '{"version":1}',
    collected_at INTEGER NOT NULL
);
CREATE INDEX idx_provider_quota_snapshots_provider ON provider_quota_snapshots(provider_id,collected_at);
CREATE INDEX idx_provider_quota_snapshots_credential ON provider_quota_snapshots(credential_id,collected_at);
CREATE TABLE channel_probes (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    credential_id TEXT REFERENCES channel_credentials(id) ON DELETE SET NULL,
    model TEXT,
    success INTEGER NOT NULL CHECK(success IN (0,1)),
    status_code INTEGER,
    latency_ms INTEGER,
    ttft_ms INTEGER,
    error_code TEXT,
    probed_at INTEGER NOT NULL
);
CREATE INDEX idx_channel_probes_provider ON channel_probes(provider_id,probed_at);
CREATE INDEX idx_channel_probes_credential ON channel_probes(credential_id,probed_at);
CREATE TABLE channel_health_state (
    provider_id TEXT PRIMARY KEY REFERENCES providers(id) ON DELETE CASCADE,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    disabled_until INTEGER,
    backoff_until INTEGER,
    reason TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE webhooks (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    secret_envelope TEXT,
    headers_json TEXT NOT NULL DEFAULT '{"version":1}',
    body_template_json TEXT NOT NULL DEFAULT '{"version":1}',
    subscriptions_json TEXT NOT NULL DEFAULT '{"version":1,"events":[]}',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX idx_webhooks_project ON webhooks(project_id,enabled);
CREATE TABLE webhook_deliveries (
    id TEXT PRIMARY KEY,
    webhook_id TEXT NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL,
    response_status INTEGER,
    next_attempt_at INTEGER,
    created_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX idx_webhook_deliveries_pending ON webhook_deliveries(status,next_attempt_at);
CREATE INDEX idx_webhook_deliveries_webhook ON webhook_deliveries(webhook_id,created_at);

CREATE TABLE data_storage_configs (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('local','s3')),
    config_json TEXT NOT NULL DEFAULT '{"version":1}',
    secret_envelope TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX idx_data_storage_configs_project ON data_storage_configs(project_id);
CREATE TABLE data_retention_policies (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    resource_type TEXT NOT NULL,
    retention_days INTEGER NOT NULL CHECK(retention_days >= 0),
    retain_payloads INTEGER NOT NULL DEFAULT 0 CHECK(retain_payloads IN (0,1)),
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id,resource_type)
);
CREATE INDEX idx_data_retention_policies_project ON data_retention_policies(project_id);
CREATE TABLE backup_configs (
    id TEXT PRIMARY KEY,
    storage_id TEXT NOT NULL REFERENCES data_storage_configs(id) ON DELETE RESTRICT,
    schedule TEXT,
    retention_count INTEGER NOT NULL DEFAULT 7 CHECK(retention_count > 0),
    resources_json TEXT NOT NULL DEFAULT '{"version":1,"resources":[]}',
    conflict_strategy TEXT NOT NULL DEFAULT 'fail' CHECK(conflict_strategy IN ('fail','skip','overwrite')),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX idx_backup_configs_storage ON backup_configs(storage_id);
CREATE TABLE backup_runs (
    id TEXT PRIMARY KEY,
    config_id TEXT REFERENCES backup_configs(id) ON DELETE SET NULL,
    storage_id TEXT NOT NULL REFERENCES data_storage_configs(id) ON DELETE RESTRICT,
    status TEXT NOT NULL,
    object_key TEXT,
    manifest_json TEXT NOT NULL DEFAULT '{"version":1}',
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    error TEXT
);
CREATE INDEX idx_backup_runs_config ON backup_runs(config_id,started_at);
CREATE INDEX idx_backup_runs_storage ON backup_runs(storage_id,started_at);

INSERT INTO projects(id,name,slug,owner_user_id,is_default,enabled,created_at,updated_at)
VALUES('00000000-0000-0000-0000-000000000001','Default','default',NULL,1,1,unixepoch(),unixepoch());
INSERT INTO permissions(id,slug,level,description,created_at) VALUES
('00000000-0000-0000-0000-000000000020','*','system','All permissions',unixepoch()),
('00000000-0000-0000-0000-000000000021','project:manage','project','Manage project resources',unixepoch()),
('00000000-0000-0000-0000-000000000022','project:read','project','Read project resources',unixepoch()),
('00000000-0000-0000-0000-000000000023','gateway:use','project','Use gateway APIs',unixepoch());
INSERT INTO roles(id,project_id,name,scope,is_system,created_at,updated_at) VALUES
('00000000-0000-0000-0000-000000000010',NULL,'owner','system',1,unixepoch(),unixepoch()),
('00000000-0000-0000-0000-000000000011',NULL,'admin','system',1,unixepoch(),unixepoch()),
('00000000-0000-0000-0000-000000000012',NULL,'member','system',1,unixepoch(),unixepoch());
INSERT INTO role_permissions(role_id,permission_id,created_at) VALUES
('00000000-0000-0000-0000-000000000010','00000000-0000-0000-0000-000000000020',unixepoch()),
('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021',unixepoch()),
('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000022',unixepoch()),
('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000023',unixepoch()),
('00000000-0000-0000-0000-000000000012','00000000-0000-0000-0000-000000000022',unixepoch()),
('00000000-0000-0000-0000-000000000012','00000000-0000-0000-0000-000000000023',unixepoch());

"#;

#[derive(FromQueryResult)]
struct Count {
    count: i64,
}

fn statement(sql: impl Into<String>) -> Statement {
    Statement::from_string(DbBackend::Sqlite, sql)
}

pub async fn migrate(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(MIGRATION_LEDGER)
        .await
        .context("failed to initialize SQLite migration ledger")?;

    let applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=2",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if applied {
        return Ok(());
    }

    let experimental_v1 = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=1",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    let transaction = db.begin().await?;
    if experimental_v1 {
        transaction
            .execute_unprepared(EXPERIMENTAL_V1_RESET)
            .await
            .context("failed to reset the unreleased experimental v1 schema")?;
    }
    transaction
        .execute_unprepared(BASE_SCHEMA)
        .await
        .context("failed to initialize SQLite base schema")?;
    transaction
        .execute_unprepared(V2_SCHEMA)
        .await
        .context("failed to initialize SQLite v2 parity schema")?;
    transaction
        .execute(statement(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(2,unixepoch())",
        ))
        .await?;
    transaction.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::{Database, QueryResult};

    use super::*;

    async fn memory_database() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys=ON;")
            .await
            .unwrap();
        db
    }

    async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
        Count::find_by_statement(statement(sql))
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .count
    }

    async fn one(db: &DatabaseConnection, sql: &str) -> QueryResult {
        db.query_one(statement(sql)).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn fresh_database_has_v2_schema_seeds_foreign_keys_and_indexes() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();

        assert_eq!(
            scalar(&db, "SELECT MAX(version) AS count FROM schema_migrations").await,
            2
        );
        for table in [
            "projects",
            "project_memberships",
            "project_invitations",
            "roles",
            "permissions",
            "user_role_bindings",
            "oidc_providers",
            "oidc_identities",
            "api_key_profiles",
            "channel_credentials",
            "channel_settings",
            "model_associations",
            "model_prices",
            "prompts",
            "prompt_protection_rules",
            "threads",
            "traces",
            "requests",
            "request_executions",
            "usage_logs",
            "provider_quota_snapshots",
            "channel_probes",
            "webhooks",
            "data_storage_configs",
            "backup_configs",
            "backup_runs",
        ] {
            assert_eq!(
                scalar(
                    &db,
                    &format!(
                        "SELECT COUNT(*) AS count FROM sqlite_master WHERE type='table' AND name='{table}'"
                    )
                )
                .await,
                1,
                "missing table {table}"
            );
        }

        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_foreign_key_check"
            )
            .await,
            0
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_foreign_key_list('project_memberships')"
            )
            .await,
            3
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_foreign_key_list('request_executions')"
            )
            .await,
            3
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_foreign_key_list('backup_runs')"
            )
            .await,
            2
        );

        for index in [
            "idx_projects_one_default",
            "idx_project_memberships_user",
            "idx_oidc_identities_user",
            "idx_api_keys_project",
            "idx_channel_credentials_provider",
            "idx_model_associations_project",
            "idx_model_prices_model_schedule",
            "idx_traces_project_started",
            "idx_request_executions_provider",
            "idx_usage_logs_model",
            "idx_channel_probes_provider",
            "idx_webhook_deliveries_pending",
            "idx_backup_runs_storage",
        ] {
            assert_eq!(
                scalar(
                    &db,
                    &format!(
                        "SELECT COUNT(*) AS count FROM sqlite_master WHERE type='index' AND name='{index}'"
                    )
                )
                .await,
                1,
                "missing index {index}"
            );
        }

        let project = one(
            &db,
            "SELECT id,slug,owner_user_id FROM projects WHERE is_default=1",
        )
        .await;
        assert_eq!(
            project.try_get::<String>("", "id").unwrap(),
            DEFAULT_PROJECT_ID
        );
        assert_eq!(project.try_get::<String>("", "slug").unwrap(), "default");
        assert_eq!(
            project
                .try_get::<Option<String>>("", "owner_user_id")
                .unwrap(),
            None
        );
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) AS count FROM roles WHERE is_system=1").await,
            3
        );
    }

    #[tokio::test]
    async fn migration_is_idempotent() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();
        let table_count = scalar(
            &db,
            "SELECT COUNT(*) AS count FROM sqlite_master WHERE type='table'",
        )
        .await;
        let index_count = scalar(
            &db,
            "SELECT COUNT(*) AS count FROM sqlite_master WHERE type='index'",
        )
        .await;

        migrate(&db).await.unwrap();

        assert_eq!(
            scalar(&db, "SELECT COUNT(*) AS count FROM schema_migrations").await,
            1
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM sqlite_master WHERE type='table'"
            )
            .await,
            table_count
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM sqlite_master WHERE type='index'"
            )
            .await,
            index_count
        );
        assert_eq!(
            scalar(&db, "SELECT COUNT(*) AS count FROM projects").await,
            1
        );
        assert_eq!(scalar(&db, "SELECT COUNT(*) AS count FROM roles").await, 3);
    }
}
