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
    pub key_hash: String,
    pub scopes: String,
    pub budget_micros: Option<i64>,
    pub spent_micros: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct RouteTarget {
    pub public_name: String,
    pub upstream_name: String,
    pub capabilities: String,
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
    pub base_url: String,
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

#[derive(Debug, Deserialize)]
pub struct ApiKeyInput {
    pub name: String,
    pub budget_micros: Option<i64>,
}
