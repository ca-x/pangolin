//! Transient protocol-aware content traversal. Replay exceptions describe native
//! protocol structure, never verification of an upstream cryptographic signature.
use serde_json::Value;

use super::{Error, Result};

pub(super) const MAX_DEPTH: usize = 32;
const MAX_SEGMENTS: usize = 4096;

pub(super) fn validate_shape(value: &Value, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::Invalid("privacy inspection depth exceeds 32"));
    }
    match value {
        Value::Array(items) => {
            for item in items {
                validate_shape(item, depth + 1)?;
            }
        }
        Value::Object(object) => {
            for value in object.values() {
                validate_shape(value, depth + 1)?;
            }
        }
        _ => (),
    }
    Ok(())
}

pub(super) fn visit(
    body: &mut Value,
    endpoint: &str,
    visitor: &mut impl FnMut(&str, &str, &mut String) -> Result<()>,
) -> Result<()> {
    validate_shape(body, 0)?;
    let mut walker = Walker {
        endpoint,
        visitor,
        segments: 0,
    };
    for field in ["messages", "input", "contents"] {
        if let Some(items) = body.get_mut(field) {
            let path = child("", field)?;
            if let Some(items) = items.as_array_mut() {
                for (index, item) in items.iter_mut().enumerate() {
                    walker.message(item, &child(&path, &index.to_string())?)?;
                }
            } else {
                walker.content(items, "user", &path)?;
            }
        }
    }
    for field in ["system", "instructions", "systemInstruction"] {
        if let Some(value) = body.get_mut(field) {
            walker.content(value, "system", &child("", field)?)?;
        }
    }
    for field in ["prompt", "query", "documents", "content", "suffix"] {
        if let Some(value) = body.get_mut(field) {
            walker.content(value, "user", &child("", field)?)?;
        }
    }
    Ok(())
}

fn child(path: &str, key: &str) -> Result<String> {
    // Account for JSON Pointer escaping before allocating either the key or its
    // accumulated path. This also covers numeric-only tool children, which do
    // not consume a text segment, and the decoded leaves of serialized JSON.
    let escaped_bytes = key.len()
        + key
            .bytes()
            .filter(|byte| matches!(byte, b'~' | b'/'))
            .count();
    if path.len().saturating_add(1).saturating_add(escaped_bytes) > 4096 {
        return Err(Error::Invalid("privacy inspection path exceeds 4096 bytes"));
    }
    Ok(format!(
        "{path}/{}",
        key.replace('~', "~0").replace('/', "~1")
    ))
}

