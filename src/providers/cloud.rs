use super::{invalid, segment};
use crate::{api::ApiError, models::RouteTarget};
use aws_credential_types::provider::ProvideCredentials;
use azure_core::credentials::TokenCredential;
use gcp_auth::TokenProvider;
use http::{HeaderMap, HeaderValue};
use serde_json::{Map, Value};
use std::{
    io::Read,
    sync::{Arc, OnceLock},
    time::SystemTime,
};

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

pub(super) fn antigravity_project(secret: &str) -> Result<String, ApiError> {
    credential(secret)?
        .get("project_id")
        .and_then(Value::as_str)
        .filter(|project| !project.is_empty() && project.len() <= 256)
        .map(str::to_owned)
        .ok_or_else(|| ApiError::Upstream("channel credential could not be resolved".into()))
}

pub async fn authenticate(
    target: &RouteTarget,
    secret: &str,
    payload: &Value,
    url: &mut String,
    headers: &mut HeaderMap,
) -> Result<(), ApiError> {
    authenticate_at(target, secret, payload, url, headers, SystemTime::now()).await
}

pub(super) async fn authenticate_at(
    target: &RouteTarget,
    secret: &str,
    payload: &Value,
    url: &mut String,
    headers: &mut HeaderMap,
    signing_time: SystemTime,
) -> Result<(), ApiError> {
    if secret.is_empty() && target.provider_kind != "ollama" {
        return Err(unavailable());
    }
    let oauth_kind_matches = match target.credential_type.as_str() {
        "oauth_codex" => matches!(
            target.provider_kind.as_str(),
            "openai" | "openai_compatible"
        ),
        "oauth_xai" => target.provider_kind == "xai",
        "oauth_claude_code" => target.provider_kind == "anthropic",
        "oauth_antigravity" => target.provider_kind == "gemini",
        "oauth_github_copilot" => target.provider_kind == "openai",
        kind if kind.starts_with("oauth_") => false,
        _ => true,
    };
    if !oauth_kind_matches {
        return Err(ApiError::Upstream(
            "channel credential could not be resolved".into(),
        ));
    }
    match target.provider_kind.as_str() {
        "anthropic" => {
            if target.credential_type == "oauth_claude_code" || secret.starts_with("sk-ant-oat") {
                header(headers, "authorization", &format!("Bearer {secret}"))?;
            } else {
                header(headers, "x-api-key", secret)?;
            }
        }
        "gemini" => {
            if target.credential_type == "oauth_antigravity" {
                let params = credential(secret)?;
                let access_token = params
                    .get("access_token")
                    .and_then(Value::as_str)
                    .filter(|token| !token.is_empty() && token.len() <= 16 * 1024)
                    .ok_or_else(|| {
                        ApiError::Upstream("channel credential could not be resolved".into())
                    })?;
                header(headers, "authorization", &format!("Bearer {access_token}"))?;
            } else {
                header(headers, "x-goog-api-key", secret)?;
            }
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
            } else if let Some(key) = explicit_secret(&params, "api_key")? {
                header(headers, "api-key", key)?;
            } else {
                let token = azure_token(&params).await?;
                header(headers, "authorization", &format!("Bearer {token}"))?;
            }
        }
        "vertex" | "gcp" => {
            let params = credential(secret)?;
            let location = vertex_string(&params, ["vertex_location", "vertex_ai_location"])?
                .unwrap_or("us-central1");
            let (token, project) = vertex_token(&params).await?;
            let mut parsed =
                reqwest::Url::parse(url).map_err(|_| invalid("invalid Vertex endpoint"))?;
            let suffix = parsed
                .path()
                .split_once("/models/")
                .map(|(_, path)| path.to_owned())
                .ok_or_else(|| invalid("Vertex operation is not supported"))?;
            parsed.set_path(&format!(
                "/v1/projects/{}/locations/{}/publishers/google/models/{suffix}",
                segment(&project),
                segment(location)
            ));
            *url = parsed.to_string();
            header(headers, "authorization", &format!("Bearer {token}"))?;
        }
        "bedrock" => {
            let params = credential(secret)?;
            let string = |name: &str| params.get(name).and_then(Value::as_str).map(str::to_owned);
            let region =
                string("region").ok_or_else(|| invalid("Bedrock credential requires region"))?;
            let credentials = aws_credentials(&params, &region).await?;
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
                .collect::<Result<Vec<_>, ApiError>>()?;
            let identity = credentials.into();
            let params = aws_sigv4::sign::v4::SigningParams::builder()
                .identity(&identity)
                .region(&region)
                .name("bedrock")
                .time(signing_time)
                .settings(aws_sigv4::http_request::SigningSettings::default())
                .build()
                .map_err(|_| unavailable())?
                .into();
            let bytes =
                serde_json::to_vec(payload).map_err(|error| ApiError::Internal(error.into()))?;
            let signable = aws_sigv4::http_request::SignableRequest::new(
                "POST",
                url.as_str(),
                signing_headers
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
                aws_sigv4::http_request::SignableBody::Bytes(&bytes),
            )
            .map_err(|_| unavailable())?;
            let (signed, _) = aws_sigv4::http_request::sign(signable, &params)
                .map_err(|_| unavailable())?
                .into_parts();
            for (name, value) in signed.headers() {
                headers.insert(
                    http::header::HeaderName::from_bytes(name.as_bytes())
                        .map_err(|_| unavailable())?,
                    HeaderValue::from_str(value).map_err(|_| unavailable())?,
                );
            }
        }
        "ollama" if secret.is_empty() => {}
        _ => {
            let resolved = aimux_provider_utils::ProviderHeaders::bearer(
                aimux_provider_utils::Credential::Explicit(
                    aimux_provider_utils::Resolvable::Value(secret.to_owned()),
                ),
                vec![],
                None,
            )
            .resolve()
            .await
            .map_err(|_| unavailable())?;
            for (name, value) in resolved {
                if let Some(value) = value {
                    headers.insert(
                        http::header::HeaderName::from_bytes(name.as_bytes())
                            .map_err(|_| unavailable())?,
                        HeaderValue::from_str(&value).map_err(|_| unavailable())?,
                    );
                }
            }
        }
    }
    Ok(())
}

