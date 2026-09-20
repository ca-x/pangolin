//! Provider implementations are deliberately hidden behind this Pangolin boundary.
//! No LiteLLM host, Python bridge or callback controls gateway orchestration.
mod cloud;
pub mod framing;
#[cfg(test)]
mod tests;
pub mod tokens;
mod transforms;
mod upstream;

use crate::{api::ApiError, models::RouteTarget};
use http::{HeaderMap, HeaderValue, Method, header};
use serde_json::Value;
pub use transforms::ResponseTransform;
pub use upstream::anthropic_text_request;
pub use upstream::http_client;

pub const KINDS: &[&str] = &[
    "openai",
    "openai_compatible",
    "anthropic",
    "gemini",
    "azure",
    "bedrock",
    "vertex",
    "gcp",
    "openrouter",
    "deepseek",
    "moonshot",
    "zhipu",
    "doubao",
    "xai",
    "groq",
    "ollama",
    "nanogpt",
    "jina",
];
pub const ENDPOINTS: &[&str] = &[
    "/v1/chat/completions",
    "/v1/completions",
    "/v1/responses",
    "/v1/responses/compact",
    "/v1/messages",
    "/v1/embeddings",
    "/v1/moderations",
    "/v1/alpha/search",
    "/v1/images/generations",
    "/v1/images/edits",
    "/v1/videos",
    "/v1/audio/speech",
    "/v1/audio/transcriptions",
    "/v1/audio/translations",
    "/v1/rerank",
    "/v1beta/models:generateContent",
    "/v1beta/models:streamGenerateContent",
    "/doubao/v3/contents/generations/tasks",
];

pub fn default_base(kind: &str) -> Option<&'static str> {
    match kind {
        "openai" => Some("https://api.openai.com/v1"),
        "anthropic" => Some("https://api.anthropic.com"),
        "gemini" => Some("https://generativelanguage.googleapis.com"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "moonshot" => Some("https://api.moonshot.cn/v1"),
        "zhipu" => Some("https://open.bigmodel.cn/api/paas/v4"),
        "doubao" => Some("https://ark.cn-beijing.volces.com/api/v3"),
        "xai" => Some("https://api.x.ai/v1"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "ollama" => Some("http://127.0.0.1:11434/v1"),
        "nanogpt" => Some("https://nano-gpt.com/api/v1"),
        "jina" => Some("https://api.jina.ai/v1"),
        _ => None,
    }
}

pub fn capability(endpoint: &str) -> &str {
    match endpoint {
        "/v1/chat/completions" => "chat",
        "/v1/completions" => "completions",
        "/v1/responses" | "/v1/responses/compact" => "responses",
        "/v1/messages" => "messages",
        "/v1/embeddings" => "embeddings",
        "/v1/moderations" => "moderations",
        "/v1/alpha/search" => "search",
        "/v1/images/generations" | "/v1/images/edits" => "images",
        "/v1/videos" | "/doubao/v3/contents/generations/tasks" => "videos",
        "/v1/audio/speech" => "speech",
        "/v1/audio/transcriptions" => "transcriptions",
        "/v1/audio/translations" => "translations",
        "/v1/rerank" => "rerank",
        "/v1beta/models:generateContent" | "/v1beta/models:streamGenerateContent" => "gemini",
        other => other,
    }
}

pub fn supports(kind: &str, capabilities: &[String], endpoint: &str, stream: bool) -> bool {
    let stream = stream || endpoint == "/v1beta/models:streamGenerateContent";
    let configured = capabilities.iter().any(|c| {
        c == capability(endpoint)
            || c == endpoint
            || (c == "chat" && matches!(capability(endpoint), "messages" | "gemini"))
    });
    if !configured {
        return false;
    }
    match kind {
        "anthropic" => {
            matches!(endpoint, "/v1/messages") || (endpoint == "/v1/chat/completions" && !stream)
        }
        "gemini" | "vertex" | "gcp" => {
            matches!(capability(endpoint), "gemini" | "embeddings")
                || (capability(endpoint) == "chat" && !stream)
        }
        "bedrock" => endpoint == "/v1/chat/completions" && !stream,
        _ => !(stream && matches!(capability(endpoint), "messages" | "gemini")),
    }
}

pub struct Prepared {
    pub url: String,
    pub headers: HeaderMap,
    pub payload: Value,
    pub response: ResponseTransform,
}

#[cfg(test)]
pub async fn prepare(
    target: &RouteTarget,
    endpoint: &str,
    payload: &Value,
    secret: &str,
    headers: HeaderMap,
    inbound: &HeaderMap,
    native_version: Option<&str>,
) -> Result<Prepared, ApiError> {
    prepare_routed(
        target,
        Route {
            protocol: endpoint,
            path: endpoint,
            native_version,
        },
        payload,
        secret,
        headers,
        inbound,
    )
    .await
}

pub struct Route<'a> {
    pub protocol: &'a str,
    pub path: &'a str,
    pub native_version: Option<&'a str>,
}

