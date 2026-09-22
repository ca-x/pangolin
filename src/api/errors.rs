use super::*;

#[derive(Clone, Copy)]
pub(crate) enum Protocol {
    OpenAi,
    Anthropic,
    Gemini,
}

pub(crate) fn protocol(path: &str) -> Protocol {
    if path == "/v1/messages" || path.starts_with("/anthropic/") {
        Protocol::Anthropic
    } else if path.starts_with("/gemini/") || path.starts_with("/v1beta/") {
        Protocol::Gemini
    } else {
        Protocol::OpenAi
    }
}

pub(crate) fn gemini_status(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 | 405 | 422 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        408 | 504 => "DEADLINE_EXCEEDED",
        409 => "ABORTED",
        413 | 429 => "RESOURCE_EXHAUSTED",
        499 => "CANCELLED",
        500 => "INTERNAL",
        501 => "UNIMPLEMENTED",
        502 | 503 => "UNAVAILABLE",
        _ => "UNKNOWN",
    }
}

pub(crate) fn anthropic_type(status: StatusCode) -> &'static str {
    match status.as_u16() {
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        413 => "request_too_large",
        429 => "rate_limit_error",
        529 | 503 => "overloaded_error",
        400..=499 => "invalid_request_error",
        _ => "api_error",
    }
}

pub(crate) fn openai_type(status: StatusCode) -> &'static str {
    match status.as_u16() {
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        409 => "conflict_error",
        429 => "rate_limit_error",
        400..=499 => "invalid_request_error",
        500 => "internal_error",
        _ => "upstream_error",
    }
}

pub(crate) fn document(protocol: Protocol, status: StatusCode, kind: &str, message: &str) -> Value {
    match protocol {
        Protocol::OpenAi => json!({"error":{"type":kind,"message":message}}),
        Protocol::Anthropic => {
            json!({"type":"error","error":{"type":anthropic_type(status),"message":message}})
        }
        Protocol::Gemini => {
            json!({"error":{"code":status.as_u16(),"status":gemini_status(status),"message":message}})
        }
    }
}

pub(crate) fn response(error: ApiError, protocol: Protocol) -> Response {
    let (status, kind, message) = error.public_parts();
    (status, Json(document(protocol, status, kind, &message))).into_response()
}

/// The one place the access layer's typed errors become protocol errors. Every
/// caller maps them here so a new access error cannot be translated differently in
/// one route than in another, and no internal cause reaches the client.
pub(crate) fn access(error: crate::access::AccessError) -> ApiError {
    use crate::access::AccessError;
    match error {
        AccessError::Unauthorized => ApiError::Unauthorized,
        AccessError::Forbidden => ApiError::Forbidden,
        AccessError::NotFound => ApiError::NotFound,
        AccessError::Invalid(message) => ApiError::BadRequest(message),
        AccessError::Conflict(message) => ApiError::Conflict(message),
        AccessError::Internal(error) => ApiError::Internal(error),
    }
}

/// An explicit upstream pass-through policy owns its raw body, including errors.
#[derive(Clone)]
pub(crate) struct PassThrough;

/// Covers errors emitted by extractors as well as early handler failures.
pub(super) async fn native_errors(request: axum::extract::Request, next: Next) -> Response {
    let path = request.uri().path();
    let public = [
        "/v1/",
        "/anthropic/",
        "/gemini/",
        "/v1beta/",
        "/jina/v1/",
        "/doubao/v3/",
        "/aisdk/",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix));
    // Admin routes answer with the same envelope, but axum's own extractor
    // rejections (a malformed body, an oversized one) bypass it and return plain
    // text, so a client cannot parse every failure the same way.
    let admin = path.starts_with("/api/admin/");
    let protocol = if admin {
        Protocol::OpenAi
    } else {
        protocol(path)
    };
    let response = next.run(request).await;
    let status = response.status();
    let already_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    if !(public || admin)
        || !(status.is_client_error() || status.is_server_error())
        || response.extensions().get::<PassThrough>().is_some()
        // An admin handler that already produced the envelope keeps its own error
        // kind; public paths are always normalised to the protocol document.
        || (admin && already_json)
    {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .unwrap_or_default();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let message = parsed
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("request failed"));
    let value = document(protocol, status, openai_type(status), message);
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Response::from_parts(parts, Body::from(value.to_string()))
}