fn unavailable() -> ApiError {
    ApiError::Upstream("channel credential could not be resolved".into())
}

fn explicit_secret<'params>(
    params: &'params Map<String, Value>,
    name: &str,
) -> Result<Option<&'params str>, ApiError> {
    params
        .get(name)
        .map(|value| {
            value
                .as_str()
                .filter(|text| {
                    !text.is_empty() && text.len() <= 16 * 1024 && !text.contains(['\r', '\n'])
                })
                .ok_or_else(unavailable)
        })
        .transpose()
}

fn azure_options(
    params: &Map<String, Value>,
) -> Result<azure_identity::ClientSecretCredentialOptions, ApiError> {
    let cloud = if let Some(authority) = explicit_secret(params, "azure_authority_host")? {
        let parsed = reqwest::Url::parse(authority).map_err(|_| unavailable())?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
        {
            return Err(unavailable());
        }
        let mut configuration = azure_core::cloud::CustomConfiguration::default();
        configuration.authority_host = parsed.to_string();
        azure_core::cloud::CloudConfiguration::Custom(configuration)
    } else {
        azure_core::cloud::CloudConfiguration::AzurePublic
    };
    let mut options = azure_identity::ClientSecretCredentialOptions::default();
    options.client_options.cloud = Some(Arc::new(cloud));
    Ok(options)
}

async fn azure_token(params: &Map<String, Value>) -> Result<String, ApiError> {
    let token = explicit_secret(params, "azure_ad_token")?;
    azure_credential_kind(params)?;
    if let Some(token) = token {
        return Ok(token.to_owned());
    }
    static CACHE: OnceLock<moka::sync::Cache<String, Arc<dyn TokenCredential>>> = OnceLock::new();
    let key = blake3::hash(&serde_json::to_vec(params).map_err(|_| unavailable())?)
        .to_hex()
        .to_string();
    let cache = CACHE.get_or_init(|| {
        moka::sync::Cache::builder()
            .max_capacity(128)
            .time_to_live(std::time::Duration::from_secs(3600))
            .build()
    });
    let credential = if let Some(credential) = cache.get(&key) {
        credential
    } else {
        let credential = azure_credential(params, azure_options(params)?)?;
        cache.insert(key, credential.clone());
        credential
    };
    let scope = azure_optional_string(params, "azure_scope")?
        .unwrap_or("https://cognitiveservices.azure.com/.default");
    credential
        .get_token(&[scope], None)
        .await
        .map(|token| token.token.secret().to_owned())
        .map_err(|_| unavailable())
}

