//! Gateway orchestration boundary. Task 4 adapters consume `Plan`/`Candidate`;
//! observability consumes redacted `Decision`s and terminal attempt outcomes.
pub mod policy;
mod protection;
mod repository;
pub mod runtime;
pub mod session;
pub mod stream;

use http::HeaderMap;
use sea_orm::DatabaseConnection;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

use crate::models::{ApiKeyCredential, RouteTarget};
use policy::{CircuitPolicy, Limits, Retry, Routing, StickyMode};
pub use runtime::Runtime;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid orchestration configuration")]
    Configuration,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("request is denied by model or prompt policy")]
    Forbidden,
    #[error("request admission rejected: {0}")]
    Admission(&'static str),
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
}
pub type Result<T> = std::result::Result<T, Error>;

const ANTHROPIC_COMPLETION_COUNT_ERROR: &str = "Anthropic requests support exactly one completion";

#[derive(Clone)]
pub struct Candidate {
    pub target: RouteTarget,
    pub provider_id: String,
    pub model_id: String,
    pub credential_id: String,
    pub priority: i32,
    pub weight: u32,
    pub limits: Limits,
    pub retry: Retry,
    pub circuit: CircuitPolicy,
    pub overrides: String,
    pub model_rules: Value,
    pub pass_user_agent: bool,
    pub endpoint: String,
}

