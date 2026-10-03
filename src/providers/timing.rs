use serde_json::Value;
use std::time::Instant;

pub fn visible_text(endpoint: &str, value: &Value) -> bool {
    fn nonempty(value: &Value) -> bool {
        value.as_str().is_some_and(|text| !text.trim().is_empty())
    }
    match crate::providers::capability(endpoint) {
        "chat" => value["choices"].as_array().is_some_and(|choices| {
            choices
                .iter()
                .any(|choice| nonempty(&choice["delta"]["content"]))
        }),
        "completions" => value["choices"]
            .as_array()
            .is_some_and(|choices| choices.iter().any(|choice| nonempty(&choice["text"]))),
        "responses" => value["type"] == "response.output_text.delta" && nonempty(&value["delta"]),
        "messages" => match value["type"].as_str() {
            Some("content_block_delta") if value["delta"]["type"] == "text_delta" => {
                nonempty(&value["delta"]["text"])
            }
            Some("content_block_start") if value["content_block"]["type"] == "text" => {
                nonempty(&value["content_block"]["text"])
            }
            _ => false,
        },
        "gemini" => value["candidates"].as_array().is_some_and(|candidates| {
            candidates.iter().any(|candidate| {
                candidate["content"]["parts"]
                    .as_array()
                    .is_some_and(|parts| {
                        parts
                            .iter()
                            .any(|part| part["thought"] != true && nonempty(&part["text"]))
                    })
            })
        }),
        _ => false,
    }
}

/// The production clock is monotonic. Tests can offset observations per task
/// while a composed upstream stream is held at independently released phases.
pub fn capture_now() -> Instant {
    let now = Instant::now();
    #[cfg(test)]
    {
        TEST_OBSERVER
            .try_with(|observer| {
                now + std::time::Duration::from_millis(
                    observer.offset.load(std::sync::atomic::Ordering::Relaxed),
                )
            })
            .unwrap_or(now)
    }
    #[cfg(not(test))]
    {
        now
    }
}

#[cfg(test)]
struct TestObserver {
    offset: std::sync::Arc<std::sync::atomic::AtomicU64>,
    sender: tokio::sync::mpsc::UnboundedSender<(Option<i64>, Option<i64>, Option<i64>)>,
}

#[cfg(test)]
tokio::task_local! {
    static TEST_OBSERVER: TestObserver;
}

#[cfg(test)]
pub async fn with_test_observer<T>(
    offset: std::sync::Arc<std::sync::atomic::AtomicU64>,
    sender: tokio::sync::mpsc::UnboundedSender<(Option<i64>, Option<i64>, Option<i64>)>,
    future: impl std::future::Future<Output = T>,
) -> T {
    TEST_OBSERVER
        .scope(TestObserver { offset, sender }, future)
        .await
}

#[cfg(test)]
pub fn observe_for_test(headers: Option<i64>, event: Option<i64>, text: Option<i64>) {
    let _ = TEST_OBSERVER.try_with(|observer| observer.sender.send((headers, event, text)));
}

/// Capture one phase against the caller's monotonic origin. The explicit
/// observation instant also permits deterministic phase-order tests.
pub fn record_headers_at(origin: Instant, observed: Instant, slot: &mut Option<i64>) {
    slot.get_or_insert(observed.saturating_duration_since(origin).as_millis() as i64);
}

pub fn record_event_at(
    origin: Instant,
    observed: Instant,
    endpoint: &str,
    data: &str,
    first_event: &mut Option<i64>,
    first_text: &mut Option<i64>,
) {
    record_headers_at(origin, observed, first_event);
    if first_text.is_none()
        && serde_json::from_str::<Value>(data).is_ok_and(|value| visible_text(endpoint, &value))
    {
        record_headers_at(origin, observed, first_text);
    }
}

#[cfg(test)]
mod tests {
    use super::visible_text;
    use serde_json::json;

    #[test]
    fn reference_timing_controlled_phases_capture_once() {
        use std::time::{Duration, Instant};
        let origin = Instant::now();
        let at = |milliseconds| origin + Duration::from_millis(milliseconds);
        let mut headers = None;
        let mut event = None;
        let mut text = None;
        assert_eq!((headers, event, text), (None, None, None));
        super::record_headers_at(origin, at(5), &mut headers);
        assert_eq!((headers, event, text), (Some(5), None, None));
        super::record_event_at(
            origin,
            at(10),
            "/v1/chat/completions",
            r#"{"choices":[],"object":"chat.completion.chunk"}"#,
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(10), None));
        super::record_event_at(
            origin,
            at(12),
            "/v1/chat/completions",
            ": heartbeat",
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(10), None));
        super::record_event_at(
            origin,
            at(15),
            "/v1/chat/completions",
            r#"{"choices":[{"delta":{"role":"assistant"}}]}"#,
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(10), None));
        super::record_event_at(
            origin,
            at(25),
            "/v1/chat/completions",
            r#"{"choices":[{"delta":{"reasoning_content":"hidden"}}]}"#,
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(10), None));
        super::record_event_at(
            origin,
            at(45),
            "/v1/chat/completions",
            r#"{"choices":[{"delta":{"content":"OK"}}]}"#,
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(10), Some(45)));
        super::record_headers_at(origin, at(60), &mut headers);
        super::record_event_at(
            origin,
            at(70),
            "/v1/chat/completions",
            r#"{"choices":[{"delta":{"content":"again"}}]}"#,
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(10), Some(45)));
        super::record_event_at(
            origin,
            at(80),
            "/v1/chat/completions",
            "[DONE]",
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(10), Some(45)));
    }

    #[test]
    fn reference_timing_visible_text_excludes_protocol_noise() {
        for value in [
            json!({"type":"response.created"}),
            json!({"choices":[{"delta":{"role":"assistant"}}]}),
            json!({"choices":[{"delta":{"reasoning_content":"think"}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"id":"call"}]}}]}),
            json!({"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"think"}}),
            json!({"candidates":[{"content":{"parts":[{"thought":true,"text":"think"}]}}]}),
        ] {
            for endpoint in [
                "/v1/chat/completions",
                "/v1/responses",
                "/v1/messages",
                "/v1beta/models:streamGenerateContent",
            ] {
                assert!(
                    !visible_text(endpoint, &value),
                    "unexpected visible text: {value}"
                );
            }
        }
        for value in [
            json!({"choices":[{"delta":{"content":"OK"}}]}),
            json!({"type":"response.output_text.delta","delta":"OK"}),
            json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"OK"}}),
            json!({"type":"content_block_start","content_block":{"type":"text","text":"OK"}}),
            json!({"candidates":[{"content":{"parts":[{"text":"OK"}]}}]}),
        ] {
            let endpoint = if value["choices"].is_array() {
                "/v1/chat/completions"
            } else if value["type"] == "response.output_text.delta" {
                "/v1/responses"
            } else if value["type"].is_string() {
                "/v1/messages"
            } else {
                "/v1beta/models:streamGenerateContent"
            };
            assert!(
                visible_text(endpoint, &value),
                "missing visible text: {value}"
            );
        }
    }
}
