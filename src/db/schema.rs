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
    kind TEXT NOT NULL CHECK(kind IN ('openai','openai_compatible','anthropic','gemini','azure','bedrock','vertex','gcp','openrouter','deepseek','moonshot','zhipu','doubao','xai','groq','ollama','nanogpt','jina')),
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
    lookup_digest TEXT NOT NULL UNIQUE,
    scopes TEXT NOT NULL DEFAULT '["gateway"]',
    budget_micros INTEGER,
    spent_micros INTEGER NOT NULL DEFAULT 0 CHECK(typeof(spent_micros)='integer' AND spent_micros>=0),
    enabled INTEGER NOT NULL DEFAULT 1,
    last_used_at INTEGER,
    created_at INTEGER NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id),
    user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    profile_id TEXT,
    key_type TEXT NOT NULL DEFAULT 'service' CHECK(key_type IN ('user','service','personal','no_auth')),
    expires_at INTEGER,
    allowed_ips_json TEXT NOT NULL DEFAULT '[]',
    denied_ips_json TEXT NOT NULL DEFAULT '[]',
    CHECK(key_type NOT IN ('user','personal') OR user_id IS NOT NULL),
    FOREIGN KEY(profile_id,project_id) REFERENCES api_key_profiles(id,project_id) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS idx_api_keys_prefix ON api_keys(key_prefix);
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_lookup_digest ON api_keys(lookup_digest);
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
    updated_at INTEGER NOT NULL,
    CHECK((scope='system' AND project_id IS NULL) OR (scope='project' AND project_id IS NOT NULL)),
    CHECK(is_system=0 OR (scope='system' AND project_id IS NULL))
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
CREATE UNIQUE INDEX idx_user_role_bindings_global_unique
ON user_role_bindings(user_id,role_id) WHERE project_id IS NULL;

CREATE TRIGGER project_memberships_role_scope_insert
BEFORE INSERT ON project_memberships
WHEN NOT EXISTS (
    SELECT 1 FROM roles r
    WHERE r.id=NEW.role_id AND (r.project_id IS NULL OR r.project_id=NEW.project_id)
)
BEGIN SELECT RAISE(ABORT,'membership role scope does not match project'); END;
CREATE TRIGGER project_memberships_role_scope_update
BEFORE UPDATE OF project_id,role_id ON project_memberships
WHEN NOT EXISTS (
    SELECT 1 FROM roles r
    WHERE r.id=NEW.role_id AND (r.project_id IS NULL OR r.project_id=NEW.project_id)
)
BEGIN SELECT RAISE(ABORT,'membership role scope does not match project'); END;

CREATE TRIGGER project_invitations_role_scope_insert
BEFORE INSERT ON project_invitations
WHEN NOT EXISTS (
    SELECT 1 FROM roles r
    WHERE r.id=NEW.role_id AND (r.project_id IS NULL OR r.project_id=NEW.project_id)
)
BEGIN SELECT RAISE(ABORT,'invitation role scope does not match project'); END;
CREATE TRIGGER project_invitations_role_scope_update
BEFORE UPDATE OF project_id,role_id ON project_invitations
WHEN NOT EXISTS (
    SELECT 1 FROM roles r
    WHERE r.id=NEW.role_id AND (r.project_id IS NULL OR r.project_id=NEW.project_id)
)
BEGIN SELECT RAISE(ABORT,'invitation role scope does not match project'); END;

CREATE TRIGGER user_role_bindings_scope_insert
BEFORE INSERT ON user_role_bindings
WHEN NOT EXISTS (
    SELECT 1 FROM roles r
    WHERE r.id=NEW.role_id AND (
        (NEW.project_id IS NULL AND r.project_id IS NULL)
        OR (NEW.project_id IS NOT NULL AND (r.project_id IS NULL OR r.project_id=NEW.project_id))
    )
)
BEGIN SELECT RAISE(ABORT,'role binding scope does not match project'); END;
CREATE TRIGGER user_role_bindings_scope_update
BEFORE UPDATE OF project_id,role_id ON user_role_bindings
WHEN NOT EXISTS (
    SELECT 1 FROM roles r
    WHERE r.id=NEW.role_id AND (
        (NEW.project_id IS NULL AND r.project_id IS NULL)
        OR (NEW.project_id IS NOT NULL AND (r.project_id IS NULL OR r.project_id=NEW.project_id))
    )
)
BEGIN SELECT RAISE(ABORT,'role binding scope does not match project'); END;

CREATE TRIGGER roles_scope_update
BEFORE UPDATE OF project_id,scope ON roles
WHEN (
    NEW.project_id IS NOT NULL AND EXISTS (
        SELECT 1 FROM project_memberships m
        WHERE m.role_id=OLD.id AND m.project_id<>NEW.project_id
    )
) OR (
    NEW.project_id IS NOT NULL AND EXISTS (
        SELECT 1 FROM project_invitations i
        WHERE i.role_id=OLD.id AND i.project_id<>NEW.project_id
    )
) OR EXISTS (
    SELECT 1 FROM user_role_bindings b
    WHERE b.role_id=OLD.id AND (
        (b.project_id IS NULL AND NEW.project_id IS NOT NULL)
        OR (b.project_id IS NOT NULL AND NEW.project_id IS NOT NULL AND b.project_id<>NEW.project_id)
    )
)
BEGIN SELECT RAISE(ABORT,'role scope update conflicts with existing assignments'); END;

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
    UNIQUE(project_id,name),
    UNIQUE(id,project_id)
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
CREATE UNIQUE INDEX idx_model_prices_global_version
ON model_prices(model_id,version) WHERE provider_id IS NULL;
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
    enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
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
    UNIQUE(project_id,api_key_id,external_id)
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
    execution_id TEXT NOT NULL UNIQUE REFERENCES execution_facts(id) ON DELETE CASCADE,
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
    sequence INTEGER NOT NULL DEFAULT 0 CHECK(sequence>=0),
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
CREATE INDEX idx_provider_quota_snapshots_sequence ON provider_quota_snapshots(provider_id,credential_id,sequence DESC,collected_at DESC,id);
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
    kind TEXT NOT NULL CHECK(kind IN ('local','s3','gcs','webdav')),
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
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id,resource_type)
);
CREATE INDEX idx_data_retention_policies_project ON data_retention_policies(project_id);
CREATE UNIQUE INDEX idx_data_retention_policies_global_resource
ON data_retention_policies(resource_type) WHERE project_id IS NULL;
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

const V3_ACCESS_SCHEMA: &str = r#"
ALTER TABLE users ADD COLUMN display_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN avatar_url TEXT;
ALTER TABLE users ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1));
ALTER TABLE users ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
UPDATE users SET display_name=email, updated_at=created_at WHERE updated_at=0;

