use super::*;

#[test]
fn review_tpm_counts_choices_and_rejects_invalid_output_limits() {
    let estimate_tokens = |payload: &Value| super::estimate_tokens(payload, "/v1/chat/completions");
    let single = json!({"model":"public","messages":[],"max_completion_tokens":100,"n":1});
    let mut multiple = single.clone();
    multiple["n"] = json!(4);
    assert_eq!(
        estimate_tokens(&multiple).unwrap() - estimate_tokens(&single).unwrap(),
        300
    );
    for n in [json!(0), json!(-1), json!(1.5), json!(u64::MAX)] {
        let mut body = single.clone();
        body["n"] = n;
        assert!(estimate_tokens(&body).is_err());
    }
    assert!(estimate_tokens(&json!({"max_tokens":u32::MAX,"n":2})).is_err());
    assert!(estimate_tokens(&json!({"max_completion_tokens":1,"max_tokens":-10})).is_err());
}

#[test]
fn review_allowed_tools_cannot_be_bypassed_with_legacy_functions() {
    for legacy in [
        json!({"functions":[{"name":"forbidden"}]}),
        json!({"function_call":{"name":"forbidden"}}),
    ] {
        assert!(protection::tools(&mut legacy.clone(), Some(&[])).is_err());
        assert!(protection::tools(&mut legacy.clone(), None).is_ok());
    }
}

#[tokio::test]
async fn review_prompt_protection_covers_nested_and_response_tool_outputs() {
    let f = database_fixture().await;
    add_model(&f, "a", "public", "actual").await;
    sql(&f,"INSERT INTO prompt_protection_rules(id,project_id,name,role_pattern,content_pattern,action,replacement,created_at,updated_at) VALUES('tool',?,'Tool','^tool$','secret-[0-9]+','redact','[MASKED]',0,0)",vec![f.key.project_id.clone().into()]).await;
    for body in [
        json!({"model":"public","messages":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"secret-123"}]}]}]}),
        json!({"model":"public","input":[{"type":"function_call_output","call_id":"t1","output":"secret-123"},{"type":"custom_tool_call_output","call_id":"t2","output":[{"type":"text","text":"secret-456"}]}]}),
    ] {
        let result = plan(&f, body.clone()).await.unwrap();
        assert!(!result.payload.to_string().contains("secret-"));
        assert!(result.payload.to_string().contains("[MASKED]"));
        assert_eq!(
            result.payload,
            serde_json::from_str::<Value>(
                &body
                    .to_string()
                    .replace("secret-123", "[MASKED]")
                    .replace("secret-456", "[MASKED]")
            )
            .unwrap()
        );
        assert!(
            result
                .decisions
                .iter()
                .any(|decision| decision.reason == "redacted")
        );
        sql(
            &f,
            "UPDATE prompt_protection_rules SET action='deny'",
            vec![],
        )
        .await;
        assert!(matches!(
            plan(&f, body.clone()).await,
            Err(Error::Forbidden)
        ));
        sql(
            &f,
            "UPDATE prompt_protection_rules SET action='redact'",
            vec![],
        )
        .await;
    }
}
use crate::{
    crypto::SecretBox,
    db,
    models::{ModelInput, ProviderInput},
};
use sea_orm::ConnectionTrait;
use serde_json::json;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

pub(super) fn candidate(id: &str, weight: u32) -> Candidate {
    Candidate {
        target: RouteTarget {
            public_name: "public".into(),
            upstream_name: "actual".into(),
            provider_name: id.into(),
            provider_kind: "openai".into(),
            base_url: "http://localhost".into(),
            secret_envelope: String::new(),
            input_price_micros: 0,
            output_price_micros: 0,
        },
        provider_id: id.into(),
        model_id: id.into(),
        credential_id: id.into(),
        priority: 100,
        weight,
        limits: Limits::default(),
        retry: Retry::default(),
        circuit: CircuitPolicy::default(),
        overrides: "{\"version\":1}".into(),
        model_rules: json!({"version":1}),
        pass_user_agent: false,
        endpoint: "/v1/chat/completions".into(),
        protocol_endpoint: "/v1/chat/completions".into(),
    }
}

struct DatabaseFixture {
    db: DatabaseConnection,
    key: ApiKeyCredential,
    secrets: SecretBox,
    url: String,
    _directory: tempfile::TempDir,
}

