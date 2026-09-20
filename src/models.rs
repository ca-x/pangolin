use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct User {
    pub id: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: String,
    pub language: String,
    pub theme: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct Model {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub public_name: String,
    pub upstream_name: String,
    pub capabilities: String,
    pub input_price_micros: i64,
    pub output_price_micros: i64,
    pub priority: i32,
    pub enabled: bool,
    pub created_at: i64,
    pub catalog_metadata_json: String,
}

#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub scopes: String,
    pub budget_micros: Option<i64>,
    pub spent_micros: i64,
    pub enabled: bool,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct ApiKeyCredential {
    pub id: String,
    pub project_id: String,
    pub user_id: Option<String>,
    pub key_hash: String,
    pub scopes: String,
    pub budget_micros: Option<i64>,
    pub spent_micros: i64,
    pub enabled: bool,
    pub expires_at: Option<i64>,
    pub allowed_ips_json: String,
    pub denied_ips_json: String,
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct RouteTarget {
    pub public_name: String,
    pub upstream_name: String,
    pub provider_name: String,
    pub provider_kind: String,
    pub base_url: String,
    pub secret_envelope: String,
    pub input_price_micros: i64,
    pub output_price_micros: i64,
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub email: String,
    pub password: String,
    pub instance_name: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ProviderInput {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct ModelInput {
    pub provider_id: String,
    pub public_name: String,
    pub upstream_name: String,
    pub capabilities: Option<Vec<String>>,
    pub input_price_micros: Option<i64>,
    pub output_price_micros: Option<i64>,
    pub priority: Option<i32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ApiKeyInput {
    pub name: String,
    pub budget_micros: Option<i64>,
    #[serde(default)]
    pub token_mode: ApiKeyTokenMode,
    pub token: Option<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyTokenMode {
    #[default]
    Generated,
    ImportExisting,
}

#[allow(dead_code)]
pub mod parity {
    use sea_orm::FromQueryResult;

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct Project {
        pub id: String,
        pub name: String,
        pub slug: String,
        pub owner_user_id: Option<String>,
        pub is_default: bool,
        pub enabled: bool,
        pub settings_json: String,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ProjectMembership {
        pub id: String,
        pub project_id: String,
        pub user_id: String,
        pub role_id: String,
        pub status: String,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ProjectInvitation {
        pub id: String,
        pub project_id: String,
        pub email: String,
        pub role_id: String,
        pub invited_by_user_id: Option<String>,
        pub token_hash: String,
        pub expires_at: i64,
        pub accepted_at: Option<i64>,
        pub created_at: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct Permission {
        pub id: String,
        pub slug: String,
        pub level: String,
        pub description: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct Role {
        pub id: String,
        pub project_id: Option<String>,
        pub name: String,
        pub scope: String,
        pub is_system: bool,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct RolePermission {
        pub role_id: String,
        pub permission_id: String,
        pub created_at: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct UserRoleBinding {
        pub id: String,
        pub user_id: String,
        pub role_id: String,
        pub project_id: Option<String>,
        pub created_at: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct OidcProvider {
        pub id: String,
        pub name: String,
        pub issuer_url: String,
        pub client_id: String,
        pub client_secret_envelope: String,
        pub scopes_json: String,
        pub claim_mapping_json: String,
        pub enabled: bool,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct OidcIdentity {
        pub id: String,
        pub provider_id: String,
        pub user_id: String,
        pub subject: String,
        pub claims_json: String,
        pub last_login_at: Option<i64>,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ApiKeyProfile {
        pub id: String,
        pub project_id: String,
        pub name: String,
        pub rpm_limit: Option<i64>,
        pub tpm_limit: Option<i64>,
        pub budget_micros: Option<i64>,
        pub routing_policy_json: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ApiKeyProfileModelMapping {
        pub id: String,
        pub profile_id: String,
        pub source_model: String,
        pub target_model: String,
        pub priority: i32,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ApiKeyProfileAllowedModel {
        pub profile_id: String,
        pub model_pattern: String,
        pub match_type: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ScopedApiKey {
        pub id: String,
        pub project_id: String,
        pub user_id: Option<String>,
        pub profile_id: Option<String>,
        pub key_type: String,
        pub expires_at: Option<i64>,
        pub allowed_ips_json: String,
        pub denied_ips_json: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct Channel {
        pub id: String,
        pub project_id: String,
        pub name: String,
        pub kind: String,
        pub base_url: String,
        pub settings_json: String,
        pub enabled: bool,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ChannelCredential {
        pub id: String,
        pub provider_id: String,
        pub credential_type: String,
        pub secret_envelope: String,
        pub suffix: String,
        pub priority: i32,
        pub enabled: bool,
        pub settings_json: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ChannelSettings {
        pub provider_id: String,
        pub endpoint_mappings_json: String,
        pub model_rules_json: String,
        pub parameter_overrides_json: String,
        pub retry_statuses_json: String,
        pub auto_disable_policy_json: String,
        pub proxy_settings_json: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ModelAssociation {
        pub id: String,
        pub project_id: String,
        pub model_id: Option<String>,
        pub provider_id: Option<String>,
        pub match_type: String,
        pub pattern: String,
        pub conditions_json: String,
        pub priority: i32,
        pub weight: i32,
        pub enabled: bool,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ModelPrice {
        pub id: String,
        pub model_id: String,
        pub provider_id: Option<String>,
        pub version: i32,
        pub currency: String,
        pub valid_from: i64,
        pub valid_until: Option<i64>,
        pub schedule_json: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ModelPriceComponent {
        pub id: String,
        pub price_id: String,
        pub kind: String,
        pub unit_size: i64,
        pub unit_price_micros: i64,
        pub tiers_json: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct Prompt {
        pub id: String,
        pub project_id: String,
        pub name: String,
        pub role: String,
        pub content: String,
        pub activation_json: String,
        pub enabled: bool,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct PromptProtectionRule {
        pub id: String,
        pub project_id: String,
        pub name: String,
        pub role_pattern: Option<String>,
        pub content_pattern: String,
        pub action: String,
        pub replacement: Option<String>,
        pub scopes_json: String,
        pub test_mode: bool,
        pub enabled: bool,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct Thread {
        pub id: String,
        pub project_id: String,
        pub api_key_id: Option<String>,
        pub user_id: Option<String>,
        pub external_id: Option<String>,
        pub metadata_json: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct Trace {
        pub id: String,
        pub thread_id: Option<String>,
        pub project_id: String,
        pub api_key_id: Option<String>,
        pub user_id: Option<String>,
        pub source: String,
        pub status: String,
        pub started_at: i64,
        pub finished_at: Option<i64>,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct Request {
        pub id: String,
        pub trace_id: String,
        pub parent_request_id: Option<String>,
        pub protocol: String,
        pub endpoint: String,
        pub requested_model: Option<String>,
        pub source_ip: Option<String>,
        pub status: String,
        pub request_metadata_json: String,
        pub response_metadata_json: String,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct RequestExecution {
        pub id: String,
        pub request_id: String,
        pub provider_id: Option<String>,
        pub credential_id: Option<String>,
        pub attempt: i32,
        pub model: Option<String>,
        pub status: String,
        pub retry_reason: Option<String>,
        pub credential_suffix: Option<String>,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct UsageLog {
        pub id: String,
        pub execution_id: String,
        pub model_id: Option<String>,
        pub price_id: Option<String>,
        pub input_tokens: i64,
        pub output_tokens: i64,
        pub cache_read_tokens: i64,
        pub cache_write_tokens: i64,
        pub reasoning_tokens: i64,
        pub request_units: i64,
        pub total_cost_micros: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct UsageCostItem {
        pub id: String,
        pub usage_log_id: String,
        pub price_component_id: Option<String>,
        pub quantity: i64,
        pub subtotal_micros: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ProviderQuotaSnapshot {
        pub id: String,
        pub provider_id: String,
        pub credential_id: Option<String>,
        pub remaining_micros: Option<i64>,
        pub quota_json: String,
        pub collected_at: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ChannelProbe {
        pub id: String,
        pub provider_id: String,
        pub credential_id: Option<String>,
        pub model: Option<String>,
        pub success: bool,
        pub status_code: Option<i32>,
        pub latency_ms: Option<i64>,
        pub ttft_ms: Option<i64>,
        pub error_code: Option<String>,
        pub probed_at: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct ChannelHealthState {
        pub provider_id: String,
        pub consecutive_failures: i64,
        pub disabled_until: Option<i64>,
        pub backoff_until: Option<i64>,
        pub reason: Option<String>,
        pub updated_at: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct Webhook {
        pub id: String,
        pub project_id: String,
        pub name: String,
        pub url: String,
        pub secret_envelope: Option<String>,
        pub headers_json: String,
        pub body_template_json: String,
        pub subscriptions_json: String,
        pub enabled: bool,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct WebhookDelivery {
        pub id: String,
        pub webhook_id: String,
        pub event_type: String,
        pub attempt: i32,
        pub status: String,
        pub response_status: Option<i32>,
        pub next_attempt_at: Option<i64>,
        pub created_at: i64,
        pub finished_at: Option<i64>,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct DataStorageConfig {
        pub id: String,
        pub project_id: Option<String>,
        pub name: String,
        pub kind: String,
        pub config_json: String,
        pub secret_envelope: Option<String>,
        pub enabled: bool,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct DataRetentionPolicy {
        pub id: String,
        pub project_id: Option<String>,
        pub resource_type: String,
        pub retention_days: i32,
        pub retain_payloads: bool,
        pub updated_at: i64,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct BackupConfig {
        pub id: String,
        pub storage_id: String,
        pub schedule: Option<String>,
        pub retention_count: i32,
        pub resources_json: String,
        pub conflict_strategy: String,
        pub enabled: bool,
    }

    #[derive(Debug, Clone, FromQueryResult)]
    pub struct BackupRun {
        pub id: String,
        pub config_id: Option<String>,
        pub storage_id: String,
        pub status: String,
        pub object_key: Option<String>,
        pub manifest_json: String,
        pub started_at: i64,
        pub finished_at: Option<i64>,
        pub error: Option<String>,
    }
}
