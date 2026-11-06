use base64::{Engine, engine::general_purpose::STANDARD};
use rustc_hash::FxHashMap;
use serde_json::Value;
use std::{collections::HashSet, io::Read, sync::OnceLock};
use tiktoken_rs::CoreBPE;

static COUNTER: OnceLock<Option<CoreBPE>> = OnceLock::new();
static CACHE: OnceLock<moka::sync::Cache<String, u32>> = OnceLock::new();

pub fn input_tokens(kind: &str, model: &str, payload: &Value) -> Option<u32> {
    if !matches!(kind, "openai" | "azure")
        || !(model == "gpt-4" || model.starts_with("gpt-4-") || model.starts_with("gpt-3.5-turbo"))
    {
        return None;
    }
    let counter = COUNTER.get_or_init(load_counter).as_ref()?;
    let key = blake3::hash(&serde_json::to_vec(&serde_json::json!([model, payload])).ok()?)
        .to_hex()
        .to_string();
    let cache = CACHE.get_or_init(|| {
        moka::sync::Cache::builder()
            .max_capacity(256)
            .time_to_live(std::time::Duration::from_secs(60))
            .build()
    });
    if let Some(count) = cache.get(&key) {
        return Some(count);
    }
    let count = count_text(counter, model, payload)?;
    cache.insert(key, count);
    Some(count)
}

fn load_counter() -> Option<CoreBPE> {
    let path = std::env::var("PANGOLIN_TOKENIZER_CL100K").ok()?;
    let mut source = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(4 * 1024 * 1024 + 1)
        .read_to_string(&mut source)
        .ok()?;
    if source.len() > 4 * 1024 * 1024 {
        return None;
    }
    parse_counter(&source)
}

fn parse_counter(source: &str) -> Option<CoreBPE> {
    let mut encoder = FxHashMap::default();
    let mut ranks = HashSet::new();
    for line in source.lines() {
        let mut parts = line.split_whitespace();
        let bytes = STANDARD.decode(parts.next()?).ok()?;
        let rank = parts.next()?.parse::<u32>().ok()?;
        if parts.next().is_some()
            || bytes.is_empty()
            || !ranks.insert(rank)
            || encoder.insert(bytes, rank).is_some()
        {
            return None;
        }
    }
    if !(0..=255).all(|byte| encoder.contains_key(&vec![byte])) {
        return None;
    }
    let special = [
        ("<|endoftext|>", 100257),
        ("<|fim_prefix|>", 100258),
        ("<|fim_middle|>", 100259),
        ("<|fim_suffix|>", 100260),
        ("<|endofprompt|>", 100276),
    ]
    .into_iter()
    .map(|(token, rank)| (token.to_owned(), rank))
    .collect();
    CoreBPE::new(encoder, special,
        "'(?i:[sdmt]|ll|ve|re)|[^\\r\\n\\p{L}\\p{N}]?+\\p{L}++|\\p{N}{1,3}+| ?[^\\s\\p{L}\\p{N}]++[\\r\\n]*+|\\s++$|\\s*[\\r\\n]|\\s+(?!\\S)|\\s",
    ).ok()
}

fn count_text(counter: &CoreBPE, model: &str, payload: &Value) -> Option<u32> {
    if payload.as_object()?.keys().any(|key| {
        !matches!(
            key.as_str(),
            "model"
                | "messages"
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
    let legacy = model == "gpt-3.5-turbo-0301";
    let mut count = 3_u32;
    for message in payload.get("messages")?.as_array()? {
        if message
            .as_object()?
            .keys()
            .any(|key| !matches!(key.as_str(), "role" | "content" | "name"))
        {
            return None;
        }
        let role = message.get("role")?.as_str()?;
        if !matches!(role, "system" | "user" | "assistant") {
            return None;
        }
        let content = message.get("content")?.as_str()?;
        count = count.checked_add(if legacy { 4 } else { 3 })?;
        for text in [role, content] {
            count = count.checked_add(u32::try_from(counter.encode_ordinary(text).len()).ok()?)?;
        }
        if let Some(name) = message.get("name") {
            count = count
                .checked_add(u32::try_from(counter.encode_ordinary(name.as_str()?).len()).ok()?)?;
            count = if legacy {
                count.checked_sub(1)?
            } else {
                count.checked_add(1)?
            };
        }
    }
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn configured_text_counts_never_invent_multimodal_or_tool_measurements() {
        let counter = tiktoken_rs::cl100k_base().unwrap();
        assert_eq!(
            count_text(
                &counter,
                "gpt-4",
                &json!({"messages":[{"role":"user","content":"hello"}]})
            ),
            Some(8)
        );
        assert!(count_text(&counter, "gpt-4", &json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.test/image"}}]}]})).is_none());
        assert!(count_text(&counter, "gpt-4", &json!({"messages":[],"tools":[]})).is_none());
        assert!(parse_counter("YQ== 1\nYg== 1").is_none());
        assert!(parse_counter("YQ== 1").is_none());
    }
}
