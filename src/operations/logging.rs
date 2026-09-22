use super::sql;
use crate::{api::ApiError, db};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Inherit,
    Off,
    #[default]
    Metadata,
    RedactedBody,
    FullBody,
}
impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Off => "off",
            Self::Metadata => "metadata",
            Self::RedactedBody => "redacted_body",
            Self::FullBody => "full_body",
        }
    }
    pub fn body(self, value: &Value) -> Option<String> {
        match self {
            Self::FullBody => Some(sanitize(value.clone(), false).to_string()),
            Self::RedactedBody => Some(sanitize(value.clone(), true).to_string()),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Policy {
    pub enabled: bool,
    pub default_level: Level,
    pub key_override_enabled: bool,
    pub key_disable_allowed: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: true,
            default_level: Level::Metadata,
            key_override_enabled: false,
            key_disable_allowed: false,
        }
    }
}
/// The strict write contract for the site policy — deliberately a different type
/// from the stored/runtime [`Policy`].
///
/// The two ends of this document fail in opposite directions, so they need
/// opposite strictness. `Policy` is parsed on the admission path, where a
/// document written by another build must never be able to fail resolution and
/// take every request in the instance down with it; it therefore ignores fields
/// it does not know (see
/// `a_stored_logging_policy_with_an_unknown_field_does_not_break_the_gateway`).
/// The write path is the only place an operator typo can install a policy nobody
/// asked for: `{"enabledd": false}` used to deserialize into the default
/// *enabled/metadata* policy and be stored and audited as a deliberate change,
/// the exact inverse of the intent. Unknown fields are denied here, at the
/// boundary, and never on the stored document.
///
/// Conversion into `Policy` happens only after this type has accepted the input.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyInput {
    enabled: bool,
    default_level: Level,
    key_override_enabled: bool,
    key_disable_allowed: bool,
}
impl Default for PolicyInput {
    /// An omitted field keeps the policy's own documented default — `enabled`
    /// stays true, it does not fall back to `bool::default()`.
    fn default() -> Self {
        let policy = Policy::default();
        Self {
            enabled: policy.enabled,
            default_level: policy.default_level,
            key_override_enabled: policy.key_override_enabled,
            key_disable_allowed: policy.key_disable_allowed,
        }
    }
}
impl PolicyInput {
    /// Parse the strict write contract. The failure is a fixed, safe message: the
    /// serde cause would quote field paths and line numbers, which tells a
    /// legitimate client nothing it does not already know from the schema and
    /// which is exactly the internal detail this boundary must not publish.
    pub fn parse(value: Value) -> Result<Self, ApiError> {
        // serde's derived struct visitor also accepts a *positional* sequence, so
        // `[]` deserializes to all-defaults and would install a policy nobody
        // wrote; `[true,"off"]` would even install a chosen one. The document is
        // an object, and nothing else is a request-logging policy.
        if !value.is_object() {
            return Err(Self::invalid());
        }
        serde_json::from_value(value).map_err(|_| Self::invalid())
    }
    fn invalid() -> ApiError {
        ApiError::BadRequest(
            "invalid request-logging policy: only enabled, default_level, key_override_enabled and key_disable_allowed are accepted, with the documented types and levels".into(),
        )
    }
}
impl From<PolicyInput> for Policy {
    fn from(input: PolicyInput) -> Self {
        Self {
            enabled: input.enabled,
            default_level: input.default_level,
            key_override_enabled: input.key_override_enabled,
            key_disable_allowed: input.key_disable_allowed,
        }
    }
}
impl Policy {
    pub fn resolve(&self, key: Level) -> Level {
        if !self.enabled {
            return Level::Off;
        }
        let default = if self.default_level == Level::Inherit {
            Level::Metadata
        } else {
            self.default_level
        };
        if !self.key_override_enabled
            || key == Level::Inherit
            || (key == Level::Off && !self.key_disable_allowed)
        {
            default
        } else {
            key
        }
    }
}
pub async fn resolve(db: &DatabaseConnection, key: &str) -> Result<Level, ApiError> {
    let row=db.query_one(sql("SELECT log_level,(SELECT value FROM settings WHERE key='request_logging') AS policy FROM api_keys WHERE id=?",vec![key.into()])).await?.ok_or(ApiError::Unauthorized)?;
    let policy = row
        .try_get::<Option<String>>("", "policy")?
        .map(|v| serde_json::from_str::<Policy>(&v))
        .transpose()
        .map_err(|e| ApiError::Internal(e.into()))?
        .unwrap_or_default();
    let level = serde_json::from_value::<Level>(Value::String(row.try_get("", "log_level")?))
        .map_err(|e| ApiError::Internal(e.into()))?;
    Ok(policy.resolve(level))
}
pub async fn set_policy(db: &impl ConnectionTrait, policy: &Policy) -> Result<(), ApiError> {
    if policy.default_level == Level::Inherit {
        return Err(ApiError::BadRequest("site default cannot inherit".into()));
    }
    db.execute(sql("INSERT INTO settings(key,value,updated_at) VALUES('request_logging',?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",vec![serde_json::to_string(policy).map_err(|e|ApiError::Internal(e.into()))?.into(),db::now().into()])).await?;
    Ok(())
}
pub fn sanitize(mut value: Value, redact: bool) -> Value {
    match &mut value {
        Value::Object(object) => {
            let media = object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|v| matches!(v, "base64" | "image" | "audio" | "document"))
                || object.contains_key("mimeType")
                || object.contains_key("mime_type")
                || object.contains_key("media_type");
            for (key, value) in object {
                let lower = key.to_ascii_lowercase().replace('-', "_");
                let sensitive = lower.contains("authorization")
                    || lower.contains("cookie")
                    || lower.contains("secret")
                    || lower.contains("password")
                    || lower.contains("credential")
                    || lower.contains("api_key")
                    || matches!(
                        lower.as_str(),
                        "api_key"
                            | "apikey"
                            | "access_token"
                            | "refresh_token"
                            | "id_token"
                            | "token"
                            | "client_secret"
                    );
                if lower.contains("base64")
                    || matches!(lower.as_str(), "b64_json" | "file_data" | "audio_data")
                    || lower == "data" && media
                {
                    *value = Value::String("[MEDIA OMITTED]".into());
                } else if sensitive
                    || redact
                        && matches!(
                            lower.as_str(),
                            "content"
                                | "text"
                                | "input"
                                | "prompt"
                                | "instructions"
                                | "arguments"
                                | "output"
                                | "image_url"
                                | "url"
                                | "data"
                        )
                {
                    *value = Value::String("[REDACTED]".into())
                } else {
                    *value = sanitize(value.take(), redact)
                }
            }
        }
        Value::Array(items) => {
            for value in items {
                *value = sanitize(value.take(), redact)
            }
        }
        Value::String(text) => {
            if text.trim_start().starts_with("data:") {
                *text = "[MEDIA OMITTED]".into();
            } else if matches!(text.trim_start().chars().next(), Some('{' | '['))
                && text.len() <= 1024 * 1024
                && let Ok(document) = serde_json::from_str::<Value>(text)
            {
                *text = sanitize(document, redact).to_string();
            } else if (text.starts_with("https://") || text.starts_with("http://"))
                && let Ok(mut url) = reqwest::Url::parse(text)
            {
                let _ = url.set_username("");
                let _ = url.set_password(None);
                url.set_query(None);
                url.set_fragment(None);
                *text = url.to_string();
            }
        }
        _ => (),
    };
    value
}
