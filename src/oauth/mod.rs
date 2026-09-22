use futures_util::StreamExt;
use oauth2::PkceCodeChallenge;
use reqwest::{Client, Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_IDP_RESPONSE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_FLOW_LIFETIME_SECS: i64 = 900;

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

    pub(crate) fn is_device(self) -> bool {
        self == Self::GithubCopilot
    }

    pub(crate) fn is_available(self) -> bool {
        matches!(self, Self::Codex | Self::Xai | Self::ClaudeCode)
    }

    pub(crate) fn supports_provider_kind(self, kind: &str) -> bool {
        match self {
            Self::Codex => matches!(kind, "openai" | "openai_compatible"),
            Self::Xai => kind == "xai",
            Self::ClaudeCode => kind == "anthropic",
            Self::Antigravity | Self::GithubCopilot => false,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderSpec {
    flow: Flow,
    authorization_endpoint: Option<&'static str>,
    token_endpoint: &'static str,
    device_endpoint: Option<&'static str>,
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
            ),
            Flow::GithubCopilot => Self {
                flow,
                authorization_endpoint: None,
                token_endpoint: "https://github.com/login/oauth/access_token",
                device_endpoint: Some("https://github.com/login/device/code"),
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
            scope,
        }
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
            scope: "read:user",
        }
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
    #[error("OAuth authorization expired")]
    Expired,
    #[error("OAuth authorization was declined")]
    Declined,
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

pub(crate) struct Token(Value);

impl Token {
    pub(crate) fn into_value(self) -> Value {
        self.0
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
    client: &Client,
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
    client: &Client,
    spec: ProviderSpec,
    client_id: &str,
    redirect_uri: &str,
    code: &str,
    state: &str,
    verifier: &str,
) -> Result<Token, Error> {
    if spec.flow == Flow::ClaudeCode {
        let response = client
            .post(spec.token_endpoint)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": client_id,
                "redirect_uri": redirect_uri,
                "code": code,
                "state": state,
                "code_verifier": verifier,
            }))
            .send()
            .await
            .map_err(|_| Error::ProviderUnavailable)?;
        let document = response_json(response).await?;
        required_text(&document, "access_token")?;
        return Ok(Token(document));
    }
    exchange(
        client,
        spec.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("code", code),
            ("code_verifier", verifier),
        ],
    )
    .await
}

pub(crate) async fn poll_device(
    client: &Client,
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

async fn exchange(client: &Client, endpoint: &str, form: &[(&str, &str)]) -> Result<Token, Error> {
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
            "authorization_pending" | "slow_down" => Err(Error::Pending),
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
            Some("authorization_pending") | Some("slow_down") => Err(Error::Pending),
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
    use axum::{Json, Router, body::Bytes, routing::post};
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
        let client = Client::new();
        for flow in [Flow::Codex, Flow::Xai] {
            let spec = ProviderSpec {
                flow,
                authorization_endpoint: Some("https://idp.example/authorize"),
                token_endpoint,
                device_endpoint: None,
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
                "client",
                "https://console.example/callback",
                "returned-code",
                &start.state,
                &start.verifier,
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
            &Client::new(),
            spec,
            "client",
            "https://console.example/callback",
            "returned-code",
            "returned-state",
            "returned-verifier",
        )
        .await
        .unwrap()
        .into_value();
        assert_eq!(token["access_token"], "claude-access");
        handle.abort();
    }

    #[test]
    fn provider_authorization_contracts_are_specific_and_unfinished_flows_are_disabled() {
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
        assert!(!Flow::Antigravity.is_available());
        assert!(!Flow::GithubCopilot.is_available());
    }

    #[tokio::test]
    async fn oauth_copilot_device_flow_clamps_polling_and_reports_pending() {
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
                        if count == 0 {
                            (
                                reqwest::StatusCode::BAD_REQUEST,
                                Json(json!({"error":"authorization_pending"})),
                            )
                        } else {
                            (
                                reqwest::StatusCode::OK,
                                Json(json!({"access_token":access_token.as_str()})),
                            )
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
            scope: "read:user",
        };
        let client = Client::new();
        let start = start_device(&client, spec, "client").await.unwrap();
        assert_eq!(start.interval, 5);
        assert_eq!(start.expires_in, MAX_FLOW_LIFETIME_SECS as u64);
        assert!(matches!(
            poll_device(&client, spec, "client", &start.device_code).await,
            Err(Error::Pending)
        ));
        let token = poll_device(&client, spec, "client", &start.device_code)
            .await
            .unwrap()
            .into_value();
        assert_eq!(token["access_token"], access_token.as_str());
        handle.abort();
    }
}
