use crate::api::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_BYTES: usize = 4 * 1024 * 1024;
pub type Extensions = BTreeMap<String, Value>;
pub fn invalid(message: &str) -> ApiError {
    ApiError::BadRequest(message.into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub id: String,
    pub url: String,
    pub revision: String,
    pub license: String,
}

impl Provenance {
    fn validate(&self) -> Result<(), ApiError> {
        if self.id.is_empty()
            || self.id.len() > 256
            || self.revision.len() > 256
            || self.license.len() > 256
            || self.url.len() > 2048
        {
            return Err(invalid("invalid catalog provenance"));
        }
        if !self.url.is_empty() {
            let url =
                reqwest::Url::parse(&self.url).map_err(|_| invalid("invalid provenance URL"))?;
            if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
                return Err(invalid("provenance URLs must use HTTPS"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub version: String,
    pub source: Provenance,
    #[serde(default)]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub models: Vec<Model>,
    #[serde(default)]
    pub extensions: Extensions,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub protocol: String,
    pub path: String,
    #[serde(default)]
    pub transport: Transport,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    #[default]
    Http,
    Websocket,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub supported_by_provider: Option<bool>,
    #[serde(default)]
    pub implemented: bool,
    pub upstream_endpoint: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub category: String,
    pub default_base_url: Option<String>,
    pub auth_strategy: String,
    pub adapter_kind: Option<String>,
    #[serde(default)]
    pub adapter_available: bool,
    #[serde(default)]
    pub default_endpoints: Vec<Endpoint>,
    #[serde(default)]
    pub model_discovery: Operation,
    #[serde(default)]
    pub quota: Operation,
    pub logo_key: String,
    pub logo_fallback: String,
    pub brand_color: Option<String>,
    #[serde(default)]
    pub monochrome: bool,
    pub source: Provenance,
    #[serde(default)]
    pub extensions: Extensions,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub streaming: Option<bool>,
    pub tools: Option<bool>,
    pub reasoning: Option<bool>,
    pub temperature: Option<bool>,
    pub vision: Option<bool>,
    pub json_schema: Option<bool>,
    pub web_search: Option<bool>,
    pub file_search: Option<bool>,
    pub computer_use: Option<bool>,
    pub caching: Option<bool>,
    pub batch: Option<bool>,
    #[serde(flatten)]
    pub extra: Extensions,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub context: Option<u64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
    #[serde(default)]
    pub extensions: Extensions,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Costs {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    pub currency: Option<String>,
    pub unit: Option<String>,
    #[serde(default)]
    pub extensions: Extensions,
}

/// The bounded public projection of an immutable card stored on a project model.
///
/// Historical rows may contain partial card objects. Such rows retain an object
/// projection with nullable facts, while a missing/non-object card has no
/// projection at all. Callers never need to expose the raw stored document.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelCardProjection {
    pub developer: Option<String>,
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    pub logo_key: Option<String>,
    pub limits: ModelCardLimitsProjection,
    pub cost_defaults: ModelCardCostsProjection,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelCardLimitsProjection {
    pub context: Option<u64>,
    pub output: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelCardCostsProjection {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub currency: Option<String>,
    pub unit: Option<String>,
}

/// One successfully parsed stored catalog metadata document. `raw` exists only
/// for the legacy admin-list compatibility field; public gateway responses use
/// `card`, which contains a fixed set of typed facts.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredModelMetadata {
    pub raw: Value,
    pub card: Option<ModelCardProjection>,
}

impl StoredModelMetadata {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = serde_json::from_str::<Value>(raw).ok()?;
        let object = raw.as_object()?;
        let card = object
            .get("card")
            .filter(|card| card.is_object())
            .map(|card| {
                let bounded_text = |value: Option<&Value>| {
                    value
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty() && value.len() <= 256)
                        .map(str::to_owned)
                };
                let nonnegative_cost = |value: Option<&Value>| {
                    value
                        .and_then(Value::as_f64)
                        .filter(|value| value.is_finite() && *value >= 0.0)
                };
                ModelCardProjection {
                    developer: bounded_text(card.get("developer")),
                    model_type: bounded_text(card.get("type")),
                    logo_key: bounded_text(object.get("logo_key")),
                    limits: ModelCardLimitsProjection {
                        context: card.pointer("/limits/context").and_then(Value::as_u64),
                        output: card.pointer("/limits/output").and_then(Value::as_u64),
                    },
                    cost_defaults: ModelCardCostsProjection {
                        input: nonnegative_cost(card.pointer("/cost_defaults/input")),
                        output: nonnegative_cost(card.pointer("/cost_defaults/output")),
                        currency: bounded_text(card.pointer("/cost_defaults/currency")),
                        unit: bounded_text(card.pointer("/cost_defaults/unit")),
                    },
                }
            });
        Some(Self { raw, card })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub upstream_id: String,
    pub name: String,
    pub developer: String,
    #[serde(rename = "type")]
    pub model_type: String,
    #[serde(default)]
    pub modalities: Modalities,
    #[serde(default)]
    pub protocols: Vec<String>,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub lifecycle: String,
    pub knowledge_cutoff: Option<String>,
    pub release_date: Option<String>,
    pub last_updated: Option<String>,
    #[serde(default)]
    pub cost_defaults: Costs,
    pub source: Provenance,
    #[serde(default)]
    pub extensions: Extensions,
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._/:".contains(&c))
}

impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Self, ApiError> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid("catalog exceeds 4 MiB"));
        }
        let mut value: Self = serde_json::from_slice(bytes)
            .map_err(|_| invalid("invalid versioned catalog document"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&mut self) -> Result<(), ApiError> {
        self.source.validate()?;
        if self.schema_version != 1 || self.version.is_empty() || self.version.len() > 128 {
            return Err(invalid("unsupported catalog schema or version"));
        }
        if self.providers.len() > 1024 || self.models.len() > 20000 {
            return Err(invalid("catalog has too many entries"));
        }
        let mut providers = BTreeSet::new();
        let mut models = BTreeSet::new();
        for provider in &mut self.providers {
            provider.validate()?;
            if !providers.insert(&provider.id) {
                return Err(invalid("duplicate catalog provider id"));
            }
        }
        for model in &self.models {
            model.validate()?;
            if !models.insert(&model.id) {
                return Err(invalid("duplicate catalog model id"));
            }
        }
        Ok(())
    }
}

impl Provider {
    pub fn validate(&mut self) -> Result<(), ApiError> {
        self.source.validate()?;
        if !identifier(&self.id) || self.name.is_empty() || self.name.len() > 256 {
            return Err(invalid("invalid provider identity"));
        }
        if !matches!(
            self.category.as_str(),
            "direct" | "aggregator" | "cloud" | "local" | "catalog"
        ) {
            return Err(invalid("invalid provider category"));
        }
        if !matches!(
            self.auth_strategy.as_str(),
            "bearer"
                | "anthropic"
                | "gemini_key"
                | "azure"
                | "aws_sigv4"
                | "gcp"
                | "none"
                | "unsupported"
        ) {
            return Err(invalid("invalid provider auth strategy"));
        }
        if let Some(base) = &self.default_base_url {
            let url =
                reqwest::Url::parse(base).map_err(|_| invalid("invalid provider base URL"))?;
            if !matches!(url.scheme(), "https" | "http")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid("invalid provider base URL"));
            }
        }
        self.adapter_available = self
            .adapter_kind
            .as_deref()
            .is_some_and(|kind| crate::providers::KINDS.contains(&kind));
        // Catalogs cannot install new transport or quota collectors.
        self.model_discovery.implemented = false;
        self.quota.implemented = false;
        let mut formats = BTreeSet::new();
        for endpoint in &self.default_endpoints {
            if !formats.insert(&endpoint.protocol)
                || !endpoint.path.starts_with('/')
                || endpoint.path.starts_with("//")
                || endpoint.path.contains("://")
                || endpoint.path.contains(['\r', '\n', '\0', '?', '#'])
            {
                return Err(invalid("invalid or duplicate provider endpoint"));
            }
            if matches!(endpoint.transport, Transport::Websocket)
                && endpoint.protocol != "responses"
            {
                return Err(invalid("WebSocket transport only supports responses"));
            }
        }
        let valid_logo = self.logo_key.split_once(':').is_some_and(|(kind, key)| {
            matches!(kind, "lobehub" | "simple-icons" | "initials") && identifier(key)
        });
        if !valid_logo
            || self.logo_fallback.is_empty()
            || self.logo_fallback.chars().count() > 3
            || !self.logo_fallback.chars().all(char::is_alphanumeric)
        {
            return Err(invalid("provider logo key or initials fallback is invalid"));
        }
        if self.brand_color.as_ref().is_some_and(|color| {
            color.len() != 7
                || !color.starts_with('#')
                || !color[1..].bytes().all(|c| c.is_ascii_hexdigit())
        }) {
            return Err(invalid("invalid brand color"));
        }
        Ok(())
    }
}

impl Model {
    pub fn validate(&self) -> Result<(), ApiError> {
        self.source.validate()?;
        if !identifier(&self.id)
            || !identifier(&self.upstream_id)
            || !identifier(&self.developer)
            || !identifier(&self.model_type)
            || self.name.is_empty()
            || self.name.len() > 256
        {
            return Err(invalid("invalid model identity"));
        }
        if self.protocols.iter().any(|v| !identifier(v))
            || self.aliases.iter().any(|v| !identifier(v))
            || self.aliases.len() > 64
        {
            return Err(invalid("invalid model protocol or alias"));
        }
        if self
            .modalities
            .input
            .iter()
            .chain(&self.modalities.output)
            .any(|v| !identifier(v))
        {
            return Err(invalid("invalid model modality"));
        }
        for cost in [
            self.cost_defaults.input,
            self.cost_defaults.output,
            self.cost_defaults.cache_read,
            self.cost_defaults.cache_write,
        ]
        .into_iter()
        .flatten()
        {
            if !cost.is_finite() || cost < 0.0 {
                return Err(invalid("model costs must be finite and nonnegative"));
            }
        }
        Ok(())
    }

    pub fn gateway_capabilities(&self) -> Vec<String> {
        let capability = match self.model_type.as_str() {
            "embedding" | "embeddings" => "embeddings",
            "rerank" => "rerank",
            "image-generation" | "image" => "images",
            "video" | "video-generation" => "videos",
            "speech" | "text-to-speech" => "speech",
            "transcription" | "speech-to-text" => "transcriptions",
            "moderation" => "moderations",
            "chat" => "chat",
            _ => return vec![],
        };
        if capability == "chat" {
            vec![
                "chat".into(),
                "responses".into(),
                "messages".into(),
                "gemini".into(),
            ]
        } else {
            vec![capability.into()]
        }
    }
}
