//! Exact-revision LiteLLM integration. Upstream values do not escape this module.
use super::invalid;
use crate::api::ApiError;
use litellm_llms::{
    anthropic::chat::transformation::AnthropicConfig,
    base_llm::chat::transformation::{BaseConfig, ProviderChatResponseData},
    bedrock::chat::converse_transformation::AmazonConverseConfig,
};
use serde_json::{Map, Value, json};

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
    let settings = litellm_http::HttpSettings::default();
    let configuration = litellm_http::Resolution::from(&settings).config;
    reqwest::Client::builder()
        .user_agent(concat!("pangolin/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(configuration.connect_timeout)
        .pool_idle_timeout(configuration.pool_idle_timeout)
        .timeout(timeout)
        .build()
}