async fn database_fixture() -> DatabaseFixture {
    let directory = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("db.sqlite").display()
    );
    let db = db::connect(&url).await.unwrap();
    let secrets = SecretBox::load(directory.path(), None).unwrap();
    db.execute(repository::statement("INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,created_at,project_id) VALUES('key-a','test','prefix','unused','test-digest',0,?)",vec![db::DEFAULT_PROJECT_ID.into()])).await.unwrap();
    DatabaseFixture {
        db,
        key: ApiKeyCredential {
            id: "key-a".into(),
            project_id: db::DEFAULT_PROJECT_ID.into(),
            user_id: None,
            key_hash: "unused".into(),
            scopes: "[\"gateway\"]".into(),
            budget_micros: None,
            spent_micros: 0,
            enabled: true,
            expires_at: None,
            allowed_ips_json: "[]".into(),
            denied_ips_json: "[]".into(),
        },
        secrets,
        url,
        _directory: directory,
    }
}

async fn add_model(
    f: &DatabaseFixture,
    name: &str,
    public: &str,
    upstream: &str,
) -> (String, String) {
    let provider = db::create_provider(
        &f.db,
        &ProviderInput {
            name: name.into(),
            kind: "openai".into(),
            base_url: "https://example.invalid".into(),
            api_key: String::new(),
        },
        f.secrets.encrypt("private-upstream-key").unwrap(),
    )
    .await
    .unwrap();
    let model = db::create_model(
        &f.db,
        &ModelInput {
            provider_id: provider.id.clone(),
            public_name: public.into(),
            upstream_name: upstream.into(),
            capabilities: None,
            input_price_micros: None,
            output_price_micros: None,
            priority: None,
        },
    )
    .await
    .unwrap();
    (provider.id, model.id)
}

async fn sql(f: &DatabaseFixture, query: &str, values: Vec<sea_orm::Value>) {
    f.db.execute(repository::statement(query, values))
        .await
        .unwrap();
}

async fn plan(f: &DatabaseFixture, body: Value) -> Result<Plan> {
    prepare(
        &f.db,
        &Runtime::default(),
        &f.key,
        load_profile(&f.db, &f.key).await?,
        body,
        &HeaderMap::new(),
        "/v1/chat/completions",
    )
    .await
}

#[tokio::test]
async fn database_candidates_exact_regex_tag_conditions_dedup_and_project_isolation() {
    let f = database_fixture().await;
    let (provider, model) = add_model(&f, "a", "base", "actual").await;
    db::create_model(
        &f.db,
        &ModelInput {
            provider_id: provider.clone(),
            public_name: "base-alias".into(),
            upstream_name: "actual".into(),
            capabilities: None,
            input_price_micros: None,
            output_price_micros: None,
            priority: None,
        },
    )
    .await
    .unwrap();
    let (foreign, _) = add_model(&f, "foreign", "requested", "foreign-private").await;
    sql(&f,"INSERT INTO projects(id,name,slug,created_at,updated_at) VALUES('foreign-project','Foreign','foreign',0,0)",vec![]).await;
    sql(
        &f,
        "UPDATE providers SET project_id='foreign-project',settings_json='invalid' WHERE id=?",
        vec![foreign.into()],
    )
    .await;
    sql(
        &f,
        "UPDATE providers SET settings_json=? WHERE id=?",
        vec![
            json!({"version":1,"tags":["eu","fast"]}).to_string().into(),
            provider.clone().into(),
        ],
    )
    .await;
    for (id, kind, pattern, priority, model_id) in [
        ("exact", "exact", "requested", 10, None),
        ("regex", "regex", "^requested$", 5, None),
        ("tag", "tag", "eu", 1, Some(model.clone())),
    ] {
        sql(&f,"INSERT INTO model_associations(id,project_id,model_id,provider_id,match_type,pattern,conditions_json,priority,weight,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,7,0,0)",vec![id.into(),f.key.project_id.clone().into(),model_id.into(),provider.clone().into(),kind.into(),pattern.into(),json!({"version":1,"field":"/body/temperature","op":"lt","value":1}).to_string().into(),priority.into()]).await;
    }
    let result = plan(&f, json!({"model":"requested","temperature":0.5}))
        .await
        .unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].target.upstream_name, "actual");
    assert_eq!(result.candidates[0].priority, 1);
    assert_eq!(result.candidates[0].weight, 7);
    assert!(result.decisions.iter().any(|d| d.stage == "dedup"));
    assert_eq!(result.decisions[0].stage, "access");
    assert_eq!(result.decisions[1].stage, "mapping");
    assert!(
        plan(&f, json!({"model":"requested","temperature":2}))
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    let listed = visible_models(&f.db, &f.key, &HeaderMap::new())
        .await
        .unwrap();
    assert!(!listed.iter().any(|m| m["id"] == "requested"));
}

