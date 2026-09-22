use futures_util::StreamExt;
use oauth2::PkceCodeChallenge;
use reqwest::{Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_IDP_RESPONSE_BYTES: usize = 64 * 1024;
const TOKEN_EXPIRY_SKEW_SECS: i64 = 60;
const COPILOT_EXPIRY_SKEW_SECS: i64 = 300;
const MAX_ACCESS_TOKEN_LIFETIME_SECS: i64 = 24 * 60 * 60;
pub(crate) const MAX_FLOW_LIFETIME_SECS: i64 = 900;

/// A deliberately narrow client for provider identity endpoints. Redirects are
/// disabled so authorization codes, refresh tokens and provider access tokens
/// cannot be replayed to a redirected origin. Every response is additionally
/// bounded by [`response_json`].
#[derive(Clone)]
pub(crate) struct HttpClient(reqwest::Client);

impl HttpClient {
    pub(crate) fn new(timeout: Duration) -> Result<Self, reqwest::Error> {
        reqwest::Client::builder()
            .user_agent(concat!("pangolin/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .timeout(timeout)
            .build()
            .map(Self)
    }

    fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.0.post(url)
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.0.get(url)
    }

    #[cfg(test)]
    fn test() -> Self {
        Self::new(Duration::from_secs(3)).unwrap()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Flow {
    Codex,
    Xai,
    ClaudeCode,
    Antigravity,
    GithubCopilot,
}

impl Flow {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "codex" => Some(Self::Codex),
            "xai" => Some(Self::Xai),
            "claude_code" => Some(Self::ClaudeCode),
            "antigravity" => Some(Self::Antigravity),
            "github_copilot" => Some(Self::GithubCopilot),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Xai => "xai",
            Self::ClaudeCode => "claude_code",
            Self::Antigravity => "antigravity",
            Self::GithubCopilot => "github_copilot",
        }
    }

    pub(crate) fn for_credential_type(value: &str) -> Option<Self> {
        value.strip_prefix("oauth_").and_then(Self::parse)
    }

    pub(crate) fn is_device(self) -> bool {
        self == Self::GithubCopilot
    }

    pub(crate) fn is_available(self) -> bool {
        true
    }

    pub(crate) fn supports_provider_kind(self, kind: &str) -> bool {
        match self {
            Self::Codex => matches!(kind, "openai" | "openai_compatible"),
            Self::Xai => kind == "xai",
            Self::ClaudeCode => kind == "anthropic",
            Self::Antigravity => kind == "gemini",
            Self::GithubCopilot => kind == "openai",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderSpec {
    flow: Flow,
    authorization_endpoint: Option<&'static str>,
    token_endpoint: &'static str,
    device_endpoint: Option<&'static str>,
    provider_token_endpoint: Option<&'static str>,
    scope: &'static str,
}

impl ProviderSpec {
    pub(crate) fn for_flow(flow: Flow) -> Self {
        match flow {
            Flow::Codex => Self::browser(
                Flow::Codex,
                "https://auth.openai.com/oauth/authorize",
                "https://auth.openai.com/oauth/token",
                "openid profile email offline_access",
            ),
            Flow::Xai => Self::browser(
                Flow::Xai,
                "https://auth.x.ai/oauth2/authorize",
                "https://auth.x.ai/oauth2/token",
                "openid profile email offline_access grok-cli:access api:access",
            ),
            Flow::ClaudeCode => Self::browser(
                Flow::ClaudeCode,
                "https://claude.ai/oauth/authorize",
                "https://api.anthropic.com/v1/oauth/token",
                "org:create_api_key user:profile user:inference",
            ),
            Flow::Antigravity => Self::browser(
                Flow::Antigravity,
                "https://accounts.google.com/o/oauth2/v2/auth",
                "https://oauth2.googleapis.com/token",
                "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs",
            )
            .with_provider_token_endpoint(
                "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
            ),
            Flow::GithubCopilot => Self {
                flow,
                authorization_endpoint: None,
                token_endpoint: "https://github.com/login/oauth/access_token",
                device_endpoint: Some("https://github.com/login/device/code"),
                provider_token_endpoint: Some(
                    "https://api.github.com/copilot_internal/v2/token",
                ),
                scope: "read:user",
            },
        }
    }

    const fn browser(
        flow: Flow,
        authorization_endpoint: &'static str,
        token_endpoint: &'static str,
        scope: &'static str,
    ) -> Self {
        Self {
            flow,
            authorization_endpoint: Some(authorization_endpoint),
            token_endpoint,
            device_endpoint: None,
            provider_token_endpoint: None,
            scope,
        }
    }

    const fn with_provider_token_endpoint(mut self, endpoint: &'static str) -> Self {
        self.provider_token_endpoint = Some(endpoint);
        self
    }

    #[cfg(test)]
    pub(crate) fn test_browser(
        flow: Flow,
        authorization_endpoint: String,
        token_endpoint: String,
    ) -> Self {
        Self::browser(
            flow,
            Box::leak(authorization_endpoint.into_boxed_str()),
            Box::leak(token_endpoint.into_boxed_str()),
            "openid",
        )
    }

    #[cfg(test)]
    pub(crate) fn test_device(device_endpoint: String, token_endpoint: String) -> Self {
        Self {
            flow: Flow::GithubCopilot,
            authorization_endpoint: None,
            token_endpoint: Box::leak(token_endpoint.into_boxed_str()),
            device_endpoint: Some(Box::leak(device_endpoint.into_boxed_str())),
            provider_token_endpoint: Some("https://api.github.com/copilot_internal/v2/token"),
            scope: "read:user",
        }
    }

    #[cfg(test)]
    pub(crate) fn test_provider_endpoint(mut self, endpoint: String) -> Self {
        self.provider_token_endpoint = Some(Box::leak(endpoint.into_boxed_str()));
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid OAuth configuration")]
    InvalidConfiguration,
    #[error("OAuth provider is temporarily unavailable")]
    ProviderUnavailable,
    #[error("OAuth authorization is pending")]
    Pending,
    #[error("OAuth provider requested slower polling")]
    SlowDown,
    #[error("OAuth authorization expired")]
    Expired,
    #[error("OAuth authorization was declined")]
    Declined,
    #[error("stored OAuth credential is invalid")]
    InvalidCredential,
}

#[derive(Serialize)]
pub(crate) struct BrowserStart {
    pub authorization_url: String,
    pub state: String,
    #[serde(skip)]
    pub verifier: String,
}

#[derive(Serialize)]
pub(crate) struct DeviceStart {
    pub verification_uri: String,
    pub user_code: String,
    pub expires_in: u64,
    pub interval: u64,
    pub state: String,
    #[serde(skip)]
    pub device_code: String,
}

pub(crate) struct BrowserExchange<'a> {
    pub client_id: &'a str,
    pub client_secret: Option<&'a str>,
    pub redirect_uri: &'a str,
    pub code: &'a str,
    pub state: &'a str,
    pub verifier: &'a str,
}

pub(crate) struct Token(Value);

impl Token {
    pub(crate) fn into_value(self) -> Value {
        self.0
    }
}

pub(crate) struct ResolvedCredential {
    secret: Zeroizing<String>,
    replacement: Option<Zeroizing<String>>,
}

impl ResolvedCredential {
    pub(crate) fn into_parts(self) -> (Zeroizing<String>, Option<Zeroizing<String>>) {
        (self.secret, self.replacement)
    }
}

pub(crate) fn credential_secret(
    credential_type: &str,
    secret: Zeroizing<String>,
) -> anyhow::Result<Zeroizing<String>> {
    if !credential_type.starts_with("oauth_") {
        return Ok(secret);
    }
    if matches!(
        credential_type,
        "oauth_antigravity" | "oauth_github_copilot"
    ) {
        anyhow::bail!("OAuth credential requires an unavailable provider adapter");
    }
    let document: Value = serde_json::from_str(&secret)
        .map_err(|_| anyhow::anyhow!("invalid encrypted OAuth credential"))?;
    let token = document
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty() && token.len() <= 16 * 1024)
        .ok_or_else(|| anyhow::anyhow!("encrypted OAuth credential has no access token"))?;
    Ok(Zeroizing::new(token.to_owned()))
}

fn random_secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub(crate) fn start_browser(
    spec: ProviderSpec,
    client_id: &str,
    redirect_uri: &str,
) -> Result<BrowserStart, Error> {
    let endpoint = spec
        .authorization_endpoint
        .ok_or(Error::InvalidConfiguration)?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let state = random_secret();
    let mut url = Url::parse(endpoint).map_err(|_| Error::InvalidConfiguration)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", spec.scope)
        .append_pair("state", &state)
        .append_pair("code_challenge", challenge.as_str())
        .append_pair("code_challenge_method", "S256");
    match spec.flow {
        Flow::Codex => {
            url.query_pairs_mut()
                .append_pair("id_token_add_organizations", "true")
                .append_pair("codex_cli_simplified_flow", "true");
        }
        Flow::Xai => {
            url.query_pairs_mut()
                .append_pair("nonce", &random_secret())
                .append_pair("plan", "generic")
                .append_pair("referrer", "pangolin");
        }
        Flow::Antigravity => {
            url.query_pairs_mut()
                .append_pair("access_type", "offline")
                .append_pair("prompt", "consent");
        }
        Flow::ClaudeCode | Flow::GithubCopilot => {}
    }
    Ok(BrowserStart {
        authorization_url: url.into(),
        state,
        verifier: verifier.secret().to_owned(),
    })
}

pub(crate) async fn start_device(
    client: &HttpClient,
    spec: ProviderSpec,
    client_id: &str,
) -> Result<DeviceStart, Error> {
    let endpoint = spec.device_endpoint.ok_or(Error::InvalidConfiguration)?;
    let response = client
        .post(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[("client_id", client_id), ("scope", spec.scope)])
        .send()
        .await
        .map_err(|_| Error::ProviderUnavailable)?;
    let document = response_json(response).await?;
    let device_code = required_text(&document, "device_code")?.to_owned();
    let user_code = required_text(&document, "user_code")?.to_owned();
    let verification_uri = required_text(&document, "verification_uri")?.to_owned();
    let verification_url = Url::parse(&verification_uri).map_err(|_| Error::ProviderUnavailable)?;
    if verification_url.scheme() != "https"
        || verification_url.host_str() != Some("github.com")
        || !verification_url.username().is_empty()
        || verification_url.password().is_some()
    {
        return Err(Error::ProviderUnavailable);
    }
    let expires_in = document["expires_in"]
        .as_u64()
        .unwrap_or(MAX_FLOW_LIFETIME_SECS as u64)
        .min(MAX_FLOW_LIFETIME_SECS as u64);
    let interval = document["interval"].as_u64().unwrap_or(5).clamp(5, 60);
    Ok(DeviceStart {
        verification_uri,
        user_code,
        expires_in,
        interval,
        state: random_secret(),
        device_code,
    })
}

pub(crate) async fn exchange_browser(
    client: &HttpClient,
    spec: ProviderSpec,
    input: BrowserExchange<'_>,
) -> Result<Token, Error> {
    if spec.flow == Flow::ClaudeCode {
        let response = client
            .post(spec.token_endpoint)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": input.client_id,
                "redirect_uri": input.redirect_uri,
                "code": input.code,
                "state": input.state,
                "code_verifier": input.verifier,
            }))
            .send()
            .await
            .map_err(|_| Error::ProviderUnavailable)?;
        let document = response_json(response).await?;
        required_text(&document, "access_token")?;
        return Ok(Token(document));
    }
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("client_id", input.client_id),
        ("redirect_uri", input.redirect_uri),
        ("code", input.code),
        ("code_verifier", input.verifier),
    ];
    if spec.flow == Flow::Antigravity {
        form.push((
            "client_secret",
            input
                .client_secret
                .filter(|secret| !secret.is_empty())
                .ok_or(Error::InvalidConfiguration)?,
        ));
    }
    exchange(client, spec.token_endpoint, &form).await
}

