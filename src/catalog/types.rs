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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCardProjection {
    pub developer: Option<String>,
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    pub logo_key: Option<String>,
    pub limits: ModelCardLimitsProjection,
    pub cost_defaults: ModelCardCostsProjection,
    pub capabilities: ModelCardCapabilitiesProjection,
    pub modalities: ModelCardModalitiesProjection,
    pub reasoning_levels: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCardLimitsProjection {
    pub context: Option<u64>,
    pub output: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCardCostsProjection {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub currency: Option<String>,
    pub unit: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCardCapabilitiesProjection {
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
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCardModalitiesProjection {
    pub input: Option<Vec<String>>,
    pub output: Option<Vec<String>>,
}

fn bounded_fact_list(value: Option<&Value>) -> Option<Vec<String>> {
    let values = value?.as_array()?;
    if values.len() > 16 {
        return None;
    }
    let mut result = Vec::new();
    for value in values {
        let text = value.as_str()?;
        if text.is_empty() || text.len() > 32 || text.chars().any(char::is_control) {
            return None;
        }
        if !result.iter().any(|item| item == text) {
            result.push(text.to_owned());
        }
    }
    Some(result)
}

fn common_fact<T: Clone + PartialEq>(values: impl Iterator<Item = Option<T>>) -> Option<T> {
    let mut values = values;
    let first = values.next()??;
    values
        .all(|value| value.as_ref() == Some(&first))
        .then_some(first)
}

fn common_limit(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    values
        .map(|value| value.filter(|value| *value > 0))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .min()
}

fn common_capability(values: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let values: Vec<_> = values.collect();
    if values.contains(&Some(false)) {
        Some(false)
    } else if !values.is_empty() && values.iter().all(|value| *value == Some(true)) {
        Some(true)
    } else {
        None
    }
}

fn common_set(values: impl Iterator<Item = Option<Vec<String>>>) -> Option<Vec<String>> {
    let mut values = values;
    let mut result = values.next()??;
    for value in values {
        let known = value?;
        result.retain(|item| known.contains(item));
    }
    Some(result)
}

fn merge_known<T>(left: Option<T>, right: Option<T>, merge: impl FnOnce(T, T) -> T) -> Option<T> {
    match (left, right) {
        (Some(left), Some(right)) => Some(merge(left, right)),
        (left, right) => left.or(right),
    }
}

fn intersect_known(left: Option<Vec<String>>, right: Option<Vec<String>>) -> Option<Vec<String>> {
    merge_known(left, right, |mut left, right| {
        left.retain(|item| right.contains(item));
        left
    })
}

impl ModelCardProjection {
    /// Combine per-credential discovery with independently configured facts.
    /// Explicit restrictions from either source win; local branding/prices win.
    /// A discovery-managed channel aggregate must never be passed as fallback.
    pub fn with_fallback(mut self, fallback: Option<&Self>) -> Self {
        if let Some(fallback) = fallback {
            self.developer = fallback.developer.clone().or(self.developer);
            self.model_type = fallback.model_type.clone().or(self.model_type);
            self.logo_key = fallback.logo_key.clone().or(self.logo_key);
            // Keep a configured price document together: borrowing a missing
            // amount from a provider with another unit/currency invents a price.
            if fallback.cost_defaults != ModelCardCostsProjection::default() {
                self.cost_defaults = fallback.cost_defaults.clone();
            }
            self.limits.context =
                merge_known(self.limits.context, fallback.limits.context, u64::min);
            self.limits.output = merge_known(self.limits.output, fallback.limits.output, u64::min);
            self.modalities.input =
                intersect_known(self.modalities.input, fallback.modalities.input.clone());
            self.modalities.output =
                intersect_known(self.modalities.output, fallback.modalities.output.clone());
            self.reasoning_levels =
                intersect_known(self.reasoning_levels, fallback.reasoning_levels.clone());
            self.capabilities.streaming = merge_known(
                self.capabilities.streaming,
                fallback.capabilities.streaming,
                |a, b| a && b,
            );
            self.capabilities.tools = merge_known(
                self.capabilities.tools,
                fallback.capabilities.tools,
                |a, b| a && b,
            );
            self.capabilities.reasoning = merge_known(
                self.capabilities.reasoning,
                fallback.capabilities.reasoning,
                |a, b| a && b,
            );
            self.capabilities.temperature = merge_known(
                self.capabilities.temperature,
                fallback.capabilities.temperature,
                |a, b| a && b,
            );
            self.capabilities.vision = merge_known(
                self.capabilities.vision,
                fallback.capabilities.vision,
                |a, b| a && b,
            );
            self.capabilities.json_schema = merge_known(
                self.capabilities.json_schema,
                fallback.capabilities.json_schema,
                |a, b| a && b,
            );
            self.capabilities.web_search = merge_known(
                self.capabilities.web_search,
                fallback.capabilities.web_search,
                |a, b| a && b,
            );
            self.capabilities.file_search = merge_known(
                self.capabilities.file_search,
                fallback.capabilities.file_search,
                |a, b| a && b,
            );
            self.capabilities.computer_use = merge_known(
                self.capabilities.computer_use,
                fallback.capabilities.computer_use,
                |a, b| a && b,
            );
            self.capabilities.caching = merge_known(
                self.capabilities.caching,
                fallback.capabilities.caching,
                |a, b| a && b,
            );
            self.capabilities.batch = merge_known(
                self.capabilities.batch,
                fallback.capabilities.batch,
                |a, b| a && b,
            );
        }
        self
    }

    /// Aggregate only candidates admitted in the same endpoint and key context.
    pub fn aggregate(cards: &[Option<Self>]) -> Option<Self> {
        if !cards.iter().any(Option::is_some) {
            return None;
        }
        let cards: Vec<_> = cards
            .iter()
            .map(|card| card.clone().unwrap_or_default())
            .collect();
        Some(Self {
            developer: common_fact(cards.iter().map(|card| card.developer.clone())),
            model_type: common_fact(cards.iter().map(|card| card.model_type.clone())),
            logo_key: common_fact(cards.iter().map(|card| card.logo_key.clone())),
            limits: ModelCardLimitsProjection {
                context: common_limit(cards.iter().map(|card| card.limits.context)),
                output: common_limit(cards.iter().map(|card| card.limits.output)),
            },
            cost_defaults: ModelCardCostsProjection {
                input: common_fact(cards.iter().map(|card| card.cost_defaults.input)),
                output: common_fact(cards.iter().map(|card| card.cost_defaults.output)),
                currency: common_fact(cards.iter().map(|card| card.cost_defaults.currency.clone())),
                unit: common_fact(cards.iter().map(|card| card.cost_defaults.unit.clone())),
            },
            capabilities: ModelCardCapabilitiesProjection {
                streaming: common_capability(cards.iter().map(|card| card.capabilities.streaming)),
                tools: common_capability(cards.iter().map(|card| card.capabilities.tools)),
                reasoning: common_capability(cards.iter().map(|card| card.capabilities.reasoning)),
                temperature: common_capability(
                    cards.iter().map(|card| card.capabilities.temperature),
                ),
                vision: common_capability(cards.iter().map(|card| card.capabilities.vision)),
                json_schema: common_capability(
                    cards.iter().map(|card| card.capabilities.json_schema),
                ),
                web_search: common_capability(
                    cards.iter().map(|card| card.capabilities.web_search),
                ),
                file_search: common_capability(
                    cards.iter().map(|card| card.capabilities.file_search),
                ),
                computer_use: common_capability(
                    cards.iter().map(|card| card.capabilities.computer_use),
                ),
                caching: common_capability(cards.iter().map(|card| card.capabilities.caching)),
                batch: common_capability(cards.iter().map(|card| card.capabilities.batch)),
            },
            modalities: ModelCardModalitiesProjection {
                input: common_set(cards.iter().map(|card| card.modalities.input.clone())),
                output: common_set(cards.iter().map(|card| card.modalities.output.clone())),
            },
            reasoning_levels: common_set(cards.iter().map(|card| card.reasoning_levels.clone())),
        })
    }
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
                    capabilities: ModelCardCapabilitiesProjection {
                        streaming: card
                            .pointer("/capabilities/streaming")
                            .and_then(Value::as_bool),
                        tools: card.pointer("/capabilities/tools").and_then(Value::as_bool),
                        reasoning: card
                            .pointer("/capabilities/reasoning")
                            .and_then(Value::as_bool),
                        temperature: card
                            .pointer("/capabilities/temperature")
                            .and_then(Value::as_bool),
                        vision: card
                            .pointer("/capabilities/vision")
                            .and_then(Value::as_bool),
                        json_schema: card
                            .pointer("/capabilities/json_schema")
                            .and_then(Value::as_bool),
                        web_search: card
                            .pointer("/capabilities/web_search")
                            .and_then(Value::as_bool),
                        file_search: card
                            .pointer("/capabilities/file_search")
                            .and_then(Value::as_bool),
                        computer_use: card
                            .pointer("/capabilities/computer_use")
                            .and_then(Value::as_bool),
                        caching: card
                            .pointer("/capabilities/caching")
                            .and_then(Value::as_bool),
                        batch: card.pointer("/capabilities/batch").and_then(Value::as_bool),
                    },
                    modalities: ModelCardModalitiesProjection {
                        input: bounded_fact_list(card.pointer("/modalities/input")),
                        output: bounded_fact_list(card.pointer("/modalities/output")),
                    },
                    reasoning_levels: bounded_fact_list(card.get("reasoning_levels")),
                    developer: bounded_text(card.get("developer")),
                    model_type: bounded_text(card.get("type")),
                    logo_key: bounded_text(object.get("logo_key"))
                        .or_else(|| bounded_text(card.get("logo_key"))),
                    limits: ModelCardLimitsProjection {
                        context: card
                            .pointer("/limits/context")
                            .and_then(Value::as_u64)
                            .filter(|value| *value > 0),
                        output: card
                            .pointer("/limits/output")
                            .and_then(Value::as_u64)
                            .filter(|value| *value > 0),
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
        // Catalogs cannot install code. Discovery is available only when this
        // build owns the adapter and the catalog names its bounded endpoint.
        self.model_discovery.implemented = self
            .adapter_kind
            .as_deref()
            .is_some_and(crate::providers::discovery::adapter_supported)
            && self.model_discovery.upstream_endpoint.is_some();
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

#[cfg(test)]
mod reference_metadata_tests {
    use super::*;
    use serde_json::json;
    fn card(value: Value) -> ModelCardProjection {
        StoredModelMetadata::parse(&json!({"card":value}).to_string())
            .unwrap()
            .card
            .unwrap()
    }
    #[test]
    fn reference_metadata_bounds_unknown_sets_and_limits() {
        let malformed = card(
            json!({"capabilities":{"vision":"true","arbitrary":"never-public"},"limits":{"context":0,"output":-1},"modalities":{"input":["image",7],"output":[]},"reasoning_levels":["x".repeat(33)]}),
        );
        assert_eq!(malformed.capabilities.vision, None);
        assert_eq!(malformed.modalities.input, None);
        assert_eq!(malformed.modalities.output, Some(vec![]));
        assert_eq!(malformed.reasoning_levels, None);
        assert_eq!(malformed.limits.context, None);
        assert!(
            !serde_json::to_string(&malformed)
                .unwrap()
                .contains("arbitrary")
        );
        assert!(
            card(json!({"reasoning_levels":vec!["low";17]}))
                .reasoning_levels
                .is_none()
        );
        let a = card(
            json!({"capabilities":{"tools":true,"vision":false},"limits":{"context":100,"output":100},"modalities":{"input":["text","image"]},"reasoning_levels":["low","high"]}),
        );
        let b = card(
            json!({"capabilities":{"tools":true},"limits":{"context":80},"modalities":{"input":["text"]},"reasoning_levels":["high"]}),
        );
        let result = ModelCardProjection::aggregate(&[Some(a.clone()), Some(b)]).unwrap();
        assert_eq!(result.limits.context, Some(80));
        assert_eq!(result.limits.output, None);
        assert_eq!(result.capabilities.tools, Some(true));
        assert_eq!(result.capabilities.vision, Some(false));
        assert_eq!(result.modalities.input, Some(vec!["text".into()]));
        assert_eq!(result.reasoning_levels, Some(vec!["high".into()]));
        let result = ModelCardProjection::aggregate(&[Some(a), None]).unwrap();
        assert_eq!(result.capabilities.tools, None);
        assert_eq!(result.capabilities.vision, Some(false));
        assert_eq!(result.modalities.input, None);
        assert_eq!(result.limits.context, None);
    }
    #[test]
    fn reference_metadata_manual_restrictions_survive_discovery() {
        let configured = card(
            json!({"developer":"local","cost_defaults":{"input":12,"currency":"USD"},"limits":{"output":80},"capabilities":{"tools":false,"vision":true},"modalities":{"input":["text"]},"reasoning_levels":["low"]}),
        );
        let discovered = card(
            json!({"developer":"remote","cost_defaults":{"input":1,"output":2,"currency":"EUR"},"limits":{"output":100},"capabilities":{"tools":true,"vision":false},"modalities":{"input":["text","image"]},"reasoning_levels":["low","high"]}),
        );
        let result = discovered.with_fallback(Some(&configured));
        assert_eq!(result.capabilities.tools, Some(false));
        assert_eq!(result.capabilities.vision, Some(false));
        assert_eq!(result.limits.output, Some(80));
        assert_eq!(result.modalities.input, Some(vec!["text".into()]));
        assert_eq!(result.reasoning_levels, Some(vec!["low".into()]));
        assert_eq!(result.developer.as_deref(), Some("local"));
        assert_eq!(result.cost_defaults.input, Some(12.0));
        assert_eq!(result.cost_defaults.output, None);
        assert_eq!(result.cost_defaults.currency.as_deref(), Some("USD"));
    }
}
