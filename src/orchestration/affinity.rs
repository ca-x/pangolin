//! Rule-driven affinity. Identifiers are scoped and hashed before entering a cache.
use super::{Candidate, Error, Result};
use crate::providers::diagnostics::{AffinityDiagnostic, AffinityReason, safe_identity};
use crate::{db, models::ApiKeyCredential};
use litellm_cache::{BaseCache, CacheEntry, CacheKwargs};
use litellm_cache_memory::InMemoryCache;
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Off,
    #[default]
    Prefer,
    Strict,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Source {
    Header(String),
    Pointer(String),
    Trace,
    Thread,
    Session,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub mode: Mode,
    pub source: Source,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    pub ttl_secs: u64,
    #[serde(default = "yes")]
    pub release_on_failure: bool,
}
impl Rule {
    pub fn validate(&self) -> Result<()> {
        if self.ttl_secs == 0 || self.ttl_secs > 86400 || self.id.is_empty() || self.id.len() > 128
        {
            return Err(Error::Configuration);
        }
        for pattern in [&self.model, &self.path, &self.user_agent]
            .into_iter()
            .flatten()
        {
            super::policy::regex(pattern)?;
        }
        match &self.source {
            Source::Header(name) if name != crate::api::TRUSTED_CLIENT_IP_HEADER => Err(
                Error::Invalid("affinity headers must be middleware-trusted"),
            ),
            Source::Pointer(pointer) if !pointer.starts_with('/') || pointer.len() > 256 => {
                Err(Error::Configuration)
            }
            _ => Ok(()),
        }
    }
}
fn yes() -> bool {
    true
}
#[derive(Clone)]
pub struct Binding {
    pub fingerprint: String,
    pub rule_id: String,
    pub diagnostic: AffinityDiagnostic,
    sequence: u64,
    completion_guard: Option<Arc<tokio::sync::Mutex<u64>>>,
    pub ttl_secs: u64,
    pub release_on_failure: bool,
}
pub struct Cache {
    backend: Arc<dyn BaseCache<Value = CacheEntry>>,
    local: Option<Arc<InMemoryCache<CacheEntry>>>,
    sequence: std::sync::atomic::AtomicU64,
    guard_limit: usize,
    pub hits: std::sync::atomic::AtomicU64,
    generation: std::sync::atomic::AtomicU64,
    project_generations: std::sync::Mutex<std::collections::HashMap<String, u64>>,
    switches: std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<u64>>>,
    >,
}
impl Default for Cache {
    fn default() -> Self {
        let local = Arc::new(InMemoryCache::new(
            Some(10000),
            Some(Duration::from_secs(1800)),
        ));
        let backend: Arc<dyn BaseCache<Value = CacheEntry>> = local.clone();
        let local = Some(local);
        #[cfg(feature = "redis-affinity")]
        let (backend, local) = std::env::var("PANGOLIN_AFFINITY_REDIS_URL")
            .ok()
            .and_then(|url| {
                litellm_cache_redis::RedisCache::new(&url, Some(Duration::from_secs(1800))).ok()
            })
            .map(|v| (Arc::new(v) as Arc<dyn BaseCache<Value = CacheEntry>>, None))
            .unwrap_or((backend, local));
        Self {
            backend,
            local,
            sequence: Default::default(),
            guard_limit: 10000,
            hits: Default::default(),
            generation: Default::default(),
            project_generations: Default::default(),
            switches: Default::default(),
        }
    }
}
impl Cache {
    #[cfg(test)]
    pub fn with_test_clock(clock: impl Fn() -> Duration + Send + Sync + 'static) -> Self {
        let local = Arc::new(InMemoryCache::with_clock(
            Some(10000),
            Some(Duration::from_secs(1800)),
            clock,
        ));
        Self {
            backend: local.clone(),
            local: Some(local),
            ..Self::default()
        }
    }
    #[cfg(test)]
    pub fn with_test_completion_capacity(capacity: usize) -> Self {
        Self {
            guard_limit: capacity,
            ..Self::with_test_clock(|| Duration::from_secs(1000))
        }
    }
    #[cfg(test)]
    pub fn run_test_capacity_tasks(&self) {
        let mut guards = self.switches.lock().unwrap();
        guards.retain(|_, guard| guard.strong_count() > 0);
        assert!(guards.len() <= self.guard_limit);
    }
    pub fn diagnostic_counts(&self) -> (u64, u64) {
        (
            self.hits.load(std::sync::atomic::Ordering::Relaxed),
            self.project_generations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len() as u64,
        )
    }