CREATE TABLE oidc_auth_states (
    state_hash TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES oidc_providers(id) ON DELETE CASCADE,
    code_verifier_envelope TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_oidc_auth_states_expiry ON oidc_auth_states(expires_at);

INSERT INTO permissions(id,slug,level,description,created_at) VALUES
('00000000-0000-0000-0000-000000000024','user:manage','system','Manage users',unixepoch()),
('00000000-0000-0000-0000-000000000025','role:manage','project','Manage roles and assignments',unixepoch()),
('00000000-0000-0000-0000-000000000026','oidc:manage','system','Manage OIDC providers',unixepoch()),
('00000000-0000-0000-0000-000000000027','api_key:manage','project','Manage API keys',unixepoch());
INSERT INTO role_permissions(role_id,permission_id,created_at) VALUES
('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000024',unixepoch()),
('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000025',unixepoch()),
('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000026',unixepoch()),
('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000027',unixepoch());
"#;

const V4_OIDC_BROWSER_BINDING: &str = r#"
ALTER TABLE oidc_auth_states ADD COLUMN browser_binding_hash TEXT NOT NULL DEFAULT '';
"#;

const V5_RESPONSE_SESSIONS: &str = r#"
CREATE UNIQUE INDEX idx_api_keys_id_project ON api_keys(id,project_id);
CREATE TABLE response_sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    api_key_id TEXT NOT NULL,
    response_id TEXT NOT NULL,
    state_envelope TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1 CHECK(version=1),
    updated_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    UNIQUE(api_key_id,response_id),
    FOREIGN KEY(api_key_id,project_id) REFERENCES api_keys(id,project_id) ON DELETE CASCADE
);
CREATE INDEX idx_response_sessions_expiry ON response_sessions(expires_at);
CREATE INDEX idx_response_sessions_scope ON response_sessions(project_id,api_key_id,updated_at);
"#;

const V6_PROTOCOL_TASKS: &str = r#"
CREATE TABLE protocol_tasks (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    api_key_id TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    upstream_id TEXT NOT NULL,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL REFERENCES channel_credentials(id) ON DELETE CASCADE,
    upstream_model TEXT NOT NULL,
    public_model TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(api_key_id,endpoint,upstream_id),
    FOREIGN KEY(api_key_id,project_id) REFERENCES api_keys(id,project_id) ON DELETE CASCADE
);
CREATE INDEX idx_protocol_tasks_project ON protocol_tasks(project_id,api_key_id);
"#;