fn azure_credential(
    params: &Map<String, Value>,
    options: azure_identity::ClientSecretCredentialOptions,
) -> Result<Arc<dyn TokenCredential>, ApiError> {
    let kind = azure_credential_kind(params)?;
    let tenant = azure_optional_string(params, "tenant_id")?;
    let client = azure_optional_string(params, "client_id")?;
    let secret = azure_optional_string(params, "client_secret")?;
    let federated_file = azure_optional_string(params, "azure_federated_token_file")?;
    let credential: Arc<dyn TokenCredential> = match kind {
        AzureCredentialKind::ClientSecret => azure_identity::ClientSecretCredential::new(
            tenant.ok_or_else(unavailable)?,
            client.ok_or_else(unavailable)?.to_owned(),
            azure_core::credentials::Secret::new(secret.ok_or_else(unavailable)?.to_owned()),
            Some(options),
        )
        .map_err(|_| unavailable())?,
        AzureCredentialKind::Workload => azure_identity::ClientAssertionCredential::new(
            tenant.ok_or_else(unavailable)?.to_owned(),
            client.ok_or_else(unavailable)?.to_owned(),
            AzureFileAssertion {
                path: federated_file.ok_or_else(unavailable)?.to_owned(),
            },
            Some(azure_identity::ClientAssertionCredentialOptions {
                client_options: options.client_options,
            }),
        )
        .map_err(|_| unavailable())?,
        AzureCredentialKind::Managed => azure_identity::ManagedIdentityCredential::new(Some(
            azure_identity::ManagedIdentityCredentialOptions {
                user_assigned_id: client
                    .map(str::to_owned)
                    .map(azure_identity::UserAssignedId::ClientId),
                client_options: options.client_options,
            },
        ))
        .map_err(|_| unavailable())?,
    };
    Ok(credential)
}

enum AzureCredentialKind {
    ClientSecret,
    Workload,
    Managed,
}

fn azure_credential_kind(params: &Map<String, Value>) -> Result<AzureCredentialKind, ApiError> {
    match azure_optional_string(params, "azure_credential")?.unwrap_or("ClientSecretCredential") {
        "ClientSecretCredential" => Ok(AzureCredentialKind::ClientSecret),
        "WorkloadIdentityCredential" => Ok(AzureCredentialKind::Workload),
        "ManagedIdentityCredential" => Ok(AzureCredentialKind::Managed),
        _ => Err(unavailable()),
    }
}

fn azure_optional_string<'params>(
    params: &'params Map<String, Value>,
    name: &str,
) -> Result<Option<&'params str>, ApiError> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.is_empty() => Ok(None),
        Some(_) => explicit_secret(params, name),
    }
}

struct AzureFileAssertion {
    path: String,
}

impl std::fmt::Debug for AzureFileAssertion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AzureFileAssertion")
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl azure_identity::ClientAssertion for AzureFileAssertion {
    async fn secret(
        &self,
        _options: Option<azure_core::http::ClientMethodOptions<'_>>,
    ) -> azure_core::Result<String> {
        let failure = || {
            azure_core::Error::with_message(
                azure_core::error::ErrorKind::Credential,
                "configured federated identity token is unavailable",
            )
        };
        let token = credential_file(self.path.clone())
            .await
            .map_err(|_| failure())?;
        let token = token.trim();
        if token.is_empty() || token.contains(['\r', '\n']) {
            return Err(failure());
        }
        Ok(token.to_owned())
    }
}

async fn vertex_token(params: &Map<String, Value>) -> Result<(String, String), ApiError> {
    let project =
        vertex_string(params, ["vertex_project", "vertex_ai_project"])?.map(str::to_owned);
    if project.as_ref().is_some_and(|project| project.len() > 256) {
        return Err(unavailable());
    }
    let explicit_token = explicit_secret(params, "access_token")?;
    if let (Some(token), Some(project)) = (explicit_token, project.as_ref()) {
        return Ok((token.to_owned(), project.clone()));
    }
    let credential = vertex_account(params).await?;
    let project = project
        .or_else(|| credential.project_id().map(str::to_owned))
        .filter(|project| !project.trim().is_empty() && project.len() <= 256)
        .ok_or_else(unavailable)?;
    let token = match explicit_token {
        Some(token) => token.to_owned(),
        None => credential
            .token(&["https://www.googleapis.com/auth/cloud-platform"])
            .await
            .map_err(|_| unavailable())?
            .as_str()
            .to_owned(),
    };
    Ok((token, project))
}

