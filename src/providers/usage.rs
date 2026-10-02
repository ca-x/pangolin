//! Protocol evidence that an output usage report is final, shared by probes and accounting.
use serde_json::Value;

pub fn final_report(endpoint: &str, value: &Value, terminal: bool) -> bool {
    match endpoint {
        "/v1/messages" => {
            value["type"] == "message_delta"
                && value
                    .pointer("/delta/stop_reason")
                    .is_some_and(Value::is_string)
                && value
                    .pointer("/usage/output_tokens")
                    .and_then(Value::as_i64)
                    .is_some_and(|n| n >= 0)
        }
        "/v1/chat/completions" | "/v1/completions" => {
            let completion_tokens = value
                .pointer("/usage/completion_tokens")
                .and_then(Value::as_i64)
                .is_some_and(|n| n >= 0);
            let choices = value["choices"].as_array();
            // The conventional aggregate: usage arrives with no choices at all.
            let aggregate = choices.is_some_and(Vec::is_empty);
            // Compatible upstreams may instead attach the same cumulative usage to
            // the chunk that closes every choice. That chunk is this protocol's
            // in-band terminal — every choice carries a non-null `finish_reason` —
            // so its report is complete even though `[DONE]` is still to come. A
            // content delta, which has no `finish_reason`, is never final.
            let finished = choices.is_some_and(|choices| {
                !choices.is_empty()
                    && choices.iter().all(|choice| {
                        choice
                            .get("finish_reason")
                            .and_then(Value::as_str)
                            .is_some_and(|reason| !reason.is_empty())
                    })
            });
            completion_tokens && (aggregate || finished)
        }
        "/v1/responses" => {
            terminal
                && value
                    .pointer("/response/usage/output_tokens")
                    .and_then(Value::as_i64)
                    .is_some_and(|n| n >= 0)
        }
        "/v1beta/models:streamGenerateContent" => {
            terminal
                && value.get("error").is_none()
                && [
                    "/usageMetadata/totalTokenCount",
                    "/usageMetadata/candidatesTokenCount",
                ]
                .iter()
                .any(|pointer| {
                    value
                        .pointer(pointer)
                        .and_then(Value::as_i64)
                        .is_some_and(|n| n >= 0)
                })
        }
        _ => false,
    }
}

// These are the counter fields already understood by the checked Usage parser.
// Keep OpenAI-compatible snake_case aliases, but never admit another protocol's
// camelCase counters or search a different response container for missing data.
const OPENAI_COUNTERS: &[&str] = &[
    "input_tokens",
    "prompt_tokens",
    "output_tokens",
    "completion_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
    "prompt_cache_hit_tokens",
    "request_units",
    "input_tokens_details/cached_tokens",
    "prompt_tokens_details/cached_tokens",
    "input_tokens_details/cache_write_tokens",
    "prompt_tokens_details/cache_write_tokens",
    "prompt_tokens_details/cache_creation_tokens",
    "cache_creation/ephemeral_1h_input_tokens",
    "output_tokens_details/reasoning_tokens",
    "completion_tokens_details/reasoning_tokens",
    "input_tokens_details/image_tokens",
    "prompt_tokens_details/image_tokens",
    "output_tokens_details/image_tokens",
    "completion_tokens_details/image_tokens",
];
const GEMINI_COUNTERS: &[&str] = &[
    "promptTokenCount",
    "candidatesTokenCount",
    "totalTokenCount",
    "cachedContentTokenCount",
    "thoughtsTokenCount",
];
const ANTHROPIC_COUNTERS: &[&str] = &[
    "input_tokens",
    "output_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
    "cache_creation/ephemeral_1h_input_tokens",
];

fn projected_report(counters: Option<&Value>, fields: &[&str]) -> Value {
    let mut projected = serde_json::Map::new();
    if let Some(counters) = counters.filter(|value| value.is_object()) {
        for field in fields {
            if let Some(value) = counters.pointer(&format!("/{field}")) {
                if let Some((object, name)) = field.split_once('/') {
                    projected
                        .entry(object.to_owned())
                        .or_insert_with(|| serde_json::json!({}))[name] = value.clone();
                } else {
                    projected.insert((*field).into(), value.clone());
                }
            }
        }
    }
    serde_json::json!({"usage":projected})
}

