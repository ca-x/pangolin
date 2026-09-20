use super::*;
use serde_json::json;

#[test]
fn logging_policy_matrix_and_secrets() {
    use logging::{Level::*, Policy};
    for enabled in [false, true] {
        for overrides in [false, true] {
            for disable in [false, true] {
                for key in [Inherit, Off, Metadata, RedactedBody, FullBody] {
                    let policy = Policy {
                        enabled,
                        default_level: Metadata,
                        key_override_enabled: overrides,
                        key_disable_allowed: disable,
                    };
                    let expected = if !enabled {
                        Off
                    } else if !overrides || key == Inherit || key == Off && !disable {
                        Metadata
                    } else {
                        key
                    };
                    assert_eq!(policy.resolve(key), expected);
                }
            }
        }
    }
    let document = json!({"input":"private text","headers":{"Authorization":"bearer sentinel","Cookie":"session-sentinel","x-api-key":"key-sentinel"},"api_key":"sentinel","nested":[{"client_secret":"sentinel"}]});
    for level in [RedactedBody, FullBody] {
        let body = level.body(&document).unwrap();
        assert!(!body.contains("sentinel"));
        assert_eq!(body.contains("private text"), level == FullBody);
    }
    assert!(Off.body(&document).is_none());
}
#[test]
fn axonhub_cached_and_write_cached_tokens_are_not_double_charged() {
    use pricing::*;
    let components = [
        ("input", 10),
        ("output", 20),
        ("cache_read", 1),
        ("cache_write", 12),
        ("reasoning", 30),
    ]
    .into_iter()
    .map(|(kind, price)| Component {
        id: String::new(),
        kind: kind.into(),
        unit_size: 1,
        unit_price_micros: price,
        tiers: vec![],
        cache_ttl: None,
    })
    .collect();
    let price = Price {
        id: None,
        model_id: "model".into(),
        ratio_millionths: 1_000_000,
        components,
    };
    let usage = Usage {
        input: 100,
        output: 50,
        cache_read: 40,
        cache_write: 10,
        reasoning: 20,
        reported: true,
        ..Default::default()
    };
    assert_eq!(
        price.calculate(&usage).unwrap().0,
        500 + 600 + 40 + 120 + 600
    );
    let anthropic = Usage::parse(
        &json!({"type":"message","usage":{"input_tokens":50,"cache_read_input_tokens":40,"cache_creation_input_tokens":10,"output_tokens":30}}),
    );
    assert_eq!(anthropic.input, 100);
}
#[tokio::test]
async fn job_claim_is_atomic_and_expired_worker_is_fenced() {
    let db = crate::db::connect("sqlite::memory:").await.unwrap();
    let id = jobs::enqueue(&db, None, "test", "same-operation", &json!({}), 10)
        .await
        .unwrap();
    assert_eq!(
        jobs::enqueue(&db, None, "test", "same-operation", &json!({}), 10)
            .await
            .unwrap(),
        id
    );
    let (a, b) = tokio::join!(jobs::claim(&db, "a", 10, 10), jobs::claim(&db, "b", 10, 10));
    let winner = a.unwrap().or(b.unwrap()).unwrap();
    assert!(jobs::claim(&db, "c", 19, 10).await.unwrap().is_none());
    let next = jobs::claim(&db, "c", 21, 10).await.unwrap().unwrap();
    assert!(next.fence > winner.fence);
    assert!(!jobs::finish(&db, &winner, 22, true).await.unwrap());
    assert!(jobs::finish(&db, &next, 22, true).await.unwrap());
}

#[test]
fn axonhub_price_schedule_priority_midnight_weekday_and_timezone_contracts() {
    use super::schedule::*;
    let rule = Rule {
        priority: 10,
        timezone: "America/New_York".into(),
        start_minute: 22 * 60,
        end_minute: 2 * 60,
        weekdays: vec![1],
        from: None,
        until: None,
        prices: std::collections::BTreeMap::from([("input".into(), 5)]),
    };
    let mut schedule = Schedule {
        version: 1,
        rules: vec![rule.clone()],
    };
    let utc = |text: &str| {
        chrono::DateTime::parse_from_rfc3339(text)
            .unwrap()
            .timestamp()
    };
    // Tuesday 01:00 New York belongs to Monday's overnight pricing window.
    assert_eq!(
        schedule
            .prices_at(utc("2026-09-22T05:00:00Z"))
            .unwrap()
            .unwrap()["input"],
        5
    );
    assert!(
        schedule
            .prices_at(utc("2026-09-22T07:00:00Z"))
            .unwrap()
            .is_none()
    );
    assert!(
        schedule
            .prices_at(utc("2026-09-23T05:00:00Z"))
            .unwrap()
            .is_none()
    );
    let mut override_rule = rule;
    override_rule.priority = 1;
    override_rule.prices.insert("input".into(), 2);
    override_rule.until = Some(utc("2026-09-22T05:30:00Z"));
    schedule.rules.push(override_rule);
    assert_eq!(
        schedule
            .prices_at(utc("2026-09-22T05:00:00Z"))
            .unwrap()
            .unwrap()["input"],
        2
    );
    assert_eq!(
        schedule
            .prices_at(utc("2026-09-22T05:45:00Z"))
            .unwrap()
            .unwrap()["input"],
        5
    );
}

