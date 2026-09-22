//! Exact-revision LiteLLM integration. Upstream values do not escape this module.
use super::invalid;
use crate::api::ApiError;
use litellm_llms::{
    anthropic::chat::transformation::AnthropicConfig,
    base_llm::chat::transformation::{BaseConfig, ProviderChatResponseData},
    bedrock::chat::converse_transformation::AmazonConverseConfig,
};
use serde_json::{Map, Value, json};
use std::{collections::HashMap, sync::Mutex};

const MAX_UPSTREAM_CLIENTS: usize = 1024;

pub struct ProxySettings<'a> {
    pub url: &'a str,
    pub username: Option<&'a str>,
    pub password: Option<&'a str>,
    pub reuse_connections: bool,
}

pub struct ClientPool {
    clients: Mutex<HashMap<String, reqwest::Client>>,
    max_capacity: usize,
}

impl Default for ClientPool {
    fn default() -> Self {
        Self {
            clients: Mutex::new(HashMap::new()),
            max_capacity: MAX_UPSTREAM_CLIENTS,
        }
    }
}

impl ClientPool {
    pub fn for_proxy(
        &self,
        default: &reqwest::Client,
        timeout: std::time::Duration,
        settings: Option<ProxySettings<'_>>,
    ) -> Result<reqwest::Client, reqwest::Error> {
        let Some(settings) = settings else {
            return Ok(default.clone());
        };
        if !settings.reuse_connections {
            return http_client_with_proxy(timeout, Some(settings));
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(settings.url.as_bytes());
        hasher.update(&[0]);
        hasher.update(settings.username.unwrap_or_default().as_bytes());
        hasher.update(&[0]);
        hasher.update(settings.password.unwrap_or_default().as_bytes());
        let key = hasher.finalize().to_hex().to_string();
        if let Some(client) = self
            .clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
            .cloned()
        {
            return Ok(client);
        }
        let client = http_client_with_proxy(timeout, Some(settings))?;
        let mut clients = self
            .clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = clients.get(&key) {
            return Ok(existing.clone());
        }
        if clients.len() >= self.max_capacity {
            clients.clear();
        }
        clients.insert(key, client.clone());
        Ok(client)
    }

    #[cfg(test)]
    pub(crate) fn with_capacity(max_capacity: usize) -> Self {
        assert!(max_capacity > 0);
        Self {
            clients: Mutex::new(HashMap::new()),
            max_capacity,
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

fn request(
    config: &dyn BaseConfig,
    provider: &str,
    model: &str,
    payload: &Value,
) -> Result<Value, ApiError> {
    let mut params = payload
        .as_object()
        .ok_or_else(|| invalid("chat body must be an object"))?
        .clone();
    params.remove("model");
    let messages = params
        .remove("messages")
        .ok_or_else(|| invalid("messages required"))?;
    let mut mapped = Map::new();
    for (key, value) in params {
        let destination = config
            .supported_openai_param_mappings()
            .iter()
            .find(|(source, _)| *source == key)
            .map(|(_, destination)| *destination)
            .unwrap_or(&key);
        mapped.insert(destination.to_owned(), value);
    }
    if let Some(reason) = litellm_core::chat_completions::chat_completions_decline_reason(
        model,
        Some(provider),
        messages.clone(),
        &mapped,
    ) {
        return Err(invalid(format!(
            "unsupported {provider} transformation: {reason}"
        )));
    }
    let messages: Vec<litellm_types::llms::openai::ChatMessage> =
        serde_json::from_value(messages).map_err(|_| invalid("unsupported chat messages"))?;
    config
        .transform_request(model, messages, mapped)
        .map(|request| request.body)
        .map_err(|_| invalid("unsupported provider request"))
}

pub fn bedrock_request(model: &str, payload: &Value) -> Result<Value, ApiError> {
    request(&AmazonConverseConfig, "bedrock", model, payload)
}

pub fn anthropic_text_request(model: &str, payload: &Value) -> Option<Value> {
    request(&AnthropicConfig, "anthropic", model, payload).ok()
}

pub fn bedrock_response(model: &str, body: Value) -> Result<Value, ApiError> {
    let converted = AmazonConverseConfig
        .transform_response(model, ProviderChatResponseData { body })
        .map_err(|_| ApiError::Upstream("unsupported Bedrock response shape".into()))?;
    let mut value = serde_json::to_value(converted).map_err(|e| ApiError::Internal(e.into()))?;
    value["object"] = json!("chat.completion");
    value["id"] = json!(format!("chatcmpl_{}", uuid::Uuid::new_v4().simple()));
    Ok(value)
}

pub fn http_client(timeout: std::time::Duration) -> Result<reqwest::Client, reqwest::Error> {
    http_client_with_proxy(timeout, None)
}

pub fn http_client_with_proxy(
    timeout: std::time::Duration,
    proxy: Option<ProxySettings<'_>>,
) -> Result<reqwest::Client, reqwest::Error> {
    let settings = litellm_http::HttpSettings::default();
    let configuration = litellm_http::Resolution::from(&settings).config;
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("pangolin/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(configuration.connect_timeout)
        .pool_idle_timeout(configuration.pool_idle_timeout)
        .timeout(timeout);
    if let Some(proxy) = proxy {
        let mut configured = reqwest::Proxy::all(proxy.url)?;
        if let Some(username) = proxy.username {
            configured = configured.basic_auth(username, proxy.password.unwrap_or_default());
        }
        builder = builder.proxy(configured);
        if !proxy.reuse_connections {
            builder = builder.pool_max_idle_per_host(0);
        }
    }
    builder.build()
}