impl Candidate {
    pub fn id(&self) -> String {
        format!(
            "{}:{}:{}",
            self.provider_id,
            self.target.upstream_name.len(),
            self.target.upstream_name
        )
    }
    pub fn resource_id(&self) -> String {
        format!("channel:{}", self.provider_id)
    }
    pub fn circuit_id(&self) -> String {
        self.id()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Decision {
    pub stage: &'static str,
    pub candidate: Option<String>,
    pub reason: &'static str,
}

pub struct Profile {
    pub id: Option<String>,
    pub budget: Option<i64>,
    pub routing: Routing,
    pub mappings: Vec<(String, String)>,
    pub allowed: Vec<(String, String)>,
}

pub struct Plan {
    pub candidates: Vec<Candidate>,
    pub payload: Value,
    /// Protected client history before current prompts or channel transformations.
    pub session_request: Value,
    pub routing: Routing,
    pub sticky: Option<String>,
    pub decisions: Vec<Decision>,
    pub estimated_tokens: u32,
    protection: Vec<protection::Rule>,
    context: Value,
}

pub async fn load_profile(db: &DatabaseConnection, key: &ApiKeyCredential) -> Result<Profile> {
    repository::profile(db, key).await
}

pub async fn visible_models(
    db: &DatabaseConnection,
    key: &ApiKeyCredential,
    headers: &HeaderMap,
) -> Result<Vec<Value>> {
    use sea_orm::ConnectionTrait;
    let profile = load_profile(db, key).await?;
    let rows=db.query_all(repository::statement("SELECT DISTINCT m.public_name,m.created_at FROM models m JOIN providers p ON p.id=m.provider_id WHERE p.project_id=? AND p.enabled=1 AND m.enabled=1 ORDER BY m.public_name",vec![key.project_id.clone().into()])).await?;
    let mut names = std::collections::BTreeMap::new();
    for row in rows {
        names.insert(
            row.try_get::<String>("", "public_name")?,
            row.try_get::<i64>("", "created_at")?,
        );
    }
    for (source, _) in &profile.mappings {
        if !source.starts_with("regex:") {
            names.insert(source.clone(), 0);
        }
    }
    let mut visible = vec![];
    for (name, created) in names {
        let mapped = match profile.map_model(&name) {
            Ok(name) => name,
            Err(Error::Forbidden) => continue,
            Err(error) => return Err(error),
        };
        for endpoint in ["/v1/chat/completions", "/v1/responses", "/v1/messages"] {
            if profile
                .routing
                .allowed_endpoints
                .as_ref()
                .is_some_and(|allowed| !allowed.iter().any(|value| value == endpoint))
            {
                continue;
            }
            let context = policy::context(&serde_json::json!({"model":mapped}), headers, endpoint);
            if !repository::candidates(db, key, &mapped, &context, &profile.routing, &mut vec![])
                .await?
                .is_empty()
            {
                visible.push(serde_json::json!({"id":name,"object":"model","created":created,"owned_by":"pangolin"}));
                break;
            }
        }
    }
    Ok(visible)
}

impl Profile {
    pub fn map_model(&self, requested: &str) -> Result<String> {
        // Access policy is evaluated on the public name before aliases are rewritten.
        if !self.allowed.is_empty() {
            let mut allowed = false;
            for (pattern, kind) in &self.allowed {
                allowed |= match kind.as_str() {
                    "exact" => pattern == requested,
                    "regex" => policy::regex(pattern)?.is_match(requested),
                    _ => return Err(Error::Configuration),
                };
            }
            if !allowed {
                return Err(Error::Forbidden);
            }
        }
        for (source, target) in &self.mappings {
            if source == requested {
                return Ok(target.clone());
            }
            if let Some(pattern) = source.strip_prefix("regex:") {
                let regex = policy::regex(pattern)?;
                if regex.is_match(requested) {
                    return Ok(regex.replace(requested, target).into_owned());
                }
            }
        }
        Ok(requested.to_owned())
    }
}

pub async fn prepare(
    db: &DatabaseConnection,
    runtime: &Runtime,
    key: &ApiKeyCredential,
    profile: Profile,
    mut payload: Value,
    headers: &HeaderMap,
    endpoint: &str,
) -> Result<Plan> {
    if profile
        .routing
        .allowed_endpoints
        .as_ref()
        .is_some_and(|allowed| !allowed.iter().any(|value| value == endpoint))
    {
        return Err(Error::Forbidden);
    }
    let requested = payload
        .get("model")
        .and_then(Value::as_str)
        .ok_or(Error::Invalid("model is required"))?;
    let mapped = profile.map_model(requested)?;
    payload["model"] = Value::String(mapped.clone());
    let scope = format!("{}:{}", key.project_id, key.id);
    let context = policy::context(&payload, headers, endpoint);
    let mut decisions = vec![
        Decision {
            stage: "access",
            candidate: None,
            reason: "project_and_model_allowed",
        },
        Decision {
            stage: "mapping",
            candidate: None,
            reason: "profile_mapping_applied",
        },
    ];
    let mut candidates =
        repository::candidates(db, key, &mapped, &context, &profile.routing, &mut decisions)
            .await?;
    candidates.retain(|candidate| {
        let available = runtime.circuit_available(&candidate.circuit_id(), &candidate.circuit);
        if !available {
            decisions.push(Decision {
                stage: "health",
                candidate: Some(candidate.id()),
                reason: "circuit_open",
            });
        }
        available
    });
    let sticky_header = match profile.routing.sticky {
        StickyMode::Off => None,
        StickyMode::Trace => Some("x-trace-id"),
        StickyMode::Session => Some("x-session-id"),
    };
    let sticky = sticky_header
        .and_then(|name| headers.get(name))
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty() && v.len() <= 256)
        .map(|v| format!("{scope}:{}:{mapped}:{endpoint}:{v}", mapped.len()));
    runtime.order(
        &format!("{scope}:{}:{mapped}:{endpoint}", mapped.len()),
        &mut candidates,
        profile.routing.strategy,
        sticky.as_deref(),
    );
    for candidate in &candidates {
        decisions.push(Decision {
            stage: "strategy",
            candidate: Some(candidate.id()),
            reason: "ordered_candidate",
        });
    }
    let protection = protection::load(db, &key.project_id).await?;
    let mut session_request = payload.clone();
    protection::apply(&protection, &mut session_request, &context, &mut vec![])?;
    protection::tools(
        &mut session_request,
        profile.routing.allowed_tools.as_deref(),
    )?;
    protection::inject(db, &key.project_id, &mut payload, &context, endpoint).await?;
    protection::apply(&protection, &mut payload, &context, &mut decisions)?;
    protection::tools(&mut payload, profile.routing.allowed_tools.as_deref())?;
    // Until a provider tokenizer is available, UTF-8 bytes plus the requested output
    // ceiling is a conservative text reservation. Non-text inputs require an explicit
    // provider token estimate before they can use TPM-limited policies.
    let estimated_tokens = estimate_tokens(&payload)?;
    let mut plan = Plan {
        candidates,
        payload,
        session_request,
        routing: profile.routing,
        sticky,
        decisions,
        estimated_tokens,
        protection,
        context,
    };
    let mut incompatible_completion_count = false;
    let mut compatible = Vec::with_capacity(plan.candidates.len());
    for candidate in std::mem::take(&mut plan.candidates) {
        match plan.candidate_request(&candidate) {
            Ok(_) => compatible.push(candidate),
            Err(Error::Invalid(ANTHROPIC_COMPLETION_COUNT_ERROR)) => {
                incompatible_completion_count = true;
                plan.decisions.push(Decision {
                    stage: "capability",
                    candidate: Some(candidate.id()),
                    reason: "unsupported_completion_count",
                });
            }
            Err(error) => return Err(error),
        }
    }
    plan.candidates = compatible;
    if plan.candidates.is_empty() && incompatible_completion_count {
        return Err(Error::Invalid(ANTHROPIC_COMPLETION_COUNT_ERROR));
    }
    Ok(plan)
}

/// Shared by admission and protocol adapters; never infer a different output
/// ceiling after admission. Validate all supplied aliases, including shadowed ones.
pub fn output_limit(payload: &Value) -> Result<u32> {
    let mut selected = None;
    for field in ["max_completion_tokens", "max_tokens", "max_output_tokens"] {
        if let Some(value) = payload.get(field) {
            let limit = value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or(Error::Invalid(
                    "output token limit must be a positive 32-bit integer",
                ))?;
            selected.get_or_insert(limit);
        }
    }
    Ok(selected.unwrap_or(4096))
}

fn completion_count(payload: &Value) -> Result<u32> {
    payload
        .get("n")
        .map(|value| {
            value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or(Error::Invalid("n must be a positive 32-bit integer"))
        })
        .transpose()
        .map(|n| n.unwrap_or(1))
}

fn estimate_tokens(payload: &Value) -> Result<u32> {
    let output = output_limit(payload)?
        .checked_mul(completion_count(payload)?)
        .ok_or(Error::Invalid("request token reservation is too large"))?;
    let input = serde_json::to_vec(payload)
        .map_err(|_| Error::Configuration)?
        .len() as u64;
    u32::try_from(input.saturating_add(u64::from(output)))
        .map_err(|_| Error::Invalid("request token reservation is too large"))
}

impl Plan {
    fn candidate_request(&self, candidate: &Candidate) -> Result<(Value, HeaderMap)> {
        let mut payload = self.payload.clone();
        payload["model"] = Value::String(candidate.target.upstream_name.clone());
        protection::transform(&mut payload, &candidate.model_rules)?;
        let (mut payload, headers) =
            policy::overrides(&payload, &candidate.overrides, &self.context)?;
        // Overrides and injected prompts cannot bypass protection/tool restrictions.
        protection::apply(&self.protection, &mut payload, &self.context, &mut vec![])?;
        protection::tools(&mut payload, self.routing.allowed_tools.as_deref())?;
        if candidate.target.provider_kind == "anthropic" && completion_count(&payload)? != 1 {
            return Err(Error::Invalid(ANTHROPIC_COMPLETION_COUNT_ERROR));
        }
        Ok((payload, headers))
    }