const V7_CATALOG: &str = r#"
CREATE TABLE catalog_sources (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 100,
    refresh_interval_secs INTEGER NOT NULL CHECK(refresh_interval_secs BETWEEN 60 AND 2592000),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    signature_policy TEXT NOT NULL CHECK(signature_policy IN ('none','optional','required')),
    public_key TEXT,
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    etag TEXT,
    last_modified TEXT,
    last_attempt_at INTEGER,
    last_success_at INTEGER,
    last_error TEXT,
    active_snapshot_id TEXT REFERENCES catalog_snapshots(id) ON DELETE SET NULL,
    previous_snapshot_id TEXT REFERENCES catalog_snapshots(id) ON DELETE SET NULL,
    CHECK(signature_policy!='required' OR public_key IS NOT NULL)
);
CREATE TABLE catalog_snapshots (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES catalog_sources(id) ON DELETE CASCADE,
    version TEXT NOT NULL,
    document_json TEXT NOT NULL CHECK(json_valid(document_json)),
    digest TEXT NOT NULL,
    source_url TEXT NOT NULL,
    signature_verified INTEGER NOT NULL CHECK(signature_verified IN (0,1)),
    signing_key TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_catalog_snapshots_source ON catalog_snapshots(source_id,created_at);
CREATE TABLE catalog_overrides (
    kind TEXT NOT NULL CHECK(kind IN ('provider','model')),
    entry_id TEXT NOT NULL,
    entry_json TEXT NOT NULL CHECK(json_valid(entry_json)),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(kind,entry_id)
);
CREATE TABLE catalog_document_extensions (
    extension_key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL CHECK(json_valid(value_json)),
    updated_at INTEGER NOT NULL
);
ALTER TABLE models ADD COLUMN catalog_metadata_json TEXT NOT NULL DEFAULT '{}';
INSERT INTO permissions(id,slug,level,description,created_at) VALUES('00000000-0000-0000-0000-000000000028','catalog:manage','system','Manage global catalog sources and overrides',unixepoch());
INSERT INTO role_permissions(role_id,permission_id,created_at) VALUES('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000028',unixepoch());
"#;

/// The durable outcome snapshot of an upstream attempt. The live observation event
/// always knew the exact status, the channel's display name and the terminal
/// classification; without these columns the projection could only be rebuilt by
/// inventing `200`, `502` and a provider UUID. Existing rows stay NULL: a status
/// nobody recorded is not a status.
const V9_EXECUTION_OUTCOME_SNAPSHOT: &str = r#"
ALTER TABLE request_executions ADD COLUMN provider_name TEXT;
ALTER TABLE request_executions ADD COLUMN http_status INTEGER;
ALTER TABLE request_executions ADD COLUMN error_kind TEXT;
"#;

/// The trace lifecycle is a durable, constrained value of its own.
///
/// `traces.status` is the execution outcome — what the run did — and it stays
/// authoritative; overloading it with archive/pin state would have made
/// "succeeded and archived" unrepresentable. Every row that predates this column
/// is active, which is what it already was, and the index serves the list's own
/// default predicate (`lifecycle!='archived'`) under the project scope.
const V10_TRACE_LIFECYCLE: &str = r#"
ALTER TABLE traces ADD COLUMN lifecycle TEXT NOT NULL DEFAULT 'active' CHECK(lifecycle IN ('active','archived','retained'));
CREATE INDEX idx_traces_project_lifecycle ON traces(project_id,lifecycle);
"#;

/// Prompt placement is logically additive and backwards compatible. SQLite
/// cannot alter a column default in place, so the transaction copies the table:
/// existing prompts keep their enabled bit and acquire the behaviour Pangolin
/// previously hard-coded (order zero and prepend), while every upgraded database
/// also receives the safe disabled default for future inserts.
const V11_PROMPT_PLACEMENT: &str = r#"
ALTER TABLE prompts RENAME TO prompts_v10;
CREATE TABLE prompts (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    activation_json TEXT NOT NULL DEFAULT '{"version":1}',
    enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
    "order" INTEGER NOT NULL DEFAULT 0,
    action TEXT NOT NULL DEFAULT 'prepend' CHECK(action IN ('prepend','append')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id,name)
);
INSERT INTO prompts(id,project_id,name,role,content,activation_json,enabled,"order",action,created_at,updated_at)
SELECT id,project_id,name,role,content,activation_json,enabled,0,'prepend',created_at,updated_at FROM prompts_v10;
DROP TABLE prompts_v10;
"#;

/// Human-readable context and archive state are independent of the existing
/// enable switch. Every rule predating this migration was enforceable when
/// enabled, so the additive default preserves it as active.
const V12_PROTECTION_METADATA: &str = r#"
ALTER TABLE prompt_protection_rules ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE prompt_protection_rules ADD COLUMN state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','archived'));
"#;

/// A self-service link callback must use the user captured when the flow
/// started. Keeping both the intent and user on the one-time state prevents a
/// callback from accepting either value from the browser or a later session.
const V13_OIDC_LINK_STATE: &str = r#"
ALTER TABLE oidc_auth_states ADD COLUMN intent TEXT NOT NULL DEFAULT 'login' CHECK(intent IN ('login','link'));
ALTER TABLE oidc_auth_states ADD COLUMN link_user_id TEXT REFERENCES users(id) ON DELETE CASCADE;
"#;

/// API-key archival is durable lifecycle state, distinct from the reversible
/// enable switch. The default keeps every pre-existing key active.
const V14_API_KEY_LIFECYCLE: &str = r#"
ALTER TABLE api_keys ADD COLUMN lifecycle TEXT NOT NULL DEFAULT 'active' CHECK(lifecycle IN ('active','archived'));
ALTER TABLE api_keys ADD COLUMN archived_at INTEGER;
CREATE INDEX idx_api_keys_project_lifecycle ON api_keys(project_id,lifecycle);
"#;

/// Model lifecycle is independent from enablement. Existing rows remain active;
/// archive is an explicit control-plane operation rather than another spelling
/// of disabled.
const V15_MODEL_LIFECYCLE: &str = r#"
ALTER TABLE models ADD COLUMN lifecycle TEXT NOT NULL DEFAULT 'active' CHECK(lifecycle IN ('active','archived'));
CREATE INDEX idx_models_project_lifecycle ON models(provider_id,lifecycle,enabled);
"#;

/// AxonHub calls its branding field `icon_url`. Pangolin deliberately stores a
/// bundled catalog key instead: login discovery must never turn an operator
/// value into a browser-side remote image request.
const V16_OIDC_LOGIN_POLICY: &str = r#"
ALTER TABLE oidc_providers ADD COLUMN login_only INTEGER NOT NULL DEFAULT 0 CHECK(login_only IN (0,1));
ALTER TABLE oidc_providers ADD COLUMN display_name TEXT NOT NULL DEFAULT '';
ALTER TABLE oidc_providers ADD COLUMN button_color TEXT CHECK(button_color IS NULL OR (length(button_color)=7 AND substr(button_color,1,1)='#' AND substr(button_color,2) NOT GLOB '*[^0-9A-Fa-f]*'));
ALTER TABLE oidc_providers ADD COLUMN logo_key TEXT;
"#;

const V17_API_KEY_PROFILE_TEMPLATES: &str = r#"
CREATE TABLE api_key_profile_templates (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL COLLATE NOCASE CHECK(length(name) BETWEEN 1 AND 128),
    document_json TEXT NOT NULL CHECK(length(document_json) <= 65536 AND json_valid(document_json)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id,name)
);
CREATE INDEX idx_api_key_profile_templates_project ON api_key_profile_templates(project_id,updated_at);
"#;

/// Invitation reuse stays finite and preserves the existing email-bound token.
/// Existing accepted invitations remain exhausted; pending invitations retain
/// the former single-use behavior.
const V18_INVITATION_REUSE: &str = r#"
ALTER TABLE project_invitations ADD COLUMN max_uses INTEGER NOT NULL DEFAULT 1 CHECK(max_uses BETWEEN 1 AND 100);
ALTER TABLE project_invitations ADD COLUMN use_count INTEGER NOT NULL DEFAULT 0 CHECK(use_count BETWEEN 0 AND max_uses);
UPDATE project_invitations SET use_count=1 WHERE accepted_at IS NOT NULL;
"#;

const V20_CHANNEL_PROXY_AND_MODEL_SYNC: &str = r#"
ALTER TABLE channel_settings ADD COLUMN proxy_url TEXT;
ALTER TABLE channel_settings ADD COLUMN proxy_username TEXT;
ALTER TABLE channel_settings ADD COLUMN proxy_secret_envelope TEXT;
ALTER TABLE channel_settings ADD COLUMN proxy_reuse_connections INTEGER NOT NULL DEFAULT 1 CHECK(proxy_reuse_connections IN (0,1));
ALTER TABLE channel_settings ADD COLUMN model_sync_error TEXT;
ALTER TABLE channel_settings ADD COLUMN model_synced_at INTEGER;
ALTER TABLE channel_settings ADD COLUMN model_sync_count INTEGER;
ALTER TABLE models ADD COLUMN discovery_managed INTEGER NOT NULL DEFAULT 0 CHECK(discovery_managed IN (0,1));
CREATE INDEX idx_models_discovery_managed ON models(provider_id,discovery_managed,lifecycle);
"#;

// Versions 19–21 are reserved by parallel parity batches. B50–B53 own v22.
// Rebuilding the storage table is required because SQLite cannot widen a CHECK
// constraint in place. `legacy_alter_table` keeps child foreign keys pointed at
// the canonical table name while the old table is moved aside.
const V22_STORAGE_DIAGNOSTICS_PROXY: &str = r#"
PRAGMA legacy_alter_table=ON;
ALTER TABLE backup_runs RENAME TO backup_runs_v21;
ALTER TABLE backup_configs RENAME TO backup_configs_v21;
ALTER TABLE data_storage_configs RENAME TO data_storage_configs_v21;
CREATE TABLE data_storage_configs (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('local','s3','gcs','webdav')),
    config_json TEXT NOT NULL DEFAULT '{"version":1}',
    secret_envelope TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
INSERT INTO data_storage_configs(
    id,project_id,name,kind,config_json,secret_envelope,enabled,revision,created_at,updated_at
)
SELECT id,project_id,name,kind,config_json,secret_envelope,enabled,revision,created_at,updated_at
FROM data_storage_configs_v21;
CREATE TABLE backup_configs (
    id TEXT PRIMARY KEY,
    storage_id TEXT NOT NULL REFERENCES data_storage_configs(id) ON DELETE RESTRICT,
    schedule TEXT,
    retention_count INTEGER NOT NULL DEFAULT 7 CHECK(retention_count > 0),
    resources_json TEXT NOT NULL DEFAULT '{"version":1,"resources":[]}',
    conflict_strategy TEXT NOT NULL DEFAULT 'fail' CHECK(conflict_strategy IN ('fail','skip','overwrite')),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1
);
INSERT INTO backup_configs(
    id,storage_id,schedule,retention_count,resources_json,conflict_strategy,enabled,created_at,updated_at,revision
)
SELECT id,storage_id,schedule,retention_count,resources_json,conflict_strategy,enabled,created_at,updated_at,revision
FROM backup_configs_v21;
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
INSERT INTO backup_runs(
    id,config_id,storage_id,status,object_key,manifest_json,started_at,finished_at,error
)
SELECT id,config_id,storage_id,status,object_key,manifest_json,started_at,finished_at,error
FROM backup_runs_v21;
DROP TABLE backup_runs_v21;
DROP TABLE backup_configs_v21;
DROP TABLE data_storage_configs_v21;
CREATE INDEX idx_data_storage_configs_project ON data_storage_configs(project_id);
CREATE INDEX idx_backup_configs_storage ON backup_configs(storage_id);
CREATE INDEX idx_backup_runs_config ON backup_runs(config_id,started_at);
CREATE INDEX idx_backup_runs_storage ON backup_runs(storage_id,started_at);
PRAGMA legacy_alter_table=OFF;

CREATE TABLE proxy_presets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    url TEXT NOT NULL,
    secret_envelope TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
ALTER TABLE channel_settings ADD COLUMN proxy_preset_id TEXT REFERENCES proxy_presets(id) ON DELETE SET NULL;
ALTER TABLE webhooks ADD COLUMN timeout_secs INTEGER NOT NULL DEFAULT 20 CHECK(timeout_secs BETWEEN 1 AND 300);
ALTER TABLE webhooks ADD COLUMN proxy_preset_id TEXT REFERENCES proxy_presets(id) ON DELETE SET NULL;
ALTER TABLE catalog_sources ADD COLUMN proxy_preset_id TEXT REFERENCES proxy_presets(id) ON DELETE SET NULL;
"#;

const V23_CREDENTIAL_RECOVERY_AND_DEVELOPER_SETTINGS: &str = r#"
ALTER TABLE models ADD COLUMN disable_developer_settings_inheritance INTEGER NOT NULL DEFAULT 0 CHECK(disable_developer_settings_inheritance IN (0,1));
CREATE TABLE credential_recovery_tokens (
    token_hash TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL REFERENCES channel_credentials(id) ON DELETE CASCADE,
    created_by TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_credential_recovery_tokens_credential ON credential_recovery_tokens(project_id,credential_id,expires_at);
"#;

/// Associations gain a tag-regex selector and a versioned channel-exclusion
/// document. Version 24 is reserved here because 18–23 belong to parallel
/// parity migrations.
const V24_ASSOCIATION_FILTERS: &str = r#"
ALTER TABLE model_associations RENAME TO model_associations_v23;
CREATE TABLE model_associations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    model_id TEXT REFERENCES models(id) ON DELETE CASCADE,
    provider_id TEXT REFERENCES providers(id) ON DELETE CASCADE,
    match_type TEXT NOT NULL CHECK(match_type IN ('exact','regex','tag','channel_tags_regex')),
    pattern TEXT NOT NULL,
    conditions_json TEXT NOT NULL DEFAULT '{"version":1}',
    exclusions_json TEXT NOT NULL DEFAULT '{"version":1}',
    priority INTEGER NOT NULL DEFAULT 100,
    weight INTEGER NOT NULL DEFAULT 1 CHECK(weight > 0),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
INSERT INTO model_associations(id,project_id,model_id,provider_id,match_type,pattern,conditions_json,exclusions_json,priority,weight,enabled,created_at,updated_at)
SELECT id,project_id,model_id,provider_id,match_type,pattern,conditions_json,'{"version":1}',priority,weight,enabled,created_at,updated_at FROM model_associations_v23;
DROP TABLE model_associations_v23;
CREATE INDEX idx_model_associations_project ON model_associations(project_id,enabled,priority);
CREATE INDEX idx_model_associations_model ON model_associations(model_id);
CREATE INDEX idx_model_associations_provider ON model_associations(provider_id);
"#;

const V25_PROVIDER_OAUTH_STATES: &str = r#"
CREATE TABLE provider_oauth_states (
    state_digest TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    flow TEXT NOT NULL CHECK(flow IN ('codex','xai','claude_code','antigravity','github_copilot')),
    client_id TEXT NOT NULL,
    redirect_uri TEXT,
    secret_envelope TEXT NOT NULL,
    interval_seconds INTEGER NOT NULL DEFAULT 0 CHECK(interval_seconds BETWEEN 0 AND 60),
    last_poll_at INTEGER,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_provider_oauth_states_expiry ON provider_oauth_states(expires_at);
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
    if !applied {
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
    }

    let access_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=3",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !access_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V3_ACCESS_SCHEMA)
            .await
            .context("failed to initialize SQLite access-control schema")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(3,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let oidc_binding_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=4",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !oidc_binding_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V4_OIDC_BROWSER_BINDING)
            .await
            .context("failed to initialize OIDC browser binding")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(4,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let sessions_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=5",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !sessions_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V5_RESPONSE_SESSIONS)
            .await
            .context("failed to initialize Responses session storage")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(5,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let tasks_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=6",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !tasks_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V6_PROTOCOL_TASKS)
            .await
            .context("failed to initialize protocol task ownership")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(6,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let catalog_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=7",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !catalog_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V7_CATALOG)
            .await
            .context("failed to initialize catalog storage")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(7,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let operations_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=8",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !operations_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(include_str!("../operations/schema.sql"))
            .await
            .context("failed to initialize authoritative operations schema")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(8,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let snapshot_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=9",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !snapshot_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V9_EXECUTION_OUTCOME_SNAPSHOT)
            .await
            .context("failed to add the execution outcome snapshot columns")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(9,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let lifecycle_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=10",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !lifecycle_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V10_TRACE_LIFECYCLE)
            .await
            .context("failed to add the trace lifecycle state")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(10,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let prompt_placement_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=11",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !prompt_placement_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V11_PROMPT_PLACEMENT)
            .await
            .context("failed to add prompt placement policy")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(11,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let protection_metadata_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=12",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !protection_metadata_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V12_PROTECTION_METADATA)
            .await
            .context("failed to add protection rule metadata")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(12,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let oidc_link_state_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=13",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !oidc_link_state_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V13_OIDC_LINK_STATE)
            .await
            .context("failed to add captured OIDC link intent")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(13,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let api_key_lifecycle_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=14",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !api_key_lifecycle_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V14_API_KEY_LIFECYCLE)
            .await
            .context("failed to add the API-key lifecycle state")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(14,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let model_lifecycle_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=15",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !model_lifecycle_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V15_MODEL_LIFECYCLE)
            .await
            .context("failed to add model lifecycle state")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(15,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let oidc_login_policy_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=16",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !oidc_login_policy_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V16_OIDC_LOGIN_POLICY)
            .await
            .context("failed to add OIDC login policy and safe branding")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(16,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let profile_templates_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=17",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !profile_templates_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V17_API_KEY_PROFILE_TEMPLATES)
            .await
            .context("failed to add API-key profile templates")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(17,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let invitation_reuse_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=18",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !invitation_reuse_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V18_INVITATION_REUSE)
            .await
            .context("failed to add bounded invitation reuse")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(18,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let channel_proxy_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=20",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !channel_proxy_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V20_CHANNEL_PROXY_AND_MODEL_SYNC)
            .await
            .context("failed to add channel proxy and model-sync state")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(20,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let storage_diagnostics_proxy_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=22",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !storage_diagnostics_proxy_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V22_STORAGE_DIAGNOSTICS_PROXY)
            .await
            .context("failed to add storage, diagnostics and outbound proxy settings")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(22,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let recovery_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=23",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !recovery_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V23_CREDENTIAL_RECOVERY_AND_DEVELOPER_SETTINGS)
            .await
            .context("failed to add credential recovery and developer settings state")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(23,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let association_filters_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=24",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !association_filters_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V24_ASSOCIATION_FILTERS)
            .await
            .context("failed to add association tag regex and exclusions")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(24,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
    let provider_oauth_applied = Count::find_by_statement(statement(
        "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=25",
    ))
    .one(db)
    .await?
    .is_some_and(|row| row.count > 0);
    if !provider_oauth_applied {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(V25_PROVIDER_OAUTH_STATES)
            .await
            .context("failed to add provider OAuth state storage")?;
        transaction
            .execute(statement(
                "INSERT INTO schema_migrations(version,applied_at) VALUES(25,unixepoch())",
            ))
            .await?;
        transaction.commit().await?;
    }
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
            25
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('project_invitations') WHERE name IN ('max_uses','use_count')"
            )
            .await,
            2
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('channel_settings') WHERE name IN ('proxy_url','proxy_username','proxy_secret_envelope','proxy_reuse_connections','model_sync_error','model_synced_at','model_sync_count')"
            )
            .await,
            7
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('models') WHERE name='discovery_managed' AND dflt_value='0'"
            )
            .await,
            1
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('model_associations') WHERE name='exclusions_json' AND dflt_value='''{\"version\":1}'''"
            )
            .await,
            1
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM sqlite_master WHERE type='table' AND name='model_associations' AND sql LIKE '%channel_tags_regex%'"
            )
            .await,
            1
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('models') WHERE name='lifecycle' AND dflt_value=\"'active'\""
            )
            .await,
            1
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
            "oidc_auth_states",
            "api_key_profiles",
            "api_key_profile_templates",
            "response_sessions",
            "protocol_tasks",
            "catalog_sources",
            "catalog_snapshots",
            "catalog_overrides",
            "catalog_document_extensions",
            "channel_credentials",
            "credential_recovery_tokens",
            "provider_oauth_states",
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
            "proxy_presets",
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

        for (table, column) in [
            ("channel_settings", "proxy_preset_id"),
            ("webhooks", "timeout_secs"),
            ("webhooks", "proxy_preset_id"),
            ("catalog_sources", "proxy_preset_id"),
        ] {
            assert_eq!(
                scalar(
                    &db,
                    &format!(
                        "SELECT COUNT(*) AS count FROM pragma_table_info('{table}') WHERE name='{column}'"
                    )
                )
                .await,
                1,
                "missing {table}.{column}"
            );
        }

        for kind in ["gcs", "webdav"] {
            db.execute(statement(&format!(
                "INSERT INTO data_storage_configs(id,name,kind,created_at,updated_at) VALUES('{kind}','{kind}','{kind}',0,0)"
            )))
            .await
            .unwrap();
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
            "idx_api_keys_lookup_digest",
            "idx_response_sessions_expiry",
            "idx_user_role_bindings_global_unique",
            "idx_channel_credentials_provider",
            "idx_provider_oauth_states_expiry",
            "idx_model_associations_project",
            "idx_model_prices_model_schedule",
            "idx_model_prices_global_version",
            "idx_traces_project_started",
            "idx_request_executions_provider",
            "idx_usage_logs_model",
            "idx_channel_probes_provider",
            "idx_webhook_deliveries_pending",
            "idx_data_retention_policies_global_resource",
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

    /// The outcome snapshot is additive and nullable: an existing instance gains the
    /// columns on startup without losing a row, and nothing is backfilled with a
    /// status or a name that was never recorded.
    #[tokio::test]
    async fn the_execution_outcome_snapshot_is_additive_and_nullable() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();
        for column in ["provider_name", "http_status", "error_kind"] {
            assert_eq!(
                scalar(
                    &db,
                    &format!(
                        "SELECT COUNT(*) AS count FROM pragma_table_info('request_executions') WHERE name='{column}' AND \"notnull\"=0"
                    )
                )
                .await,
                1,
                "request_executions.{column} must exist and be nullable"
            );
        }
        // A second initialization adds nothing and keeps the ledger at one row per
        // applied version.
        migrate(&db).await.unwrap();
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('request_executions') WHERE name='http_status'"
            )
            .await,
            1
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=9"
            )
            .await,
            1
        );

        // An instance initialized before this migration: a row already exists, the
        // columns do not, and the ledger has no version 9. Re-initializing must add
        // the columns and leave the row unmeasured rather than guessing a status.
        db.execute_unprepared(
            "PRAGMA foreign_keys=OFF;
             INSERT INTO request_executions(id,request_id,attempt,model,status,started_at) VALUES('legacy','legacy-request',1,'m','succeeded',0);
             ALTER TABLE request_executions DROP COLUMN provider_name;
             ALTER TABLE request_executions DROP COLUMN http_status;
             ALTER TABLE request_executions DROP COLUMN error_kind;
             DELETE FROM schema_migrations WHERE version=9;
             PRAGMA foreign_keys=ON;",
        )
        .await
        .unwrap();
        migrate(&db).await.unwrap();
        let legacy = one(
            &db,
            "SELECT http_status,provider_name,error_kind FROM request_executions WHERE id='legacy'",
        )
        .await;
        assert!(
            legacy
                .try_get::<Option<i64>>("", "http_status")
                .unwrap()
                .is_none()
        );
        assert!(
            legacy
                .try_get::<Option<String>>("", "provider_name")
                .unwrap()
                .is_none()
        );
        assert!(
            legacy
                .try_get::<Option<String>>("", "error_kind")
                .unwrap()
                .is_none()
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
            22
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

    #[tokio::test]
    async fn api_key_owner_and_profile_deletion_fail_closed() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();
        db.execute_unprepared(
            r#"
            INSERT INTO users(id,email,password_hash,role,language,theme,created_at)
            VALUES('user-delete','delete@example.com','hash','member','en','system:bronze',1);
            INSERT INTO api_key_profiles(id,project_id,name,created_at,updated_at)
            VALUES('profile-delete','00000000-0000-0000-0000-000000000001','restricted',1,1);
            INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,created_at,project_id,user_id,key_type)
            VALUES('personal-key','personal','personal','hash','personal-digest',1,'00000000-0000-0000-0000-000000000001','user-delete','personal');
            INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,created_at,project_id,profile_id)
            VALUES('profile-key','profiled','profiled','hash','profile-digest',1,'00000000-0000-0000-0000-000000000001','profile-delete');
            "#,
        )
        .await
        .unwrap();

        assert!(
            db.execute_unprepared("UPDATE api_keys SET user_id=NULL WHERE id='personal-key'")
                .await
                .is_err(),
            "personal-key updates must not remove their owner"
        );

        db.execute_unprepared("DELETE FROM users WHERE id='user-delete'")
            .await
            .unwrap();
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM api_keys WHERE id='personal-key'"
            )
            .await,
            0,
            "deleting a personal-key owner must revoke the key"
        );

        assert!(
            db.execute_unprepared("DELETE FROM api_key_profiles WHERE id='profile-delete'")
                .await
                .is_err(),
            "a profile still used by an API key must not be deleted"
        );
        assert_eq!(
            one(
                &db,
                "SELECT profile_id FROM api_keys WHERE id='profile-key'"
            )
            .await
            .try_get::<String>("", "profile_id")
            .unwrap(),
            "profile-delete"
        );
    }

    #[tokio::test]
    async fn role_scope_constraints_reject_cross_project_inserts_and_updates() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();
        db.execute_unprepared(
            r#"
            INSERT INTO users(id,email,password_hash,role,language,theme,created_at)
            VALUES('scope-user','scope@example.com','hash','member','en','system:bronze',1);
            INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at)
            VALUES('project-b','Project B','project-b',0,1,1,1);
            INSERT INTO roles(id,project_id,name,scope,is_system,created_at,updated_at) VALUES
            ('role-a','00000000-0000-0000-0000-000000000001','role-a','project',0,1,1),
            ('role-b','project-b','role-b','project',0,1,1);
            "#,
        )
        .await
        .unwrap();

        assert!(
            db.execute_unprepared(
                "INSERT INTO project_memberships(id,project_id,user_id,role_id,created_at,updated_at) VALUES('bad-membership','00000000-0000-0000-0000-000000000001','scope-user','role-b',1,1)"
            )
            .await
            .is_err(),
            "a membership must not reference another project's role"
        );
        db.execute_unprepared(
            "INSERT INTO project_memberships(id,project_id,user_id,role_id,created_at,updated_at) VALUES('membership-a','00000000-0000-0000-0000-000000000001','scope-user','role-a',1,1)",
        )
        .await
        .unwrap();
        assert!(
            db.execute_unprepared(
                "UPDATE project_memberships SET project_id='project-b' WHERE id='membership-a'"
            )
            .await
            .is_err(),
            "membership updates must preserve role scope"
        );

        assert!(
            db.execute_unprepared(
                "INSERT INTO project_invitations(id,project_id,email,role_id,token_hash,expires_at,created_at) VALUES('bad-invite','00000000-0000-0000-0000-000000000001','invite@example.com','role-b','bad-token',100,1)"
            )
            .await
            .is_err(),
            "an invitation must not reference another project's role"
        );
        db.execute_unprepared(
            "INSERT INTO project_invitations(id,project_id,email,role_id,token_hash,expires_at,created_at) VALUES('invite-a','00000000-0000-0000-0000-000000000001','invite@example.com','role-a','good-token',100,1)",
        )
        .await
        .unwrap();
        assert!(
            db.execute_unprepared(
                "UPDATE project_invitations SET project_id='project-b' WHERE id='invite-a'"
            )
            .await
            .is_err(),
            "invitation updates must preserve role scope"
        );
        assert!(
            db.execute_unprepared("UPDATE roles SET project_id='project-b' WHERE id='role-a'")
                .await
                .is_err(),
            "role updates must preserve existing assignment scopes"
        );
    }

    #[tokio::test]
    async fn role_binding_scope_constraints_reject_invalid_inserts_and_updates() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();
        db.execute_unprepared(
            r#"
            INSERT INTO users(id,email,password_hash,role,language,theme,created_at)
            VALUES('binding-user','binding@example.com','hash','member','en','system:bronze',1);
            INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at)
            VALUES('project-b','Project B','project-b',0,1,1,1);
            INSERT INTO roles(id,project_id,name,scope,is_system,created_at,updated_at) VALUES
            ('role-a','00000000-0000-0000-0000-000000000001','role-a','project',0,1,1),
            ('role-b','project-b','role-b','project',0,1,1);
            "#,
        )
        .await
        .unwrap();

        assert!(
            db.execute_unprepared(
                "INSERT INTO user_role_bindings(id,user_id,role_id,project_id,created_at) VALUES('bad-global','binding-user','role-a',NULL,1)"
            )
            .await
            .is_err(),
            "a project role must not receive a global binding"
        );
        assert!(
            db.execute_unprepared(
                "INSERT INTO user_role_bindings(id,user_id,role_id,project_id,created_at) VALUES('bad-project','binding-user','role-b','00000000-0000-0000-0000-000000000001',1)"
            )
            .await
            .is_err(),
            "a binding must not reference another project's role"
        );
        db.execute_unprepared(
            "INSERT INTO user_role_bindings(id,user_id,role_id,project_id,created_at) VALUES('binding-a','binding-user','role-a','00000000-0000-0000-0000-000000000001',1)",
        )
        .await
        .unwrap();
        assert!(
            db.execute_unprepared(
                "UPDATE user_role_bindings SET project_id=NULL WHERE id='binding-a'"
            )
            .await
            .is_err(),
            "binding updates must not globalize project roles"
        );
        assert!(
            db.execute_unprepared(
                "UPDATE user_role_bindings SET project_id='project-b' WHERE id='binding-a'"
            )
            .await
            .is_err(),
            "binding updates must preserve project role scope"
        );
    }

    #[tokio::test]
    async fn api_keys_reject_cross_project_profiles_on_insert_and_update() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();
        db.execute_unprepared(
            r#"
            INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at)
            VALUES('project-b','Project B','project-b',0,1,1,1);
            INSERT INTO api_key_profiles(id,project_id,name,created_at,updated_at) VALUES
            ('profile-a','00000000-0000-0000-0000-000000000001','profile-a',1,1),
            ('profile-b','project-b','profile-b',1,1);
            "#,
        )
        .await
        .unwrap();

        assert!(
            db.execute_unprepared(
                "INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,created_at,project_id,profile_id) VALUES('bad-key','bad','bad-key','hash','bad-digest',1,'00000000-0000-0000-0000-000000000001','profile-b')"
            )
            .await
            .is_err(),
            "an API key must not reference another project's profile"
        );
        db.execute_unprepared(
            "INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,created_at,project_id,profile_id) VALUES('key-a','key-a','key-a','hash','key-a-digest',1,'00000000-0000-0000-0000-000000000001','profile-a')",
        )
        .await
        .unwrap();
        assert!(
            db.execute_unprepared("UPDATE api_keys SET profile_id='profile-b' WHERE id='key-a'")
                .await
                .is_err(),
            "API-key profile updates must preserve project scope"
        );
        assert!(
            db.execute_unprepared("UPDATE api_keys SET project_id='project-b' WHERE id='key-a'")
                .await
                .is_err(),
            "API-key project updates must preserve profile scope"
        );
        assert!(
            db.execute_unprepared(
                "UPDATE api_key_profiles SET project_id='project-b' WHERE id='profile-a'"
            )
            .await
            .is_err(),
            "profile project updates must preserve API-key scope"
        );
    }

    #[tokio::test]
    async fn null_scoped_uniqueness_is_enforced() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();
        db.execute_unprepared(
            r#"
            INSERT INTO users(id,email,password_hash,role,language,theme,created_at)
            VALUES('unique-user','unique@example.com','hash','member','en','system:bronze',1);
            INSERT INTO providers(id,name,kind,base_url,enabled,created_at,updated_at,project_id)
            VALUES('price-provider','Price Provider','openai','https://example.test',1,1,1,'00000000-0000-0000-0000-000000000001');
            INSERT INTO models(id,provider_id,public_name,upstream_name,created_at)
            VALUES('price-model','price-provider','price-model','price-model',1);
            INSERT INTO model_prices(id,model_id,provider_id,version,valid_from,created_at)
            VALUES('global-price-1','price-model',NULL,1,1,1);
            INSERT INTO user_role_bindings(id,user_id,role_id,project_id,created_at)
            VALUES('global-binding-1','unique-user','00000000-0000-0000-0000-000000000012',NULL,1);
            INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at)
            VALUES('global-retention-1',NULL,'request_body',30,1);
            "#,
        )
        .await
        .unwrap();

        assert!(
            db.execute_unprepared(
                "INSERT INTO model_prices(id,model_id,provider_id,version,valid_from,created_at) VALUES('global-price-2','price-model',NULL,1,2,2)"
            )
            .await
            .is_err(),
            "global model-price versions must be unique"
        );
        assert!(
            db.execute_unprepared(
                "INSERT INTO user_role_bindings(id,user_id,role_id,project_id,created_at) VALUES('global-binding-2','unique-user','00000000-0000-0000-0000-000000000012',NULL,2)"
            )
            .await
            .is_err(),
            "global user-role bindings must be unique"
        );
        assert!(
            db.execute_unprepared(
                "INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES('global-retention-2',NULL,'request_body',90,2)"
            )
            .await
            .is_err(),
            "global retention policies must be unique per resource"
        );
    }

    /// The trace lifecycle is its own durable, constrained value: the execution
    /// outcome in `traces.status` stays authoritative and is never overloaded with
    /// archive/pin state, a database that predates the column gains it with every
    /// existing row active, and a second initialization records version 10 exactly
    /// once instead of re-running the `ALTER TABLE`.
    #[tokio::test]
    async fn the_trace_lifecycle_state_is_additive_constrained_and_recorded_once() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();

        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('traces') WHERE name='lifecycle' AND \"notnull\"=1"
            )
            .await,
            1,
            "traces.lifecycle must exist and be NOT NULL"
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('traces') WHERE name='lifecycle' AND dflt_value='''active'''"
            )
            .await,
            1,
            "existing rows must become active, so the default has to be 'active'"
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=10"
            )
            .await,
            1
        );

        db.execute_unprepared(
            "INSERT INTO traces(id,project_id,status,started_at) VALUES('fresh-trace','00000000-0000-0000-0000-000000000001','succeeded',1)",
        )
        .await
        .unwrap();
        let fresh = one(
            &db,
            "SELECT lifecycle,status FROM traces WHERE id='fresh-trace'",
        )
        .await;
        assert_eq!(
            fresh.try_get::<String>("", "lifecycle").unwrap(),
            "active",
            "a new trace starts active"
        );
        assert_eq!(
            fresh.try_get::<String>("", "status").unwrap(),
            "succeeded",
            "the execution outcome is a different fact and must be untouched"
        );
        for invalid in ["'deleted'", "'ACTIVE'", "''", "NULL"] {
            assert!(
                db.execute_unprepared(&format!(
                    "UPDATE traces SET lifecycle={invalid} WHERE id='fresh-trace'"
                ))
                .await
                .is_err(),
                "the lifecycle constraint must reject {invalid}"
            );
        }

        // A database initialized before this migration: the column and the index
        // do not exist, the ledger has no version 10, and a row is already there.
        db.execute_unprepared(
            "PRAGMA foreign_keys=OFF;
             INSERT INTO traces(id,project_id,status,started_at) VALUES('legacy-trace','00000000-0000-0000-0000-000000000001','failed',1);
             DROP INDEX IF EXISTS idx_traces_project_lifecycle;
             ALTER TABLE traces DROP COLUMN lifecycle;
             DELETE FROM schema_migrations WHERE version=10;
             PRAGMA foreign_keys=ON;",
        )
        .await
        .unwrap();
        migrate(&db).await.unwrap();
        let legacy = one(
            &db,
            "SELECT lifecycle,status FROM traces WHERE id='legacy-trace'",
        )
        .await;
        assert_eq!(
            legacy.try_get::<String>("", "lifecycle").unwrap(),
            "active",
            "a pre-v10 row becomes active rather than guessing a lifecycle"
        );
        assert_eq!(
            legacy.try_get::<String>("", "status").unwrap(),
            "failed",
            "the migration must not rewrite the execution outcome"
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=10"
            )
            .await,
            1
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('traces') WHERE name='lifecycle'"
            )
            .await,
            1,
            "a repeat migration must not add the column twice"
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM sqlite_master WHERE type='index' AND name='idx_traces_project_lifecycle'"
            )
            .await,
            1
        );
    }

    /// Prompt placement is durable policy, not a console-only hint. A fresh
    /// database defaults to a safe disabled/prepend/order-zero record, while an
    /// upgrade adds only the new placement columns and keeps every existing
    /// prompt's enabled state and prepend behaviour intact.
    #[tokio::test]
    async fn prompt_placement_migration_is_additive_safe_and_idempotent() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();

        assert_eq!(
            scalar(&db, "SELECT MAX(version) AS count FROM schema_migrations").await,
            25
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('prompts') WHERE name='order' AND \"notnull\"=1 AND dflt_value='0'"
            )
            .await,
            1,
            "prompts.order must be non-null with its compatibility default"
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('prompts') WHERE name='action' AND \"notnull\"=1 AND dflt_value='''prepend'''"
            )
            .await,
            1,
            "prompts.action must be non-null with its compatibility default"
        );

        db.execute_unprepared(
            "INSERT INTO prompts(id,project_id,name,role,content,created_at,updated_at)
             VALUES('fresh-prompt','00000000-0000-0000-0000-000000000001','Fresh','system','x',1,1)",
        )
        .await
        .unwrap();
        let fresh = one(
            &db,
            "SELECT enabled,\"order\",action FROM prompts WHERE id='fresh-prompt'",
        )
        .await;
        assert_eq!(fresh.try_get::<i64>("", "enabled").unwrap(), 0);
        assert_eq!(fresh.try_get::<i64>("", "order").unwrap(), 0);
        assert_eq!(fresh.try_get::<String>("", "action").unwrap(), "prepend");
        assert!(
            db.execute_unprepared("UPDATE prompts SET action='around' WHERE id='fresh-prompt'")
                .await
                .is_err(),
            "the database must reject an unknown placement action"
        );

        db.execute_unprepared(
            "INSERT INTO prompts(id,project_id,name,role,content,enabled,created_at,updated_at)
             VALUES('legacy-prompt','00000000-0000-0000-0000-000000000001','Legacy','system','legacy',1,2,2);
             ALTER TABLE prompts RENAME TO prompts_current;
             CREATE TABLE prompts (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 name TEXT NOT NULL,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 activation_json TEXT NOT NULL DEFAULT '{\"version\":1}',
                 enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 UNIQUE(project_id,name)
             );
             INSERT INTO prompts(id,project_id,name,role,content,activation_json,enabled,created_at,updated_at)
             SELECT id,project_id,name,role,content,activation_json,enabled,created_at,updated_at FROM prompts_current;
             DROP TABLE prompts_current;
             DELETE FROM schema_migrations WHERE version=11;",
        )
        .await
        .unwrap();
        migrate(&db).await.unwrap();
        let legacy = one(
            &db,
            "SELECT enabled,\"order\",action FROM prompts WHERE id='legacy-prompt'",
        )
        .await;
        assert_eq!(legacy.try_get::<i64>("", "enabled").unwrap(), 1);
        assert_eq!(legacy.try_get::<i64>("", "order").unwrap(), 0);
        assert_eq!(legacy.try_get::<String>("", "action").unwrap(), "prepend");
        db.execute_unprepared(
            "INSERT INTO prompts(id,project_id,name,role,content,created_at,updated_at)
             VALUES('upgraded-default','00000000-0000-0000-0000-000000000001','Upgraded default','system','x',3,3)",
        )
        .await
        .unwrap();
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM prompts WHERE id='upgraded-default' AND enabled=0"
            )
            .await,
            1,
            "an upgraded database must have the same disabled default as a fresh one"
        );

        migrate(&db).await.unwrap();
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=11"
            )
            .await,
            1
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('prompts') WHERE name IN ('order','action')"
            )
            .await,
            2
        );
    }

    /// Protection metadata is additive policy state. Existing rules were all
    /// enforceable, so an upgrade must classify them as active; future rows get
    /// the same default, and SQLite itself rejects states the orchestrator does
    /// not understand.
    #[tokio::test]
    async fn protection_metadata_migration_is_additive_constrained_and_idempotent() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();

        assert_eq!(
            scalar(&db, "SELECT MAX(version) AS count FROM schema_migrations").await,
            25
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('prompt_protection_rules') WHERE name IN ('description','state') AND \"notnull\"=1"
            )
            .await,
            2
        );
        db.execute_unprepared(
            "INSERT INTO prompt_protection_rules(id,project_id,name,content_pattern,action,created_at,updated_at)
             VALUES('fresh-rule','00000000-0000-0000-0000-000000000001','Fresh','secret','deny',1,1)",
        )
        .await
        .unwrap();
        let fresh = one(
            &db,
            "SELECT description,state FROM prompt_protection_rules WHERE id='fresh-rule'",
        )
        .await;
        assert_eq!(fresh.try_get::<String>("", "description").unwrap(), "");
        assert_eq!(fresh.try_get::<String>("", "state").unwrap(), "active");
        assert!(
            db.execute_unprepared(
                "UPDATE prompt_protection_rules SET state='deleted' WHERE id='fresh-rule'"
            )
            .await
            .is_err(),
            "the database must reject an unknown rule state"
        );

        db.execute_unprepared(
            "ALTER TABLE prompt_protection_rules RENAME TO prompt_protection_rules_current;
             CREATE TABLE prompt_protection_rules (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 name TEXT NOT NULL,
                 role_pattern TEXT,
                 content_pattern TEXT NOT NULL,
                 action TEXT NOT NULL CHECK(action IN ('deny','redact')),
                 replacement TEXT,
                 scopes_json TEXT NOT NULL DEFAULT '{\"version\":1}',
                 test_mode INTEGER NOT NULL DEFAULT 0 CHECK(test_mode IN (0,1)),
                 enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             INSERT INTO prompt_protection_rules(id,project_id,name,role_pattern,content_pattern,action,replacement,scopes_json,test_mode,enabled,created_at,updated_at)
             SELECT id,project_id,name,role_pattern,content_pattern,action,replacement,scopes_json,test_mode,enabled,created_at,updated_at FROM prompt_protection_rules_current;
             DROP TABLE prompt_protection_rules_current;
             DELETE FROM schema_migrations WHERE version=12;",
        )
        .await
        .unwrap();

        migrate(&db).await.unwrap();
        let legacy = one(
            &db,
            "SELECT description,state FROM prompt_protection_rules WHERE id='fresh-rule'",
        )
        .await;
        assert_eq!(legacy.try_get::<String>("", "description").unwrap(), "");
        assert_eq!(legacy.try_get::<String>("", "state").unwrap(), "active");

        migrate(&db).await.unwrap();
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=12"
            )
            .await,
            1
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('prompt_protection_rules') WHERE name IN ('description','state')"
            )
            .await,
            2
        );
    }

    #[tokio::test]
    async fn api_key_lifecycle_migration_keeps_existing_keys_active() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();
        db.execute_unprepared(
            "DROP INDEX idx_api_keys_project_lifecycle;
             ALTER TABLE api_keys DROP COLUMN archived_at;
             ALTER TABLE api_keys DROP COLUMN lifecycle;
             DELETE FROM schema_migrations WHERE version=14;
             INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,created_at,project_id) VALUES('legacy-key','legacy','legacy-prefix','hash','legacy-digest',1,'00000000-0000-0000-0000-000000000001');",
        )
        .await
        .unwrap();

        migrate(&db).await.unwrap();
        let legacy = one(
            &db,
            "SELECT lifecycle,archived_at FROM api_keys WHERE id='legacy-key'",
        )
        .await;
        assert_eq!(legacy.try_get::<String>("", "lifecycle").unwrap(), "active");
        assert_eq!(
            legacy.try_get::<Option<i64>>("", "archived_at").unwrap(),
            None
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=14"
            )
            .await,
            1
        );
        migrate(&db).await.unwrap();
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=14"
            )
            .await,
            1
        );
    }

    #[tokio::test]
    async fn oidc_login_policy_migration_has_safe_defaults_and_is_idempotent() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();

        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_table_info('oidc_providers') WHERE name IN ('login_only','display_name','button_color','logo_key')"
            )
            .await,
            4
        );
        db.execute_unprepared(
            "INSERT INTO oidc_providers(id,name,issuer_url,client_id,client_secret_envelope,created_at,updated_at)
             VALUES('legacy-oidc','Legacy SSO','https://id.example.test','client','encrypted',1,1);",
        )
        .await
        .unwrap();
        let legacy = one(
            &db,
            "SELECT login_only,display_name,button_color,logo_key FROM oidc_providers WHERE id='legacy-oidc'",
        )
        .await;
        assert!(!legacy.try_get::<bool>("", "login_only").unwrap());
        assert_eq!(legacy.try_get::<String>("", "display_name").unwrap(), "");
        assert_eq!(
            legacy
                .try_get::<Option<String>>("", "button_color")
                .unwrap(),
            None
        );
        assert_eq!(
            legacy.try_get::<Option<String>>("", "logo_key").unwrap(),
            None
        );

        migrate(&db).await.unwrap();
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=16"
            )
            .await,
            1
        );
    }

    #[tokio::test]
    async fn invitation_reuse_migration_preserves_single_use_state() {
        let db = memory_database().await;
        migrate(&db).await.unwrap();
        db.execute_unprepared(
            "ALTER TABLE project_invitations DROP COLUMN use_count;
             ALTER TABLE project_invitations DROP COLUMN max_uses;
             DELETE FROM schema_migrations WHERE version=18;
             INSERT INTO project_invitations(id,project_id,email,role_id,token_hash,expires_at,accepted_at,created_at)
             VALUES
               ('pending-invite','00000000-0000-0000-0000-000000000001','pending@example.com','00000000-0000-0000-0000-000000000012','pending-hash',9999999999,NULL,1),
               ('accepted-invite','00000000-0000-0000-0000-000000000001','accepted@example.com','00000000-0000-0000-0000-000000000012','accepted-hash',9999999999,2,1);",
        )
        .await
        .unwrap();

        migrate(&db).await.unwrap();

        let pending = one(
            &db,
            "SELECT max_uses,use_count FROM project_invitations WHERE id='pending-invite'",
        )
        .await;
        assert_eq!(pending.try_get::<i64>("", "max_uses").unwrap(), 1);
        assert_eq!(pending.try_get::<i64>("", "use_count").unwrap(), 0);
        let accepted = one(
            &db,
            "SELECT max_uses,use_count FROM project_invitations WHERE id='accepted-invite'",
        )
        .await;
        assert_eq!(accepted.try_get::<i64>("", "max_uses").unwrap(), 1);
        assert_eq!(accepted.try_get::<i64>("", "use_count").unwrap(), 1);

        migrate(&db).await.unwrap();
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM schema_migrations WHERE version=18"
            )
            .await,
            1
        );
    }

    #[tokio::test]
    async fn storage_rebuild_preserves_revision_and_timestamps_by_name() {
        let db = memory_database().await;
        db.execute_unprepared(
            r#"
            CREATE TABLE projects(id TEXT PRIMARY KEY);
            INSERT INTO projects(id) VALUES('project');
            CREATE TABLE data_storage_configs (
                id TEXT PRIMARY KEY,
                project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                kind TEXT NOT NULL CHECK(kind IN ('local','s3')),
                config_json TEXT NOT NULL DEFAULT '{"version":1}',
                secret_envelope TEXT,
                enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                revision INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE backup_configs (
                id TEXT PRIMARY KEY,
                storage_id TEXT NOT NULL REFERENCES data_storage_configs(id) ON DELETE RESTRICT,
                schedule TEXT,
                retention_count INTEGER NOT NULL DEFAULT 7 CHECK(retention_count > 0),
                resources_json TEXT NOT NULL DEFAULT '{"version":1,"resources":[]}',
                conflict_strategy TEXT NOT NULL DEFAULT 'fail' CHECK(conflict_strategy IN ('fail','skip','overwrite')),
                enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                revision INTEGER NOT NULL DEFAULT 1
            );
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
            CREATE TABLE channel_settings(provider_id TEXT PRIMARY KEY);
            CREATE TABLE webhooks(id TEXT PRIMARY KEY);
            CREATE TABLE catalog_sources(id TEXT PRIMARY KEY);
            INSERT INTO data_storage_configs(
                id,project_id,name,kind,config_json,enabled,created_at,updated_at,revision
            ) VALUES('storage','project','Storage','s3','{"kind":"s3"}',1,100,200,7);
            INSERT INTO backup_configs(
                id,storage_id,created_at,updated_at,revision
            ) VALUES('config','storage',300,400,9);
            INSERT INTO backup_runs(id,config_id,storage_id,status,started_at)
            VALUES('run','config','storage','succeeded',500);
            "#,
        )
        .await
        .unwrap();

        db.execute_unprepared(V22_STORAGE_DIAGNOSTICS_PROXY)
            .await
            .unwrap();

        let storage = one(
            &db,
            "SELECT revision,created_at,updated_at FROM data_storage_configs WHERE id='storage'",
        )
        .await;
        assert_eq!(storage.try_get::<i64>("", "revision").unwrap(), 7);
        assert_eq!(storage.try_get::<i64>("", "created_at").unwrap(), 100);
        assert_eq!(storage.try_get::<i64>("", "updated_at").unwrap(), 200);
        let config = one(
            &db,
            "SELECT revision,created_at,updated_at FROM backup_configs WHERE id='config'",
        )
        .await;
        assert_eq!(config.try_get::<i64>("", "revision").unwrap(), 9);
        assert_eq!(config.try_get::<i64>("", "created_at").unwrap(), 300);
        assert_eq!(config.try_get::<i64>("", "updated_at").unwrap(), 400);
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) AS count FROM pragma_foreign_key_check"
            )
            .await,
            0
        );
    }
}