const MAX_CLOUD_CREDENTIAL_BYTES: usize = 64 * 1024;

async fn credential_file(path: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || -> Result<String, ApiError> {
        let metadata = std::fs::metadata(&path).map_err(|_| unavailable())?;
        if !metadata.is_file() || metadata.len() > MAX_CLOUD_CREDENTIAL_BYTES as u64 {
            return Err(unavailable());
        }
        let file = std::fs::File::open(path).map_err(|_| unavailable())?;
        if !file.metadata().map_err(|_| unavailable())?.is_file() {
            return Err(unavailable());
        }
        let mut contents = String::new();
        file.take(MAX_CLOUD_CREDENTIAL_BYTES as u64 + 1)
            .read_to_string(&mut contents)
            .map_err(|_| unavailable())?;
        if contents.len() > MAX_CLOUD_CREDENTIAL_BYTES {
            return Err(unavailable());
        }
        Ok(contents)
    })
    .await
    .map_err(|_| unavailable())?
}

async fn vertex_account(
    params: &Map<String, Value>,
) -> Result<Arc<gcp_auth::CustomServiceAccount>, ApiError> {
    static CACHE: OnceLock<moka::sync::Cache<String, Arc<gcp_auth::CustomServiceAccount>>> =
        OnceLock::new();
    let source = vertex_credentials(params)?;
    let source = if source.trim_start().starts_with('{') {
        source
    } else {
        credential_file(source).await?
    };
    if source.len() > MAX_CLOUD_CREDENTIAL_BYTES {
        return Err(unavailable());
    }
    let key = blake3::hash(source.as_bytes()).to_hex().to_string();
    let cache = CACHE.get_or_init(|| {
        moka::sync::Cache::builder()
            .max_capacity(128)
            .time_to_live(std::time::Duration::from_secs(3600))
            .build()
    });
    let credential = if let Some(credential) = cache.get(&key) {
        credential
    } else {
        let credential = Arc::new(
            gcp_auth::CustomServiceAccount::from_json(&source).map_err(|_| unavailable())?,
        );
        cache.insert(key, credential.clone());
        credential
    };
    Ok(credential)
}

fn vertex_string<'params>(
    params: &'params Map<String, Value>,
    names: [&str; 2],
) -> Result<Option<&'params str>, ApiError> {
    for name in names {
        match params.get(name) {
            None | Some(Value::Null) => continue,
            Some(Value::String(text)) if text.trim().is_empty() => continue,
            Some(Value::String(text)) => return Ok(Some(text)),
            Some(_) => return Err(unavailable()),
        }
    }
    Ok(None)
}

fn vertex_credentials(params: &Map<String, Value>) -> Result<String, ApiError> {
    for name in ["vertex_credentials", "vertex_ai_credentials"] {
        match params.get(name) {
            None | Some(Value::Null) => continue,
            Some(Value::String(text)) if text.trim().is_empty() => continue,
            Some(Value::String(text)) => return Ok(text.clone()),
            Some(Value::Object(object)) if object.is_empty() => continue,
            Some(source @ Value::Object(_)) => return Ok(source.to_string()),
            Some(_) => return Err(unavailable()),
        }
    }
    Err(unavailable())
}

