use super::{ensure_fields, invalid};
use crate::api::ApiError;
use serde_json::{Value, json};

pub(super) fn request(payload: &Value) -> Result<Value, ApiError> {
    ensure_fields(
        payload,
        &[
            "model",
            "messages",
            "max_tokens",
            "max_completion_tokens",
            "max_output_tokens",
            "temperature",
            "top_p",
            "stop",
            "stream",
            "n",
        ],
    )?;
    if payload["stream"] == true || payload.get("n").is_some_and(|value| value != 1) {
        return Err(invalid(
            "Bedrock text conversion requires one non-streaming completion",
        ));
    }
    let source = payload["messages"]
        .as_array()
        .filter(|messages| !messages.is_empty())
        .ok_or_else(|| invalid("messages must be a nonempty array"))?;
    let mut system = Vec::new();
    let mut messages = Vec::new();
    for message in source {
        ensure_fields(message, &["role", "content"])?;
        let content = message["content"]
            .as_str()
            .ok_or_else(|| invalid("Bedrock conversion supports text messages only"))?;
        match message["role"].as_str() {
            Some("system") => system.push(json!({"text": content})),
            Some(role @ ("user" | "assistant")) => messages.push(json!({
                "role": role, "content": [{"text": content}],
            })),
            _ => return Err(invalid("unsupported Bedrock message role")),
        }
    }
    if messages.is_empty() {
        return Err(invalid("Bedrock requires a conversational message"));
    }
    let mut inference = json!({"maxTokens": crate::orchestration::output_limit(payload)?});
    for (source, destination) in [("temperature", "temperature"), ("top_p", "topP")] {
        if let Some(value) = payload.get(source) {
            if !value.is_number() {
                return Err(invalid("invalid Bedrock sampling parameter"));
            }
            inference[destination] = value.clone();
        }
    }
    if let Some(stop) = payload.get("stop") {
        inference["stopSequences"] = match stop {
            Value::String(text) => json!([text]),
            Value::Array(values) if values.iter().all(Value::is_string) => stop.clone(),
            _ => return Err(invalid("stop must be text or an array of text")),
        };
    }
    let mut request = json!({"messages": messages, "inferenceConfig": inference});
    if !system.is_empty() {
        request["system"] = json!(system);
    }
    Ok(request)
}

pub(super) fn response(model: &str, body: Value) -> Result<Value, ApiError> {
    let blocks = body
        .pointer("/output/message/content")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::Upstream("unsupported Bedrock response shape".into()))?;
    let mut text = String::new();
    for block in blocks {
        if block.as_object().is_none_or(|value| value.len() != 1) {
            return Err(ApiError::Upstream(
                "unsupported Bedrock response shape".into(),
            ));
        }
        text.push_str(
            block["text"]
                .as_str()
                .ok_or_else(|| ApiError::Upstream("unsupported Bedrock response shape".into()))?,
        );
    }
    let reason = match body["stopReason"].as_str() {
        Some("end_turn" | "stop_sequence") => "stop",
        Some("max_tokens") => "length",
        Some("content_filtered" | "guardrail_intervened") => "content_filter",
        _ => return Err(ApiError::Upstream("unsupported Bedrock stop reason".into())),
    };
    let mut usage = json!({});
    let measured = super::usage::response_usage(
        &super::ResponseTransform::Bedrock(model.to_owned()),
        &body,
        "/v1/chat/completions",
    );
    if measured.reported && !measured.invalid {
        for (present, count, destination) in [
            (measured.presence.input, measured.input, "prompt_tokens"),
            (
                measured.presence.output,
                measured.output,
                "completion_tokens",
            ),
        ] {
            if present {
                usage[destination] = json!(count);
            }
        }
        if measured.presence.input
            && measured.presence.output
            && let Some(total) = measured.input.checked_add(measured.output)
        {
            usage["total_tokens"] = json!(total);
        }
        if measured.presence.cache_read {
            usage["prompt_tokens_details"]["cached_tokens"] = json!(measured.cache_read);
        }
        if measured.presence.cache_write {
            usage["prompt_tokens_details"]["cache_write_tokens"] = json!(measured.cache_write);
        }
    }
    Ok(json!({
        "id": format!("chatcmpl_{}", uuid::Uuid::new_v4().simple()),
        "object": "chat.completion", "created": crate::db::now(), "model": model,
        "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": reason}],
        "usage": usage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_usage_preserves_gross_input_and_missing_measurements() {
        let body =
            json!({"output":{"message":{"content":[{"text":"ok"}]}},"stopReason":"end_turn"});
        let mut complete = body.clone();
        complete["usage"] = json!({"inputTokens":10,"outputTokens":5,"cacheReadInputTokens":2,"cacheWriteInputTokens":3,"totalTokens":15});
        let normalized = response("model", complete).unwrap();
        assert_eq!(normalized["usage"]["prompt_tokens"], 15);
        assert_eq!(normalized["usage"]["total_tokens"], 20);
        assert_eq!(
            normalized["usage"]["prompt_tokens_details"]["cached_tokens"],
            2
        );
        assert_eq!(
            normalized["usage"]["prompt_tokens_details"]["cache_write_tokens"],
            3
        );
        let mut partial = body.clone();
        partial["usage"] = json!({"inputTokens":10,"outputTokens":0,"totalTokens":10});
        let normalized = response("model", partial).unwrap();
        assert!(normalized["usage"].get("prompt_tokens").is_none());
        assert!(normalized["usage"].get("total_tokens").is_none());
        assert_eq!(normalized["usage"]["completion_tokens"], 0);
        assert_eq!(response("model", body).unwrap()["usage"], json!({}));
    }

    #[test]
    fn unsupported_request_and_response_shapes_are_rejected() {
        assert!(
            request(&json!({"messages":[{"role":"user","content":"hi"}],"tools":[{}]})).is_err()
        );
        assert!(
            request(&json!({"messages":[{"role":"user","content":[{"type":"image_url"}]}]}))
                .is_err()
        );
        assert!(
            response(
                "model",
                json!({"output":{"message":{"content":[{"toolUse":{}}]}},"stopReason":"tool_use"})
            )
            .is_err()
        );
    }
}
