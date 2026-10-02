use serde_json::Value;
use std::time::Instant;

pub fn visible_text(value: &Value) -> bool {
    fn nonempty(value: &Value) -> bool {
        value.as_str().is_some_and(|text| !text.trim().is_empty())
    }
    if value["choices"].as_array().is_some_and(|choices| {
        choices
            .iter()
            .any(|choice| nonempty(&choice["delta"]["content"]))
    }) {
        return true;
    }
    match value["type"].as_str() {
        Some("response.output_text.delta") => return nonempty(&value["delta"]),
        Some("content_block_delta") if value["delta"]["type"] == "text_delta" => {
            return nonempty(&value["delta"]["text"]);
        }
        Some("content_block_start") if value["content_block"]["type"] == "text" => {
            return nonempty(&value["content_block"]["text"]);
        }
        _ => {}
    }
    value["candidates"].as_array().is_some_and(|candidates| {
        candidates.iter().any(|candidate| {
            candidate["content"]["parts"]
                .as_array()
                .is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|part| part["thought"] != true && nonempty(&part["text"]))
                })
        })
    })
}

/// Capture one phase against the caller's monotonic origin. The explicit
/// observation instant also permits deterministic phase-order tests.
pub fn record_headers_at(origin: Instant, observed: Instant, slot: &mut Option<i64>) {
    slot.get_or_insert(observed.saturating_duration_since(origin).as_millis() as i64);
}

pub fn record_event_at(
    origin: Instant,
    observed: Instant,
    data: &str,
    first_event: &mut Option<i64>,
    first_text: &mut Option<i64>,
) {
    record_headers_at(origin, observed, first_event);
    if first_text.is_none()
        && serde_json::from_str::<Value>(data).is_ok_and(|value| visible_text(&value))
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
            at(15),
            r#"{"choices":[{"delta":{"role":"assistant"}}]}"#,
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(15), None));
        super::record_event_at(
            origin,
            at(25),
            r#"{"choices":[{"delta":{"reasoning_content":"hidden"}}]}"#,
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(15), None));
        super::record_event_at(
            origin,
            at(45),
            r#"{"choices":[{"delta":{"content":"OK"}}]}"#,
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(15), Some(45)));
        super::record_headers_at(origin, at(60), &mut headers);
        super::record_event_at(
            origin,
            at(70),
            r#"{"choices":[{"delta":{"content":"again"}}]}"#,
            &mut event,
            &mut text,
        );
        assert_eq!((headers, event, text), (Some(5), Some(15), Some(45)));
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
            assert!(!visible_text(&value), "unexpected visible text: {value}");
        }
        for value in [
            json!({"choices":[{"delta":{"content":"OK"}}]}),
            json!({"type":"response.output_text.delta","delta":"OK"}),
            json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"OK"}}),
            json!({"type":"content_block_start","content_block":{"type":"text","text":"OK"}}),
            json!({"candidates":[{"content":{"parts":[{"text":"OK"}]}}]}),
        ] {
            assert!(visible_text(&value), "missing visible text: {value}");
        }
    }
}