pub(crate) async fn poll_device(
    client: &HttpClient,
    spec: ProviderSpec,
    client_id: &str,
    device_code: &str,
) -> Result<Token, Error> {
    exchange(
        client,
        spec.token_endpoint,
        &[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ],
    )
    .await
}

/// Turn the identity-provider token into the complete provider credential before
/// it is persisted. Provider-specific validation happens here so a successful
/// setup can never create a credential that the gateway is unable to use.
pub(crate) async fn finalize_credential(
    client: &HttpClient,
    spec: ProviderSpec,
    client_id: &str,
    client_secret: Option<&str>,
    token: Token,
    now: i64,
) -> Result<Token, Error> {
    match spec.flow {
        Flow::Antigravity => {
            let client_secret = client_secret
                .filter(|secret| !secret.is_empty() && secret.len() <= 4096)
                .ok_or(Error::InvalidConfiguration)?;
            let access_token = required_secret(&token.0, "access_token")?;
            let refresh_token = required_secret(&token.0, "refresh_token")?;
            let project_id = resolve_antigravity_project(client, spec, access_token).await?;
            Ok(Token(json!({
                "version": 1,
                "flow": spec.flow.as_str(),
                "client_id": client_id,
                "client_secret": client_secret,
                "access_token": access_token,
                "refresh_token": refresh_token,
                "expires_at": access_token_expiry(&token.0, now),
                "project_id": project_id,
            })))
        }
        Flow::GithubCopilot => {
            let github_access_token = required_secret(&token.0, "access_token")?;
            let (copilot_token, copilot_expires_at) =
                exchange_copilot_token(client, spec, github_access_token, now).await?;
            Ok(Token(json!({
                "version": 1,
                "flow": spec.flow.as_str(),
                "client_id": client_id,
                "github_access_token": github_access_token,
                "copilot_token": copilot_token,
                "copilot_expires_at": copilot_expires_at,
            })))
        }
        Flow::Codex | Flow::Xai | Flow::ClaudeCode => Ok(token),
    }
}

