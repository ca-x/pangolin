use super::*;

#[test]
fn provider_quota_shapes_normalize_and_unknown_stays_unmeasured() {
    let cases = [
        (
            json!({"data":{"limit_remaining":"12.5","reset_at":200}}),
            12_500_000,
            Some(200),
        ),
        (
            json!({"remaining":7,"period":"weekly","period_end":300}),
            7_000_000,
            Some(300),
        ),
        (
            json!({"usage":{"used":3,"limit":10},"window":"monthly"}),
            7_000_000,
            None,
        ),
    ];
    for (shape, remaining, end) in cases {
        let normalized = runtime::normalize_quota(
            &shape,
            &json!({}),
            "https://quota-user:quota-password@quota.example/status",
        )
        .unwrap();
        assert_eq!(normalized.remaining_micros, Some(remaining));
        assert_eq!(normalized.period_end, end);
        assert_eq!(normalized.document["measured"], true);
        assert_eq!(
            normalized.document["source_url"],
            "https://quota.example/status"
        );
        assert!(!normalized.document.to_string().contains("quota-password"));
    }
    let unknown = runtime::normalize_quota(
        &json!({"vendor":{"mystery":true}}),
        &json!({}),
        "https://quota.example/status",
    )
    .unwrap();
    assert_eq!(unknown.remaining_micros, None);
    assert_eq!(unknown.document["measured"], false);
    assert_eq!(unknown.document["status"], "unknown");
}
use serde_json::json;

