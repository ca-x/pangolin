use super::{ensure_fields, invalid, transforms::text_content};
use crate::api::ApiError;
use serde_json::{Value, json};
use std::collections::HashSet;

fn identifier(value: &Value) -> Result<&str, ApiError> {
    value
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("function tools require non-empty string names and ids"))
}

pub(super) fn message(message: &Value) -> Result<Vec<Value>, ApiError> {
    ensure_fields(message, &["role", "content"])?;
    let role = message["role"]
        .as_str()
        .filter(|role| matches!(*role, "user" | "assistant"))
        .ok_or_else(|| invalid("unsupported Anthropic role"))?;
    let content = &message["content"];
    if content.is_string() {
        return Ok(vec![json!({"role":role,"content":content})]);
    }
    let parts = content
        .as_array()
        .ok_or_else(|| invalid("Anthropic content must be text or an array of blocks"))?;
    let mut text = String::new();
    let mut has_text = false;
    let mut calls = vec![];
    let mut ids = HashSet::new();
    let mut output = vec![];
    for part in parts {
        match (role, part["type"].as_str()) {
            (_, Some("text")) => {
                ensure_fields(part, &["type", "text"])?;
                if !calls.is_empty() {
                    return Err(invalid("text after tool_use cannot be represented in chat"));
                }
                text.push_str(
                    part["text"]
                        .as_str()
                        .ok_or_else(|| invalid("text part requires text"))?,
                );
                has_text = true;
            }
            ("assistant", Some("tool_use")) => {
                ensure_fields(part, &["type", "id", "name", "input"])?;
                let id = identifier(&part["id"])?;
                let name = identifier(&part["name"])?;
                if !ids.insert(id) {
                    return Err(invalid("duplicate tool_use ids are unsupported"));
                }
                let input = part
                    .get("input")
                    .filter(|value| value.is_object())
                    .ok_or_else(|| invalid("tool_use input must be a JSON object"))?;
                let arguments = serde_json::to_string(input)
                    .map_err(|_| invalid("tool_use input must be JSON"))?;
                calls.push(json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments}}));
            }
            ("user", Some("tool_result")) => {
                ensure_fields(part, &["type", "tool_use_id", "content", "is_error"])?;
                if has_text {
                    return Err(invalid("tool_result must precede user text"));
                }
                if part.get("is_error").is_some_and(|value| value != false) {
                    return Err(invalid("error tool_result cannot be represented in chat"));
                }
                let id = identifier(&part["tool_use_id"])?;
                let content = part
                    .get("content")
                    .map(text_content)
                    .transpose()?
                    .unwrap_or_default();
                output.push(json!({"role":"tool","tool_call_id":id,"content":content}));
            }
            _ => return Err(invalid("unsupported Anthropic content block or role")),
        }
    }
    if role == "assistant" {
        let mut message = json!({"role":role,"content":if has_text || calls.is_empty(){json!(text)}else{Value::Null}});
        if !calls.is_empty() {
            message["tool_calls"] = json!(calls);
        }
        output.push(message);
    } else if has_text || output.is_empty() {
        output.push(json!({"role":role,"content":text}));
    }
    Ok(output)
}

