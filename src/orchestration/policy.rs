//! Versioned request policy. Invalid operator configuration fails closed.
use std::collections::BTreeMap;

use http::{HeaderMap, HeaderName, HeaderValue};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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

fn evaluate(condition: &Value, context: &Value, depth: usize) -> Result<bool> {
    if depth > 16 {
        return Err(Error::Configuration);
    }
    let object = condition.as_object().ok_or(Error::Configuration)?;
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
    if !field.starts_with('/') {
        return Err(Error::Configuration);
    }
    let actual = context.pointer(field);
    let expected = object.get("value").unwrap_or(&Value::Null);
    let op = object
        .get("op")
        .and_then(Value::as_str)
        .ok_or(Error::Configuration)?;
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
    headers
        .iter()
        .filter(|(name, _)| !sensitive_header(name.as_str()))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.to_string(), v.to_owned()))
        })
        .collect()
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

pub fn context(body: &Value, headers: &HeaderMap, endpoint: &str) -> Value {
    json!({"body":body,"headers":safe_headers(headers),"endpoint":endpoint})
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
