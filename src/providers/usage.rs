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
        ResponseTransform::Identity => Usage::parse_for(value, endpoint == "/v1/messages"),
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
        // Gemini source totals include thoughts/cache; OpenAI source completion
        // counts already include reasoning. The parser distinguishes native keys.
        ResponseTransform::GeminiChat(_)
        | ResponseTransform::GeminiEmbedding
        | ResponseTransform::ChatGemini
        | ResponseTransform::ChatMessages(_) => Usage::parse_for(value, false),
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
}