/// Metrics-only projection from a known wire protocol. Keep the original event
/// for framing, terminal decisions, response identifiers and body capture.
/// Anthropic's delta type survives so its existing checked split-usage merge
/// still distinguishes cumulative output from corrected input/cache components.
pub fn protocol_report(endpoint: &str, value: &Value, streamed: bool) -> Value {
    match endpoint {
        "/v1beta/models:generateContent" | "/v1beta/models:streamGenerateContent" => {
            projected_report(value.get("usageMetadata"), GEMINI_COUNTERS)
        }
        "/v1/messages" => {
            let counters = if streamed && value["type"] == "message_start" {
                value.pointer("/message/usage")
            } else {
                value.get("usage")
            };
            let mut report = projected_report(counters, ANTHROPIC_COUNTERS);
            if streamed
                && let Some(kind @ ("message_start" | "message_delta")) = value["type"].as_str()
            {
                report["type"] = Value::String(kind.into());
            }
            report
        }
        "/v1/responses" if streamed => {
            projected_report(value.pointer("/response/usage"), OPENAI_COUNTERS)
        }
        _ => projected_report(value.get("usage"), OPENAI_COUNTERS),
    }
}

pub fn protocol_usage(
    endpoint: &str,
    value: &Value,
    streamed: bool,
) -> crate::operations::pricing::Usage {
    crate::operations::pricing::Usage::parse_for(
        &protocol_report(endpoint, value, streamed),
        endpoint == "/v1/messages",
    )
}