#[tokio::test]
async fn quota_health_capability_and_disabled_credentials_are_filtered() {
    let f = database_fixture().await;
    let (provider, _) = add_model(&f, "a", "public", "actual").await;
    let body = json!({"model":"public"});
    assert_eq!(plan(&f, body.clone()).await.unwrap().candidates.len(), 1);
    sql(&f,"INSERT INTO provider_quota_snapshots(id,provider_id,remaining_micros,collected_at) VALUES('quota',?,0,0)",vec![provider.clone().into()]).await;
    sql(&f,"INSERT INTO provider_quota_snapshots(id,provider_id,credential_id,remaining_micros,collected_at) VALUES('credential-quota',?,?,100,1)",vec![provider.clone().into(),provider.clone().into()]).await;
    let blocked = plan(&f, body.clone()).await.unwrap();
    assert!(blocked.candidates.is_empty());
    assert!(
        blocked
            .decisions
            .iter()
            .any(|d| d.reason == "quota_exhausted")
    );
    sql(
        &f,
        "UPDATE provider_quota_snapshots SET period_end=1 WHERE id='quota'",
        vec![],
    )
    .await;
    assert_eq!(plan(&f, body.clone()).await.unwrap().candidates.len(), 1);
    sql(
        &f,
        "INSERT INTO channel_health_state(provider_id,backoff_until,updated_at) VALUES(?,?,0)",
        vec![provider.clone().into(), (db::now() + 60).into()],
    )
    .await;
    assert!(plan(&f, body.clone()).await.unwrap().candidates.is_empty());
    sql(&f, "DELETE FROM channel_health_state", vec![]).await;
    sql(&f, "UPDATE channel_credentials SET enabled=0", vec![]).await;
    assert!(plan(&f, body.clone()).await.unwrap().candidates.is_empty());
    sql(&f, "UPDATE channel_credentials SET enabled=1", vec![]).await;
    sql(
        &f,
        "UPDATE models SET capabilities='[\"embeddings\"]'",
        vec![],
    )
    .await;
    assert!(plan(&f, body).await.unwrap().candidates.is_empty());
}

#[tokio::test]
async fn database_profile_mapping_allowed_models_limits_and_budget_are_binding() {
    let f = database_fixture().await;
    add_model(&f, "a", "base", "actual").await;
    sql(&f,"INSERT INTO api_key_profiles(id,project_id,name,rpm_limit,tpm_limit,budget_micros,routing_policy_json,created_at,updated_at) VALUES('profile',?,'restricted',2,10000,100,?,0,0)",vec![f.key.project_id.clone().into(),json!({"version":1,"strategy":"round_robin","sticky":"trace"}).to_string().into()]).await;
    sql(&f, "UPDATE api_keys SET profile_id='profile'", vec![]).await;
    sql(&f,"INSERT INTO api_key_profile_model_mappings(id,profile_id,source_model,target_model) VALUES('mapping','profile','client','base')",vec![]).await;
    sql(&f,"INSERT INTO api_key_profile_allowed_models(profile_id,model_pattern,match_type) VALUES('profile','client','exact')",vec![]).await;
    let result = plan(&f, json!({"model":"client"})).await.unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.routing.limits.rpm, Some(2));
    assert_eq!(result.payload["model"], "base");
    assert!(matches!(
        plan(&f, json!({"model":"base"})).await,
        Err(Error::Forbidden)
    ));
    let visible = visible_models(&f.db, &f.key, &HeaderMap::new())
        .await
        .unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0]["id"], "client");
    sql(&f, "UPDATE api_keys SET spent_micros=100", vec![]).await;
    assert!(matches!(
        load_profile(&f.db, &f.key).await,
        Err(Error::Admission("profile_budget"))
    ));
}

