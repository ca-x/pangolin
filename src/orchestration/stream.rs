//! SSE framing uses a maintained parser. A complete first event is read before
//! downstream commitment; terminal detection is protocol-aware.
use super::policy::{ErrorMode, Retry};
use axum::body::Bytes;
use eventsource_stream::Event;
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

pub type Events = Pin<Box<dyn Stream<Item = std::result::Result<Event, std::io::Error>> + Send>>;

pub fn events(response: reqwest::Response) -> Events {
    let buffered = Arc::new(AtomicUsize::new(0));
    let input_buffered = buffered.clone();
    let bytes = response.bytes_stream().map(move |chunk| {
        let chunk = chunk.map_err(|_| std::io::Error::other("upstream stream transport error"))?;
        if input_buffered
            .fetch_add(chunk.len(), Ordering::Relaxed)
            .saturating_add(chunk.len())
            > 1024 * 1024
        {
            return Err(std::io::Error::other("upstream SSE event exceeds limit"));
        }
        Ok(chunk)
    });
    Box::pin(
        crate::providers::framing::frames(bytes).filter_map(move |event| {
            std::future::ready(match event {
                Ok(frame) => frame.data.map(|data| {
                    buffered.store(0, Ordering::Relaxed);
                    Ok(Event {
                        event: frame.event.unwrap_or_else(|| "message".into()),
                        data,
                        id: frame.id.unwrap_or_default(),
                        retry: frame.retry.map(Duration::from_millis),
                    })
                }),
                Err(_) => Some(Err(std::io::Error::other("invalid upstream SSE event"))),
            })
        }),
    )
}

pub async fn next(
    events: &mut Events,
    timeout_ms: u64,
) -> std::result::Result<Option<Event>, std::io::Error> {
    tokio::time::timeout(Duration::from_millis(timeout_ms), events.next())
        .await
        .map_err(|_| std::io::Error::other("upstream SSE event timeout"))?
        .transpose()
}

pub fn encode(event: &Event) -> Bytes {
    let mut frame = String::new();
    if !event.event.is_empty() && event.event != "message" {
        frame.push_str("event: ");
        frame.push_str(&event.event);
        frame.push('\n');
    }
    if !event.id.is_empty() {
        frame.push_str("id: ");
        frame.push_str(&event.id);
        frame.push('\n');
    }
    if let Some(retry) = event.retry {
        frame.push_str(&format!("retry: {}\n", retry.as_millis()));
    }
    for line in event.data.split('\n') {
        frame.push_str("data: ");
        frame.push_str(line);
        frame.push('\n');
    }
    frame.push('\n');
    Bytes::from(frame)
}

pub fn error_event(mut event: Event, policy: &Retry, endpoint: &str) -> Event {
    if matches!(policy.error_mode, ErrorMode::PassThrough) {
        return event;
    }
    let message = if matches!(policy.error_mode, ErrorMode::Custom) {
        policy
            .error_message
            .as_deref()
            .unwrap_or("upstream request failed")
    } else {
        "upstream request failed"
    };
    let error = json!({"type":"upstream_error","message":message});
    // Build a new minimal error envelope: upstream IDs, output, debugging fields
    // and nested error metadata may also contain sensitive material.
    let data = if endpoint == "/v1/responses" {
        let status = response_failure(&event).unwrap_or("failed");
        event.event = format!("response.{status}");
        json!({"type":event.event,"response":{"status":status,"error":error}})
    } else {
        let status = event_status(&event);
        event.event = "error".into();
        native_error(endpoint, message, status)
    };
    event.id.clear();
    event.retry = None;
    event.data = data.to_string();
    event
}

/// A transport failure after the first downstream event has no upstream error
/// envelope to pass through.  Give every policy a protocol-valid, safe terminal
/// event instead of tearing down the client stream with an I/O error.
pub fn interrupted_event(policy: &Retry, endpoint: &str) -> Event {
    let event = if endpoint == "/v1/responses" {
        Event {
            event: "response.failed".into(),
            data: json!({
                "type": "response.failed",
                "response": {
                    "status": "failed",
                    "error": {
                        "type": "upstream_error",
                        "message": "upstream stream interrupted"
                    }
                }
            })
            .to_string(),
            ..Event::default()
        }
    } else {
        Event {
            event: "error".into(),
            data: native_error(
                endpoint,
                "upstream stream interrupted",
                http::StatusCode::BAD_GATEWAY,
            )
            .to_string(),
            ..Event::default()
        }
    };
    error_event(event, policy, endpoint)
}

