use super::{ensure_fields, invalid};
use crate::api::ApiError;
use serde_json::{Value, json};

#[derive(Clone)]
pub enum ResponseTransform {
    Identity,
    Antigravity(Box<ResponseTransform>),
    GeminiChat(String),
    ChatGemini,
    ChatMessages(String),
    GeminiEmbedding,
    Bedrock(String),
}

impl ResponseTransform {
    pub fn apply(&self, body: Value) -> Result<Value, ApiError> {
        match self {
            Self::Identity => Ok(body),
            Self::Antigravity(inner) => {
                let response = body
                    .get("response")
                    .cloned()
                    .ok_or_else(|| invalid("Antigravity response missing response"))?;
                inner.apply(response)
            }
            Self::Bedrock(model) => super::upstream::bedrock_response(model, body),
            Self::GeminiEmbedding => {
                let items = body["embeddings"]
                    .as_array()
                    .ok_or_else(|| invalid("Gemini embeddings response missing embeddings"))?;
                Ok(
                    json!({"object":"list","data":items.iter().enumerate().map(|(index,item)|json!({"object":"embedding","index":index,"embedding":item["values"]})).collect::<Vec<_>>()}),
                )
            }
            Self::GeminiChat(model) => {
                let candidates = body["candidates"]
                    .as_array()
                    .ok_or_else(|| invalid("Gemini response missing candidates"))?;
                let choices = candidates.iter().enumerate().map(|(index,c)| {
                    let text = gemini_text(&c["content"])?;
                    Ok(json!({"index":index,"message":{"role":"assistant","content":text},"finish_reason":match c["finishReason"].as_str(){Some("MAX_TOKENS")=>"length",Some("SAFETY" | "RECITATION")=>"content_filter",_=>"stop"}}))
                }).collect::<Result<Vec<_>,ApiError>>()?;
                Ok(
                    json!({"id":body.get("responseId").cloned().unwrap_or_else(||json!(format!("chatcmpl_{}",uuid::Uuid::new_v4().simple()))),"object":"chat.completion","model":model,"created":crate::db::now(),"choices":choices,"usage":{"prompt_tokens":body["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0),"completion_tokens":body["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0),"total_tokens":body["usageMetadata"]["totalTokenCount"].as_u64().unwrap_or(0)}}),
                )
            }
            Self::ChatGemini => {
                let choices = text_choices(&body)?;
                Ok(
                    json!({"candidates":choices.into_iter().map(|(text,reason)|json!({"content":{"role":"model","parts":[{"text":text}]},"finishReason":if reason=="length"{"MAX_TOKENS"}else{"STOP"}})).collect::<Vec<_>>(),"usageMetadata":{"promptTokenCount":body["usage"]["prompt_tokens"],"candidatesTokenCount":body["usage"]["completion_tokens"],"totalTokenCount":body["usage"]["total_tokens"]}}),
                )
            }
            Self::ChatMessages(model) => {
                let choices = text_choices(&body)?;
                if choices.len() != 1 {
                    return Err(invalid(
                        "Anthropic transformation requires exactly one choice",
                    ));
                }
                let (text, reason) = &choices[0];
                Ok(
                    json!({"id":body["id"],"type":"message","role":"assistant","model":model,"content":[{"type":"text","text":text}],"stop_reason":if reason=="length"{"max_tokens"}else{"end_turn"},"stop_sequence":null,"usage":{"input_tokens":body["usage"]["prompt_tokens"],"output_tokens":body["usage"]["completion_tokens"]}}),
                )
            }
        }
    }
    pub fn identity(&self) -> bool {
        matches!(self, Self::Identity)
    }
}

fn text_choices(body: &Value) -> Result<Vec<(String, String)>, ApiError> {
    body["choices"]
        .as_array()
        .ok_or_else(|| invalid("chat response missing choices"))?
        .iter()
        .map(|choice| {
            ensure_fields(&choice["message"], &["role", "content"])?;
            let text = choice["message"]["content"]
                .as_str()
                .ok_or_else(|| invalid("unsupported non-text chat response"))?;
            Ok((
                text.to_owned(),
                choice["finish_reason"]
                    .as_str()
                    .unwrap_or("stop")
                    .to_owned(),
            ))
        })
        .collect()
}

fn text_content(value: &Value) -> Result<String, ApiError> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_owned());
    }
    value
        .as_array()
        .ok_or_else(|| invalid("only text content is supported by this transformation"))?
        .iter()
        .map(|part| {
            ensure_fields(part, &["type", "text"])?;
            if part["type"] != "text" {
                return Err(invalid(
                    "non-text content requires the native provider endpoint",
                ));
            }
            part["text"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("text part requires text"))
        })
        .collect()
}

fn gemini_text(content: &Value) -> Result<String, ApiError> {
    ensure_fields(content, &["role", "parts"])?;
    content["parts"]
        .as_array()
        .ok_or_else(|| invalid("Gemini content requires parts"))?
        .iter()
        .map(|part| {
            ensure_fields(part, &["text"])?;
            part["text"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("non-text Gemini part requires native endpoint"))
        })
        .collect()
}

