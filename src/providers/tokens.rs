//! Optional provider-matched tokenizer files are operator configuration, never
//! request fields. Unsupported shapes retain the conservative byte reservation.
use litellm_cache::{BaseCache, CacheEntry};
use litellm_cache_memory::InMemoryCache;
use litellm_token_counter::{CountableRequest, TokenCounter};
use serde_json::Value;
use std::sync::OnceLock;

static COUNTER: OnceLock<Option<TokenCounter>> = OnceLock::new();
static CACHE: OnceLock<InMemoryCache<CacheEntry>> = OnceLock::new();

pub fn input_tokens(kind: &str, model: &str, payload: &Value) -> Option<u32> {
    if !matches!(kind, "openai" | "azure")
        || !(model == "gpt-4" || model.starts_with("gpt-4-") || model.starts_with("gpt-3.5-turbo"))
    {
        return None;
    }
    let counter = COUNTER
        .get_or_init(|| {
            let path = std::env::var("PANGOLIN_TOKENIZER_CL100K").ok()?;
            let data = std::fs::read_to_string(path).ok()?;
            TokenCounter::from_cl100k_ranks(&data).ok()
        })
        .as_ref()?;
    if payload.as_object()?.keys().any(|key| {
        !matches!(
            key.as_str(),
            "model"
                | "messages"
                | "tools"
                | "tool_choice"
                | "stream"
                | "temperature"
                | "top_p"
                | "max_tokens"
                | "max_completion_tokens"
                | "n"
                | "stop"
        )
    }) {
        return None;
    }
    let body = serde_json::to_vec(payload).ok()?;
    let key = blake3::hash(&body).to_hex().to_string();
    let cache = CACHE
        .get_or_init(|| InMemoryCache::new(Some(256), Some(std::time::Duration::from_secs(60))));
    let interface: &dyn BaseCache<Value = CacheEntry> = cache;
    if let Some(count) = interface.get_cache(&key, &Default::default()).ok()? {
        return count.response.as_u64().and_then(|n| u32::try_from(n).ok());
    }
    let request = CountableRequest::parse(&body).ok()?;
    let count = u32::try_from(counter.count_request(&request).ok()?.input_tokens).ok()?;
    interface
        .set_cache(
            &key,
            CacheEntry {
                timestamp: crate::db::now() as f64,
                response: serde_json::json!(count),
            },
            Default::default(),
        )
        .ok()?;
    Some(count)
}