#[test]
fn integer_cost_tiers_cache_ttls_rounding_and_overflow() {
    use super::pricing::*;
    let price = Price {
        id: None,
        model_id: "test".into(),
        ratio_millionths: 500000,
        components: vec![Component {
            id: "in".into(),
            kind: "input".into(),
            unit_size: 1,
            unit_price_micros: 10,
            tiers: vec![
                Tier {
                    up_to: Some(100),
                    unit_price_micros: 10,
                },
                Tier {
                    up_to: None,
                    unit_price_micros: 5,
                },
            ],
            cache_ttl: None,
        }],
    };
    assert_eq!(
        price
            .calculate(&Usage {
                input: 150,
                ..Default::default()
            })
            .unwrap()
            .0,
        625
    );
    let fractional = Price {
        components: vec![Component {
            id: "fraction".into(),
            kind: "input".into(),
            unit_size: 1000000,
            unit_price_micros: 1,
            tiers: vec![],
            cache_ttl: None,
        }],
        ..price.clone()
    };
    assert_eq!(
        fractional
            .calculate(&Usage {
                input: 1,
                ..Default::default()
            })
            .unwrap()
            .0,
        1
    );
    let mut overflowing = price;
    overflowing.components[0].tiers.clear();
    overflowing.components[0].unit_price_micros = i64::MAX;
    assert!(
        overflowing
            .calculate(&Usage {
                input: i64::MAX,
                ..Default::default()
            })
            .is_err()
    );
}

#[test]
fn cc_switch_nonstandard_usage_delta_corrections_and_media_log_contracts() {
    assert!(!pricing::Usage::parse(&json!({"usage":{}})).reported);
    assert!(
        !pricing::Usage::parse(&json!({"usage":{"input_tokens":10,"output_tokens":"invalid"}}))
            .reported
    );
    assert_eq!(runtime::decimal_micros("1e-6", 1_000_000).unwrap(), 1);
    assert_eq!(
        runtime::decimal_micros("1.23456789", 1_000_000).unwrap(),
        1234567
    );
    use pricing::Usage;
    let deepseek = Usage::parse(
        &json!({"usage":{"prompt_tokens":1200,"completion_tokens":40,"prompt_cache_hit_tokens":1000,"prompt_tokens_details":{"cache_write_tokens":50}}}),
    );
    assert_eq!(
        (deepseek.input, deepseek.cache_read, deepseek.cache_write),
        (1200, 1000, 50)
    );
    let codex = Usage::parse(
        &json!({"usage":{"input_tokens":1200,"output_tokens":40,"cache_read_input_tokens":1000,"cache_creation_input_tokens":50}}),
    );
    assert_eq!(codex.input, 1200);
    let gemini = Usage::parse(
        &json!({"usageMetadata":{"promptTokenCount":100,"totalTokenCount":180,"thoughtsTokenCount":30,"cachedContentTokenCount":60}}),
    );
    assert_eq!((gemini.output, gemini.reasoning), (80, 30));
    let image = Usage::parse(
        &json!({"type":"image_generation.completed","usage":{"input_tokens":1600,"input_tokens_details":{"image_tokens":1500},"output_tokens":1200,"output_tokens_details":{"image_tokens":1200}}}),
    );
    assert_eq!((image.image_input, image.image_output), (1500, 1200));
    let mut anthropic = Usage::default();
    anthropic.merge_event(&json!({"type":"message_start","message":{"usage":{"input_tokens":200000,"cache_read_input_tokens":180000,"cache_creation_input_tokens":2000}}}),true);
    anthropic.merge_event(&json!({"type":"message_delta","usage":{"input_tokens":80000,"cache_read_input_tokens":120000,"cache_creation_input_tokens":500,"output_tokens":1000}}),true);
    assert_eq!(
        (
            anthropic.input,
            anthropic.cache_read,
            anthropic.cache_write,
            anthropic.output
        ),
        (200500, 120000, 500, 1000)
    );
    let cached = Usage::parse_for(
        &json!({"usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":500}}),
        true,
    );
    assert!(cached.reported);
    assert_eq!(cached.input, 500);
    let body = json!({"messages":[{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"binary-sentinel"}},{"type":"image_url","image_url":{"url":"data:image/png;base64,another-sentinel"}}]}],"contents":[{"parts":[{"inlineData":{"mimeType":"image/png","data":"gemini-sentinel"}}]}],"b64_json":"output-sentinel","tool_output":json!({"source":{"type":"base64","data":"tool-sentinel"}}).to_string(),"download":"https://user:password@example.com/image?signature=secret"});
    let sanitized = logging::Level::FullBody.body(&body).unwrap();
    assert!(!sanitized.contains("sentinel"));
    assert!(!sanitized.contains("password"));
    assert!(!sanitized.contains("signature"));
    assert!(body.to_string().contains("binary-sentinel"));
}
