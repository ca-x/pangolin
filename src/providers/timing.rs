use serde_json::Value;

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

#[cfg(test)]
mod tests {
    use super::visible_text;
    use serde_json::json;

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
