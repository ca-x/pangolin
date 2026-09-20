//! Rule-driven affinity. Identifiers are scoped and hashed before entering a cache.
use super::{Candidate, Error, Result};
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
    pub ttl_secs: u64,
    pub release_on_failure: bool,
}
pub struct Cache {
    backend: Arc<dyn BaseCache<Value = CacheEntry>>,
    pub hits: std::sync::atomic::AtomicU64,
    generation: std::sync::atomic::AtomicU64,
    switches: std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>,
    >,
}
impl Default for Cache {
    fn default() -> Self {
        let backend: Arc<dyn BaseCache<Value = CacheEntry>> = Arc::new(InMemoryCache::new(
            Some(10000),
            Some(Duration::from_secs(1800)),
        ));
        #[cfg(feature = "redis-affinity")]
        let backend = std::env::var("PANGOLIN_AFFINITY_REDIS_URL")
            .ok()
            .and_then(|url| {
                litellm_cache_redis::RedisCache::new(&url, Some(Duration::from_secs(1800))).ok()
            })
            .map(|v| Arc::new(v) as Arc<dyn BaseCache<Value = CacheEntry>>)
            .unwrap_or(backend);
        Self {
            backend,
            hits: Default::default(),
            generation: Default::default(),
            switches: Default::default(),
        }
    }
}
impl Cache {
    pub fn reset(&self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn switch_lock(&self, key: &str) -> Option<Arc<tokio::sync::Mutex<()>>> {
        let mut locks = self.switches.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(lock) = locks.get(key).and_then(std::sync::Weak::upgrade) {
            return Some(lock);
        }
        if locks.len() >= 10000 {
            locks.retain(|_, v| v.strong_count() > 0);
            if locks.len() >= 10000 {
                return None;
            }
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
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
                    self.generation.load(std::sync::atomic::Ordering::Relaxed),
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
            if let Ok(Some(entry)) = self
                .backend
                .async_get_cache(&fingerprint, &CacheKwargs::default())
                .await
            {
                if let Some(index) = candidates.iter().position(|c| entry.response == c.id()) {
                    candidates[..=index].rotate_right(1);
                    if rule.mode == Mode::Strict {
                        candidates.truncate(1)
                    }
                    self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                } else {
                    // Authoritative eligibility always overrides cache state, including explicit disable.
                    let _ = self.backend.async_delete_cache(&fingerprint).await;
                    if rule.mode == Mode::Strict {
                        return Err(Error::Admission("affinity_channel_unavailable"));
                    }
                }
            }
            return Ok(Some(Binding {
                fingerprint,
                ttl_secs: rule.ttl_secs,
                release_on_failure: rule.release_on_failure,
            }));
        }
        Ok(None)
    }
    pub async fn finish(&self, binding: &Binding, candidate: &str, success: bool) {
        let Some(lock) = self.switch_lock(&binding.fingerprint) else {
            return;
        };
        let Ok(_guard) = tokio::time::timeout(Duration::from_secs(2), lock.lock_owned()).await
        else {
            return;
        };
        let current = self
            .backend
            .async_get_cache(&binding.fingerprint, &CacheKwargs::default())
            .await
            .ok()
            .flatten();
        if success
            && current
                .as_ref()
                .is_some_and(|entry| entry.response == candidate)
        {
            return;
        }
        if success {
            let _ = self
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
                .await;
        } else if binding.release_on_failure
            && current
                .as_ref()
                .is_some_and(|entry| entry.response == candidate)
        {
            let _ = self.backend.async_delete_cache(&binding.fingerprint).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cc_switch_scoped_switches_are_singleflight_and_old_failures_do_not_erase_new_success()
    {
        let cache = Arc::new(Cache::default());
        let binding = Binding {
            fingerprint: "hashed-scope".into(),
            ttl_secs: 60,
            release_on_failure: true,
        };
        let mut tasks = vec![];
        for _ in 0..64 {
            let cache = cache.clone();
            let binding = binding.clone();
            tasks.push(tokio::spawn(async move {
                cache.finish(&binding, "a", true).await
            }));
        }
        for task in tasks {
            task.await.unwrap()
        }
        cache.finish(&binding, "b", true).await;
        cache.finish(&binding, "a", false).await;
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
        cache.finish(&binding, "b", false).await;
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
