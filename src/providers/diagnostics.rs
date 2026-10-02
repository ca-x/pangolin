//! Bounded execution metadata. No diagnostic is built from an error's display
//! text, payload value, provider URL or arbitrary field name.
use crate::api::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_CONVERSION: usize = 64;
pub const MAX_AFFINITY: usize = 16;
const PATHS: &[&str] = &[
    "tools",
    "tool_choice",
    "messages",
    "messages[].content",
    "messages[].role",
    "messages[].tool_calls",
    "input",
    "stream",
    "system",
    "stop",
    "contents[].parts",
    "choices[].message",
    "candidates[].content.parts",
    "content",
    "response",
    "generationConfig",
    "choices",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Request,
    Response,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionCode {
    UnsupportedRequestShape,
    UnsupportedResponseShape,
    UnsupportedTools,
    UnsupportedNontextContent,
    UnsupportedStreaming,
    RolesNormalized,
    StopArrayNormalized,
    TextPartsNormalized,
}
impl ConversionCode {
    fn message(self) -> &'static str {
        match self {
            Self::UnsupportedRequestShape => "unsupported cross-protocol request shape",
            Self::UnsupportedResponseShape => "unsupported cross-protocol response shape",
            Self::UnsupportedTools => "tools require the native provider endpoint",
            Self::UnsupportedNontextContent => {
                "non-text content requires the native provider endpoint"
            }
            Self::UnsupportedStreaming => {
                "cross-protocol streaming requires the native provider endpoint"
            }
            Self::RolesNormalized => "supported message roles normalized",
            Self::StopArrayNormalized => "stop sequence normalized to an array",
            Self::TextPartsNormalized => "supported text parts normalized",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversionDiagnostic {
    pub phase: Phase,
    pub code: ConversionCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    reason: ConversionCode,
}
impl std::fmt::Display for ConversionDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code.message())
    }
}
impl ConversionDiagnostic {
    pub fn new(phase: Phase, code: ConversionCode, path: Option<&'static str>) -> Self {
        assert!(path.is_none_or(|path| PATHS.contains(&path)));
        Self {
            phase,
            code,
            path: path.map(str::to_owned),
            reason: code,
        }
    }
    pub fn rejection(phase: Phase, error: &ApiError) -> Self {
        if let ApiError::Conversion(diagnostic) = error {
            let mut diagnostic = diagnostic.as_ref().clone();
            diagnostic.phase = phase;
            if phase == Phase::Response {
                diagnostic.code = ConversionCode::UnsupportedResponseShape;
                diagnostic.reason = diagnostic.code;
            }
            return diagnostic;
        }
        Self::new(
            phase,
            match phase {
                Phase::Request => ConversionCode::UnsupportedRequestShape,
                Phase::Response => ConversionCode::UnsupportedResponseShape,
            },
            None,
        )
    }
    fn valid(&self) -> bool {
        self.reason == self.code
            && self
                .path
                .as_deref()
                .is_none_or(|path| path.len() <= 256 && PATHS.contains(&path))
    }
}

pub fn unsupported(code: ConversionCode, path: Option<&'static str>) -> ApiError {
    ApiError::Conversion(Box::new(ConversionDiagnostic::new(
        Phase::Request,
        code,
        path,
    )))
}
pub fn unsupported_field(key: &str) -> ApiError {
    use ConversionCode::*;
    let (code, path) = match key {
        "tools" | "functions" => (UnsupportedTools, Some("tools")),
        "tool_calls" => (UnsupportedTools, Some("messages[].tool_calls")),
        "tool_choice" | "function_call" => (UnsupportedTools, Some("tool_choice")),
        "image_url" | "inlineData" | "fileData" | "input_audio" => {
            (UnsupportedNontextContent, Some("messages[].content"))
        }
        _ => (UnsupportedRequestShape, None),
    };
    unsupported(code, path)
}

pub fn cross_protocol(kind: &str, endpoint: &str) -> bool {
    match kind {
        "anthropic" => endpoint == "/v1/chat/completions",
        "gemini" | "vertex" | "gcp" => {
            matches!(endpoint, "/v1/chat/completions" | "/v1/embeddings")
        }
        "bedrock" => true,
        _ => endpoint == "/v1/messages" || endpoint.starts_with("/v1beta/models:"),
    }
}

