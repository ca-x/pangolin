//! Provider implementations are deliberately hidden behind this Pangolin boundary.
//! No LiteLLM host, Python bridge or callback controls gateway orchestration.
mod cloud;
pub mod discovery;
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
pub use upstream::{ClientPool, ProxySettings, http_client};

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

/// Whether this request asked for a streamed response, decided once when the
/// request is admitted.
///
/// This is the protocol's own decision, not something to infer later from a
/// response or from a captured payload: the endpoints that accept a `stream` field
/// answer with it (an absent field means the same thing there as `false`, because
/// that is how the gateway encodes the response), the Gemini streaming action
/// answers by URL shape, and an endpoint with no stream choice has no answer to
/// record — `None`, which the console prints as unmeasured rather than as a `false`
/// nobody decided.
pub fn streamed(endpoint: &str, payload: &Value) -> Option<bool> {
    match endpoint {
        "/v1beta/models:streamGenerateContent" => Some(true),
        "/v1beta/models:generateContent" => Some(false),
        _ => {
            if !matches!(
                capability(endpoint),
                "chat" | "completions" | "responses" | "messages" | "gemini"
            ) {
                return None;
            }
            match payload.get("stream") {
                // The gateway answers a missing flag the same way it answers
                // `false`, so that is this request's decision too.
                None => Some(false),
                Some(Value::Bool(value)) => Some(*value),
                // A value that is not a boolean is not a stream decision. The
                // gateway refuses such a request before it is admitted, so this
                // only ever keeps the record from claiming one.
                Some(_) => None,
            }
        }
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
    match (
        target.provider_kind.as_str(),
        target.credential_type.as_str(),
    ) {
        ("gemini", "oauth_antigravity") => {
            if endpoint == "/v1/chat/completions" {
                payload = transforms::chat_to_gemini(&payload)?;
                response = ResponseTransform::Antigravity(Box::new(ResponseTransform::GeminiChat(
                    target.public_name.clone(),
                )));
            } else if matches!(
                endpoint,
                "/v1beta/models:generateContent" | "/v1beta/models:streamGenerateContent"
            ) {
                response = ResponseTransform::Antigravity(Box::new(ResponseTransform::Identity));
            } else {
                return Err(invalid("unsupported Antigravity operation"));
            }
            payload
                .as_object_mut()
                .ok_or_else(|| invalid("request must be an object"))?
                .remove("model");
            payload.as_object_mut().unwrap().remove("stream");
            let project = cloud::antigravity_project(secret)?;
            payload = serde_json::json!({
                "project": project,
                "model": target.upstream_name,
                "request": payload,
                "requestType": "agent",
                "userAgent": "pangolin",
                "requestId": format!("agent-{}", uuid::Uuid::new_v4()),
            });
            path = if endpoint == "/v1beta/models:streamGenerateContent" {
                "/v1internal:streamGenerateContent?alt=sse".into()
            } else {
                "/v1internal:generateContent".into()
            };
            headers.insert(
                header::USER_AGENT,
                HeaderValue::from_static(concat!("pangolin/", env!("CARGO_PKG_VERSION"))),
            );
            headers.insert(
                "x-goog-api-client",
                HeaderValue::from_static("google-cloud-sdk vscode_cloudshelleditor/0.1"),
            );
            headers.insert(
                "client-metadata",
                HeaderValue::from_static(
                    r#"{"ideType":"ANTIGRAVITY","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#,
                ),
            );
        }
        ("openai", "oauth_github_copilot") => {
            if endpoint != "/v1/chat/completions" {
                return Err(invalid("unsupported GitHub Copilot operation"));
            }
            path = "/chat/completions".into();
            for (name, value) in [
                ("editor-version", "vscode/1.95.0"),
                ("editor-plugin-version", "copilot-chat/0.26.7"),
                ("user-agent", "GitHubCopilotChat/0.26.7"),
                ("openai-intent", "conversation-edits"),
                ("copilot-integration-id", "vscode-chat"),
                ("x-github-api-version", "2025-04-01"),
                ("x-vscode-user-agent-library-version", "electron-fetch"),
            ] {
                headers.insert(
                    http::header::HeaderName::from_static(name),
                    HeaderValue::from_static(value),
                );
            }
            if copilot_has_vision(&payload) {
                headers.insert("copilot-vision-request", HeaderValue::from_static("true"));
            }
            let inferred = payload["messages"]
                .as_array()
                .and_then(|messages| messages.last())
                .map(|message| {
                    let tool_result = message["content"]
                        .as_array()
                        .and_then(|parts| parts.last())
                        .and_then(|part| part["type"].as_str())
                        == Some("tool_result");
                    if message["role"] == "user" && !tool_result {
                        "user"
                    } else {
                        "agent"
                    }
                })
                .unwrap_or("user");
            let initiator = inbound
                .get("x-initiator")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| matches!(*value, "user" | "agent"))
                .unwrap_or(inferred);
            headers.insert("x-initiator", HeaderValue::from_str(initiator).unwrap());
        }
        ("gemini" | "vertex" | "gcp", _) => {
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
        ("bedrock", _) => {
            payload = upstream::bedrock_request(&target.upstream_name, &payload)?;
            response = ResponseTransform::Bedrock(target.public_name.clone());
            path = format!("/model/{}/converse", segment(&target.upstream_name));
        }
        (_, _) if endpoint.starts_with("/v1beta/models:") => {
            payload = transforms::gemini_to_chat(&payload)?;
            path = "/v1/chat/completions".into();
            response = ResponseTransform::ChatGemini;
        }
        (_, _) if endpoint == "/v1/messages" && target.provider_kind != "anthropic" => {
            payload = transforms::messages_to_chat(&payload)?;
            path = "/v1/chat/completions".into();
            response = ResponseTransform::ChatMessages(target.public_name.clone());
        }
        (_, _) => {}
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

fn copilot_has_vision(payload: &Value) -> bool {
    payload["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|message| {
            message["content"].as_array().is_some_and(|parts| {
                parts.iter().any(|part| {
                    part["type"] == "image_url"
                        || part.get("image_url").is_some_and(|value| !value.is_null())
                        || part["text"]
                            .as_str()
                            .is_some_and(|text| text.starts_with("data:image/"))
                })
            }) || message["content"]
                .as_str()
                .is_some_and(|text| text.starts_with("data:image/"))
        })
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