pub fn chat_to_gemini(body: &Value) -> Result<Value, ApiError> {
    ensure_fields(
        body,
        &[
            "model",
            "messages",
            "stream",
            "max_tokens",
            "max_completion_tokens",
            "temperature",
            "top_p",
            "stop",
            "n",
        ],
    )?;
    if body["stream"] == true {
        return Err(invalid(
            "cross-protocol Gemini streaming is unsupported; use native streamGenerateContent",
        ));
    }
    let mut contents = vec![];
    let mut system = vec![];
    for message in body["messages"]
        .as_array()
        .ok_or_else(|| invalid("messages required"))?
    {
        ensure_fields(message, &["role", "content"])?;
        let text = text_content(&message["content"])?;
        match message["role"].as_str() {
            Some("system" | "developer") => system.push(json!({"text":text})),
            Some("user") => contents.push(json!({"role":"user","parts":[{"text":text}]})),
            Some("assistant") => contents.push(json!({"role":"model","parts":[{"text":text}]})),
            _ => return Err(invalid("unsupported chat role for Gemini")),
        }
    }
    let mut config = json!({"maxOutputTokens":crate::orchestration::output_limit(body)?});
    for (source, target) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("n", "candidateCount"),
    ] {
        if let Some(value) = body.get(source) {
            config[target] = value.clone();
        }
    }
    if let Some(stop) = body.get("stop") {
        config["stopSequences"] = if stop.is_string() {
            json!([stop])
        } else {
            stop.clone()
        };
    }
    let mut result = json!({"contents":contents,"generationConfig":config});
    if !system.is_empty() {
        result["systemInstruction"] = json!({"parts":system});
    }
    Ok(result)
}

pub fn gemini_to_chat(body: &Value) -> Result<Value, ApiError> {
    ensure_fields(
        body,
        &[
            "model",
            "stream",
            "contents",
            "generationConfig",
            "systemInstruction",
        ],
    )?;
    if body["stream"] == true {
        return Err(invalid(
            "cross-protocol Gemini streaming requires native Gemini channel",
        ));
    }
    let mut messages = vec![];
    if let Some(system) = body.get("systemInstruction") {
        messages.push(json!({"role":"system","content":gemini_text(system)?}));
    }
    for content in body["contents"]
        .as_array()
        .ok_or_else(|| invalid("Gemini contents required"))?
    {
        let role = match content["role"].as_str() {
            Some("model") => "assistant",
            Some("user") | None => "user",
            _ => return Err(invalid("unsupported Gemini role")),
        };
        messages.push(json!({"role":role,"content":gemini_text(content)?}));
    }
    let mut output = json!({"model":body["model"],"messages":messages});
    if let Some(config) = body.get("generationConfig") {
        ensure_fields(
            config,
            &[
                "maxOutputTokens",
                "temperature",
                "topP",
                "stopSequences",
                "candidateCount",
            ],
        )?;
        for (source, target) in [
            ("maxOutputTokens", "max_tokens"),
            ("temperature", "temperature"),
            ("topP", "top_p"),
            ("stopSequences", "stop"),
            ("candidateCount", "n"),
        ] {
            if let Some(value) = config.get(source) {
                output[target] = value.clone();
            }
        }
    }
    Ok(output)
}

pub fn messages_to_chat(body: &Value) -> Result<Value, ApiError> {
    ensure_fields(
        body,
        &[
            "model",
            "messages",
            "system",
            "max_tokens",
            "temperature",
            "top_p",
            "stop_sequences",
            "stream",
        ],
    )?;
    if body["stream"] == true {
        return Err(invalid(
            "cross-protocol Messages streaming requires native Anthropic channel",
        ));
    }
    let mut messages = vec![];
    if let Some(system) = body.get("system") {
        messages.push(json!({"role":"system","content":text_content(system)?}));
    }
    for message in body["messages"]
        .as_array()
        .ok_or_else(|| invalid("messages required"))?
    {
        ensure_fields(message, &["role", "content"])?;
        if !matches!(message["role"].as_str(), Some("user" | "assistant")) {
            return Err(invalid("unsupported Anthropic role"));
        }
        messages.push(json!({"role":message["role"],"content":text_content(&message["content"])?}));
    }
    let mut output = json!({"model":body["model"],"messages":messages});
    for (source, target) in [
        ("max_tokens", "max_tokens"),
        ("temperature", "temperature"),
        ("top_p", "top_p"),
        ("stop_sequences", "stop"),
    ] {
        if let Some(value) = body.get(source) {
            output[target] = value.clone();
        }
    }
    Ok(output)
}

pub fn embedding_to_gemini(body: &Value) -> Result<Value, ApiError> {
    ensure_fields(body, &["model", "input", "dimensions"])?;
    let values = if body["input"].is_string() {
        vec![body["input"].clone()]
    } else {
        body["input"]
            .as_array()
            .cloned()
            .ok_or_else(|| invalid("embedding input must be text or text array"))?
    };
    let requests=values.into_iter().map(|value|{
        if !value.is_string(){return Err(invalid("Gemini cannot translate token-array embeddings"));}
        let mut request=json!({"model":format!("models/{}",body["model"].as_str().unwrap_or("")),"content":{"parts":[{"text":value}]}});
        if let Some(dimensions)=body.get("dimensions"){request["outputDimensionality"]=dimensions.clone();}
        Ok(request)
    }).collect::<Result<Vec<_>,ApiError>>()?;
    Ok(json!({"requests":requests}))
}
