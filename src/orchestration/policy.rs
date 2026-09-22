//! Versioned request policy. Invalid operator configuration fails closed.
use std::collections::BTreeMap;

use http::{HeaderMap, HeaderName, HeaderValue};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use super::{Error, Result};

pub fn document(raw: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(raw).map_err(|_| Error::Configuration)?;
    if !value.is_object() || value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(Error::Configuration);
    }
    Ok(value)
}

pub fn regex(pattern: &str) -> Result<Regex> {
    Regex::new(pattern).map_err(|_| Error::Configuration)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    #[default]
    Failover,
    RoundRobin,
    Weighted,
    LeastInflight,
    Latency,
    Adaptive,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub rpm: Option<u32>,
    pub tpm: Option<u32>,
    pub concurrent: Option<u32>,
    pub queue: u32,
    pub queue_timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Routing {
    pub version: u32,
    pub strategy: Strategy,
    pub sticky: StickyMode,
    pub max_attempts: usize,
    pub allowed_tags: Vec<String>,
    pub tag_mode: TagMode,
    pub allowed_tools: Option<Vec<String>>,
    pub allowed_endpoints: Option<Vec<String>>,
    pub limits: Limits,
}

impl Default for Routing {
    fn default() -> Self {
        Self {
            version: 1,
            strategy: Strategy::Failover,
            sticky: StickyMode::Off,
            max_attempts: 3,
            allowed_tags: vec![],
            tag_mode: TagMode::Any,
            allowed_tools: None,
            allowed_endpoints: None,
            limits: Limits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StickyMode {
    #[default]
    Off,
    Trace,
    Session,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagMode {
    #[default]
    Any,
    All,
    None,
}

impl Routing {
    pub fn parse(raw: &str) -> Result<Self> {
        let policy: Self =
            serde_json::from_value(document(raw)?).map_err(|_| Error::Configuration)?;
        if policy.max_attempts == 0 || policy.max_attempts > 32 {
            return Err(Error::Configuration);
        }
        Ok(policy)
    }
    pub fn allows_tags(&self, tags: &[String]) -> bool {
        if self.allowed_tags.is_empty() {
            return true;
        }
        let matches = |tag: &String| tags.contains(tag);
        match self.tag_mode {
            TagMode::Any => self.allowed_tags.iter().any(matches),
            TagMode::All => self.allowed_tags.iter().all(matches),
            TagMode::None => !self.allowed_tags.iter().any(matches),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AssociationExclusions {
    pub version: u32,
    pub channel_name_patterns: Vec<String>,
    pub channel_ids: Vec<String>,
    pub channel_tags: Vec<String>,
}

impl AssociationExclusions {
    pub fn parse(value: &Value) -> Result<Self> {
        let exclusions: Self = serde_json::from_value(document(&value.to_string())?)
            .map_err(|_| Error::Configuration)?;
        for values in [
            &exclusions.channel_name_patterns,
            &exclusions.channel_ids,
            &exclusions.channel_tags,
        ] {
            if values.len() > 64
                || values
                    .iter()
                    .any(|value| value.is_empty() || value.len() > 256)
            {
                return Err(Error::Configuration);
            }
        }
        for pattern in &exclusions.channel_name_patterns {
            regex(pattern)?;
        }
        Ok(exclusions)
    }

    pub fn excludes(&self, channel_id: &str, channel_name: &str, tags: &[String]) -> Result<bool> {
        if self.channel_ids.iter().any(|id| id == channel_id)
            || self
                .channel_tags
                .iter()
                .any(|excluded| tags.iter().any(|tag| tag == excluded))
        {
            return Ok(true);
        }
        for pattern in &self.channel_name_patterns {
            if regex(pattern)?.is_match(channel_name) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Retry {
    pub version: u32,
    pub statuses: Vec<u16>,
    pub error_patterns: Vec<String>,
    pub attempts: usize,
    pub delay_ms: u64,
    pub transport: bool,
    pub empty_success: bool,
    pub first_event_timeout_ms: u64,
    pub event_timeout_ms: u64,
    pub error_mode: ErrorMode,
    pub error_message: Option<String>,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            version: 1,
            statuses: vec![408, 409, 429, 500, 502, 503, 504],
            error_patterns: vec![],
            attempts: 1,
            delay_ms: 0,
            transport: true,
            empty_success: false,
            first_event_timeout_ms: 30_000,
            event_timeout_ms: 60_000,
            error_mode: ErrorMode::Normalized,
            error_message: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorMode {
    PassThrough,
    #[default]
    Normalized,
    Custom,
}

impl Retry {
    pub fn parse(raw: &str) -> Result<Self> {
        let policy: Self =
            serde_json::from_value(document(raw)?).map_err(|_| Error::Configuration)?;
        if policy.attempts == 0
            || policy.attempts > 32
            || policy.delay_ms > 60_000
            || policy.first_event_timeout_ms == 0
            || policy.event_timeout_ms == 0
        {
            return Err(Error::Configuration);
        }
        for pattern in &policy.error_patterns {
            regex(pattern)?;
        }
        Ok(policy)
    }
    pub fn retry_status(&self, status: u16, body: &str) -> bool {
        self.statuses.contains(&status)
            || self
                .error_patterns
                .iter()
                .any(|pattern| regex(pattern).is_ok_and(|r| r.is_match(body)))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CircuitPolicy {
    pub enabled: bool,
    pub failures: usize,
    pub window_ms: u64,
    pub recovery_ms: u64,
}

impl Default for CircuitPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            failures: 5,
            window_ms: 60_000,
            recovery_ms: 30_000,
        }
    }
}

/// A deliberately small typed condition language; unknown fields/operators never match.
pub fn matches(condition: &Value, context: &Value) -> Result<bool> {
    evaluate(condition, context, 0)
}

/// Check a condition tree exactly as the evaluator will, before it is stored.
///
/// The evaluator rejects unknown keys, a missing `field`/`op`, a bad regex and a
/// value of the wrong type — but it runs per request, so one malformed condition
/// used to abort candidate generation for the whole project and surface as a
/// generic 500 on every request, with nothing naming the offending rule. Running
/// it here against a synthetic context turns that into a 400 on the form.
pub fn validate_conditions(condition: &Value) -> Result<()> {
    let context = json!({
        "body": {},
        "headers": {},
        "endpoint": "/v1/chat/completions",
        "project_id": "",
        "api_key_id": "",
        "daily_time": 0,
        "daily_time_timezone": "UTC",
        "daily_time_source": "gateway_clock",
        "has_image": false,
        "has_video": false,
        "has_document": false,
        "has_audio": false,
        "stream": false,
        "request_format": "openai_chat",
    });
    matches(condition, &context).map(|_| ())
}

#[derive(Clone, Copy)]
enum ConditionField {
    Dynamic,
    String,
    Number,
    Boolean,
}

fn pointer_segments(pointer: &str) -> Result<Vec<String>> {
    if !pointer.starts_with('/') {
        return Err(Error::Configuration);
    }
    pointer[1..]
        .split('/')
        .map(|segment| {
            let mut decoded = String::with_capacity(segment.len());
            let mut chars = segment.chars();
            while let Some(character) = chars.next() {
                if character != '~' {
                    decoded.push(character);
                    continue;
                }
                match chars.next() {
                    Some('0') => decoded.push('~'),
                    Some('1') => decoded.push('/'),
                    _ => return Err(Error::Configuration),
                }
            }
            Ok(decoded)
        })
        .collect()
}

fn condition_field(field: &str) -> Result<ConditionField> {
    let segments = pointer_segments(field)?;
    let Some(root) = segments.first().map(String::as_str) else {
        return Err(Error::Configuration);
    };
    match root {
        "body" => {
            if segments
                .iter()
                .skip(1)
                .any(|segment| sensitive_header(segment))
            {
                return Err(Error::Configuration);
            }
            Ok(ConditionField::Dynamic)
        }
        "headers" => {
            let Some(name) = segments.get(1) else {
                return Err(Error::Configuration);
            };
            if segments.len() != 2 || sensitive_header(name) {
                return Err(Error::Configuration);
            }
            Ok(ConditionField::String)
        }
        "endpoint"
        | "project_id"
        | "api_key_id"
        | "daily_time_timezone"
        | "daily_time_source"
        | "request_format"
            if segments.len() == 1 =>
        {
            Ok(ConditionField::String)
        }
        "daily_time" if segments.len() == 1 => Ok(ConditionField::Number),
        "has_image" | "has_video" | "has_document" | "has_audio" | "stream"
            if segments.len() == 1 =>
        {
            Ok(ConditionField::Boolean)
        }
        _ => Err(Error::Configuration),
    }
}

fn validate_operator(field: ConditionField, op: &str, expected: &Value) -> Result<()> {
    if op == "exists" {
        return expected
            .is_boolean()
            .then_some(())
            .ok_or(Error::Configuration);
    }
    let allowed = match field {
        ConditionField::Dynamic => matches!(
            op,
            "eq" | "ne" | "in" | "contains" | "regex" | "gt" | "gte" | "lt" | "lte"
        ),
        ConditionField::String => matches!(op, "eq" | "ne" | "in" | "contains" | "regex"),
        ConditionField::Number => matches!(op, "eq" | "ne" | "in" | "gt" | "gte" | "lt" | "lte"),
        ConditionField::Boolean => matches!(op, "eq" | "ne" | "in"),
    };
    if !allowed {
        return Err(Error::Configuration);
    }
    let values = if op == "in" {
        expected.as_array().ok_or(Error::Configuration)?.as_slice()
    } else {
        std::slice::from_ref(expected)
    };
    match field {
        ConditionField::Dynamic => {}
        ConditionField::String if values.iter().all(|value| value.is_string()) => {}
        ConditionField::Number
            if values
                .iter()
                .all(|value| value.as_u64().is_some_and(|minute| minute < 24 * 60)) => {}
        ConditionField::Boolean if values.iter().all(|value| value.is_boolean()) => {}
        _ => return Err(Error::Configuration),
    }
    if op == "regex" {
        regex(expected.as_str().ok_or(Error::Configuration)?)?;
    }
    Ok(())
}

fn evaluate(condition: &Value, context: &Value, depth: usize) -> Result<bool> {
    if depth > 16 {
        return Err(Error::Configuration);
    }
    let object = condition.as_object().ok_or(Error::Configuration)?;
    if object
        .get("version")
        .is_some_and(|version| version.as_u64() != Some(1))
    {
        return Err(Error::Configuration);
    }
    if object.is_empty() || (object.len() == 1 && object.get("version") == Some(&json!(1))) {
        return Ok(true);
    }
    for key in ["all", "any"] {
        if let Some(children) = object.get(key) {
            if object.keys().any(|k| k != key && k != "version") {
                return Err(Error::Configuration);
            }
            let results = children
                .as_array()
                .ok_or(Error::Configuration)?
                .iter()
                .map(|child| evaluate(child, context, depth + 1))
                .collect::<Result<Vec<_>>>()?;
            return Ok(if key == "all" {
                results.iter().all(|v| *v)
            } else {
                results.iter().any(|v| *v)
            });
        }
    }
    if object
        .keys()
        .any(|k| !["version", "field", "op", "value"].contains(&k.as_str()))
    {
        return Err(Error::Configuration);
    }
    let field = object
        .get("field")
        .and_then(Value::as_str)
        .ok_or(Error::Configuration)?;
    let field_kind = condition_field(field)?;
    let expected = object.get("value").unwrap_or(&Value::Null);
    let op = object
        .get("op")
        .and_then(Value::as_str)
        .ok_or(Error::Configuration)?;
    validate_operator(field_kind, op, expected)?;
    let actual = context.pointer(field);
    Ok(match op {
        "exists" => actual.is_some() == expected.as_bool().ok_or(Error::Configuration)?,
        "eq" => actual == Some(expected),
        "ne" => actual.is_some() && actual != Some(expected),
        "in" => expected
            .as_array()
            .ok_or(Error::Configuration)?
            .iter()
            .any(|v| actual == Some(v)),
        "contains" => actual.is_some_and(|a| match a {
            Value::Array(v) => v.contains(expected),
            Value::String(v) => expected.as_str().is_some_and(|s| v.contains(s)),
            _ => false,
        }),
        "regex" => {
            let regex = regex(expected.as_str().ok_or(Error::Configuration)?)?;
            actual
                .and_then(Value::as_str)
                .is_some_and(|s| regex.is_match(s))
        }
        "gt" | "gte" | "lt" | "lte" => {
            let value = expected.as_f64().ok_or(Error::Configuration)?;
            actual.and_then(Value::as_f64).is_some_and(|n| match op {
                "gt" => n > value,
                "gte" => n >= value,
                "lt" => n < value,
                _ => n <= value,
            })
        }
        _ => return Err(Error::Configuration),
    })
}

pub fn safe_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    const MAX_HEADERS: usize = 64;
    const MAX_VALUE_BYTES: usize = 1_024;
    const MAX_TOTAL_BYTES: usize = 8 * 1_024;

    let mut result = BTreeMap::new();
    let mut total = 0;
    for (name, value) in headers.iter() {
        if result.len() == MAX_HEADERS || sensitive_header(name.as_str()) {
            continue;
        }
        let Ok(value) = value.to_str() else {
            continue;
        };
        let bytes = name.as_str().len() + value.len();
        if value.len() > MAX_VALUE_BYTES || total + bytes > MAX_TOTAL_BYTES {
            continue;
        }
        total += bytes;
        result.insert(name.to_string(), value.to_owned());
    }
    result
}

pub fn sensitive_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("authorization")
        || name.contains("key")
        || name.contains("token")
        || name.contains("secret")
        || name.contains("password")
        || name.contains("credential")
        || name.contains("cookie")
        || name == "x-pangolin-trusted-client-ip"
}

pub fn context(
    body: &Value,
    headers: &HeaderMap,
    endpoint: &str,
    principal: Option<(&str, &str)>,
) -> Value {
    context_at(
        body,
        headers,
        endpoint,
        principal,
        OffsetDateTime::now_utc(),
    )
}

const MAX_CONTEXT_DEPTH: usize = 16;
const MAX_CONTEXT_NODES: usize = 512;
const MAX_CONTEXT_STRING_BYTES: usize = 4_096;
const MAX_CONTEXT_KEY_BYTES: usize = 128;

fn bounded_body(value: &Value, depth: usize, remaining: &mut usize) -> Option<Value> {
    if depth > MAX_CONTEXT_DEPTH || *remaining == 0 {
        return None;
    }
    *remaining -= 1;
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Some(value.clone()),
        Value::String(value) => {
            (value.len() <= MAX_CONTEXT_STRING_BYTES).then(|| Value::String(value.clone()))
        }
        Value::Array(values) => Some(Value::Array(
            values
                .iter()
                .filter_map(|value| bounded_body(value, depth + 1, remaining))
                .collect(),
        )),
        Value::Object(values) => {
            let mut result = Map::new();
            // Preserve cheap top-level facts such as model and stream before a
            // large messages array can exhaust the shared node budget.
            for containers in [false, true] {
                for (key, value) in values {
                    let is_container = value.is_array() || value.is_object();
                    if is_container != containers {
                        continue;
                    }
                    if key.len() > MAX_CONTEXT_KEY_BYTES || sensitive_header(key) {
                        continue;
                    }
                    if let Some(value) = bounded_body(value, depth + 1, remaining) {
                        result.insert(key.clone(), value);
                    }
                }
            }
            Some(Value::Object(result))
        }
    }
}

#[derive(Default)]
struct MediaPresence {
    image: bool,
    video: bool,
    document: bool,
    audio: bool,
}

impl MediaPresence {
    fn observe(&mut self, value: &str) {
        let value = value.to_ascii_lowercase();
        self.image |= value.starts_with("image/")
            || matches!(value.as_str(), "image" | "image_url" | "input_image");
        self.video |= value.starts_with("video/")
            || matches!(value.as_str(), "video" | "video_url" | "input_video");
        self.audio |= value.starts_with("audio/")
            || matches!(value.as_str(), "audio" | "audio_url" | "input_audio");
        self.document |= value == "document"
            || value == "input_file"
            || value == "file"
            || value == "file_url"
            || value == "application/pdf"
            || value.starts_with("text/");
    }
}

fn media_presence(value: &Value, presence: &mut MediaPresence) {
    match value {
        Value::String(value) => {
            if let Some(media_type) = value
                .strip_prefix("data:")
                .and_then(|value| value.split(';').next())
            {
                presence.observe(media_type);
            }
        }
        Value::Array(values) => {
            for value in values {
                media_presence(value, presence);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if matches!(
                    key.as_str(),
                    "image"
                        | "image_url"
                        | "input_image"
                        | "video"
                        | "video_url"
                        | "input_video"
                        | "audio"
                        | "audio_url"
                        | "input_audio"
                        | "document"
                        | "input_file"
                        | "file"
                        | "file_url"
                ) {
                    presence.observe(key);
                }
                if matches!(
                    key.as_str(),
                    "type" | "media_type" | "mime_type" | "content_type"
                ) && let Some(value) = value.as_str()
                {
                    presence.observe(value);
                }
                media_presence(value, presence);
            }
        }
        _ => {}
    }
}

fn request_format(endpoint: &str) -> &'static str {
    match endpoint {
        "/v1/chat/completions" => "openai_chat",
        "/v1/responses" => "openai_responses",
        "/v1/messages" => "anthropic_messages",
        _ => "unknown",
    }
}

pub fn context_at(
    body: &Value,
    headers: &HeaderMap,
    endpoint: &str,
    principal: Option<(&str, &str)>,
    now: OffsetDateTime,
) -> Value {
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let mut remaining = MAX_CONTEXT_NODES;
    let body = bounded_body(body, 0, &mut remaining).unwrap_or_else(|| json!({}));
    let mut media = MediaPresence::default();
    media_presence(&body, &mut media);
    let mut context = json!({
        "body": body,
        "headers": safe_headers(headers),
        "endpoint": endpoint,
        "daily_time": u16::from(now.hour()) * 60 + u16::from(now.minute()),
        "daily_time_timezone": "UTC",
        "daily_time_source": "gateway_clock",
        "has_image": media.image,
        "has_video": media.video,
        "has_document": media.document,
        "has_audio": media.audio,
        "stream": stream,
        "request_format": request_format(endpoint),
    });
    if let Some((project_id, api_key_id)) = principal {
        // A condition may be scoped to the calling key. It has to be a field
        // rather than a header because credentials are stripped from the context,
        // so a header could never express it.
        context["project_id"] = json!(project_id);
        context["api_key_id"] = json!(api_key_id);
    }
    context
}

/// Merge and RFC 6902 patch operations are applied to a fresh clone for every attempt.
pub fn overrides(body: &Value, raw: &str, context: &Value) -> Result<(Value, HeaderMap)> {
    let doc = document(raw)?;
    let mut result = body.clone();
    let mut headers = HeaderMap::new();
    if let Some(operations) = doc.get("operations") {
        for operation in operations.as_array().ok_or(Error::Configuration)? {
            if let Some(condition) = operation.get("when")
                && !matches(condition, context)?
            {
                continue;
            }
            if let Some(merge) = operation.get("merge") {
                let merge = expand(merge, context)?;
                if !merge.is_object() {
                    return Err(Error::Configuration);
                }
                json_patch::merge(&mut result, &merge);
            }
            if let Some(patch) = operation.get("patch") {
                let patch: json_patch::Patch = serde_json::from_value(expand(patch, context)?)
                    .map_err(|_| Error::Configuration)?;
                json_patch::patch(&mut result, &patch).map_err(|_| Error::Configuration)?;
            }
            if let Some(entries) = operation.get("headers") {
                for (name, value) in entries.as_object().ok_or(Error::Configuration)? {
                    if sensitive_header(name)
                        || matches!(
                            name.to_ascii_lowercase().as_str(),
                            "host"
                                | "content-length"
                                | "transfer-encoding"
                                | "connection"
                                | "keep-alive"
                                | "proxy-connection"
                                | "te"
                                | "trailer"
                                | "upgrade"
                        )
                    {
                        return Err(Error::Configuration);
                    }
                    let name =
                        HeaderName::try_from(name.as_str()).map_err(|_| Error::Configuration)?;
                    if value.is_null() {
                        headers.remove(name);
                    } else {
                        let value = expand(value, context)?;
                        headers.insert(
                            name,
                            HeaderValue::try_from(value.as_str().ok_or(Error::Configuration)?)
                                .map_err(|_| Error::Configuration)?,
                        );
                    }
                }
            }
        }
    }
    // Routing/security fields must not be changed after admission/capability decisions.
    if !result.is_object()
        || result.get("model") != body.get("model")
        || result.get("stream") != body.get("stream")
    {
        return Err(Error::Configuration);
    }
    Ok((result, headers))
}

fn expand(template: &Value, context: &Value) -> Result<Value> {
    match template {
        Value::Object(map) if map.len() == 1 && map.contains_key("$request") => {
            let pointer = map["$request"].as_str().ok_or(Error::Configuration)?;
            context
                .pointer(pointer)
                .cloned()
                .ok_or(Error::Configuration)
        }
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| Ok((k.clone(), expand(v, context)?)))
            .collect::<Result<serde_json::Map<_, _>>>()
            .map(Value::Object),
        Value::Array(values) => values
            .iter()
            .map(|v| expand(v, context))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        _ => Ok(template.clone()),
    }
}