/// Resolve an encrypted-at-rest credential to the short-lived value sent to an
/// upstream. A replacement document is returned only when a refresh occurred;
/// callers must encrypt and durably persist it before using the returned token.
pub(crate) async fn resolve_credential(
    client: &HttpClient,
    spec: ProviderSpec,
    credential_type: &str,
    secret: Zeroizing<String>,
    now: i64,
) -> Result<ResolvedCredential, Error> {
    match credential_type {
        "oauth_antigravity" if spec.flow == Flow::Antigravity => {
            let mut document = stored_document(&secret, Flow::Antigravity)?;
            let expires_at = document
                .get("expires_at")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let replacement = if expires_at <= now.saturating_add(TOKEN_EXPIRY_SKEW_SECS) {
                document = refresh_antigravity(client, spec, &document, now).await?;
                Some(Zeroizing::new(document.to_string()))
            } else {
                None
            };
            let access_token = required_secret(&document, "access_token")?;
            let project_id = required_identifier(&document, "project_id")?;
            Ok(ResolvedCredential {
                secret: Zeroizing::new(
                    json!({"access_token":access_token,"project_id":project_id}).to_string(),
                ),
                replacement,
            })
        }
        "oauth_github_copilot" if spec.flow == Flow::GithubCopilot => {
            let mut document = stored_document(&secret, Flow::GithubCopilot)?;
            let expires_at = document
                .get("copilot_expires_at")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let replacement = if expires_at <= now.saturating_add(COPILOT_EXPIRY_SKEW_SECS) {
                let github_access_token = required_secret(&document, "github_access_token")?;
                let (token, expires_at) =
                    exchange_copilot_token(client, spec, github_access_token, now).await?;
                document["copilot_token"] = Value::String(token);
                document["copilot_expires_at"] = Value::from(expires_at);
                Some(Zeroizing::new(document.to_string()))
            } else {
                None
            };
            let token = required_secret(&document, "copilot_token")?;
            Ok(ResolvedCredential {
                secret: Zeroizing::new(token.to_owned()),
                replacement,
            })
        }
        "oauth_antigravity" | "oauth_github_copilot" => Err(Error::InvalidCredential),
        _ => credential_secret(credential_type, secret)
            .map(|secret| ResolvedCredential {
                secret,
                replacement: None,
            })
            .map_err(|_| Error::InvalidCredential),
    }
}

