use crate::api::ApiError;
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_DISCOVERY_BYTES: usize = 1024 * 1024;
pub const MAX_DISCOVERED_MODELS: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredModel {
    pub id: String,
    pub capabilities: Vec<String>,
}

pub fn adapter_supported(kind: &str) -> bool {
    crate::providers::KINDS.contains(&kind) && !matches!(kind, "bedrock" | "vertex" | "gcp")
}

pub async fn models(
    client: &reqwest::Client,
    kind: &str,
    base_url: &str,
    secret: &str,
) -> Result<Vec<DiscoveredModel>, ApiError> {
    let url = discovery_url(kind, base_url)?;
    let request = authenticated_request(client, kind, url, secret)?;
    let response = request
        .send()
        .await
        .map_err(|_| ApiError::Upstream("model discovery request failed".into()))?;
    if !response.status().is_success() {
        return Err(ApiError::Upstream("model discovery request failed".into()));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| ApiError::Upstream("model discovery response failed".into()))?;
        if bytes.len() + chunk.len() > MAX_DISCOVERY_BYTES {
            return Err(ApiError::Upstream(
                "model discovery response exceeds 1 MiB".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    parse(kind, &bytes)
}

fn authenticated_request(
    client: &reqwest::Client,
    kind: &str,
    url: String,
    secret: &str,
) -> Result<reqwest::RequestBuilder, ApiError> {
    let request = client.get(url);
    Ok(match kind {
        "anthropic" => request
            .header("x-api-key", secret)
            .header("anthropic-version", "2023-06-01"),
        "gemini" => request.header("x-goog-api-key", secret),
        "azure" if secret.trim_start().starts_with('{') => {
            let credential: Value = serde_json::from_str(secret).map_err(|_| {
                ApiError::BadRequest(
                    "model discovery is not available for this Azure credential adapter".into(),
                )
            })?;
            if let Some(api_key) = credential.get("api_key").and_then(Value::as_str) {
                request.header("api-key", api_key)
            } else if let Some(token) = credential.get("azure_ad_token").and_then(Value::as_str) {
                request.bearer_auth(token)
            } else {
                return Err(ApiError::BadRequest(
                    "model discovery is not available for this Azure credential adapter".into(),
                ));
            }
        }
        "azure" => request.header("api-key", secret),
        "bedrock" | "vertex" | "gcp" => {
            return Err(ApiError::BadRequest(
                "model discovery is not available for this credential adapter".into(),
            ));
        }
        _ => request.bearer_auth(secret),
    })
}

fn discovery_url(kind: &str, base_url: &str) -> Result<String, ApiError> {
    let base = base_url.trim_end_matches('/');
    let url = match kind {
        "gemini" => {
            let root = ["/v1beta", "/v1alpha", "/v1"]
                .into_iter()
                .find_map(|suffix| base.strip_suffix(suffix))
                .unwrap_or(base);
            format!("{root}/v1beta/models")
        }
        "anthropic" if base.ends_with("/v1") => format!("{base}/models"),
        "anthropic" => format!("{base}/v1/models"),
        _ => format!("{base}/models"),
    };
    let url = reqwest::Url::parse(&url)
        .map_err(|_| ApiError::BadRequest("invalid provider model-discovery URL".into()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::BadRequest(
            "invalid provider model-discovery URL".into(),
        ));
    }
    Ok(url.to_string())
}

fn parse(kind: &str, bytes: &[u8]) -> Result<Vec<DiscoveredModel>, ApiError> {
    let document: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::Upstream("model discovery response is not JSON".into()))?;
    let rows = if kind == "gemini" {
        document.get("models")
    } else {
        document.get("data")
    }
    .and_then(Value::as_array)
    .ok_or_else(|| ApiError::Upstream("model discovery response has no model list".into()))?;
    if rows.len() > MAX_DISCOVERED_MODELS {
        return Err(ApiError::Upstream(
            "model discovery response has too many models".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut models = Vec::with_capacity(rows.len());
    for row in rows {
        let raw = row
            .get(if kind == "gemini" { "name" } else { "id" })
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ApiError::Upstream("model discovery returned an invalid model".into())
            })?;
        let id = raw.strip_prefix("models/").unwrap_or(raw);
        if !crate::catalog::types::identifier(id) {
            return Err(ApiError::Upstream(
                "model discovery returned an invalid model".into(),
            ));
        }
        if seen.insert(id.to_owned()) {
            models.push(DiscoveredModel {
                id: id.to_owned(),
                capabilities: vec!["chat".into()],
            });
        }
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_openai_and_gemini_discovery_without_duplicates() {
        assert_eq!(
            parse("openai", br#"{"data":[{"id":"gpt-4o"},{"id":"gpt-4o"}]}"#).unwrap(),
            vec![DiscoveredModel {
                id: "gpt-4o".into(),
                capabilities: vec!["chat".into()]
            }]
        );
        assert_eq!(
            parse(
                "gemini",
                br#"{"models":[{"name":"models/gemini-2.5-pro"}]}"#
            )
            .unwrap()[0]
                .id,
            "gemini-2.5-pro"
        );
    }

    #[test]
    fn rejects_an_unbounded_or_malformed_model_list() {
        let too_many = json!({
            "data": (0..=MAX_DISCOVERED_MODELS)
                .map(|index| json!({"id":format!("model-{index}")}))
                .collect::<Vec<_>>()
        });
        assert!(parse("openai", &serde_json::to_vec(&too_many).unwrap()).is_err());
        assert!(parse("openai", br#"{"data":[{"id":"bad model"}]}"#).is_err());
    }

    #[test]
    fn azure_discovery_extracts_only_supported_credentials() {
        let client = reqwest::Client::new();
        let secret = r#"{"api_key":"azure-key","client_secret":"must-not-leak"}"#;
        let request = authenticated_request(
            &client,
            "azure",
            "https://azure.invalid/models".into(),
            secret,
        )
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(request.headers()["api-key"], "azure-key");
        assert!(!format!("{:?}", request.headers()).contains("must-not-leak"));

        let token = authenticated_request(
            &client,
            "azure",
            "https://azure.invalid/models".into(),
            r#"{"azure_ad_token":"azure-token"}"#,
        )
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(token.headers()["authorization"], "Bearer azure-token");

        assert!(
            authenticated_request(
                &client,
                "azure",
                "https://azure.invalid/models".into(),
                r#"{"client_secret":"must-not-leak"}"#,
            )
            .is_err()
        );
    }
}