    pub fn attempt_payload(&self, candidate: &Candidate) -> Result<(Value, HeaderMap, u32)> {
        let (mut payload, headers) = self.candidate_request(candidate)?;
        if self.routing.limits.tpm.is_some() || candidate.limits.tpm.is_some() {
            validate_token_reservation(&mut payload)?;
        }
        let tokens = estimate_tokens(&payload)?;
        Ok((payload, headers, tokens))
    }
}

fn validate_token_reservation(payload: &mut Value) -> Result<()> {
    fn non_text(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                map.get("type").and_then(Value::as_str).is_some_and(|kind| {
                    matches!(
                        kind,
                        "image_url"
                            | "input_image"
                            | "input_audio"
                            | "audio"
                            | "image"
                            | "file"
                            | "input_file"
                    )
                }) || map.values().any(non_text)
            }
            Value::Array(items) => items.iter().any(non_text),
            _ => false,
        }
    }
    if non_text(payload) {
        return Err(Error::Invalid(
            "TPM admission for media requires a provider token estimator",
        ));
    }
    // Enforce the reserved output ceiling at the provider boundary.
    if payload.get("max_tokens").is_none()
        && payload.get("max_completion_tokens").is_none()
        && payload.get("max_output_tokens").is_none()
    {
        let field = if payload.get("input").is_some() {
            "max_output_tokens"
        } else {
            "max_tokens"
        };
        payload[field] = serde_json::json!(4096);
    }
    Ok(())
}

pub struct AttemptGuard {
    pub channel: runtime::Permit,
    pub circuit: runtime::CircuitPermit,
    runtime: Arc<Runtime>,
    sticky: Option<String>,
    candidate: Candidate,
}

#[derive(Clone, Copy)]
pub enum AttemptOutcome {
    Success,
    UpstreamFailure,
    /// The request failed locally; do not treat it as provider health evidence.
    LocalFailure,
}

impl AttemptGuard {
    pub async fn acquire(
        runtime: Arc<Runtime>,
        candidate: &Candidate,
        sticky: Option<String>,
        tokens: u32,
    ) -> Result<Self> {
        let channel = runtime
            .admit(&candidate.resource_id(), &candidate.limits, tokens)
            .await?;
        let circuit = runtime.enter_circuit(&candidate.circuit_id(), &candidate.circuit)?;
        Ok(Self {
            channel,
            circuit,
            runtime,
            sticky,
            candidate: candidate.clone(),
        })
    }
    pub fn finish(&mut self, outcome: AttemptOutcome) {
        self.channel
            .finish(matches!(outcome, AttemptOutcome::Success));
        match outcome {
            AttemptOutcome::Success => {
                self.circuit.finish(true);
                self.runtime.bind(self.sticky.as_deref(), &self.candidate);
            }
            AttemptOutcome::UpstreamFailure => self.circuit.finish(false),
            AttemptOutcome::LocalFailure => (),
        }
    }
}

#[cfg(test)]
mod tests;