async fn aws_credentials(
    params: &Map<String, Value>,
    region: &str,
) -> Result<aws_credential_types::Credentials, ApiError> {
    let credentials = match (
        explicit_secret(params, "access_key_id")?,
        explicit_secret(params, "secret_access_key")?,
    ) {
        (Some(key), Some(secret)) if !key.is_empty() && !secret.is_empty() => {
            aws_credential_types::Credentials::new(
                key,
                secret,
                explicit_secret(params, "session_token")?.map(str::to_owned),
                None,
                "pangolin-channel",
            )
        }
        (None, None) => {
            let profile = explicit_secret(params, "profile")?.ok_or_else(unavailable)?;
            aws_config::profile::ProfileFileCredentialsProvider::builder()
                .profile_name(profile)
                .build()
                .provide_credentials()
                .await
                .map_err(|_| unavailable())?
        }
        _ => return Err(unavailable()),
    };
    if let Some(role) = explicit_secret(params, "role_arn")? {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_types::region::Region::new(region.to_owned()))
            .credentials_provider(credentials)
            .load()
            .await;
        aws_config::sts::AssumeRoleProvider::builder(role)
            .configure(&config)
            .session_name("pangolin")
            .build()
            .await
            .provide_credentials()
            .await
            .map_err(|_| unavailable())
    } else {
        Ok(credentials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::{
        encoding::AsDer,
        rsa::{KeyPair, KeySize},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::json;
    use std::io::Write;

    #[derive(Debug, Default)]
    struct AzureTokenTransport {
        requests: std::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl azure_core::http::HttpClient for AzureTokenTransport {
        async fn execute_request(
            &self,
            request: &azure_core::http::Request,
        ) -> azure_core::Result<azure_core::http::AsyncRawResponse> {
            let body = azure_core::Bytes::from(request.body());
            self.requests.lock().unwrap().push((
                request.url().to_string(),
                String::from_utf8(body.to_vec()).unwrap(),
            ));
            let expires_on = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 3600;
            Ok(azure_core::http::AsyncRawResponse::from_bytes(
                azure_core::http::StatusCode::Ok,
                azure_core::http::headers::Headers::new(),
                json!({"access_token":"mock-azure-token","token_type":"Bearer","expires_in":3600,"expires_on":expires_on.to_string(),"resource":"https://cognitiveservices.azure.com"})
                    .to_string(),
            ))
        }
    }

    fn azure_mock_options(
        params: &Map<String, Value>,
        transport: Arc<AzureTokenTransport>,
    ) -> azure_identity::ClientSecretCredentialOptions {
        let mut options = azure_options(params).unwrap();
        options.client_options.transport = Some(azure_core::http::Transport::new(transport));
        options
    }

    #[tokio::test]
    async fn azure_workload_identity_selector_exchanges_explicit_federated_file() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"mock-federated-assertion").unwrap();
        let params = json!({"azure_credential":"WorkloadIdentityCredential","tenant_id":"channel-tenant","client_id":"channel-client","azure_federated_token_file":file.path().to_str().unwrap(),"azure_authority_host":"https://login.microsoftonline.us"});
        let params = params.as_object().unwrap();
        let transport = Arc::new(AzureTokenTransport::default());
        let credential =
            azure_credential(params, azure_mock_options(params, transport.clone())).unwrap();
        let token = credential
            .get_token(&["https://cognitiveservices.azure.com/.default"], None)
            .await
            .unwrap();
        assert_eq!(token.token.secret(), "mock-azure-token");
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].0,
            "https://login.microsoftonline.us/channel-tenant/oauth2/v2.0/token"
        );
        let form = reqwest::Url::parse(&format!("http://localhost/?{}", requests[0].1))
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(form["client_id"], "channel-client");
        assert_eq!(form["client_assertion"], "mock-federated-assertion");
        assert_eq!(form["grant_type"], "client_credentials");
        assert_eq!(
            form["scope"],
            "https://cognitiveservices.azure.com/.default"
        );
        assert!(!form.contains_key("client_secret"));
    }

    #[tokio::test]
    async fn azure_managed_identity_selector_uses_explicit_client_without_client_secret() {
        let params =
            json!({"azure_credential":"ManagedIdentityCredential","client_id":"channel-client"});
        let params = params.as_object().unwrap();
        let transport = Arc::new(AzureTokenTransport::default());
        let credential =
            azure_credential(params, azure_mock_options(params, transport.clone())).unwrap();
        let token = credential
            .get_token(&["https://cognitiveservices.azure.com/.default"], None)
            .await
            .unwrap();
        assert_eq!(token.token.secret(), "mock-azure-token");
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let url = reqwest::Url::parse(&requests[0].0).unwrap();
        let query = url
            .query_pairs()
            .into_owned()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query["client_id"], "channel-client");
        assert_eq!(query["resource"], "https://cognitiveservices.azure.com");
    }

    #[test]
    fn azure_unknown_identity_selectors_do_not_fall_back_to_client_secret() {
        for selector in [json!("UnknownCredential"), json!(42), json!([])] {
            let params = json!({"azure_credential":selector,"tenant_id":"channel-tenant","client_id":"channel-client","client_secret":"mock-secret"});
            let params = params.as_object().unwrap();
            assert!(azure_credential(params, azure_options(params).unwrap()).is_err());
        }
    }

    #[test]
    fn azure_unset_selector_preserves_explicit_client_secret_configuration() {
        for selector in [Value::Null, json!("")] {
            let params = json!({"azure_credential":selector,"tenant_id":"channel-tenant","client_id":"channel-client","client_secret":"mock-secret"});
            let params = params.as_object().unwrap();
            assert!(azure_credential(params, azure_options(params).unwrap()).is_ok());
        }
    }

    #[tokio::test]
    async fn azure_identity_chains_and_refresh_never_switch_to_another_identity() {
        for selector in ["DefaultAzureCredential", "DeploymentIdentityCredential"] {
            let params = json!({"azure_credential":selector,"tenant_id":"channel-tenant","client_id":"channel-client","client_secret":"mock-secret","azure_ad_token":"mock-explicit-token"});
            let params = params.as_object().unwrap();
            assert!(azure_token(params).await.is_err());
            assert!(azure_credential(params, azure_options(params).unwrap()).is_err());
        }
        let params = json!({"enable_azure_ad_token_refresh":true});
        assert!(azure_token(params.as_object().unwrap()).await.is_err());
    }

    #[tokio::test]
    async fn azure_federated_assertion_reads_rotation_and_rejects_unbounded_or_invalid_files() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"first-assertion\n").unwrap();
        let assertion = AzureFileAssertion {
            path: file.path().to_str().unwrap().to_owned(),
        };
        assert_eq!(
            azure_identity::ClientAssertion::secret(&assertion, None)
                .await
                .unwrap(),
            "first-assertion"
        );
        std::fs::write(file.path(), "second-assertion").unwrap();
        assert_eq!(
            azure_identity::ClientAssertion::secret(&assertion, None)
                .await
                .unwrap(),
            "second-assertion"
        );
        for malformed in [
            " ".to_owned(),
            "broken\nassertion".to_owned(),
            "x".repeat(MAX_CLOUD_CREDENTIAL_BYTES + 1),
        ] {
            std::fs::write(file.path(), malformed).unwrap();
            assert!(
                azure_identity::ClientAssertion::secret(&assertion, None)
                    .await
                    .is_err()
            );
        }
        let directory = tempfile::tempdir().unwrap();
        for path in [
            directory.path().to_path_buf(),
            directory.path().join("missing-token"),
        ] {
            let assertion = AzureFileAssertion {
                path: path.to_str().unwrap().to_owned(),
            };
            assert!(
                azure_identity::ClientAssertion::secret(&assertion, None)
                    .await
                    .is_err()
            );
        }
    }

    #[test]
    fn azure_identity_configuration_never_discovers_ambient_client_credentials() {
        const CHILD: &str = "PANGOLIN_TEST_AZURE_ENV_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(b"ambient-assertion-must-not-be-read")
                .unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "providers::cloud::tests::azure_identity_configuration_never_discovers_ambient_client_credentials"])
                .env(CHILD, "1")
                .env("AZURE_TENANT_ID", "ambient-tenant")
                .env("AZURE_CLIENT_ID", "ambient-client")
                .env("AZURE_CLIENT_SECRET", "ambient-secret")
                .env("AZURE_FEDERATED_TOKEN_FILE", file.path())
                .status().unwrap();
            assert!(status.success());
            return;
        }
        let transport = Arc::new(AzureTokenTransport::default());
        for missing in ["tenant_id", "client_id", "azure_federated_token_file"] {
            let mut params = json!({"azure_credential":"WorkloadIdentityCredential","tenant_id":"explicit-tenant","client_id":"explicit-client","azure_federated_token_file":"must-not-read"});
            params.as_object_mut().unwrap().remove(missing);
            let params = params.as_object().unwrap();
            assert!(
                azure_credential(params, azure_mock_options(params, transport.clone())).is_err()
            );
        }
        let empty = Map::new();
        assert!(azure_credential(&empty, azure_mock_options(&empty, transport.clone())).is_err());
        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn azure_explicit_identity_never_downgrades_to_an_alternate_client_secret() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"selected-workload-assertion").unwrap();
        for selector in ["WorkloadIdentityCredential", "ManagedIdentityCredential"] {
            let params = json!({"azure_credential":selector,"tenant_id":"channel-tenant","client_id":"channel-client","client_secret":"must-not-use-alternate-secret","azure_federated_token_file":file.path().to_str().unwrap()});
            let params = params.as_object().unwrap();
            let transport = Arc::new(AzureTokenTransport::default());
            let credential =
                azure_credential(params, azure_mock_options(params, transport.clone())).unwrap();
            credential
                .get_token(&["https://cognitiveservices.azure.com/.default"], None)
                .await
                .unwrap();
            let requests = transport.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert!(!requests[0].1.contains("client_secret"));
            if selector == "WorkloadIdentityCredential" {
                assert!(requests[0].1.contains("selected-workload-assertion"));
            } else {
                assert!(requests[0].0.contains("client_id=channel-client"));
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let params = json!({"azure_credential":"WorkloadIdentityCredential","tenant_id":"channel-tenant","client_id":"channel-client","client_secret":"must-not-use-alternate-secret","azure_federated_token_file":directory.path().join("missing-token").to_str().unwrap()});
        let params = params.as_object().unwrap();
        let transport = Arc::new(AzureTokenTransport::default());
        let credential =
            azure_credential(params, azure_mock_options(params, transport.clone())).unwrap();
        assert!(
            credential
                .get_token(&["https://cognitiveservices.azure.com/.default"], None)
                .await
                .is_err()
        );
        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn azure_managed_system_identity_is_selected_explicitly() {
        let params = json!({"azure_credential":"ManagedIdentityCredential","client_id":null});
        let params = params.as_object().unwrap();
        let transport = Arc::new(AzureTokenTransport::default());
        let credential =
            azure_credential(params, azure_mock_options(params, transport.clone())).unwrap();
        let token = credential
            .get_token(&["https://cognitiveservices.azure.com/.default"], None)
            .await
            .unwrap();
        assert_eq!(token.token.secret(), "mock-azure-token");
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let url = reqwest::Url::parse(&requests[0].0).unwrap();
        assert!(!url.query_pairs().any(|(name, _)| name == "client_id"));
    }

    fn vertex_test_account() -> Value {
        static ACCOUNT: OnceLock<Value> = OnceLock::new();
        ACCOUNT
            .get_or_init(|| {
                let key = KeyPair::generate(KeySize::Rsa2048).unwrap();
                let encoded = STANDARD.encode(key.as_der().unwrap().as_ref());
                let lines = encoded
                    .as_bytes()
                    .chunks(64)
                    .map(|chunk| std::str::from_utf8(chunk).unwrap())
                    .collect::<Vec<_>>()
                    .join("\n");
                json!({
                    "type":"service_account",
                    "project_id":"credential-project",
                    "private_key_id":"temporary-test-key",
                    "private_key":format!("-----BEGIN PRIVATE KEY-----\n{lines}\n-----END PRIVATE KEY-----\n"),
                    "client_email":"fixture@example.test",
                    "client_id":"temporary-client",
                    "token_uri":"http://127.0.0.1:1/must-not-exchange"
                })
            })
            .clone()
    }

    #[tokio::test]
    async fn vertex_external_token_resolves_project_from_explicit_account_without_exchange() {
        let account = vertex_test_account();
        for field in ["vertex_credentials", "vertex_ai_credentials"] {
            for source in [account.clone(), json!(account.to_string())] {
                for project in [Value::Null, json!("  ")] {
                    let mut params =
                        json!({"access_token":"external-token","vertex_project":project});
                    params[field] = source.clone();
                    assert_eq!(
                        vertex_token(params.as_object().unwrap()).await.unwrap(),
                        ("external-token".to_owned(), "credential-project".to_owned())
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn vertex_explicit_account_files_support_aliases_and_rotation() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut account = vertex_test_account();
        file.write_all(account.to_string().as_bytes()).unwrap();
        for field in ["vertex_credentials", "vertex_ai_credentials"] {
            let params = json!({field:file.path().to_str().unwrap()});
            let credential = vertex_account(params.as_object().unwrap()).await.unwrap();
            assert_eq!(credential.project_id(), Some("credential-project"));
            let mut params = params;
            params["access_token"] = json!("external-token");
            assert_eq!(
                vertex_token(params.as_object().unwrap()).await.unwrap(),
                ("external-token".to_owned(), "credential-project".to_owned())
            );
        }
        account["project_id"] = json!("rotated-project");
        std::fs::write(file.path(), account.to_string()).unwrap();
        let params = json!({"vertex_credentials":file.path().to_str().unwrap()});
        assert_eq!(
            vertex_account(params.as_object().unwrap())
                .await
                .unwrap()
                .project_id(),
            Some("rotated-project")
        );
    }

    #[tokio::test]
    async fn vertex_account_sources_reject_missing_nonregular_oversized_and_malformed_files() {
        let directory = tempfile::tempdir().unwrap();
        let oversized = directory.path().join("oversized.json");
        std::fs::write(&oversized, vec![b' '; MAX_CLOUD_CREDENTIAL_BYTES + 1]).unwrap();
        let malformed = directory.path().join("malformed.json");
        std::fs::write(&malformed, "not a service account").unwrap();
        for path in [
            directory.path().join("missing.json"),
            directory.path().to_path_buf(),
            oversized,
            malformed,
        ] {
            let params = json!({"vertex_credentials":path.to_str().unwrap()});
            assert!(vertex_account(params.as_object().unwrap()).await.is_err());
        }
        let mut account = vertex_test_account();
        account["padding"] = json!(" ".repeat(MAX_CLOUD_CREDENTIAL_BYTES));
        let params = json!({"vertex_credentials":account});
        assert!(vertex_account(params.as_object().unwrap()).await.is_err());
    }

    #[test]
    fn vertex_credentials_accept_legacy_string_and_object_aliases() {
        let credentials = json!({"type":"service_account","project_id":"test-project"});
        for source in [credentials.clone(), json!(credentials.to_string())] {
            let params = json!({"vertex_ai_credentials":source});
            let resolved = vertex_credentials(params.as_object().unwrap()).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&resolved).unwrap(),
                credentials
            );
        }
    }

    #[test]
    fn vertex_credentials_use_canonical_precedence_and_skip_unset_values() {
        let canonical = json!({"project_id":"canonical"});
        let legacy = json!({"project_id":"legacy"});
        let params = json!({"vertex_credentials":canonical,"vertex_ai_credentials":legacy});
        assert_eq!(
            serde_json::from_str::<Value>(
                &vertex_credentials(params.as_object().unwrap()).unwrap()
            )
            .unwrap(),
            canonical
        );
        for unset in [Value::Null, json!("  "), json!({})] {
            let params = json!({"vertex_credentials":unset,"vertex_ai_credentials":legacy});
            assert_eq!(
                serde_json::from_str::<Value>(
                    &vertex_credentials(params.as_object().unwrap()).unwrap()
                )
                .unwrap(),
                legacy
            );
        }
        for malformed in [json!(12), json!([]), json!(false)] {
            let params = json!({"vertex_credentials":malformed,"vertex_ai_credentials":legacy});
            assert!(vertex_credentials(params.as_object().unwrap()).is_err());
        }
    }

    #[test]
    fn azure_authority_is_explicit_and_rejects_unsafe_shapes() {
        let empty = Map::new();
        assert_eq!(
            azure_options(&empty)
                .unwrap()
                .client_options
                .cloud
                .as_deref(),
            Some(&azure_core::cloud::CloudConfiguration::AzurePublic)
        );
        let params = json!({"azure_authority_host":"https://login.microsoftonline.us"});
        let options = azure_options(params.as_object().unwrap()).unwrap();
        let Some(azure_core::cloud::CloudConfiguration::Custom(custom)) =
            options.client_options.cloud.as_deref()
        else {
            panic!("expected explicit authority")
        };
        assert_eq!(custom.authority_host, "https://login.microsoftonline.us/");
        for authority in [
            "http://example.com",
            "https://user:password@example.com",
            "https://example.com/path",
            "https://example.com/?token=secret",
            "",
        ] {
            assert!(
                azure_options(
                    json!({"azure_authority_host":authority})
                        .as_object()
                        .unwrap()
                )
                .is_err()
            );
        }
    }

    #[tokio::test]
    async fn malformed_explicit_tokens_never_fall_through_to_another_source() {
        for token in [json!(""), json!(null), json!(123), json!("bad\ntoken")] {
            let params = json!({"azure_ad_token":token,"tenant_id":"tenant","client_id":"client","client_secret":"secret"});
            assert!(azure_token(params.as_object().unwrap()).await.is_err());
            let params =
                json!({"access_token":token,"vertex_project":"project","vertex_credentials":{}});
            assert!(vertex_token(params.as_object().unwrap()).await.is_err());
        }
        let params =
            json!({"access_key_id":null,"secret_access_key":null,"profile":"must-not-load"});
        assert!(
            aws_credentials(params.as_object().unwrap(), "us-east-1")
                .await
                .is_err()
        );
    }
}