pub async fn prepare_routed(
    target: &RouteTarget,
    route: Route<'_>,
    payload: &Value,
    secret: &str,
    mut headers: HeaderMap,
    inbound: &HeaderMap,
) -> Result<Prepared, ApiError> {
    let endpoint = route.protocol;
    let native_version = route.native_version;
    let mut payload = payload.clone();
    let mut path = route.path.to_owned();
    let mut response = ResponseTransform::Identity;
    match target.provider_kind.as_str() {
        "gemini" | "vertex" | "gcp" => {
            if endpoint == "/v1/chat/completions" {
                payload = transforms::chat_to_gemini(&payload)?;
                response = ResponseTransform::GeminiChat(target.public_name.clone());
            } else if endpoint == "/v1/embeddings" {
                payload = transforms::embedding_to_gemini(&payload)?;
                response = ResponseTransform::GeminiEmbedding;
            }
            let action = if endpoint == "/v1/embeddings" {
                "batchEmbedContents"
            } else if payload["stream"] == true {
                "streamGenerateContent"
            } else {
                "generateContent"
            };
            payload
                .as_object_mut()
                .ok_or_else(|| invalid("request must be an object"))?
                .remove("model");
            payload.as_object_mut().unwrap().remove("stream");
            let version = native_version.unwrap_or(
                if target.base_url.trim_end_matches('/').ends_with("/v1") {
                    "v1"
                } else {
                    "v1beta"
                },
            );
            path = format!(
                "/{version}/models/{}:{action}",
                segment(&target.upstream_name)
            );
            if action == "streamGenerateContent" {
                path.push_str("?alt=sse");
            }
        }
        "bedrock" => {
            payload = upstream::bedrock_request(&target.upstream_name, &payload)?;
            response = ResponseTransform::Bedrock(target.public_name.clone());
            path = format!("/model/{}/converse", segment(&target.upstream_name));
        }
        _ if endpoint.starts_with("/v1beta/models:") => {
            payload = transforms::gemini_to_chat(&payload)?;
            path = "/v1/chat/completions".into();
            response = ResponseTransform::ChatGemini;
        }
        _ if endpoint == "/v1/messages" && target.provider_kind != "anthropic" => {
            payload = transforms::messages_to_chat(&payload)?;
            path = "/v1/chat/completions".into();
            response = ResponseTransform::ChatMessages(target.public_name.clone());
        }
        _ => {}
    }
    if route.path != endpoint && !route.path.contains('{') {
        path = route.path.to_owned();
    }
    if path.starts_with("/doubao/v3/") {
        path = path.replacen("/doubao/v3/", "/api/v3/", 1);
    }
    // Client credentials can never override the encrypted channel credential.
    for name in ["authorization", "x-api-key", "api-key", "x-goog-api-key"] {
        headers.remove(name);
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let mut base = target.base_url.trim_end_matches('/');
    if matches!(target.provider_kind.as_str(), "gemini" | "vertex" | "gcp") {
        for suffix in ["/v1beta", "/v1alpha", "/v1"] {
            if let Some(root) = base.strip_suffix(suffix) {
                base = root;
                break;
            }
        }
    }
    if (target.provider_kind == "zhipu" && base.ends_with("/v4")
        || target.provider_kind == "doubao" && base.ends_with("/api/v3")
        || target.provider_kind == "azure" && base.contains("/openai/"))
        && path.starts_with("/v1/")
    {
        path = path.trim_start_matches("/v1").into();
    }
    let mut url = join(base, &path);
    cloud::authenticate(target, secret, &payload, &mut url, &mut headers).await?;
    if target.provider_kind == "anthropic" {
        for name in ["anthropic-version", "anthropic-beta"] {
            if let Some(value) = inbound.get(name) {
                headers.insert(name, value.clone());
            }
        }
        headers
            .entry("anthropic-version")
            .or_insert(HeaderValue::from_static("2023-06-01"));
    }
    Ok(Prepared {
        url,
        headers,
        payload,
        response,
    })
}

pub fn join(base: &str, endpoint: &str) -> String {
    let base = base.trim_end_matches('/');
    for prefix in ["/v1beta", "/v1", "/api/v3"] {
        if base.ends_with(prefix)
            && let Some(suffix) = endpoint.strip_prefix(prefix)
        {
            return format!("{base}{suffix}");
        }
    }
    format!("{base}{endpoint}")
}

pub fn segment(value: &str) -> String {
    let mut url = reqwest::Url::parse("https://placeholder.invalid/").unwrap();
    url.path_segments_mut().unwrap().clear().push(value);
    url.path().trim_start_matches('/').to_owned()
}

pub fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::BadRequest(message.into())
}

pub fn ensure_fields(value: &Value, allowed: &[&str]) -> Result<(), ApiError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("request must be a JSON object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid(format!("unsupported cross-protocol field: {key}")));
    }
    Ok(())
}

pub fn is_media(endpoint: &str) -> bool {
    matches!(
        capability(endpoint),
        "images" | "videos" | "speech" | "transcriptions" | "translations"
    )
}

pub fn request_method(delete: bool) -> Method {
    if delete { Method::DELETE } else { Method::GET }
}