    pub fn reset(&self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.project_generations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
    pub fn reset_project(&self, project: &str) {
        let mut generations = self
            .project_generations
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *generations.entry(project.into()).or_default() += 1;
    }
    fn generation_for(&self, project: &str) -> (u64, u64) {
        (
            self.generation.load(std::sync::atomic::Ordering::Relaxed),
            *self
                .project_generations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(project)
                .unwrap_or(&0),
        )
    }
    fn switch_lock(&self, key: &str) -> Option<Arc<tokio::sync::Mutex<u64>>> {
        let mut locks = self.switches.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(lock) = locks.get(key).and_then(std::sync::Weak::upgrade) {
            return Some(lock);
        }
        if locks.len() >= self.guard_limit {
            locks.retain(|_, v| v.strong_count() > 0);
            if locks.len() >= self.guard_limit {
                return None;
            }
        }
        let lock = Arc::new(tokio::sync::Mutex::new(0));
        locks.insert(key.into(), Arc::downgrade(&lock));
        Some(lock)
    }
    pub async fn select(
        &self,
        db: &DatabaseConnection,
        key: &ApiKeyCredential,
        headers: &http::HeaderMap,
        body: &Value,
        path: &str,
        candidates: &mut Vec<Candidate>,
    ) -> Result<Option<Binding>> {
        let row = db
            .query_one(super::repository::statement(
                "SELECT settings_json FROM projects WHERE id=?",
                vec![key.project_id.clone().into()],
            ))
            .await?
            .ok_or(Error::Forbidden)?;
        let settings: Value = serde_json::from_str(&row.try_get::<String>("", "settings_json")?)
            .map_err(|_| Error::Configuration)?;
        let rules: Vec<Rule> = if let Some(rules) = settings.get("affinity_rules") {
            serde_json::from_value(rules.clone()).map_err(|_| Error::Configuration)?
        } else {
            vec![
                Rule {
                    id: "codex".into(),
                    mode: Mode::Prefer,
                    source: Source::Pointer("/prompt_cache_key".into()),
                    model: None,
                    path: Some("^/v1/responses".into()),
                    user_agent: None,
                    ttl_secs: 1800,
                    release_on_failure: true,
                },
                Rule {
                    id: "claude".into(),
                    mode: Mode::Prefer,
                    source: Source::Pointer("/metadata/user_id".into()),
                    model: None,
                    path: Some("^/v1/messages$".into()),
                    user_agent: None,
                    ttl_secs: 1800,
                    release_on_failure: true,
                },
            ]
        };
        if rules.len() > 64 {
            return Err(Error::Configuration);
        }
        for rule in rules {
            rule.validate()?;
            let mut matched = true;
            for (pattern, value) in [
                (&rule.model, body["model"].as_str().unwrap_or("")),
                (&rule.path, path),
                (
                    &rule.user_agent,
                    headers
                        .get("user-agent")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or(""),
                ),
            ] {
                if let Some(pattern) = pattern {
                    matched &= super::policy::regex(pattern)?.is_match(value)
                }
            }
            if !matched {
                continue;
            }
            if rule.mode == Mode::Off {
                return Ok(None);
            }
            let value = match &rule.source {
                Source::Header(name) => {
                    if name != crate::api::TRUSTED_CLIENT_IP_HEADER {
                        return Err(Error::Invalid(
                            "affinity headers must be middleware-trusted",
                        ));
                    }
                    headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned)
                }
                Source::Pointer(pointer) => {
                    if !pointer.starts_with('/') || pointer.len() > 256 {
                        return Err(Error::Configuration);
                    }
                    body.pointer(pointer)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }
                Source::Trace | Source::Thread | Source::Session => headers
                    .get(match rule.source {
                        Source::Trace => "x-trace-id",
                        Source::Thread => "x-thread-id",
                        _ => "x-session-id",
                    })
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned),
            };
            let Some(value) = value.filter(|v| !v.is_empty() && v.len() <= 1024) else {
                continue;
            };
            let fingerprint = blake3::hash(
                json!([
                    self.generation_for(&key.project_id),
                    key.project_id,
                    key.id,
                    rule.id,
                    body["model"],
                    path,
                    value
                ])
                .to_string()
                .as_bytes(),
            )
            .to_hex()
            .to_string();
            // Snapshot the local cache's own expiry before get removes an expired
            // entry. Redis does not expose this observation through BaseCache;
            // do not fabricate an expiry when its backend cannot measure one.
            let expires_at = self.expires_at(&fingerprint);
            let mut diagnostic = AffinityDiagnostic {
                rule_id: safe_identity(&rule.id),
                scope_digest: fingerprint.clone(),
                reason: AffinityReason::First,
                provider_id: None,
                expires_at: None,
            };
            if let Ok(Some(entry)) = self
                .backend
                .async_get_cache(&fingerprint, &CacheKwargs::default())
                .await
            {
                if let Some(index) = candidates.iter().position(|c| entry.response == c.id()) {
                    diagnostic.reason = AffinityReason::Hit;
                    diagnostic.provider_id = Some(safe_identity(&candidates[index].provider_id));
                    diagnostic.expires_at = expires_at;
                    candidates[..=index].rotate_right(1);
                    if rule.mode == Mode::Strict {
                        candidates.truncate(1);
                    }
                    self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                } else {
                    // Authoritative eligibility overrides every cache observation.
                    diagnostic.reason = AffinityReason::Ineligible;
                    let _ = self.backend.async_delete_cache(&fingerprint).await;
                    if rule.mode == Mode::Strict {
                        tracing::debug!(
                            stage = "affinity",
                            reason = "ineligible",
                            "routing decision"
                        );
                        return Err(Error::Admission("affinity_channel_unavailable"));
                    }
                }
            } else if expires_at.is_some_and(|expiry| expiry <= db::now()) {
                diagnostic.reason = AffinityReason::Expired;
                diagnostic.expires_at = expires_at;
            }
            return Ok(Some(Binding {
                rule_id: diagnostic.rule_id.clone(),
                completion_guard: self.switch_lock(&fingerprint),
                fingerprint,
                diagnostic,
                sequence: self
                    .sequence
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1,
                ttl_secs: rule.ttl_secs,
                release_on_failure: rule.release_on_failure,
            }));
        }
        Ok(None)
    }
    fn expires_at(&self, key: &str) -> Option<i64> {
        self.local
            .as_ref()?
            .expires_at(key)
            .ok()
            .flatten()
            .and_then(|expiry| i64::try_from(expiry.as_secs()).ok())
    }
    pub async fn finish(
        &self,
        binding: &Binding,
        candidate: &str,
        provider: &str,
        success: bool,
    ) -> Vec<AffinityDiagnostic> {
        let event = |reason, expires_at| AffinityDiagnostic {
            rule_id: binding.rule_id.clone(),
            scope_digest: binding.fingerprint.clone(),
            reason,
            provider_id: Some(safe_identity(provider)),
            expires_at,
        };
        let mut diagnostics = if success {
            vec![]
        } else {
            vec![event(AffinityReason::CandidateFailed, None)]
        };
        // Every in-flight binding retains this scope's ordering state, including
        // when the backend evicts/expires its ordinary binding. A bounded weak
        // registry never evicts a live guard; absent safety state means no mutation.
        let Some(lock) = binding.completion_guard.clone() else {
            return diagnostics;
        };
        let Ok(mut completed) =
            tokio::time::timeout(Duration::from_secs(2), lock.lock_owned()).await
        else {
            return diagnostics;
        };
        let current = self
            .backend
            .async_get_cache(&binding.fingerprint, &CacheKwargs::default())
            .await
            .ok()
            .flatten();
        // Sequence ordering is process-local, not a distributed-consistency claim.
        if *completed > binding.sequence {
            return diagnostics;
        }
        if success
            && current
                .as_ref()
                .is_some_and(|entry| entry.response == candidate)
        {
            *completed = binding.sequence;
            return diagnostics;
        }
        if success {
            if self
                .backend
                .async_set_cache(
                    &binding.fingerprint,
                    CacheEntry {
                        timestamp: db::now() as f64,
                        response: json!(candidate),
                    },
                    CacheKwargs {
                        ttl: Some(Duration::from_secs(binding.ttl_secs)),
                        ..Default::default()
                    },
                )
                .await
                .is_ok()
            {
                *completed = binding.sequence;
                diagnostics.push(event(
                    AffinityReason::Established,
                    self.expires_at(&binding.fingerprint),
                ));
            }
        } else if binding.release_on_failure
            && current
                .as_ref()
                .is_some_and(|entry| entry.response == candidate)
            && self
                .backend
                .async_delete_cache(&binding.fingerprint)
                .await
                .is_ok()
        {
            *completed = binding.sequence;
            diagnostics.push(event(AffinityReason::Released, None));
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_reset_does_not_invalidate_other_project_generation() {
        let cache = Cache::default();
        let a = cache.generation_for("a");
        let b = cache.generation_for("b");
        cache.reset_project("a");
        assert_ne!(cache.generation_for("a"), a);
        assert_eq!(cache.generation_for("b"), b);
        cache.reset();
        assert_ne!(cache.generation_for("b"), b);
    }
    #[tokio::test]
    async fn cc_switch_scoped_switches_are_singleflight_and_old_failures_do_not_erase_new_success()
    {
        let cache = Arc::new(Cache::default());
        let binding = Binding {
            fingerprint: blake3::hash(b"hashed-scope").to_hex().to_string(),
            rule_id: "test".into(),
            diagnostic: AffinityDiagnostic {
                rule_id: "test".into(),
                scope_digest: blake3::hash(b"hashed-scope").to_hex().to_string(),
                reason: AffinityReason::First,
                provider_id: None,
                expires_at: None,
            },
            sequence: 1,
            completion_guard: cache.switch_lock(blake3::hash(b"hashed-scope").to_hex().as_ref()),
            ttl_secs: 60,
            release_on_failure: true,
        };
        let mut tasks = vec![];
        for _ in 0..64 {
            let cache = cache.clone();
            let binding = binding.clone();
            tasks.push(tokio::spawn(async move {
                cache.finish(&binding, "a", "provider-a", true).await
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        cache.finish(&binding, "b", "provider-b", true).await;
        cache.finish(&binding, "a", "provider-a", false).await;
        assert_eq!(
            cache
                .backend
                .async_get_cache(&binding.fingerprint, &CacheKwargs::default())
                .await
                .unwrap()
                .unwrap()
                .response,
            "b"
        );
        cache.finish(&binding, "b", "provider-b", false).await;
        assert!(
            cache
                .backend
                .async_get_cache(&binding.fingerprint, &CacheKwargs::default())
                .await
                .unwrap()
                .is_none()
        );
    }
}
