use crate::orchestration::policy::Retry;
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::sql;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QuotaRoutingMode {
    IgnoreQuota,
    #[default]
    RemoveOnExhausted,
    Backpressure,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SystemSettings {
    pub instance_name: String,
    pub branding_name: String,
    pub favicon_url: String,
    pub onboarding_complete: bool,
    pub currency: String,
    pub timezone: String,
    pub retry_policy: Value,
    pub quota_collection_enabled: bool,
    pub quota_routing_mode: QuotaRoutingMode,
    pub cors_allowed_origins: Vec<String>,
    pub request_timeout_ms: u64,
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            instance_name: "Pangolin".into(),
            branding_name: "Pangolin / 鲮鲤".into(),
            favicon_url: "/logo.webp".into(),
            onboarding_complete: false,
            currency: "USD".into(),
            timezone: String::new(),
            retry_policy: json!({"version":1,"attempts":1,"error_mode":"normalized"}),
            quota_collection_enabled: true,
            quota_routing_mode: QuotaRoutingMode::RemoveOnExhausted,
            cors_allowed_origins: Vec::new(),
            request_timeout_ms: 600_000,
        }
    }
}

impl SystemSettings {
    pub fn validate(&self) -> bool {
        !self.instance_name.trim().is_empty()
            && self.instance_name.len() <= 128
            && !self.branding_name.trim().is_empty()
            && self.branding_name.len() <= 128
            && self.favicon_url.len() <= 2048
            && (self.favicon_url.starts_with('/') || self.favicon_url.starts_with("https://"))
            && self.currency.len() == 3
            && self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            && (self.timezone.is_empty() || self.timezone.parse::<chrono_tz::Tz>().is_ok())
            && Retry::parse(&self.retry_policy.to_string()).is_ok()
            && self
                .retry_policy
                .get("error_message")
                .and_then(Value::as_str)
                .is_none_or(|message| message.len() <= 1024)
            && valid_http_policy(&self.cors_allowed_origins, self.request_timeout_ms)
    }

    /// Stored settings are forward-compatible: each recognized value is accepted
    /// independently, while malformed or future fields fall back to today's safe
    /// default. Writes use serde's closed input contract plus `validate`.
    pub fn from_stored(value: Value) -> Self {
        let defaults = Self::default();
        let Some(object) = value.as_object() else {
            return defaults;
        };
        let text = |key: &str, fallback: &str| {
            object
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or(fallback)
                .to_owned()
        };
        let mut candidate = Self {
            instance_name: text("instance_name", &defaults.instance_name),
            branding_name: text("branding_name", &defaults.branding_name),
            favicon_url: text("favicon_url", &defaults.favicon_url),
            onboarding_complete: object
                .get("onboarding_complete")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.onboarding_complete),
            currency: text("currency", &defaults.currency),
            timezone: text("timezone", &defaults.timezone),
            retry_policy: object
                .get("retry_policy")
                .filter(|policy| policy.is_object())
                .cloned()
                .unwrap_or_else(|| defaults.retry_policy.clone()),
            quota_collection_enabled: object
                .get("quota_collection_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.quota_collection_enabled),
            quota_routing_mode: object
                .get("quota_routing_mode")
                .cloned()
                .and_then(|mode| serde_json::from_value(mode).ok())
                .unwrap_or(defaults.quota_routing_mode),
            cors_allowed_origins: object
                .get("cors_allowed_origins")
                .cloned()
                .and_then(|origins| serde_json::from_value(origins).ok())
                .unwrap_or_else(|| defaults.cors_allowed_origins.clone()),
            request_timeout_ms: object
                .get("request_timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(defaults.request_timeout_ms),
        };
        if !candidate.validate() {
            if candidate.currency.len() != 3
                || !candidate
                    .currency
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase())
            {
                candidate.currency = defaults.currency;
            }
            if !candidate.timezone.is_empty()
                && candidate.timezone.parse::<chrono_tz::Tz>().is_err()
            {
                candidate.timezone = defaults.timezone;
            }
            if Retry::parse(&candidate.retry_policy.to_string()).is_err() {
                candidate.retry_policy = defaults.retry_policy.clone();
            }
            if candidate
                .retry_policy
                .get("error_message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.len() > 1024)
            {
                candidate.retry_policy = defaults.retry_policy.clone();
            }
            if candidate.instance_name.trim().is_empty() || candidate.instance_name.len() > 128 {
                candidate.instance_name = defaults.instance_name;
            }
            if candidate.branding_name.trim().is_empty() || candidate.branding_name.len() > 128 {
                candidate.branding_name = defaults.branding_name;
            }
            if candidate.favicon_url.len() > 2048
                || !(candidate.favicon_url.starts_with('/')
                    || candidate.favicon_url.starts_with("https://"))
            {
                candidate.favicon_url = defaults.favicon_url;
            }
            if !valid_http_policy(
                &candidate.cors_allowed_origins,
                candidate.request_timeout_ms,
            ) {
                candidate.cors_allowed_origins = defaults.cors_allowed_origins;
                candidate.request_timeout_ms = defaults.request_timeout_ms;
            }
        }
        candidate
    }

    pub fn effective_retry(&self, channel: &str) -> Result<Retry, crate::orchestration::Error> {
        let mut merged = self.retry_policy.as_object().cloned().unwrap_or_default();
        let override_value: Value = serde_json::from_str(channel)
            .map_err(|_| crate::orchestration::Error::Configuration)?;
        let override_object = override_value
            .as_object()
            .ok_or(crate::orchestration::Error::Configuration)?;
        for (key, value) in override_object {
            merged.insert(key.clone(), value.clone());
        }
        Retry::parse(&Value::Object(merged).to_string())
    }
}

fn valid_http_policy(origins: &[String], request_timeout_ms: u64) -> bool {
    (100..=3_600_000).contains(&request_timeout_ms)
        && origins.len() <= 32
        && origins.iter().enumerate().all(|(index, origin)| {
            origin.len() <= 2048
                && canonical_origin(origin).as_deref() == Some(origin.as_str())
                && !origins[..index].contains(origin)
        })
}

fn canonical_origin(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

pub async fn load(db: &DatabaseConnection) -> Result<SystemSettings, sea_orm::DbErr> {
    let row = db
        .query_one(sql("SELECT value FROM settings WHERE key='system'", vec![]))
        .await?;
    let stored = row
        .map(|row| row.try_get::<String>("", "value"))
        .transpose()?;
    if let Some(value) = stored.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()) {
        return Ok(SystemSettings::from_stored(value));
    }
    let legacy_name = db
        .query_one(sql(
            "SELECT value FROM settings WHERE key='instance_name'",
            vec![],
        ))
        .await?
        .map(|row| row.try_get::<String>("", "value"))
        .transpose()?;
    let mut defaults = SystemSettings::from_stored(Value::Object(Map::new()));
    if let Some(name) = legacy_name.filter(|name| !name.trim().is_empty() && name.len() <= 128) {
        defaults.instance_name = name.clone();
        defaults.branding_name = name;
    }
    Ok(defaults)
}