/// Called only after a successful, explicit cross-protocol request transform.
/// Native pass-through never produces a normalization explanation. Stop-array
/// capability describes the completed transform, not a provider-family guess.
pub fn normalizations(
    payload: &Value,
    native: bool,
    normalizes_stop_to_array: bool,
) -> Vec<ConversionDiagnostic> {
    if native {
        return vec![];
    }
    use ConversionCode::*;
    let mut diagnostics = vec![];
    if payload["messages"].as_array().is_some_and(|messages| {
        messages
            .iter()
            .any(|message| message["role"] == "developer")
    }) {
        diagnostics.push(ConversionDiagnostic::new(
            Phase::Request,
            RolesNormalized,
            Some("messages[].role"),
        ));
    }
    if normalizes_stop_to_array && payload["stop"].is_string() {
        diagnostics.push(ConversionDiagnostic::new(
            Phase::Request,
            StopArrayNormalized,
            Some("stop"),
        ));
    }
    if payload["messages"]
        .as_array()
        .is_some_and(|messages| messages.iter().any(|message| message["content"].is_array()))
    {
        diagnostics.push(ConversionDiagnostic::new(
            Phase::Request,
            TextPartsNormalized,
            Some("messages[].content"),
        ));
    }
    diagnostics
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AffinityReason {
    First,
    Hit,
    Expired,
    Ineligible,
    Established,
    Released,
    CandidateFailed,
}
impl AffinityReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Hit => "hit",
            Self::Expired => "expired",
            Self::Ineligible => "ineligible",
            Self::Established => "established",
            Self::Released => "released",
            Self::CandidateFailed => "candidate_failed",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AffinityDiagnostic {
    pub rule_id: String,
    pub scope_digest: String,
    pub reason: AffinityReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}
fn identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
pub fn safe_identity(value: &str) -> String {
    if identity(value) {
        value.to_owned()
    } else {
        blake3::hash(value.as_bytes()).to_hex().to_string()
    }
}
impl AffinityDiagnostic {
    fn valid(&self) -> bool {
        identity(&self.rule_id)
            && self.scope_digest.len() == 64
            && self.scope_digest.bytes().all(|b| b.is_ascii_hexdigit())
            && self.provider_id.as_deref().is_none_or(identity)
            && self.expires_at.is_none_or(|expiry| expiry >= 0)
    }
}

pub fn push_conversion(
    values: &mut Vec<ConversionDiagnostic>,
    diagnostic: ConversionDiagnostic,
) -> Result<(), ApiError> {
    if values.len() >= MAX_CONVERSION || !diagnostic.valid() {
        return Err(ApiError::BadRequest(
            "conversion diagnostic bounds exceeded".into(),
        ));
    }
    values.push(diagnostic);
    Ok(())
}
pub fn push_affinity(
    values: &mut Vec<AffinityDiagnostic>,
    diagnostic: AffinityDiagnostic,
) -> Result<(), ApiError> {
    if values.len() >= MAX_AFFINITY || !diagnostic.valid() {
        return Err(ApiError::BadRequest(
            "affinity diagnostic bounds exceeded".into(),
        ));
    }
    values.push(diagnostic);
    Ok(())
}

/// Old/manual metadata is untrusted. Fail closed to an empty array; never echo
/// unknown keys, strings or oversized entries from SQLite through detail APIs.
pub fn sanitize_document(document: &mut Value) {
    if let Some(raw) = document.get_mut("conversion_diagnostics") {
        let parsed = raw
            .as_str()
            .filter(|s| s.len() <= MAX_CONVERSION * 768)
            .and_then(|s| serde_json::from_str::<Vec<ConversionDiagnostic>>(s).ok())
            .filter(|values| {
                values.len() <= MAX_CONVERSION && values.iter().all(ConversionDiagnostic::valid)
            })
            .unwrap_or_default();
        *raw = serde_json::to_value(parsed).unwrap_or_else(|_| serde_json::json!([]));
    }
    if let Some(raw) = document.get_mut("affinity_diagnostics") {
        let parsed = raw
            .as_str()
            .filter(|s| s.len() <= MAX_AFFINITY * 512)
            .and_then(|s| serde_json::from_str::<Vec<AffinityDiagnostic>>(s).ok())
            .filter(|values| {
                values.len() <= MAX_AFFINITY && values.iter().all(AffinityDiagnostic::valid)
            })
            .unwrap_or_default();
        *raw = serde_json::to_value(parsed).unwrap_or_else(|_| serde_json::json!([]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reference_conversion_diagnostics_bounds_and_old_metadata_fail_closed() {
        let diagnostic = ConversionDiagnostic::new(
            Phase::Request,
            ConversionCode::UnsupportedTools,
            Some("tools"),
        );
        let mut values = vec![diagnostic.clone(); MAX_CONVERSION];
        assert!(push_conversion(&mut values, diagnostic.clone()).is_err());
        let valid = serde_json::to_value(&diagnostic).unwrap();
        for raw in [
            json!("private legacy string"),
            json!([{"phase":"request","code":"unsupported_tools","path":"private field","reason":"unsupported_tools"}]),
            json!([{"phase":"request","code":"unsupported_tools","path":"tools","reason":"private reason"}]),
            json!([{"phase":"request","code":"private code","reason":"private code"}]),
            json!([{"phase":"request","code":"unsupported_tools","reason":"unsupported_tools","private_extra":"private value"}]),
            json!(vec![valid.clone(); MAX_CONVERSION + 1]),
        ] {
            let mut document = json!({"conversion_diagnostics":raw.to_string()});
            sanitize_document(&mut document);
            assert_eq!(document["conversion_diagnostics"], json!([]));
            assert!(!document.to_string().contains("private"));
        }
        let mut document = json!({"conversion_diagnostics":json!([valid.clone()]).to_string()});
        sanitize_document(&mut document);
        assert_eq!(document["conversion_diagnostics"], json!([valid]));
    }

    #[test]
    fn reference_affinity_diagnostics_bounds_and_ids_fail_closed() {
        let diagnostic = AffinityDiagnostic {
            rule_id: "rule".into(),
            scope_digest: blake3::hash(b"scope").to_hex().to_string(),
            reason: AffinityReason::Hit,
            provider_id: Some("provider".into()),
            expires_at: Some(1000),
        };
        let mut values = vec![diagnostic.clone(); MAX_AFFINITY];
        assert!(push_affinity(&mut values, diagnostic.clone()).is_err());
        let valid = serde_json::to_value(&diagnostic).unwrap();
        for field in ["rule_id", "scope_digest", "provider_id", "reason"] {
            let mut invalid = valid.clone();
            invalid[field] = json!("private arbitrary value");
            let mut document = json!({"affinity_diagnostics":json!([invalid]).to_string()});
            sanitize_document(&mut document);
            assert_eq!(document["affinity_diagnostics"], json!([]));
        }
        let mut oversized =
            json!({"affinity_diagnostics":json!(vec![valid; MAX_AFFINITY+1]).to_string()});
        sanitize_document(&mut oversized);
        assert_eq!(oversized["affinity_diagnostics"], json!([]));
        assert_eq!(safe_identity("https://private.invalid/key").len(), 64);
    }
}
