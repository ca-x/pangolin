use super::{invalid, segment};
use crate::{api::ApiError, models::RouteTarget};
use http::{HeaderMap, HeaderValue};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::OnceLock, time::SystemTime};

fn header(headers: &mut HeaderMap, name: &'static str, value: &str) -> Result<(), ApiError> {
    headers.insert(
        name,
        HeaderValue::from_str(value).map_err(|_| invalid("invalid channel credential header"))?,
    );
    Ok(())
}

fn credential(secret: &str) -> Result<Map<String, Value>, ApiError> {
    serde_json::from_str(secret)
        .map_err(|_| invalid("cloud channel credential must be a JSON object"))
}

pub async fn authenticate(
    target: &RouteTarget,
    secret: &str,
    payload: &Value,
    url: &mut String,
    headers: &mut HeaderMap,
) -> Result<(), ApiError> {
    let unavailable = |_| ApiError::Upstream("channel credential could not be resolved".into());
    match target.provider_kind.as_str() {
        "anthropic" => {
            if secret.starts_with("sk-ant-oat") {
                header(headers, "authorization", &format!("Bearer {secret}"))?;
            } else {
                header(headers, "x-api-key", secret)?;
            }
        }
        "gemini" => {
            header(headers, "x-goog-api-key", secret)?;
        }
        "azure" => {
            let params = if secret.trim_start().starts_with('{') {
                credential(secret)?
            } else {
                Map::new()
            };
            let api_version = params
                .get("api_version")
                .and_then(Value::as_str)
                .unwrap_or("2024-10-21");
            let mut parsed =
                reqwest::Url::parse(url).map_err(|_| invalid("invalid Azure endpoint"))?;
            if !parsed.path().contains("/openai/") {
                let operation = parsed.path().trim_start_matches("/v1/").to_owned();
                if operation.starts_with("responses") {
                    parsed.set_path(&format!("/openai/v1/{operation}"));
                } else {
                    parsed.set_path(&format!(
                        "/openai/deployments/{}/{operation}",
                        segment(&target.upstream_name)
                    ));
                }
            }
            if !parsed.path().starts_with("/openai/v1/")
                && !parsed.query_pairs().any(|(key, _)| key == "api-version")
            {
                parsed
                    .query_pairs_mut()
                    .append_pair("api-version", api_version);
            }
            *url = parsed.to_string();
            if params.is_empty() {
                header(headers, "api-key", secret)?;
            } else if let Some(key) = params.get("api_key").and_then(Value::as_str) {
                header(headers, "api-key", key)?;
            } else {
                let inputs = litellm_auth_azure::AzureAuthInputs::from_sourced_optional_params(
                    &params,
                    &BTreeMap::new(),
                )
                .map_err(unavailable)?;
                let resolved = litellm_auth_azure::AzureAuthService::default()
                    .get_azure_ad_token(&inputs, &|_| None)
                    .await
                    .map_err(unavailable)?
                    .ok_or_else(|| invalid("Azure credential has no API key or token source"))?;
                header(
                    headers,
                    "authorization",
                    &format!("Bearer {}", resolved.value().secret().expose()),
                )?;
            }
        }
        "vertex" | "gcp" => {
            static AUTH: OnceLock<litellm_auth_gcp::VertexAuth> = OnceLock::new();
            let params = credential(secret)?;
            let config = litellm_auth_gcp::VertexConfig::from_sourced_optional_params(
                &params,
                &BTreeMap::new(),
            )
            .map_err(unavailable)?;
            let resolved = AUTH
                .get_or_init(Default::default)
                .validate_environment(
                    vec![],
                    params.get("access_token").and_then(Value::as_str),
                    &config,
                    &|_| None,
                )
                .await
                .map_err(unavailable)?;
            let location = config.location().unwrap_or("us-central1");
            let mut parsed =
                reqwest::Url::parse(url).map_err(|_| invalid("invalid Vertex endpoint"))?;
            let suffix = parsed
                .path()
                .split_once("/models/")
                .map(|(_, path)| path.to_owned())
                .ok_or_else(|| invalid("Vertex operation is not supported"))?;
            parsed.set_path(&format!(
                "/v1/projects/{}/locations/{}/publishers/google/models/{suffix}",
                segment(&resolved.project_id),
                segment(location)
            ));
            *url = parsed.to_string();
            for (name, value) in resolved.headers {
                headers.insert(
                    http::header::HeaderName::from_bytes(name.as_bytes())
                        .map_err(|_| invalid("invalid cloud header"))?,
                    HeaderValue::from_str(&value).map_err(|_| invalid("invalid cloud header"))?,
                );
            }
        }
        "bedrock" => {
            let params = credential(secret)?;
            let string = |name: &str| params.get(name).and_then(Value::as_str).map(str::to_owned);
            let region =
                string("region").ok_or_else(|| invalid("Bedrock credential requires region"))?;
            let config = litellm_auth_aws::AwsAuthConfig {
                access_key_id: string("access_key_id"),
                secret_access_key: string("secret_access_key"),
                session_token: string("session_token"),
                region_name: Some(region.clone()),
                role_name: string("role_arn"),
                profile_name: string("profile"),
                ..Default::default()
            };
            let credentials = litellm_auth_aws::resolve_credentials(config, &|_| None)
                .await
                .map_err(|_| ApiError::Upstream("AWS credential resolution failed".into()))?;
            let signing_headers = headers
                .iter()
                .map(|(name, value)| {
                    Ok((
                        name.to_string(),
                        value
                            .to_str()
                            .map_err(|_| invalid("invalid signing header"))?
                            .to_owned(),
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, ApiError>>()?;
            let signed = litellm_auth_aws::sign_bedrock_post(
                url,
                &serde_json::to_vec(payload).map_err(|e| ApiError::Internal(e.into()))?,
                &signing_headers,
                &region,
                &credentials,
                SystemTime::now(),
            )
            .map_err(|_| ApiError::Upstream("AWS request signing failed".into()))?;
            for (name, value) in signed {
                headers.insert(
                    http::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                    HeaderValue::from_str(&value).map_err(|_| invalid("invalid AWS header"))?,
                );
            }
        }
        "ollama" if secret.is_empty() => {}
        _ => {
            let pairs = litellm_auth::http::apply_credential(
                vec![],
                secret,
                litellm_auth::CredentialPlacement::Bearer,
            )
            .map_err(unavailable)?;
            for (_, value) in pairs {
                header(headers, "authorization", &value)?;
            }
        }
    }
    Ok(())
}