#[tokio::test]
async fn prompt_protection_injection_preview_and_tools_do_not_reorder_history() {
    let f = database_fixture().await;
    let (provider, _) = add_model(&f, "a", "public", "actual").await;
    sql(&f,"INSERT INTO prompts(id,project_id,name,role,content,created_at,updated_at) VALUES('prompt',?,'System','system','injected',0,0)",vec![f.key.project_id.clone().into()]).await;
    sql(&f,"INSERT INTO prompt_protection_rules(id,project_id,name,role_pattern,content_pattern,action,replacement,created_at,updated_at) VALUES('redact',?,'Redact','^user$','secret-[0-9]+','redact','[MASKED]',0,0)",vec![f.key.project_id.clone().into()]).await;
    sql(&f,"INSERT INTO prompt_protection_rules(id,project_id,name,content_pattern,action,test_mode,created_at,updated_at) VALUES('test',?,'Preview','preview','deny',1,0,0)",vec![f.key.project_id.clone().into()]).await;
    sql(
        &f,
        "UPDATE projects SET settings_json=? WHERE id=?",
        vec![
            json!({"version":1,"routing":{"version":1,"allowed_tools":["a"]}})
                .to_string()
                .into(),
            f.key.project_id.clone().into(),
        ],
    )
    .await;
    let body = json!({"model":"public","messages":[{"role":"user","content":[{"type":"text","text":"secret-123 preview"}]},{"role":"assistant","content":"secret-123","tool_calls":[{"id":"call","function":{"name":"old"}}]},{"role":"tool","tool_call_id":"call","content":"result"}],"tools":[{"type":"function","function":{"name":"a"}},{"type":"function","function":{"name":"b"}}]});
    let result = plan(&f, body.clone()).await.unwrap();
    assert_eq!(result.payload["messages"][0]["content"], "injected");
    assert_eq!(
        result.payload["messages"][1]["content"][0]["text"],
        "[MASKED] preview"
    );
    assert_eq!(result.payload["messages"][2], body["messages"][1]);
    assert_eq!(result.payload["messages"][3], body["messages"][2]);
    assert_eq!(result.payload["tools"].as_array().unwrap().len(), 1);
    assert!(result.decisions.iter().any(|d| d.reason == "test_match"));
    sql(&f,"UPDATE channel_settings SET parameter_overrides_json=? WHERE provider_id=?",vec![json!({"version":1,"operations":[{"patch":[{"op":"replace","path":"/messages/1/content/0/text","value":"secret-789"}]}]}).to_string().into(),provider.into()]).await;
    let result = plan(&f, body.clone()).await.unwrap();
    let (attempt, _, _) = result.attempt_payload(&result.candidates[0]).unwrap();
    assert_eq!(attempt["messages"][1]["content"][0]["text"], "[MASKED]");
    sql(
        &f,
        "UPDATE prompt_protection_rules SET test_mode=0 WHERE id='test'",
        vec![],
    )
    .await;
    assert!(matches!(plan(&f, body).await, Err(Error::Forbidden)));
}