fn event_status(event: &Event) -> http::StatusCode {
    let value: Value = serde_json::from_str(&event.data).unwrap_or(Value::Null);
    if let Some(status) = value
        .pointer("/error/code")
        .and_then(Value::as_u64)
        .and_then(|code| u16::try_from(code).ok())
        .and_then(|code| http::StatusCode::from_u16(code).ok())
        .filter(|status| status.is_client_error() || status.is_server_error())
    {
        return status;
    }
    match value.pointer("/error/type").and_then(Value::as_str) {
        Some("authentication_error") => http::StatusCode::UNAUTHORIZED,
        Some("permission_error") => http::StatusCode::FORBIDDEN,
        Some("rate_limit_error") => http::StatusCode::TOO_MANY_REQUESTS,
        Some("overloaded_error") => http::StatusCode::from_u16(529).unwrap(),
        Some("invalid_request_error") => http::StatusCode::BAD_REQUEST,
        _ => http::StatusCode::BAD_GATEWAY,
    }
}

fn native_error(endpoint: &str, message: &str, status: http::StatusCode) -> Value {
    let protocol = crate::api::errors::protocol(endpoint);
    let mut value = crate::api::errors::document(protocol, status, "upstream_error", message);
    if matches!(protocol, crate::api::errors::Protocol::OpenAi) {
        value["type"] = json!("error");
    }
    value
}

pub struct TerminalState {
    expected: usize,
    finished: std::collections::BTreeSet<u64>,
}
impl TerminalState {
    pub fn new(payload: &Value) -> Self {
        Self {
            expected: payload
                .pointer("/generationConfig/candidateCount")
                .and_then(Value::as_u64)
                .and_then(|count| usize::try_from(count).ok())
                .unwrap_or(1),
            finished: Default::default(),
        }
    }
    pub fn terminal(&mut self, event: &Event, endpoint: &str) -> bool {
        if endpoint != "/v1beta/models:streamGenerateContent" || failed(event) {
            return terminal(event, endpoint);
        }
        if let Ok(value) = serde_json::from_str::<Value>(&event.data)
            && let Some(candidates) = value["candidates"].as_array()
        {
            for candidate in candidates {
                let index = candidate["index"]
                    .as_u64()
                    .or_else(|| (self.expected == 1).then_some(0));
                if candidate["finishReason"].is_string()
                    && let Some(index) = index
                    && index < self.expected as u64
                {
                    self.finished.insert(index);
                }
            }
        }
        self.finished.len() == self.expected
    }
}

pub fn failed(event: &Event) -> bool {
    if event.event == "error" || response_failure(event).is_some() {
        return true;
    }
    serde_json::from_str::<Value>(&event.data)
        .is_ok_and(|v| v.get("error").is_some_and(|error| !error.is_null()) || v["type"] == "error")
}

fn response_failure(event: &Event) -> Option<&'static str> {
    let value = serde_json::from_str::<Value>(&event.data).unwrap_or(Value::Null);
    for kind in [
        event.event.as_str(),
        value.get("type").and_then(Value::as_str).unwrap_or(""),
    ] {
        match kind {
            "response.failed" => return Some("failed"),
            "response.incomplete" => return Some("incomplete"),
            "response.cancelled" | "response.canceled" => return Some("cancelled"),
            _ => (),
        }
    }
    if event.event == "response.completed" || value["type"] == "response.completed" {
        return match value.pointer("/response/status").and_then(Value::as_str) {
            Some("completed") if value.pointer("/response/error").is_none_or(Value::is_null) => {
                None
            }
            Some("incomplete") => Some("incomplete"),
            Some("cancelled" | "canceled") => Some("cancelled"),
            _ => Some("failed"),
        };
    }
    None
}

pub fn terminal(event: &Event, endpoint: &str) -> bool {
    if failed(event) {
        return true;
    }
    match endpoint {
        "/v1/chat/completions" | "/v1/completions" => event.data.trim() == "[DONE]",
        "/v1/messages" => {
            event.event == "message_stop"
                || serde_json::from_str::<Value>(&event.data)
                    .is_ok_and(|v| v["type"] == "message_stop")
        }
        "/v1/responses" => completed_response(event).is_some(),
        "/v1beta/models:streamGenerateContent" => serde_json::from_str::<Value>(&event.data)
            .ok()
            .and_then(|value| value.get("candidates").and_then(Value::as_array).cloned())
            .is_some_and(|candidates| {
                !candidates.is_empty()
                    && candidates.iter().all(|candidate| {
                        candidate
                            .get("finishReason")
                            .and_then(Value::as_str)
                            .is_some()
                    })
            }),
        _ => false,
    }
}

pub fn completed_response(event: &Event) -> Option<Value> {
    if failed(event) {
        return None;
    }
    let value: Value = serde_json::from_str(&event.data).ok()?;
    if value["type"] != "response.completed" && event.event != "response.completed" {
        return None;
    }
    let response = value.get("response")?;
    (response["status"] == "completed").then(|| response.clone())
}