#[test]
fn logging_policy_matrix_and_secrets() {
    use logging::{Level::*, Policy};
    for enabled in [false, true] {
        for overrides in [false, true] {
            for disable in [false, true] {
                for key in [Inherit, Off, Metadata, RedactedBody, FullBody] {
                    let policy = Policy {
                        version: logging::CURRENT_POLICY_VERSION,
                        enabled,
                        default_level: Metadata,
                        key_override_enabled: overrides,
                        key_disable_allowed: disable,
                        live_preview_enabled: false,
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
fn request_logging_policy_version_contract_round_trips_and_refuses_unknown_versions() {
    use logging::{
        CURRENT_POLICY_VERSION, Level, Policy, PolicyInput, StoredPolicyError, parse_stored,
    };

    let versioned = json!({
        "version": CURRENT_POLICY_VERSION,
        "enabled": false,
        "default_level": "redacted_body",
        "key_override_enabled": true,
        "key_disable_allowed": true,
    });
    let policy: Policy = PolicyInput::parse(versioned.clone()).unwrap().into();
    assert_eq!(serde_json::to_value(&policy).unwrap(), versioned);

    let defaulted: Policy = PolicyInput::parse(json!({})).unwrap().into();
    assert_eq!(defaulted.version, CURRENT_POLICY_VERSION);
    assert_eq!(defaulted.default_level, Level::Metadata);

    let legacy = parse_stored(
        &json!({
            "enabled": true,
            "default_level": "off",
            "key_override_enabled": false,
            "key_disable_allowed": false,
        })
        .to_string(),
    )
    .expect("the unversioned legacy shape is upgraded to version 1");
    assert_eq!(legacy.version, CURRENT_POLICY_VERSION);
    assert_eq!(legacy.default_level, Level::Off);

    let tolerant = parse_stored(
        &json!({
            "version": CURRENT_POLICY_VERSION,
            "enabled": true,
            "default_level": "full_body",
            "key_override_enabled": true,
            "key_disable_allowed": true,
            "future_field": {"kept_by_a_newer_build": true},
        })
        .to_string(),
    )
    .expect("unknown stored fields are tolerated within a supported version");
    assert_eq!(tolerant.resolve(Level::Inherit), Level::FullBody);

    let error = PolicyInput::parse(json!({"version": 2})).unwrap_err();
    let (status, kind, _) = error.public_parts();
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(kind, "invalid_request_error");
    assert_eq!(
        parse_stored(r#"{"version":2,"enabled":true}"#),
        Err(StoredPolicyError::UnsupportedVersion),
    );
    assert_eq!(
        parse_stored(r#"{"version":"one","enabled":true}"#),
        Err(StoredPolicyError::Malformed),
    );
    assert_eq!(
        parse_stored(r#"{"version":1,"default_level":"inherit"}"#),
        Err(StoredPolicyError::Malformed),
        "inherit is a key-level choice, never a valid stored site default",
    );
}

/// The write contract and the stored/runtime contract are different by design:
/// the same document is rejected at the boundary and tolerated in the record
/// system. Both halves are asserted here so neither can be "fixed" into the
/// other.
#[test]
fn the_request_logging_write_contract_is_strict_while_the_stored_policy_tolerates() {
    use logging::{Level, Policy, PolicyInput};

    // Accepted, and converted only after validation. An omitted field keeps the
    // policy's documented default, so `enabled` stays true.
    let parsed: Policy = PolicyInput::parse(json!({"default_level":"off"}))
        .unwrap()
        .into();
    assert!(parsed.enabled);
    assert_eq!(parsed.default_level, Level::Off);
    assert!(!parsed.key_override_enabled);
    assert!(!parsed.key_disable_allowed);
    assert_eq!(parsed.resolve(Level::FullBody), Level::Off);

    let full = json!({"version":logging::CURRENT_POLICY_VERSION,"enabled":false,"default_level":"redacted_body","key_override_enabled":true,"key_disable_allowed":true});
    let parsed: Policy = PolicyInput::parse(full.clone()).unwrap().into();
    assert_eq!(
        serde_json::to_value(&parsed).unwrap(),
        full,
        "an accepted document round-trips unchanged"
    );

    // Rejected: the shapes an operator typo takes. `[]` matters because serde's
    // derived struct visitor reads a positional sequence as a struct, so it used
    // to install an all-default policy.
    for rejected in [
        json!({"enabledd": false}),
        json!({"default_level":"verbose"}),
        json!({"default_level":"Metadata"}),
        json!({"default_level":3}),
        json!({"enabled":"no"}),
        json!({"key_disable_allowed":null}),
        json!([]),
        json!([true, "off"]),
        json!("off"),
        json!(null),
    ] {
        let error = PolicyInput::parse(rejected.clone()).unwrap_err();
        let (status, kind, message) = error.public_parts();
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{rejected}");
        assert_eq!(kind, "invalid_request_error", "{rejected}");
        assert!(message.contains("request-logging"), "{rejected} {message}");
        assert!(
            !message.contains("unknown field") && !message.contains("line "),
            "no serde cause may reach the client: {rejected} {message}"
        );
    }
    let defaulted: Policy = PolicyInput::parse(json!({})).unwrap().into();
    assert_eq!(
        serde_json::to_value(&defaulted).unwrap(),
        serde_json::to_value(Policy::default()).unwrap(),
        "an empty document is the documented default policy, not an all-false one"
    );

    // The stored/runtime policy stays tolerant: an unknown field must not fail
    // resolution on the admission path.
    let stored = logging::parse_stored(
        &json!({
            "version": logging::CURRENT_POLICY_VERSION,
            "enabled": true,
            "default_level": "full_body",
            "key_override_enabled": true,
            "key_disable_allowed": true,
            "retained_from_an_older_build": true,
        })
        .to_string(),
    )
    .expect("a stored document with a forward-compatible field must still parse");
    assert_eq!(
        stored.resolve(Level::Inherit),
        Level::FullBody,
        "the known fields of a stored document still decide the effective level"
    );
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
        tier_mode: TierMode::Marginal,
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
            tier_mode: TierMode::Marginal,
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
            tier_mode: TierMode::Marginal,
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
fn b36_pricing_volume_tiers_charge_every_unit_at_the_matched_rate() {
    use super::pricing::*;

    let component = |tier_mode| Component {
        id: "input".into(),
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
        tier_mode,
        cache_ttl: None,
    };
    let price = |tier_mode| Price {
        id: None,
        model_id: "tiered".into(),
        ratio_millionths: 1_000_000,
        components: vec![component(tier_mode)],
    };

    let marginal = price(TierMode::Marginal);
    let volume = price(TierMode::Volume);
    let usage = |input| Usage {
        input,
        ..Default::default()
    };

    assert_eq!(marginal.calculate(&usage(100)).unwrap().0, 1_000);
    assert_eq!(marginal.calculate(&usage(101)).unwrap().0, 1_005);
    assert_eq!(volume.calculate(&usage(100)).unwrap().0, 1_000);
    assert_eq!(volume.calculate(&usage(101)).unwrap().0, 505);
    assert_eq!(
        volume
            .upper_bound(150, &json!({}), "/v1/chat/completions")
            .unwrap(),
        750
    );
}

#[tokio::test]
async fn b37_pricing_channel_scope_wins_only_for_the_matching_channel() {
    use crate::{
        models::RouteTarget,
        orchestration::{
            Candidate,
            policy::{CircuitPolicy, Limits, Retry},
        },
    };
    use sea_orm::ConnectionTrait;

    let db = crate::db::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared(
        "INSERT INTO providers(id,project_id,name,kind,base_url,enabled,created_at,updated_at) VALUES
         ('price-channel-a','00000000-0000-0000-0000-000000000001','A','openai','https://a.invalid',1,0,0),
         ('price-channel-b','00000000-0000-0000-0000-000000000001','B','openai','https://b.invalid',1,0,0);
         INSERT INTO models(id,provider_id,public_name,upstream_name,created_at) VALUES
         ('price-model','price-channel-a','public','upstream',0);
         INSERT INTO model_prices(id,model_id,provider_id,version,valid_from,created_at) VALUES
         ('price-global','price-model',NULL,1,0,0),
         ('price-channel','price-model','price-channel-a',1,0,0);
         INSERT INTO model_price_components(id,price_id,kind,unit_size,unit_price_micros) VALUES
         ('component-global','price-global','input',1,10),
         ('component-channel','price-channel','input',1,3);",
    )
    .await
    .unwrap();
    let candidate = |provider_id: &str| Candidate {
        target: RouteTarget {
            public_name: "public".into(),
            upstream_name: "upstream".into(),
            provider_name: provider_id.into(),
            provider_kind: "openai".into(),
            base_url: "https://example.invalid".into(),
            credential_type: "api_key".into(),
            secret_envelope: String::new(),
            proxy_url: None,
            proxy_username: None,
            proxy_secret_envelope: None,
            proxy_reuse_connections: true,
            proxy_preset_id: None,
            input_price_micros: 0,
            output_price_micros: 0,
        },
        provider_id: provider_id.into(),
        model_id: "price-model".into(),
        credential_id: "credential".into(),
        priority: 0,
        credential_priority: 0,
        provider_priority: 100,
        weight: 1,
        limits: Limits::default(),
        retry: Retry::default(),
        circuit: CircuitPolicy::default(),
        overrides: "{}".into(),
        model_rules: json!({}),
        pass_user_agent: false,
        endpoint: "/v1/chat/completions".into(),
        protocol_endpoint: "/v1/chat/completions".into(),
    };

    let scoped = pricing::snapshot(&db, &candidate("price-channel-a"), 1_000_000)
        .await
        .unwrap();
    let global = pricing::snapshot(&db, &candidate("price-channel-b"), 1_000_000)
        .await
        .unwrap();
    assert_eq!(scoped.id.as_deref(), Some("price-channel"));
    assert_eq!(global.id.as_deref(), Some("price-global"));
    assert_eq!(
        scoped
            .calculate(&pricing::Usage {
                input: 1,
                ..Default::default()
            })
            .unwrap()
            .0,
        3
    );
    assert_eq!(
        global
            .calculate(&pricing::Usage {
                input: 1,
                ..Default::default()
            })
            .unwrap()
            .0,
        10
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