fn stored_document(secret: &str, flow: Flow) -> Result<Value, Error> {
    if secret.len() > MAX_IDP_RESPONSE_BYTES {
        return Err(Error::InvalidCredential);
    }
    let document: Value = serde_json::from_str(secret).map_err(|_| Error::InvalidCredential)?;
    if document.get("version").and_then(Value::as_u64) != Some(1)
        || document.get("flow").and_then(Value::as_str) != Some(flow.as_str())
        || !document.is_object()
    {
        return Err(Error::InvalidCredential);
    }
    Ok(document)
}

async fn refresh_antigravity(
    client: &HttpClient,
    spec: ProviderSpec,
    document: &Value,
    now: i64,
) -> Result<Value, Error> {
    let client_id = required_identifier(document, "client_id")?;
    let client_secret = required_secret(document, "client_secret")?;
    let refresh_token = required_secret(document, "refresh_token")?;
    let project_id = required_identifier(document, "project_id")?;
    let refreshed = exchange(
        client,
        spec.token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
        ],
    )
    .await?
    .0;
    let access_token = required_secret(&refreshed, "access_token")?;
    Ok(json!({
        "version": 1,
        "flow": Flow::Antigravity.as_str(),
        "client_id": client_id,
        "client_secret": client_secret,
        "access_token": access_token,
        "refresh_token": refreshed.get("refresh_token").and_then(Value::as_str).unwrap_or(refresh_token),
        "expires_at": access_token_expiry(&refreshed, now),
        "project_id": project_id,
    }))
}

async fn resolve_antigravity_project(
    client: &HttpClient,
    spec: ProviderSpec,
    access_token: &str,
) -> Result<String, Error> {
    let endpoint = spec
        .provider_token_endpoint
        .ok_or(Error::InvalidConfiguration)?;
    let response = client
        .post(endpoint)
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            "x-goog-api-client",
            "google-cloud-sdk vscode_cloudshelleditor/0.1",
        )
        .header(
            "client-metadata",
            r#"{"ideType":"ANTIGRAVITY","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#,
        )
        .json(&json!({
            "metadata": {
                "ideType": "ANTIGRAVITY",
                "platform": "PLATFORM_UNSPECIFIED",
                "pluginType": "GEMINI",
            }
        }))
        .send()
        .await
        .map_err(|_| Error::ProviderUnavailable)?;
    let document = response_json(response).await?;
    let project = match document.get("cloudaicompanionProject") {
        Some(Value::String(project)) => Some(project.as_str()),
        Some(Value::Object(project)) => project.get("id").and_then(Value::as_str),
        _ => None,
    }
    .filter(|project| valid_identifier(project))
    .ok_or(Error::ProviderUnavailable)?;
    Ok(project.to_owned())
}