struct Walker<'a, F> {
    endpoint: &'a str,
    visitor: &'a mut F,
    segments: usize,
}
impl<F: FnMut(&str, &str, &mut String) -> Result<()>> Walker<'_, F> {
    fn text(&mut self, value: &mut String, role: &str, path: &str) -> Result<()> {
        self.segments += 1;
        if self.segments > MAX_SEGMENTS {
            return Err(Error::Invalid("privacy inspection segments exceed 4096"));
        }
        (self.visitor)(role, path, value)
    }

    fn tool(&mut self, value: &mut Value, role: &str, path: &str) -> Result<()> {
        match value {
            Value::String(text) => {
                // A valid serialized payload is rewritten through its string
                // leaves, preserving keys and scalar types. Non-JSON transcripts
                // remain ordinary inspectable text. Keep original bytes on a miss.
                if let Some(mut document) = parse_tool_json(text, path.matches('/').count())? {
                    validate_shape(&document, path.matches('/').count())?;
                    let original = document.clone();
                    self.tool(&mut document, role, &child(path, "$json")?)?;
                    if document != original {
                        *text = document.to_string();
                    }
                    Ok(())
                } else {
                    self.text(text, role, path)
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter_mut().enumerate() {
                    self.tool(value, role, &child(path, &index.to_string())?)?;
                }
                Ok(())
            }
            Value::Object(object) => {
                for (key, value) in object.iter_mut() {
                    self.tool(value, role, &child(path, key)?)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn field(
        &mut self,
        value: &mut Value,
        key: &str,
        role: &str,
        path: &str,
        tool: bool,
    ) -> Result<()> {
        if let Some(value) = value.get_mut(key) {
            if tool {
                self.tool(value, role, &child(path, key)?)?;
            } else {
                self.content(value, role, &child(path, key)?)?;
            }
        }
        Ok(())
    }

    fn message(&mut self, value: &mut Value, path: &str) -> Result<()> {
        if !value.is_object() {
            return self.content(value, "user", path);
        }
        let kind = value["type"].as_str().unwrap_or("").to_owned();
        let role = value["role"].as_str().unwrap_or("user").to_owned();
        let responses = self.endpoint.starts_with("/v1/responses") && path.starts_with("/input/");
        let replay_role = responses && (value.get("role").is_none() || role == "assistant");
        // Preserve the historical semantic tool-output coverage even on mixed
        // protocol payloads. Opaque replay exemptions still require Responses.
        if matches!(
            kind.as_str(),
            "function_call_output" | "custom_tool_call_output"
        ) {
            if let Some(output) = value.get_mut("output") {
                if let Some(items) = output.as_array_mut() {
                    for (index, item) in items.iter_mut().enumerate() {
                        let item_path = child(&child(path, "output")?, &index.to_string())?;
                        if replay_role
                            && item["type"] == "encrypted_content"
                            && item["encrypted_content"].is_string()
                        {
                            continue;
                        }
                        self.tool_result(item, &item_path)?;
                    }
                } else {
                    self.tool_result(output, &child(path, "output")?)?;
                }
            }
            return Ok(());
        }
        if responses {
            if replay_role
                && matches!(
                    kind.as_str(),
                    "reasoning" | "compaction" | "compaction_summary"
                )
            {
                validate_encrypted_carrier(value)?;
                for (key, value) in value.as_object_mut().expect("object").iter_mut() {
                    if key != "encrypted_content" && !protocol_key(key) {
                        self.content(value, "assistant", &child(path, key)?)?;
                    }
                }
                return Ok(());
            }
            match kind.as_str() {
                "function_call" | "custom_tool_call" => {
                    self.field(value, "arguments", "assistant", path, true)?;
                    self.field(value, "input", "assistant", path, true)?;
                    return Ok(());
                }
                _ => (),
            }
        }
        // Genuine carriers are only exempt in assistant records at their native
        // array locations. Named properties in ordinary content remain content.
        if role == "tool" {
            if let Some(content) = value.get_mut("content") {
                self.tool_result(content, &child(path, "content")?)?;
            }
        } else {
            self.field(value, "content", &role, path, false)?;
        }
        self.field(
            value,
            "parts",
            if role == "model" { "assistant" } else { &role },
            path,
            false,
        )?;
        for key in ["text", "thinking", "signature", "encrypted_content", "data"] {
            if value.get(key).is_some() {
                self.field(value, key, &role, path, false)?;
            }
        }
        if let Some(calls) = value.get_mut("tool_calls").and_then(Value::as_array_mut) {
            for (index, call) in calls.iter_mut().enumerate() {
                let call_path = child(&child(path, "tool_calls")?, &index.to_string())?;
                if let Some(function) = call.get_mut("function") {
                    self.field(
                        function,
                        "arguments",
                        &role,
                        &child(&call_path, "function")?,
                        true,
                    )?;
                }
                if let Some(custom) = call.get_mut("custom") {
                    self.field(custom, "input", &role, &child(&call_path, "custom")?, true)?;
                }
                self.chat_tool_metadata(call, &role, &call_path)?;
                let genuine = self.endpoint == "/v1/chat/completions"
                    && role == "assistant"
                    && call["type"] == "function"
                    && native_slot(&call_path, "messages", "tool_calls");
                if let Some(fields) = call.pointer_mut("/function/provider_specific_fields") {
                    let fields_path =
                        child(&child(&call_path, "function")?, "provider_specific_fields")?;
                    if genuine {
                        self.metadata(fields, &role, &fields_path, false)?;
                    } else {
                        self.tool(fields, &role, &fields_path)?;
                    }
                }
            }
        }
        if let Some(function) = value.get_mut("function_call") {
            self.field(
                function,
                "arguments",
                &role,
                &child(path, "function_call")?,
                true,
            )?;
        }
        for key in ["thinking_blocks", "reasoning_details"] {
            if let Some(items) = value.get_mut(key).and_then(Value::as_array_mut) {
                for (index, item) in items.iter_mut().enumerate() {
                    self.reasoning(
                        item,
                        &role,
                        &child(&child(path, key)?, &index.to_string())?,
                        key,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn chat_tool_metadata(&mut self, value: &mut Value, role: &str, path: &str) -> Result<()> {
        let genuine = self.endpoint == "/v1/chat/completions"
            && role == "assistant"
            && value["type"] == "function"
            && native_slot(path, "messages", "tool_calls");
        for key in ["extra_content", "provider_specific_fields"] {
            if let Some(value) = value.get_mut(key) {
                if genuine {
                    self.metadata(value, role, &child(path, key)?, key == "extra_content")?;
                } else {
                    self.tool(value, role, &child(path, key)?)?;
                }
            }
        }
        Ok(())
    }

    fn metadata(&mut self, value: &mut Value, role: &str, path: &str, google: bool) -> Result<()> {
        if let Some(object) = value.as_object_mut() {
            for (key, value) in object {
                if google && key == "google" {
                    self.metadata(value, role, &child(path, key)?, false)?;
                } else if !(!google && key == "thought_signature" && value.is_string()) {
                    self.tool(value, role, &child(path, key)?)?;
                }
            }
        } else {
            self.tool(value, role, path)?;
        }
        Ok(())
    }

    fn reasoning(&mut self, value: &mut Value, role: &str, path: &str, field: &str) -> Result<()> {
        let native = role == "assistant"
            && self.endpoint == "/v1/chat/completions"
            && native_slot(path, "messages", field)
            && value.is_object();
        let signed = native && value["signature"].as_str().is_some_and(|s| !s.is_empty());
        let kind = value["type"].as_str().unwrap_or("").to_owned();
        if field == "thinking_blocks"
            && native
            && (kind == "redacted_thinking" && value["data"].is_string()
                || kind == "thinking" && signed && value["thinking"].is_string())
        {
            return Ok(());
        }
        if field == "reasoning_details"
            && native
            && (kind == "reasoning.encrypted" && value["data"].is_string()
                || kind == "reasoning.text" && signed && value["text"].is_string())
        {
            return Ok(());
        }
        self.tool(value, role, path)
    }

    fn content(&mut self, value: &mut Value, role: &str, path: &str) -> Result<()> {
        match value {
            // Tool-result text wrappers and bare results share one parser. This
            // preserves embedded container JSON scalars rather than matching its
            // serialized numeric tokens as if they were ordinary text.
            Value::String(_) if role == "tool" => self.tool(value, role, path),
            Value::String(text) => self.text(text, role, path),
            Value::Array(items) => {
                for (index, value) in items.iter_mut().enumerate() {
                    self.content(value, role, &child(path, &index.to_string())?)?;
                }
                Ok(())
            }
            Value::Object(_) => {
                let kind = value["type"].as_str().unwrap_or("").to_owned();
                if matches!(
                    kind.as_str(),
                    "image"
                        | "input_image"
                        | "image_url"
                        | "audio"
                        | "input_audio"
                        | "document"
                        | "input_file"
                        | "file"
                        | "base64"
                ) {
                    return Ok(());
                }
                let native_thinking = role == "assistant"
                    && matches!(self.endpoint, "/v1/messages" | "/v1/chat/completions")
                    && native_slot(path, "messages", "content");
                if native_thinking
                    && (kind == "redacted_thinking" && value["data"].is_string()
                        || kind == "thinking"
                            && value["thinking"].is_string()
                            && value["signature"].as_str().is_some_and(|s| !s.is_empty()))
                {
                    return Ok(());
                }
                if role == "assistant"
                    && self.endpoint == "/v1/messages"
                    && native_slot(path, "messages", "content")
                    && kind == "web_search_tool_result"
                {
                    if let Some(items) = value.get_mut("content").and_then(Value::as_array_mut) {
                        for (index, item) in items.iter_mut().enumerate() {
                            let item_path = child(&child(path, "content")?, &index.to_string())?;
                            if item["type"] == "web_search_result" {
                                validate_encrypted_carrier(item)?;
                                for (key, leaf) in
                                    item.as_object_mut().expect("typed record").iter_mut()
                                {
                                    if key != "encrypted_content" && !protocol_key(key) {
                                        self.content(leaf, role, &child(&item_path, key)?)?;
                                    }
                                }
                            } else {
                                self.content(item, role, &item_path)?;
                            }
                        }
                    }
                    return Ok(());
                }
                let semantic_role = if kind == "tool_result"
                    && role == "user"
                    && native_slot(path, "messages", "content")
                    && matches!(self.endpoint, "/v1/messages" | "/v1/chat/completions")
                {
                    "tool"
                } else {
                    role
                };
                if kind == "tool_use" {
                    self.field(value, "input", role, path, true)?;
                }
                for key in [
                    "functionCall",
                    "function_call",
                    "functionResponse",
                    "function_response",
                ] {
                    if let Some(call) = value.get_mut(key) {
                        let call_path = child(path, key)?;
                        self.field(call, "args", role, &call_path, true)?;
                        self.field(call, "response", "tool", &call_path, true)?;
                    }
                }
                let keys = value
                    .as_object()
                    .expect("object")
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                for key in keys {
                    if protocol_key(&key)
                        || matches!(
                            key.as_str(),
                            "functionCall"
                                | "function_call"
                                | "functionResponse"
                                | "function_response"
                        )
                    {
                        continue;
                    }
                    if matches!(
                        key.as_str(),
                        "inlineData" | "inline_data" | "fileData" | "file_data"
                    ) && self.endpoint.starts_with("/v1beta/models:")
                        && native_slot(path, "contents", "parts")
                    {
                        continue;
                    }
                    if key == "input" && kind == "tool_use" {
                        continue;
                    }
                    if matches!(key.as_str(), "thoughtSignature" | "thought_signature")
                        && role == "assistant"
                        && self.endpoint.starts_with("/v1beta/models:")
                        && native_slot(path, "contents", "parts")
                        && value[&key].is_string()
                    {
                        continue;
                    }
                    if let Some(value) = value.get_mut(&key) {
                        if semantic_role == "tool" && key == "content" {
                            self.tool_result(value, &child(path, &key)?)?;
                        } else {
                            self.content(value, semantic_role, &child(path, &key)?)?;
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn tool_result(&mut self, value: &mut Value, path: &str) -> Result<()> {
        if value.is_array()
            || value["type"].as_str().is_some_and(|kind| {
                matches!(
                    kind,
                    "text"
                        | "input_text"
                        | "output_text"
                        | "image"
                        | "input_image"
                        | "image_url"
                        | "document"
                        | "input_file"
                        | "audio"
                        | "input_audio"
                )
            })
        {
            self.content(value, "tool", path)
        } else {
            self.tool(value, "tool", path)
        }
    }
}

// Opaque replay is a string carrier. Non-string values at the encrypted field
// are unsupported ordinary content and must fail before provider forwarding.
fn validate_encrypted_carrier(record: &Value) -> Result<()> {
    if record
        .get("encrypted_content")
        .is_some_and(|carrier| !carrier.is_string())
    {
        return Err(Error::Invalid(
            "privacy replay encrypted_content must be a string",
        ));
    }
    Ok(())
}

fn protocol_key(key: &str) -> bool {
    matches!(
        key,
        "role"
            | "type"
            | "id"
            | "name"
            | "call_id"
            | "tool_use_id"
            | "item_id"
            | "file_id"
            | "previous_response_id"
            | "status"
            | "cache_control"
    )
}

fn native_slot(path: &str, root: &str, field: &str) -> bool {
    let parts = path.split('/').skip(1).collect::<Vec<_>>();
    parts.len() == 4
        && parts[0] == root
        && parts[1].parse::<usize>().is_ok()
        && parts[2] == field
        && parts[3].parse::<usize>().is_ok()
}

fn parse_tool_json(text: &str, depth: usize) -> Result<Option<Value>> {
    if !matches!(text.trim_start().bytes().next(), Some(b'{' | b'[')) {
        return Ok(None);
    }
    // Check nested containers before serde's own recursion limit could turn an
    // overdeep JSON payload into an ordinary transcript and silently skip bounds.
    let mut level = depth;
    let mut quoted = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    level += 1;
                    if level > MAX_DEPTH {
                        return Err(Error::Invalid("privacy inspection depth exceeds 32"));
                    }
                }
                b'}' | b']' => level = level.saturating_sub(1),
                _ => (),
            }
        }
    }
    let Ok(document) = serde_json::from_str::<Value>(text) else {
        return Ok(None);
    };
    // serde_json accepts duplicate keys. Reject such payloads before changing
    // any leaf: reserialization would otherwise silently discard a sibling.
    let mut parser = serde_json::Deserializer::from_str(text);
    use serde::Deserialize;
    UniqueJson::deserialize(&mut parser)
        .map_err(|_| Error::Invalid("privacy tool JSON contains duplicate keys"))?;
    Ok(Some(document))
}

struct UniqueJson;
impl<'de> serde::Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<UniqueJson, M::Error> {
                let mut keys = std::collections::HashSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key) {
                        return Err(serde::de::Error::custom("duplicate key"));
                    }
                    map.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<UniqueJson, S::Error> {
                while seq.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                _: bool,
            ) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