/// Read quantities from the actual upstream protocol, before a compatibility
/// transform fills absent counters or changes their meaning. The existing Usage
/// parser retains validation, presence and checked cache/reasoning arithmetic.
pub fn response_usage(
    transform: &super::ResponseTransform,
    value: &Value,
    endpoint: &str,
) -> crate::operations::pricing::Usage {
    use super::ResponseTransform;
    use crate::operations::pricing::Usage;
    match transform {
        ResponseTransform::Identity => protocol_usage(endpoint, value, false),
        ResponseTransform::Antigravity(inner) => value
            .get("response")
            .map(|value| response_usage(inner, value, endpoint))
            .unwrap_or_default(),
        ResponseTransform::Bedrock(_) => {
            // Converse's gross input is input + cache read + cache write, matching
            // the pinned adapter. Preserve missing/invalid native fields instead
            // of that adapter's display-only zero defaults.
            let mut counters = serde_json::Map::new();
            for (source, destination) in [
                ("inputTokens", "input_tokens"),
                ("outputTokens", "output_tokens"),
                ("cacheReadInputTokens", "cache_read_input_tokens"),
                ("cacheWriteInputTokens", "cache_creation_input_tokens"),
            ] {
                if let Some(value) = value["usage"].get(source) {
                    counters.insert(destination.into(), value.clone());
                }
            }
            Usage::parse_for(&serde_json::json!({"usage":counters}), true)
        }
        ResponseTransform::GeminiChat(_) | ResponseTransform::GeminiEmbedding => Usage::parse_for(
            &projected_report(value.get("usageMetadata"), GEMINI_COUNTERS),
            false,
        ),
        ResponseTransform::ChatGemini | ResponseTransform::ChatMessages(_) => Usage::parse_for(
            &projected_report(value.get("usage"), OPENAI_COUNTERS),
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ResponseTransform;
    use serde_json::json;

    #[test]
    fn reference_gateway_source_usage_wrapped_gemini_preserves_counter_trust() {
        let transform =
            ResponseTransform::Antigravity(Box::new(ResponseTransform::GeminiChat("model".into())));
        let missing = response_usage(&transform, &json!({"response":{}}), "/v1/chat/completions");
        assert!(!missing.reported && !missing.presence.output);
        let valid = response_usage(
            &transform,
            &json!({"response":{"usageMetadata":{"promptTokenCount":3,"totalTokenCount":10,"candidatesTokenCount":2,"thoughtsTokenCount":5}}}),
            "/v1/chat/completions",
        );
        assert!(valid.reported && valid.presence.output);
        assert_eq!((valid.input, valid.output, valid.reasoning), (3, 7, 5));
        let malformed = response_usage(
            &transform,
            &json!({"response":{"usageMetadata":{"promptTokenCount":3,"totalTokenCount":"10"}}}),
            "/v1/chat/completions",
        );
        assert!(!malformed.reported && malformed.invalid);
    }

    #[test]
    fn reference_gateway_source_usage_bedrock_missing_cache_parts_do_not_invent_input() {
        let transform = ResponseTransform::Bedrock("model".into());
        let usage = response_usage(
            &transform,
            &json!({"usage":{"inputTokens":3,"outputTokens":0}}),
            "/v1/chat/completions",
        );
        assert!(usage.reported && usage.presence.output);
        assert!(!usage.presence.input);
        assert_eq!(usage.output, 0);
        let malformed = response_usage(
            &transform,
            &json!({"usage":{"inputTokens":3,"outputTokens":-1}}),
            "/v1/chat/completions",
        );
        assert!(!malformed.reported && malformed.invalid);
    }
    #[test]
    fn reference_source_envelope_gemini_ignores_foreign_containers_and_counters() {
        let transform = ResponseTransform::GeminiChat("model".into());
        for output in [Some(7), None, Some(0)] {
            let mut raw = json!({"usage":{"prompt_tokens":0,"completion_tokens":9}});
            if let Some(output) = output {
                raw["usageMetadata"] = json!({"promptTokenCount":3,"totalTokenCount":3+output,"candidatesTokenCount":0,"thoughtsTokenCount":output,"input_tokens":0,"output_tokens":99});
            }
            let measured = response_usage(&transform, &raw, "/v1/chat/completions");
            assert_eq!(measured.presence.output, output.is_some());
            assert_eq!(measured.output, output.unwrap_or(0));
            assert_eq!(measured.reported, output.is_some());
            if output.is_some() {
                assert_eq!(measured.input, 3);
            }
        }
        let foreign_fields = response_usage(
            &transform,
            &json!({"usageMetadata":{"prompt_tokens":3,"completion_tokens":7}}),
            "/v1/chat/completions",
        );
        assert!(!foreign_fields.reported && !foreign_fields.presence.output);
    }

    #[test]
    fn reference_source_envelope_openai_ignores_foreign_containers_and_counters() {
        let transform = ResponseTransform::ChatMessages("model".into());
        for output in [Some(7), None, Some(0)] {
            let mut raw = json!({"usageMetadata":{"promptTokenCount":3,"totalTokenCount":10}});
            if let Some(output) = output {
                raw["usage"] = json!({"prompt_tokens":3,"completion_tokens":output,"promptTokenCount":0,"totalTokenCount":99,"thoughtsTokenCount":99});
            }
            let measured = response_usage(&transform, &raw, "/v1/messages");
            assert_eq!(measured.presence.output, output.is_some());
            assert_eq!(measured.output, output.unwrap_or(0));
            assert_eq!(measured.reported, output.is_some());
            if output.is_some() {
                assert_eq!(measured.input, 3);
                assert_eq!(measured.reasoning, 0);
            }
        }
        let foreign_fields = response_usage(
            &transform,
            &json!({"usage":{"promptTokenCount":3,"totalTokenCount":10}}),
            "/v1/messages",
        );
        assert!(!foreign_fields.reported && !foreign_fields.presence.output);
    }

    #[test]
    fn reference_source_envelope_openai_compatibility_fields_remain_measured() {
        let measured = response_usage(
            &ResponseTransform::ChatGemini,
            &json!({"usage":{
                "input_tokens":10,"output_tokens":7,"request_units":1,
                "input_tokens_details":{"cached_tokens":2,"cache_write_tokens":1,"image_tokens":3},
                "output_tokens_details":{"reasoning_tokens":4,"image_tokens":1}
            }}),
            "/v1beta/models:generateContent",
        );
        assert!(measured.reported && measured.presence.input && measured.presence.output);
        assert_eq!(
            (
                measured.input,
                measured.output,
                measured.cache_read,
                measured.cache_write,
                measured.reasoning,
                measured.units
            ),
            (10, 7, 2, 1, 4, 1)
        );
        assert_eq!((measured.image_input, measured.image_output), (3, 1));
        let compatibility = response_usage(
            &ResponseTransform::ChatMessages("model".into()),
            &json!({"usage":{"prompt_tokens":10,"completion_tokens":7,"prompt_cache_hit_tokens":2,"prompt_tokens_details":{"cache_creation_tokens":1},"completion_tokens_details":{"reasoning_tokens":4}}}),
            "/v1/messages",
        );
        assert!(
            compatibility.reported && compatibility.presence.input && compatibility.presence.output
        );
        assert_eq!(
            (
                compatibility.input,
                compatibility.output,
                compatibility.cache_read,
                compatibility.cache_write,
                compatibility.reasoning
            ),
            (10, 7, 2, 1, 4)
        );
    }
}