async fn exchange_copilot_token(
    client: &HttpClient,
    spec: ProviderSpec,
    github_access_token: &str,
    now: i64,
) -> Result<(String, i64), Error> {
    let endpoint = spec
        .provider_token_endpoint
        .ok_or(Error::InvalidConfiguration)?;
    let response = client
        .get(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("token {github_access_token}"),
        )
        .send()
        .await
        .map_err(|_| Error::ProviderUnavailable)?;
    let document = response_json(response).await?;
    let token = required_secret(&document, "token")?.to_owned();
    let expires_at = document
        .get("expires_at")
        .and_then(Value::as_i64)
        .filter(|expires_at| *expires_at > now)
        .ok_or(Error::ProviderUnavailable)?;
    Ok((token, expires_at))
}

fn access_token_expiry(document: &Value, now: i64) -> i64 {
    let lifetime = document
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .clamp(0, MAX_ACCESS_TOKEN_LIFETIME_SECS);
    now.saturating_add(lifetime)
}

fn required_secret<'a>(document: &'a Value, key: &str) -> Result<&'a str, Error> {
    document
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= 16 * 1024 && !value.chars().any(char::is_control)
        })
        .ok_or(Error::InvalidCredential)
}

fn required_identifier<'a>(document: &'a Value, key: &str) -> Result<&'a str, Error> {
    document
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| valid_identifier(value))
        .ok_or(Error::InvalidCredential)
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

async fn exchange(
    client: &HttpClient,
    endpoint: &str,
    form: &[(&str, &str)],
) -> Result<Token, Error> {
    let response = client
        .post(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|_| Error::ProviderUnavailable)?;
    let document = response_json(response).await?;
    if let Some(error) = document.get("error").and_then(Value::as_str) {
        return match error {
            "authorization_pending" => Err(Error::Pending),
            "slow_down" => Err(Error::SlowDown),
            "expired_token" => Err(Error::Expired),
            "access_denied" => Err(Error::Declined),
            _ => Err(Error::ProviderUnavailable),
        };
    }
    required_text(&document, "access_token")?;
    Ok(Token(document))
}

async fn response_json(response: Response) -> Result<Value, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IDP_RESPONSE_BYTES as u64)
    {
        return Err(Error::ProviderUnavailable);
    }
    let success = response.status().is_success();
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| Error::ProviderUnavailable)?;
        if bytes.len() + chunk.len() > MAX_IDP_RESPONSE_BYTES {
            return Err(Error::ProviderUnavailable);
        }
        bytes.extend_from_slice(&chunk);
    }
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| Error::ProviderUnavailable)?;
    if success {
        Ok(document)
    } else {
        match document.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => Err(Error::Pending),
            Some("slow_down") => Err(Error::SlowDown),
            Some("expired_token") => Err(Error::Expired),
            Some("access_denied") => Err(Error::Declined),
            _ => Err(Error::ProviderUnavailable),
        }
    }
}

