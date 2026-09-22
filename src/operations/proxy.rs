use super::sql;
use crate::{
    api::{ApiError, AppState},
    catalog::refresh::public_address,
};
use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

#[derive(Clone)]
pub struct Resolved {
    url: reqwest::Url,
    addresses: Vec<SocketAddr>,
    username: Option<String>,
    password: Option<String>,
}

#[cfg(test)]
pub fn resolved_for_test(url: &str, addresses: Vec<SocketAddr>) -> Resolved {
    Resolved {
        url: reqwest::Url::parse(url).unwrap(),
        addresses,
        username: None,
        password: None,
    }
}

pub fn validate_url(value: &str) -> Result<reqwest::Url, ApiError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| ApiError::BadRequest("invalid outbound proxy URL".into()))?;
    let host = url
        .host_str()
        .ok_or_else(|| ApiError::BadRequest("invalid outbound proxy URL".into()))?;
    let test_loopback = cfg!(test) && host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if value.len() > 2048
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || (!test_loopback
            && (host.eq_ignore_ascii_case("localhost")
                || host.ends_with(".localhost")
                || host.ends_with(".local")
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| !public_address(ip))))
    {
        return Err(ApiError::BadRequest("invalid outbound proxy URL".into()));
    }
    Ok(url)
}

pub fn envelope(state: &AppState, preset: &str, credentials: &Value) -> Result<String, ApiError> {
    let username = credentials["username"].as_str();
    let password = credentials["password"].as_str();
    if !credentials.is_object()
        || username.is_none_or(|value| value.is_empty() || value.len() > 1024)
        || password.is_none_or(|value| value.is_empty() || value.len() > 1024)
        || serde_json::to_vec(credentials).map_or(true, |encoded| encoded.len() > 4096)
    {
        return Err(ApiError::BadRequest("invalid proxy credentials".into()));
    }
    Ok(state.secrets.encrypt(
        &json!({
            "version":1,
            "proxy_id":preset,
            "credentials":credentials
        })
        .to_string(),
    )?)
}

pub async fn resolve(state: &AppState, preset: Option<&str>) -> Result<Option<Resolved>, ApiError> {
    let Some(preset) = preset else {
        return Ok(None);
    };
    let row = state
        .db
        .query_one(sql(
            "SELECT url,secret_envelope FROM proxy_presets WHERE id=? AND enabled=1",
            vec![preset.into()],
        ))
        .await?
        .ok_or(ApiError::NotFound)?;
    let url = validate_url(&row.try_get::<String>("", "url")?)?;
    let host = url
        .host_str()
        .ok_or_else(|| ApiError::BadRequest("invalid outbound proxy URL".into()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ApiError::BadRequest("invalid outbound proxy URL".into()))?;
    let addresses = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| ApiError::Upstream("proxy DNS lookup timed out".into()))?
    .map_err(|_| ApiError::Upstream("proxy DNS lookup failed".into()))?
    .collect::<Vec<_>>();
    if addresses.is_empty()
        || (!cfg!(test)
            && addresses
                .iter()
                .any(|address| !public_address(address.ip())))
    {
        return Err(ApiError::BadRequest(
            "outbound proxy must be publicly routable".into(),
        ));
    }
    let (username, password) =
        if let Some(envelope) = row.try_get::<Option<String>>("", "secret_envelope")? {
            let secret: Value = serde_json::from_str(&state.secrets.decrypt(&envelope)?)
                .map_err(|_| ApiError::BadRequest("invalid proxy credentials".into()))?;
            if secret["version"] != 1 || secret["proxy_id"] != preset {
                return Err(ApiError::Forbidden);
            }
            (
                secret
                    .pointer("/credentials/username")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                secret
                    .pointer("/credentials/password")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            )
        } else {
            (None, None)
        };
    Ok(Some(Resolved {
        url,
        addresses,
        username,
        password,
    }))
}

pub fn client(resolved: Option<&Resolved>, timeout: Duration) -> Result<reqwest::Client, ApiError> {
    let builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(5));
    apply(builder, resolved)?
        .build()
        .map_err(|error| ApiError::Internal(error.into()))
}

pub fn apply(
    mut builder: reqwest::ClientBuilder,
    resolved: Option<&Resolved>,
) -> Result<reqwest::ClientBuilder, ApiError> {
    if let Some(resolved) = resolved {
        let mut proxy = reqwest::Proxy::all(resolved.url.as_str())
            .map_err(|_| ApiError::BadRequest("invalid outbound proxy URL".into()))?;
        if let (Some(username), Some(password)) = (&resolved.username, &resolved.password) {
            proxy = proxy.basic_auth(username, password);
        }
        builder = builder.proxy(proxy);
        let host = resolved
            .url
            .host_str()
            .ok_or_else(|| ApiError::BadRequest("invalid outbound proxy URL".into()))?;
        builder = builder.resolve_to_addrs(host, &resolved.addresses);
    } else {
        builder = builder.no_proxy();
    }
    Ok(builder)
}