#[tokio::test]
async fn sqlite_responses_survive_reopen_and_enforce_expiry_and_scope_even_on_cache_hit() {
    let f = database_fixture().await;
    let sessions = session::Sessions::default();
    let request = json!({"input":"private prompt"});
    let response = json!({"id":"response","status":"completed","output":[{"type":"custom_tool_call","name":"exec","call_id":"id","input":"secret","status":"completed"}]});
    sessions
        .persist(&f.db, &f.secrets, &f.key, &request, &response)
        .await
        .unwrap();
    let row =
        f.db.query_one(repository::statement(
            "SELECT state_envelope FROM response_sessions",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(
        !row.try_get::<String>("", "state_envelope")
            .unwrap()
            .contains("private prompt")
    );
    f.db.clone().close().await.unwrap();
    let database = db::connect(&f.url).await.unwrap();
    let sessions = session::Sessions::default();
    let secrets = SecretBox::load(f._directory.path(), None).unwrap();
    let mut next = json!({"previous_response_id":"response","input":[{"type":"custom_tool_call_output","call_id":"id","output":"ok"}]});
    let mut other = f.key.clone();
    other.id = "key-other".into();
    assert!(
        sessions
            .restore(&database, &secrets, &other, &mut next.clone())
            .await
            .is_err()
    );
    other = f.key.clone();
    other.project_id = "other-project".into();
    assert!(
        sessions
            .restore(&database, &secrets, &other, &mut next.clone())
            .await
            .is_err()
    );
    sessions
        .restore(&database, &secrets, &f.key, &mut next)
        .await
        .unwrap();
    assert_eq!(next["input"].as_array().unwrap().len(), 3);
    assert!(next["input"][1].get("status").is_none());
    database
        .execute_unprepared("UPDATE response_sessions SET expires_at=0")
        .await
        .unwrap();
    assert!(
        sessions
            .restore(
                &database,
                &secrets,
                &f.key,
                &mut json!({"previous_response_id":"response","input":"again"})
            )
            .await
            .is_err()
    );
    sessions
        .persist(
            &database,
            &secrets,
            &f.key,
            &request,
            &json!({"id":"new","status":"completed","output":[]}),
        )
        .await
        .unwrap();
    assert!(
        database
            .query_one(repository::statement(
                "SELECT id FROM response_sessions WHERE response_id='response'",
                vec![]
            ))
            .await
            .unwrap()
            .is_none()
    );
    database
        .execute_unprepared("DELETE FROM api_keys WHERE id='key-a'")
        .await
        .unwrap();
    assert!(
        database
            .query_one(repository::statement(
                "SELECT id FROM response_sessions",
                vec![]
            ))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn least_inflight_latency_and_adaptive_use_terminal_measurements() {
    let runtime = Runtime::default();
    let base = vec![candidate("a", 1), candidate("b", 1)];
    let mut held = runtime
        .admit("channel:a", &Limits::default(), 1)
        .await
        .unwrap();
    let mut values = base.clone();
    runtime.order("scope", &mut values, policy::Strategy::LeastInflight, None);
    assert_eq!(values[0].provider_id, "b");
    tokio::time::sleep(Duration::from_millis(5)).await;
    held.finish(true);
    drop(held);
    let mut fast = runtime
        .admit("channel:b", &Limits::default(), 1)
        .await
        .unwrap();
    fast.finish(true);
    drop(fast);
    for strategy in [policy::Strategy::Latency, policy::Strategy::Adaptive] {
        let mut values = base.clone();
        runtime.order("scope", &mut values, strategy, None);
        assert_eq!(values[0].provider_id, "b");
    }
}

#[tokio::test]
async fn queue_timeout_releases_waiter_and_tpm_retries_charge_again() {
    let runtime = Runtime::default();
    let limits = Limits {
        concurrent: Some(1),
        queue: 1,
        queue_timeout_ms: 5,
        ..Default::default()
    };
    let held = runtime.admit("channel", &limits, 1).await.unwrap();
    assert!(matches!(
        runtime.admit("channel", &limits, 1).await,
        Err(Error::Admission("queue_timeout"))
    ));
    drop(held);
    assert!(runtime.admit("channel", &limits, 1).await.is_ok());
    let permit = runtime
        .admit(
            "key",
            &Limits {
                tpm: Some(10),
                ..Default::default()
            },
            6,
        )
        .await
        .unwrap();
    assert!(permit.reserve_retry_tokens(5).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_peak_and_terminal_counters_are_exact_under_contention() {
    let runtime = Arc::new(Runtime::default());
    let peak = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let mut tasks = vec![];
    for i in 0..100 {
        let runtime = runtime.clone();
        let peak = peak.clone();
        let active = active.clone();
        tasks.push(tokio::spawn(async move {
            let mut permit = runtime
                .admit(
                    "concurrent",
                    &Limits {
                        concurrent: Some(5),
                        queue: 100,
                        queue_timeout_ms: 1000,
                        ..Default::default()
                    },
                    1,
                )
                .await
                .unwrap();
            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(count, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(1)).await;
            active.fetch_sub(1, Ordering::SeqCst);
            permit.finish(i % 2 == 0);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(peak.load(Ordering::SeqCst), 5);
    let metrics = runtime.metrics();
    assert!(metrics.contains("pangolin_orchestration_inflight{resource=\"other\"} 0"));
    assert!(metrics.contains("pangolin_orchestration_completed_total{resource=\"other\"} 100"));
    assert!(metrics.contains("pangolin_orchestration_failed_total{resource=\"other\"} 50"));
}

#[test]
fn tag_modes_regex_validation_retry_policies_and_tools_are_explicit() {
    for (mode, expected) in [("any", true), ("all", false), ("none", false)] {
        let policy = Routing::parse(
            &json!({"version":1,"allowed_tags":["eu","fast"],"tag_mode":mode}).to_string(),
        )
        .unwrap();
        assert_eq!(policy.allows_tags(&["eu".into()]), expected);
    }
    assert!(Retry::parse(&json!({"version":1,"error_patterns":["["]}).to_string()).is_err());
    let retry = Retry::parse(
        &json!({"version":1,"statuses":[400],"error_patterns":["temporar.*"]}).to_string(),
    )
    .unwrap();
    assert!(retry.retry_status(400, ""));
    assert!(!retry.retry_status(401, "no"));
    assert!(retry.retry_status(401, "temporary outage"));
    let original = json!({"tools":[{"type":"function","name":"a"}],"tool_choice":{"type":"allowed_tools","mode":"required","tools":[{"type":"function","name":"a"}]}});
    let mut body = original.clone();
    protection::tools(&mut body, Some(&["a".into()])).unwrap();
    assert_eq!(body, original);
    assert!(protection::tools(&mut body, Some(&["b".into()])).is_err());
}

#[tokio::test]
async fn upstream_model_rules_endpoint_and_tpm_media_policies_are_enforced() {
    let f = database_fixture().await;
    let (provider, _) = add_model(&f, "a", "public", "vendor/GPT").await;
    sql(&f,"UPDATE channel_settings SET model_rules_json=? WHERE provider_id=?",vec![json!({"version":1,"strip_prefix":"vendor/","lowercase":true,"mappings":{"gpt":"renamed"},"prefix":"deploy/","developer_to_system":true,"force_content_array":true,"reasoning_effort":{"auto":"medium"}}).to_string().into(),provider.clone().into()]).await;
    let result = plan(
        &f,
        json!({"model":"public","messages":[{"role":"developer","content":"instruction"}]}),
    )
    .await
    .unwrap();
    assert_eq!(result.candidates[0].target.upstream_name, "deploy/renamed");
    let (body, _, _) = result.attempt_payload(&result.candidates[0]).unwrap();
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"][0]["text"], "instruction");
    assert_eq!(body["reasoning_effort"], "medium");
    sql(
        &f,
        "UPDATE projects SET settings_json=? WHERE id=?",
        vec![
            json!({"version":1,"routing":{"version":1,"allowed_endpoints":["/v1/responses"]}})
                .to_string()
                .into(),
            f.key.project_id.clone().into(),
        ],
    )
    .await;
    assert!(matches!(
        plan(&f, json!({"model":"public"})).await,
        Err(Error::Forbidden)
    ));
    sql(
        &f,
        "UPDATE projects SET settings_json=? WHERE id=?",
        vec![
            json!({"version":1,"routing":{"version":1,"limits":{"tpm":10000}}})
                .to_string()
                .into(),
            f.key.project_id.clone().into(),
        ],
    )
    .await;
    let result=plan(&f,json!({"model":"public","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.invalid/image"}}]}]})).await.unwrap();
    assert!(result.attempt_payload(&result.candidates[0]).is_err());
    let result = plan(&f, json!({"model":"public"})).await.unwrap();
    assert_eq!(
        result.attempt_payload(&result.candidates[0]).unwrap().0["max_tokens"],
        4096
    );
}

#[tokio::test]
async fn credential_quota_selects_next_eligible_secret_and_schema_rejects_foreign_session_owner() {
    let f = database_fixture().await;
    let (provider, _) = add_model(&f, "a", "public", "actual").await;
    sql(&f,"INSERT INTO channel_credentials(id,provider_id,secret_envelope,priority,created_at,updated_at) VALUES('fallback',?,?,200,0,0)",vec![provider.clone().into(),f.secrets.encrypt("fallback-key").unwrap().into()]).await;
    sql(&f,"INSERT INTO provider_quota_snapshots(id,provider_id,credential_id,remaining_micros,collected_at) VALUES('quota',?,?,0,0)",vec![provider.clone().into(),provider.into()]).await;
    let result = plan(&f, json!({"model":"public"})).await.unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].credential_id, "fallback");
    assert_eq!(
        f.secrets
            .decrypt(&result.candidates[0].target.secret_envelope)
            .unwrap()
            .as_str(),
        "fallback-key"
    );
    sql(&f,"INSERT INTO projects(id,name,slug,created_at,updated_at) VALUES('other','Other','other',0,0)",vec![]).await;
    assert!(f.db.execute_unprepared("INSERT INTO response_sessions(id,project_id,api_key_id,response_id,state_envelope,updated_at,expires_at) VALUES('bad','other','key-a','response','ciphertext',0,100)").await.is_err());
}

#[test]
fn nested_conditions_fail_closed_and_use_sanitized_headers() {
    let headers = HeaderMap::from_iter([
        (
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer secret"),
        ),
        (
            http::HeaderName::from_static("x-region"),
            http::HeaderValue::from_static("eu"),
        ),
    ]);
    let context = policy::context(
        &json!({"model":"gpt","temperature":0.5}),
        &headers,
        "/v1/chat/completions",
    );
    assert!(context["headers"].get("authorization").is_none());
    assert!(policy::matches(&json!({"all":[{"field":"/body/model","op":"regex","value":"^gpt$"},{"any":[{"field":"/headers/x-region","op":"eq","value":"eu"},{"field":"/body/temperature","op":"gt","value":1}]}]}),&context).unwrap());
    assert!(
        !policy::matches(&json!({"field":"/missing","op":"ne","value":"x"}), &context).unwrap()
    );
    assert!(
        policy::matches(
            &json!({"field":"/body/model","op":"typo","value":"gpt"}),
            &context
        )
        .is_err()
    );
}

#[test]
fn mappings_enforce_public_model_access_before_rewrite() {
    let profile = Profile {
        id: None,
        budget: None,
        routing: Routing::default(),
        mappings: vec![("regex:^friendly-(.*)$".into(), "provider-$1".into())],
        allowed: vec![("friendly-one".into(), "exact".into())],
    };
    assert_eq!(profile.map_model("friendly-one").unwrap(), "provider-one");
    assert!(matches!(
        profile.map_model("provider-one"),
        Err(Error::Forbidden)
    ));
}

#[test]
fn override_merge_patch_template_and_protected_fields() {
    let original = json!({"model":"a","stream":false,"temperature":1,"metadata":{"old":"x"}});
    let context = policy::context(&original, &HeaderMap::new(), "/v1/chat/completions");
    let policy=json!({"version":1,"operations":[{"merge":{"temperature":0.3,"metadata":{"old":null,"model":{"$request":"/body/model"}}}},{"patch":[{"op":"add","path":"/seed","value":42}],"headers":{"x-model":{"$request":"/body/model"}}}]}).to_string();
    let (result, headers) = policy::overrides(&original, &policy, &context).unwrap();
    assert_eq!(result["metadata"], json!({"model":"a"}));
    assert_eq!(result["seed"], 42);
    assert_eq!(headers["x-model"], "a");
    assert_eq!(original["temperature"], 1);
    for merge in [json!({"model":"b"}), json!({"stream":true})] {
        assert!(
            policy::overrides(
                &original,
                &json!({"version":1,"operations":[{"merge":merge}]}).to_string(),
                &context
            )
            .is_err()
        );
    }
    assert!(
        policy::overrides(
            &original,
            &json!({"version":1,"operations":[{"headers":{"authorization":"secret"}}]}).to_string(),
            &context
        )
        .is_err()
    );
}

#[test]
fn strategy_simulations_priority_round_robin_weights_and_sticky_fallback() {
    use rand::SeedableRng;
    let runtime = Runtime::default();
    let base = vec![candidate("a", 1), candidate("b", 3), candidate("c", 6)];
    let mut counts = [0; 3];
    for _ in 0..300 {
        let mut values = base.clone();
        runtime.order("scope", &mut values, policy::Strategy::RoundRobin, None);
        counts[(values[0].provider_id.as_bytes()[0] - b'a') as usize] += 1;
    }
    assert_eq!(counts, [100, 100, 100]);
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    let mut weights = [0; 3];
    for _ in 0..10_000 {
        let mut values = base.clone();
        runtime::weighted_order(&mut values, &mut rng);
        weights[(values[0].provider_id.as_bytes()[0] - b'a') as usize] += 1;
    }
    assert!((800..1200).contains(&weights[0]));
    assert!((2700..3300).contains(&weights[1]));
    assert!((5600..6400).contains(&weights[2]));
    runtime.bind(Some("key:session"), &base[2]);
    let mut values = base.clone();
    runtime.order(
        "scope",
        &mut values,
        policy::Strategy::Failover,
        Some("key:session"),
    );
    assert_eq!(values[0].provider_id, "c");
    values.retain(|c| c.provider_id != "c");
    runtime.order(
        "scope",
        &mut values,
        policy::Strategy::Failover,
        Some("key:session"),
    );
    assert_eq!(values[0].provider_id, "a");
    for strategy in [
        policy::Strategy::Failover,
        policy::Strategy::RoundRobin,
        policy::Strategy::Weighted,
        policy::Strategy::LeastInflight,
        policy::Strategy::Latency,
        policy::Strategy::Adaptive,
    ] {
        let mut values = base.clone();
        values[2].priority = 1;
        runtime.order("scope", &mut values, strategy, None);
        assert_eq!(values[0].provider_id, "c");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_admission_rpm_queue_and_cancellation_never_leak() {
    let runtime = Arc::new(Runtime::default());
    let limits = Limits {
        rpm: Some(20),
        ..Default::default()
    };
    let accepted = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(100));
    let mut joins = vec![];
    for _ in 0..100 {
        let runtime = runtime.clone();
        let limits = limits.clone();
        let accepted = accepted.clone();
        let barrier = barrier.clone();
        joins.push(tokio::spawn(async move {
            barrier.wait().await;
            if let Ok(mut permit) = runtime.admit("race", &limits, 1).await {
                accepted.fetch_add(1, Ordering::Relaxed);
                permit.finish(true);
            }
        }));
    }
    for join in joins {
        join.await.unwrap();
    }
    assert_eq!(accepted.load(Ordering::Relaxed), 20);
    let limits = Limits {
        concurrent: Some(1),
        queue: 1,
        queue_timeout_ms: 1000,
        ..Default::default()
    };
    let held = runtime.admit("queue", &limits, 1).await.unwrap();
    let queued = {
        let runtime = runtime.clone();
        let limits = limits.clone();
        tokio::spawn(async move { runtime.admit("queue", &limits, 1).await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(matches!(
        runtime.admit("queue", &limits, 1).await,
        Err(Error::Admission("queue_full"))
    ));
    queued.abort();
    let _ = queued.await;
    drop(held);
    assert!(runtime.admit("queue", &limits, 1).await.is_ok());
    let tokens = Limits {
        tpm: Some(10),
        ..Default::default()
    };
    assert!(runtime.admit("tokens", &tokens, 11).await.is_err());
    assert!(runtime.admit("tokens", &tokens, 6).await.is_ok());
    assert!(runtime.admit("tokens", &tokens, 5).await.is_err());
}

#[tokio::test]
async fn circuit_is_model_scoped_and_half_open_probe_is_exclusive() {
    let runtime = Runtime::default();
    let policy = CircuitPolicy {
        enabled: true,
        failures: 2,
        window_ms: 1000,
        recovery_ms: 10,
    };
    for _ in 0..2 {
        runtime
            .enter_circuit("channel:model-a", &policy)
            .unwrap()
            .finish(false);
    }
    assert!(!runtime.circuit_available("channel:model-a", &policy));
    assert!(runtime.circuit_available("channel:model-b", &policy));
    tokio::time::sleep(Duration::from_millis(15)).await;
    let probe = runtime.enter_circuit("channel:model-a", &policy).unwrap();
    assert!(runtime.enter_circuit("channel:model-a", &policy).is_err());
    drop(probe);
    let mut probe = runtime.enter_circuit("channel:model-a", &policy).unwrap();
    probe.finish(true);
    assert!(runtime.circuit_available("channel:model-a", &policy));
}

#[test]
fn response_sessions_preserve_order_normalize_status_and_deny_cross_key_reads() {
    let sessions = session::Sessions::default();
    sessions.record("project:key-a",&json!({"input":"hello"}),&json!({"id":"resp_1","status":"completed","output":[{"type":"function_call","call_id":"call_1","name":"run","arguments":"{}","status":"completed"}]}));
    let mut body = json!({"instructions":"new","previous_response_id":"resp_1","input":[{"type":"function_call_output","call_id":"call_1","output":"ok"}]});
    assert!(sessions.prepare("project:key-b", &mut body).is_err());
    sessions.prepare("project:key-a", &mut body).unwrap();
    assert!(body.get("previous_response_id").is_none());
    assert_eq!(body["input"].as_array().unwrap().len(), 3);
    assert!(body["input"][1].get("status").is_none());
    assert_eq!(body["instructions"], "new");
}
