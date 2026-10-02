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