fn required_text<'a>(document: &'a Value, key: &str) -> Result<&'a str, Error> {
    document
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .ok_or(Error::ProviderUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        body::Bytes,
        response::Redirect,
        routing::{get, post},
    };
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    async fn server(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}"), handle)
    }

    fn endpoint(value: String) -> &'static str {
        Box::leak(value.into_boxed_str())
    }

    #[tokio::test]
    async fn oauth_browser_flows_use_pkce_and_accept_tokens_without_exposing_them() {
        let access_token = Arc::new(random_secret());
        let refresh_token = Arc::new(random_secret());
        let response_access_token = Arc::clone(&access_token);
        let response_refresh_token = Arc::clone(&refresh_token);
        let app = Router::new().route(
            "/token",
            post(move |body: Bytes| {
                let access_token = Arc::clone(&response_access_token);
                let refresh_token = Arc::clone(&response_refresh_token);
                async move {
                    let form = String::from_utf8(body.to_vec()).unwrap();
                    assert!(form.contains("code_verifier="), "{form}");
                    assert!(form.contains("code=returned-code"), "{form}");
                    Json(json!({"access_token":access_token.as_str(),"refresh_token":refresh_token.as_str(),"token_type":"bearer"}))
                }
            }),
        );
        let (base, handle) = server(app).await;
        let token_endpoint = endpoint(format!("{base}/token"));
        let client = HttpClient::test();
        for flow in [Flow::Codex, Flow::Xai] {
            let spec = ProviderSpec {
                flow,
                authorization_endpoint: Some("https://idp.example/authorize"),
                token_endpoint,
                device_endpoint: None,
                provider_token_endpoint: None,
                scope: "openid",
            };
            let start = start_browser(spec, "client", "https://console.example/callback").unwrap();
            assert!(
                start
                    .authorization_url
                    .contains("code_challenge_method=S256")
            );
            assert!(!start.authorization_url.contains(&start.verifier));
            let token = exchange_browser(
                &client,
                spec,
                BrowserExchange {
                    client_id: "client",
                    client_secret: None,
                    redirect_uri: "https://console.example/callback",
                    code: "returned-code",
                    state: &start.state,
                    verifier: &start.verifier,
                },
            )
            .await
            .unwrap()
            .into_value();
            assert_eq!(token["access_token"], access_token.as_str(), "{flow:?}");
        }
        handle.abort();
    }

    #[tokio::test]
    async fn claude_code_uses_its_json_exchange_contract() {
        let app = Router::new().route(
            "/token",
            post(
                |headers: http::HeaderMap, Json(body): Json<Value>| async move {
                    assert_eq!(
                        headers.get(http::header::CONTENT_TYPE).unwrap(),
                        "application/json"
                    );
                    assert_eq!(body["code"], "returned-code");
                    assert_eq!(body["state"], "returned-state");
                    assert_eq!(body["code_verifier"], "returned-verifier");
                    Json(json!({"access_token":"claude-access","refresh_token":"claude-refresh"}))
                },
            ),
        );
        let (base, handle) = server(app).await;
        let spec = ProviderSpec::test_browser(
            Flow::ClaudeCode,
            "https://claude.example/authorize".into(),
            format!("{base}/token"),
        );
        let token = exchange_browser(
            &HttpClient::test(),
            spec,
            BrowserExchange {
                client_id: "client",
                client_secret: None,
                redirect_uri: "https://console.example/callback",
                code: "returned-code",
                state: "returned-state",
                verifier: "returned-verifier",
            },
        )
        .await
        .unwrap()
        .into_value();
        assert_eq!(token["access_token"], "claude-access");
        handle.abort();
    }

    #[test]
    fn provider_authorization_contracts_are_specific_and_bound_to_channel_kinds() {
        let claude = ProviderSpec::for_flow(Flow::ClaudeCode);
        assert_eq!(
            claude.token_endpoint,
            "https://api.anthropic.com/v1/oauth/token"
        );
        assert!(claude.scope.contains("user:inference"));

        let xai = ProviderSpec::for_flow(Flow::Xai);
        assert!(xai.scope.contains("grok-cli:access"));
        assert!(xai.scope.contains("api:access"));

        let antigravity = ProviderSpec::for_flow(Flow::Antigravity);
        assert!(antigravity.scope.contains("auth/cloud-platform"));
        let start =
            start_browser(antigravity, "client", "https://console.example/callback").unwrap();
        let url = Url::parse(&start.authorization_url).unwrap();
        let query = url
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            query.get("access_type").map(|value| value.as_ref()),
            Some("offline")
        );
        assert_eq!(
            query.get("prompt").map(|value| value.as_ref()),
            Some("consent")
        );
        assert!(Flow::Antigravity.is_available());
        assert!(Flow::GithubCopilot.is_available());
        assert!(Flow::Antigravity.supports_provider_kind("gemini"));
        assert!(!Flow::Antigravity.supports_provider_kind("openai"));
        assert!(Flow::GithubCopilot.supports_provider_kind("openai"));
        assert!(!Flow::GithubCopilot.supports_provider_kind("openai_compatible"));
    }

    #[tokio::test]
    async fn oauth_copilot_device_flow_clamps_polling_and_distinguishes_slow_down() {
        let polls = Arc::new(AtomicUsize::new(0));
        let poll_counter = Arc::clone(&polls);
        let device_code = Arc::new(random_secret());
        let access_token = Arc::new(random_secret());
        let response_device_code = Arc::clone(&device_code);
        let response_access_token = Arc::clone(&access_token);
        let app = Router::new()
            .route(
                "/device",
                post(move || {
                    let device_code = Arc::clone(&response_device_code);
                    async move {
                        Json(json!({
                            "device_code":device_code.as_str(),
                            "user_code":"ABCD-EFGH",
                            "verification_uri":"https://github.com/login/device",
                            "expires_in":99999,
                            "interval":1
                        }))
                    }
                }),
            )
            .route(
                "/token",
                post(move || {
                    let count = poll_counter.fetch_add(1, Ordering::SeqCst);
                    let access_token = Arc::clone(&response_access_token);
                    async move {
                        match count {
                            0 => (
                                reqwest::StatusCode::BAD_REQUEST,
                                Json(json!({"error":"authorization_pending"})),
                            ),
                            1 => (reqwest::StatusCode::OK, Json(json!({"error":"slow_down"}))),
                            _ => (
                                reqwest::StatusCode::OK,
                                Json(json!({"access_token":access_token.as_str()})),
                            ),
                        }
                    }
                }),
            );
        let (base, handle) = server(app).await;
        let spec = ProviderSpec {
            flow: Flow::GithubCopilot,
            authorization_endpoint: None,
            token_endpoint: endpoint(format!("{base}/token")),
            device_endpoint: Some(endpoint(format!("{base}/device"))),
            provider_token_endpoint: Some(endpoint(format!("{base}/copilot-token"))),
            scope: "read:user",
        };
        let client = HttpClient::test();
        let start = start_device(&client, spec, "client").await.unwrap();
        assert_eq!(start.interval, 5);
        assert_eq!(start.expires_in, MAX_FLOW_LIFETIME_SECS as u64);
        assert!(matches!(
            poll_device(&client, spec, "client", &start.device_code).await,
            Err(Error::Pending)
        ));
        assert!(matches!(
            poll_device(&client, spec, "client", &start.device_code).await,
            Err(Error::SlowDown)
        ));
        let token = poll_device(&client, spec, "client", &start.device_code)
            .await
            .unwrap()
            .into_value();
        assert_eq!(token["access_token"], access_token.as_str());
        handle.abort();
    }

    #[tokio::test]
    async fn antigravity_resolves_code_assist_project_and_refreshes_access() {
        let initial_access = Arc::new(random_secret());
        let refreshed_access = Arc::new(random_secret());
        let refresh_token = Arc::new(random_secret());
        let client_secret = Arc::new(random_secret());
        let exchanges = Arc::new(AtomicUsize::new(0));
        let exchange_counter = Arc::clone(&exchanges);
        let response_initial = Arc::clone(&initial_access);
        let response_refreshed = Arc::clone(&refreshed_access);
        let response_refresh = Arc::clone(&refresh_token);
        let expected_secret = Arc::clone(&client_secret);
        let expected_project_access = Arc::clone(&initial_access);
        let app = Router::new()
            .route(
                "/token",
                post(move |body: Bytes| {
                    let initial = Arc::clone(&response_initial);
                    let refreshed = Arc::clone(&response_refreshed);
                    let refresh = Arc::clone(&response_refresh);
                    let secret = Arc::clone(&expected_secret);
                    let count = exchange_counter.fetch_add(1, Ordering::SeqCst);
                    async move {
                        let form = String::from_utf8(body.to_vec()).unwrap();
                        assert!(form.contains(&format!("client_secret={secret}")), "{form}");
                        if count == 0 {
                            assert!(form.contains("grant_type=authorization_code"), "{form}");
                            Json(json!({
                                "access_token": initial.as_str(),
                                "refresh_token": refresh.as_str(),
                                "expires_in": 1,
                            }))
                        } else {
                            assert!(form.contains("grant_type=refresh_token"), "{form}");
                            assert!(form.contains(&format!("refresh_token={refresh}")), "{form}");
                            Json(json!({"access_token":refreshed.as_str(),"expires_in":3600}))
                        }
                    }
                }),
            )
            .route(
                "/project",
                post(move |headers: http::HeaderMap, Json(body): Json<Value>| {
                    let access = Arc::clone(&expected_project_access);
                    async move {
                        assert_eq!(
                            headers[http::header::AUTHORIZATION],
                            format!("Bearer {access}")
                        );
                        assert_eq!(
                            headers["x-goog-api-client"],
                            "google-cloud-sdk vscode_cloudshelleditor/0.1"
                        );
                        assert_eq!(body["metadata"]["ideType"], "ANTIGRAVITY");
                        Json(json!({"cloudaicompanionProject":{"id":"code-assist-project"}}))
                    }
                }),
            );
        let (base, handle) = server(app).await;
        let spec = ProviderSpec::test_browser(
            Flow::Antigravity,
            "https://accounts.example/authorize".into(),
            format!("{base}/token"),
        )
        .test_provider_endpoint(format!("{base}/project"));
        let client = HttpClient::test();
        let exchanged = exchange_browser(
            &client,
            spec,
            BrowserExchange {
                client_id: "antigravity-client",
                client_secret: Some(&client_secret),
                redirect_uri: "https://console.example/callback",
                code: "authorization-code",
                state: "oauth-state",
                verifier: "pkce-verifier",
            },
        )
        .await
        .unwrap();
        let stored = finalize_credential(
            &client,
            spec,
            "antigravity-client",
            Some(&client_secret),
            exchanged,
            100,
        )
        .await
        .unwrap()
        .into_value();
        assert_eq!(stored["project_id"], "code-assist-project");
        assert_eq!(stored["refresh_token"], refresh_token.as_str());

        let resolved = resolve_credential(
            &client,
            spec,
            "oauth_antigravity",
            Zeroizing::new(stored.to_string()),
            102,
        )
        .await
        .unwrap();
        let (provider_secret, replacement) = resolved.into_parts();
        let provider_secret: Value = serde_json::from_str(&provider_secret).unwrap();
        assert_eq!(provider_secret["access_token"], refreshed_access.as_str());
        assert_eq!(provider_secret["project_id"], "code-assist-project");
        let replacement = replacement.unwrap();
        assert!(replacement.contains(refreshed_access.as_str()));
        assert!(replacement.contains(refresh_token.as_str()));
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);

        let resolved = resolve_credential(&client, spec, "oauth_antigravity", replacement, 103)
            .await
            .unwrap();
        assert!(resolved.into_parts().1.is_none());
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        handle.abort();
    }

    #[tokio::test]
    async fn copilot_exchanges_github_token_and_refreshes_derived_token() {
        let github_token = Arc::new(random_secret());
        let first_copilot = Arc::new(random_secret());
        let refreshed_copilot = Arc::new(random_secret());
        let exchanges = Arc::new(AtomicUsize::new(0));
        let exchange_counter = Arc::clone(&exchanges);
        let expected_github = Arc::clone(&github_token);
        let response_first = Arc::clone(&first_copilot);
        let response_refreshed = Arc::clone(&refreshed_copilot);
        let app = Router::new().route(
            "/copilot-token",
            get(move |headers: http::HeaderMap| {
                let github = Arc::clone(&expected_github);
                let first = Arc::clone(&response_first);
                let refreshed = Arc::clone(&response_refreshed);
                let count = exchange_counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    assert_eq!(
                        headers[http::header::AUTHORIZATION],
                        format!("token {github}")
                    );
                    if count == 0 {
                        Json(json!({"token":first.as_str(),"expires_at":1000}))
                    } else {
                        Json(json!({"token":refreshed.as_str(),"expires_at":2000}))
                    }
                }
            }),
        );
        let (base, handle) = server(app).await;
        let spec = ProviderSpec::test_device(
            format!("{base}/unused-device"),
            format!("{base}/unused-token"),
        )
        .test_provider_endpoint(format!("{base}/copilot-token"));
        let client = HttpClient::test();
        let stored = finalize_credential(
            &client,
            spec,
            "github-client",
            None,
            Token(json!({"access_token":github_token.as_str()})),
            100,
        )
        .await
        .unwrap()
        .into_value();
        assert_eq!(stored["copilot_token"], first_copilot.as_str());

        let resolved = resolve_credential(
            &client,
            spec,
            "oauth_github_copilot",
            Zeroizing::new(stored.to_string()),
            200,
        )
        .await
        .unwrap();
        assert_eq!(resolved.into_parts().0.as_str(), first_copilot.as_str());
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);

        let resolved = resolve_credential(
            &client,
            spec,
            "oauth_github_copilot",
            Zeroizing::new(stored.to_string()),
            800,
        )
        .await
        .unwrap();
        let (token, replacement) = resolved.into_parts();
        assert_eq!(token.as_str(), refreshed_copilot.as_str());
        assert!(replacement.unwrap().contains(refreshed_copilot.as_str()));
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        handle.abort();
    }

    #[tokio::test]
    async fn oauth_http_boundary_rejects_redirects_and_oversized_bodies() {
        let redirected = Arc::new(AtomicUsize::new(0));
        let redirected_count = Arc::clone(&redirected);
        let app = Router::new()
            .route("/redirect", post(|| async { Redirect::temporary("/sink") }))
            .route(
                "/sink",
                post(move || {
                    redirected_count.fetch_add(1, Ordering::SeqCst);
                    async { Json(json!({"access_token":"must-not-be-reached"})) }
                }),
            )
            .route(
                "/large",
                post(|| async { "x".repeat(MAX_IDP_RESPONSE_BYTES + 1) }),
            );
        let (base, handle) = server(app).await;
        let client = HttpClient::test();
        for path in ["redirect", "large"] {
            let spec = ProviderSpec::test_browser(
                Flow::Codex,
                "https://idp.example/authorize".into(),
                format!("{base}/{path}"),
            );
            assert!(matches!(
                exchange_browser(
                    &client,
                    spec,
                    BrowserExchange {
                        client_id: "client",
                        client_secret: None,
                        redirect_uri: "https://console.example/callback",
                        code: "code",
                        state: "state",
                        verifier: "verifier",
                    },
                )
                .await,
                Err(Error::ProviderUnavailable)
            ));
        }
        assert_eq!(redirected.load(Ordering::SeqCst), 0);
        handle.abort();
    }
}
