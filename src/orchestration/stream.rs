//! SSE framing uses a maintained parser. A complete first event is read before
//! downstream commitment; terminal detection is protocol-aware.
use axum::body::Bytes;
use eventsource_stream::{Event, Eventsource};
use futures_util::{Stream, StreamExt};
use serde_json::Value;
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
    Box::pin(bytes.eventsource().map(move |event| {
        buffered.store(0, Ordering::Relaxed);
        event.map_err(|_| std::io::Error::other("invalid upstream SSE event"))
    }))
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

pub fn failed(event: &Event) -> bool {
    if event.event == "error"
        || event.event == "response.failed"
        || event.event == "response.incomplete"
    {
        return true;
    }
    serde_json::from_str::<Value>(&event.data).is_ok_and(|v| {
        v.get("error").is_some()
            || matches!(
                v.get("type").and_then(Value::as_str),
                Some("error" | "response.failed" | "response.incomplete")
            )
    })
}

pub fn terminal(event: &Event, endpoint: &str) -> bool {
    match endpoint {
        "/v1/chat/completions" => event.data.trim() == "[DONE]",
        "/v1/messages" => {
            event.event == "message_stop"
                || serde_json::from_str::<Value>(&event.data)
                    .is_ok_and(|v| v["type"] == "message_stop")
        }
        "/v1/responses" => completed_response(event).is_some(),
        _ => false,
    }
}

pub fn completed_response(event: &Event) -> Option<Value> {
    let value: Value = serde_json::from_str(&event.data).ok()?;
    if value["type"] != "response.completed" && event.event != "response.completed" {
        return None;
    }
    let response = value.get("response")?;
    (response["status"] == "completed").then(|| response.clone())
}