pub(super) fn tools(body: &Value, output: &mut Value) -> Result<(), ApiError> {
    let mut names = HashSet::new();
    if let Some(tools) = body.get("tools") {
        let tools = tools
            .as_array()
            .ok_or_else(|| invalid("tools must be an array"))?;
        let mut converted = Vec::with_capacity(tools.len());
        for tool in tools {
            ensure_fields(tool, &["name", "description", "input_schema"])?;
            let name = identifier(&tool["name"])?;
            if !names.insert(name) {
                return Err(invalid("duplicate function tool names are unsupported"));
            }
            let schema = tool
                .get("input_schema")
                .filter(|value| value.is_object())
                .ok_or_else(|| invalid("function input_schema must be a JSON object"))?;
            let mut function = json!({"name":name,"parameters":schema});
            if let Some(description) = tool.get("description") {
                if !description.is_string() {
                    return Err(invalid("function description must be text"));
                }
                function["description"] = description.clone();
            }
            converted.push(json!({"type":"function","function":function}));
        }
        output["tools"] = json!(converted);
    }
    if let Some(choice) = body.get("tool_choice") {
        ensure_fields(choice, &["type", "name", "disable_parallel_tool_use"])?;
        let kind = choice["type"]
            .as_str()
            .ok_or_else(|| invalid("tool_choice requires a type"))?;
        if kind != "tool" && choice.get("name").is_some() {
            return Err(invalid("only named tool_choice accepts a name"));
        }
        output["tool_choice"] = match kind {
            "auto" | "none" => json!(kind),
            "any" if !names.is_empty() => json!("required"),
            "tool" => {
                let name = identifier(&choice["name"])?;
                if !names.contains(name) {
                    return Err(invalid("tool_choice must name a declared function"));
                }
                json!({"type":"function","function":{"name":name}})
            }
            _ => return Err(invalid("unsupported tool_choice")),
        };
        if let Some(disabled) = choice.get("disable_parallel_tool_use") {
            output["parallel_tool_calls"] = json!(
                !disabled
                    .as_bool()
                    .ok_or_else(|| invalid("disable_parallel_tool_use must be a boolean"))?
            );
        }
    }
    Ok(())
}

pub(super) fn chat_response(model: &str, body: &Value) -> Result<Value, ApiError> {
    let choices = body["choices"]
        .as_array()
        .ok_or_else(|| invalid("chat response missing choices"))?;
    if choices.len() != 1 {
        return Err(invalid(
            "Anthropic transformation requires exactly one choice",
        ));
    }
    let choice = &choices[0];
    let message = &choice["message"];
    ensure_fields(
        message,
        &["role", "content", "tool_calls", "refusal", "annotations"],
    )?;
    if message
        .get("refusal")
        .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
    {
        return Err(invalid("unsupported chat refusal metadata"));
    }
    if message
        .get("annotations")
        .is_some_and(|value| !matches!(value, Value::Array(annotations) if annotations.is_empty()))
    {
        return Err(invalid("unsupported chat annotations metadata"));
    }
    if message["role"] != "assistant" {
        return Err(invalid("chat response requires an assistant message"));
    }
    let calls = match message.get("tool_calls") {
        None | Some(Value::Null) => &[][..],
        Some(calls) => calls
            .as_array()
            .ok_or_else(|| invalid("tool_calls must be an array"))?
            .as_slice(),
    };
    let mut content = vec![];
    match message.get("content") {
        Some(Value::String(text)) => content.push(json!({"type":"text","text":text})),
        None | Some(Value::Null) if !calls.is_empty() => {}
        _ => return Err(invalid("unsupported non-text chat response")),
    }
    let mut ids = HashSet::new();
    for call in calls {
        ensure_fields(call, &["id", "type", "function"])?;
        if call["type"] != "function" {
            return Err(invalid("only function tool calls are supported"));
        }
        let id = identifier(&call["id"])?;
        if !ids.insert(id) {
            return Err(invalid("duplicate tool call ids are unsupported"));
        }
        let function = &call["function"];
        ensure_fields(function, &["name", "arguments"])?;
        let name = identifier(&function["name"])?;
        let arguments = function["arguments"]
            .as_str()
            .ok_or_else(|| invalid("function arguments must be JSON text"))?;
        let input: Value = serde_json::from_str(arguments)
            .map_err(|_| invalid("function arguments must be a valid JSON object"))?;
        if !input.is_object() {
            return Err(invalid("function arguments must be a JSON object"));
        }
        content.push(json!({"type":"tool_use","id":id,"name":name,"input":input}));
    }
    let stop_reason = match choice["finish_reason"].as_str() {
        Some("tool_calls") if !calls.is_empty() => "tool_use",
        Some("stop") if calls.is_empty() => "end_turn",
        Some("length") => "max_tokens",
        _ => return Err(invalid("unsupported or inconsistent chat finish reason")),
    };
    Ok(
        json!({"id":body["id"],"type":"message","role":"assistant","model":model,"content":content,"stop_reason":stop_reason,"stop_sequence":null,"usage":{"input_tokens":body["usage"]["prompt_tokens"],"output_tokens":body["usage"]["completion_tokens"]}}),
    )
}
