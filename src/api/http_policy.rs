use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

use super::AppState;
use crate::operations::sql;

pub(crate) const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 600_000;
const MAX_REQUEST_TIMEOUT_MS: u64 = 3_600_000;
const MAX_CORS_ORIGINS: usize = 32;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct HttpPolicy {
    #[serde(default)]
    pub cors_allowed_origins: Vec<String>,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
}

impl Default for HttpPolicy {
    fn default() -> Self {
        Self {
            cors_allowed_origins: Vec::new(),
            request_timeout_ms: DEFAULT_REQUEST_TIMEOUT_MS,
        }
    }
}

const fn default_request_timeout_ms() -> u64 {
    DEFAULT_REQUEST_TIMEOUT_MS
}

impl HttpPolicy {
    pub(crate) fn validate(&self) -> bool {
        (100..=MAX_REQUEST_TIMEOUT_MS).contains(&self.request_timeout_ms)
            && self.cors_allowed_origins.len() <= MAX_CORS_ORIGINS
            && self
                .cors_allowed_origins
                .iter()
                .enumerate()
                .all(|(index, origin)| {
                    origin.len() <= 2048
                        && canonical_origin(origin).as_deref() == Some(origin.as_str())
                        && !self.cors_allowed_origins[..index].contains(origin)
                })
    }

    fn allows(&self, origin: &str) -> bool {
        self.cors_allowed_origins
            .iter()
            .any(|allowed| allowed == origin)
    }
}

pub(crate) fn canonical_origin(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

async fn effective_policy(state: &AppState) -> HttpPolicy {
    let stored = state
        .db
        .query_one(sql("SELECT value FROM settings WHERE key='system'", vec![]))
        .await
        .ok()
        .flatten()
        .and_then(|row| row.try_get::<String>("", "value").ok())
        .and_then(|document| serde_json::from_str::<HttpPolicy>(&document).ok());
    match stored {
        Some(policy) if policy.validate() => policy,
        _ => HttpPolicy::default(),
    }
}

pub(crate) async fn enforce(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let policy = effective_policy(&state).await;
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let allowed = origin.as_deref().is_some_and(|value| policy.allows(value));
    let preflight = request.method() == Method::OPTIONS
        && request
            .headers()
            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);

    let mut response = if preflight && allowed {
        StatusCode::NO_CONTENT.into_response()
    } else {
        match tokio::time::timeout(
            Duration::from_millis(policy.request_timeout_ms),
            next.run(request),
        )
        .await
        {
            Ok(response) => response,
            Err(_) => (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({"error":{"type":"timeout_error","message":"request timed out"}})),
            )
                .into_response(),
        }
    };

    if origin.is_some() {
        response
            .headers_mut()
            .append(header::VARY, HeaderValue::from_static("Origin"));
    }
    if allowed {
        let headers = response.headers_mut();
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_str(origin.as_deref().unwrap_or_default())
                .expect("validated origin is an HTTP header value"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        if preflight {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, HEAD, POST, PUT, DELETE, OPTIONS"),
            );
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("authorization, content-type, x-api-key, x-pangolin-csrf"),
            );
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_policy_rejects_wildcards_paths_and_unbounded_timeouts() {
        let default = HttpPolicy::default();
        assert!(default.validate());
        assert!(default.cors_allowed_origins.is_empty());
        assert_eq!(default.request_timeout_ms, DEFAULT_REQUEST_TIMEOUT_MS);
        for origin in [
            "*",
            "https://example.test/path",
            "https://user@example.test",
        ] {
            assert!(canonical_origin(origin).is_none(), "{origin}");
        }
        assert_eq!(
            canonical_origin("https://example.test:8443"),
            Some("https://example.test:8443".into())
        );
        assert!(
            !HttpPolicy {
                cors_allowed_origins: vec![],
                request_timeout_ms: 99,
            }
            .validate()
        );
    }
}
