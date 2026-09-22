use super::*;
use crate::operations::{
    self as ops,
    backup::{Conflict, Selection},
    logging::Policy,
};

async fn number(f: &Fixture, query: &str) -> i64 {
    f.state
        .db
        .query_one(ops::sql(query, vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}

#[tokio::test]
async fn task5_review_partial_anthropic_usage_keeps_reservation_until_final_delta() {
    for mode in [
        "cancel",
        "eof",
        "error",
        "timeout",
        "stop_without_usage",
        "final",
    ] {
        let f = fixture(Router::new().route("/v1/messages", post(move || async move {
            let stream = async_stream::stream! {
                yield Ok::<_, std::io::Error>(Bytes::from_static(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":20,\"output_tokens\":0}}}\n\n"));
                yield Ok(Bytes::from_static(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"generated\"}}\n\n"));
                match mode {
                    "cancel" | "timeout" => tokio::time::sleep(Duration::from_secs(30)).await,
                    "error" => yield Ok(Bytes::from_static(b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\"}}\n\n")),
                    "final" => yield Ok(Bytes::from_static(b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")),
                    "stop_without_usage" => yield Ok(Bytes::from_static(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")),
                    _ => (),
                }
            };
            ([(header::CONTENT_TYPE, "text/event-stream")], Body::from_stream(stream))
        }))).await;
        sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
        sql(&f, "UPDATE providers SET kind='anthropic'", vec![]).await;
        sql(&f, "UPDATE models SET capabilities='[\"messages\"]',input_price_micros=1000000,output_price_micros=1000000", vec![]).await;
        sql(
            &f,
            "UPDATE channel_settings SET retry_statuses_json=?",
            vec![
                json!({"version":1,"event_timeout_ms":10})
                    .to_string()
                    .into(),
            ],
        )
        .await;
        let response = request(&f, "/v1/messages", json!({"model":"public","max_tokens":100,"stream":true,"messages":[{"role":"user","content":"hi"}]})).await;
        assert_eq!(response.status(), StatusCode::OK, "{mode}");
        let reserved = number(&f, "SELECT SUM(reserved_micros) AS n FROM execution_facts").await;
        if mode == "cancel" {
            let mut body = response.into_body().into_data_stream();
            assert!(body.next().await.unwrap().is_ok());
            assert!(body.next().await.unwrap().is_ok());
            drop(body);
        } else {
            to_bytes(response.into_body(), 8192).await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            while number(&f, "SELECT COUNT(*) AS n FROM usage_logs").await == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            number(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
            if mode == "final" { 27 } else { reserved },
            "{mode}"
        );
        assert_eq!(
            number(
                &f,
                "SELECT COUNT(*) AS n FROM usage_logs WHERE settlement_kind='reported'"
            )
            .await,
            i64::from(mode == "final"),
            "{mode}"
        );
    }
}

#[tokio::test]
async fn task5_review_budget_without_tpm_enforces_protocol_output_ceiling() {
    let f = fixture(Router::new()).await;
    sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
    sql(
        &f,
        "UPDATE models SET capabilities='[\"chat\",\"responses\",\"gemini\"]'",
        vec![],
    )
    .await;
    let key = db::authenticate_api_key(&f.state.db, &f.token, None)
        .await
        .unwrap()
        .unwrap();
    for (endpoint, body, pointer, expected) in [
        (
            "/v1/chat/completions",
            json!({"model":"public","messages":[]}),
            "/max_tokens",
            4096,
        ),
        (
            "/v1/chat/completions",
            json!({"model":"public","messages":[],"max_completion_tokens":31}),
            "/max_completion_tokens",
            31,
        ),
        (
            "/v1/responses",
            json!({"model":"public","input":"hi"}),
            "/max_output_tokens",
            4096,
        ),
        (
            "/v1/responses",
            json!({"model":"public","input":"hi","max_tokens":17}),
            "/max_output_tokens",
            17,
        ),
        (
            "/v1/responses",
            json!({"model":"public","input":"hi","max_tokens":17,"max_output_tokens":9}),
            "/max_output_tokens",
            9,
        ),
        (
            "/v1beta/models:generateContent",
            json!({"model":"public","contents":[]}),
            "/generationConfig/maxOutputTokens",
            4096,
        ),
        (
            "/v1beta/models:generateContent",
            json!({"model":"public","contents":[],"generationConfig":{"maxOutputTokens":9000,"candidateCount":2}}),
            "/generationConfig/maxOutputTokens",
            9000,
        ),
    ] {
        let plan = orchestration::prepare(
            &f.state.db,
            &f.state.orchestrator,
            &key,
            orchestration::load_profile(&f.state.db, &key)
                .await
                .unwrap(),
            body,
            &HeaderMap::new(),
            endpoint,
        )
        .await
        .unwrap();
        assert!(plan.routing.limits.tpm.is_none());
        let (payload, _, tokens) = plan.attempt_payload(&plan.candidates[0]).unwrap();
        assert_eq!(
            payload.pointer(pointer),
            Some(&json!(expected)),
            "{endpoint}: {payload}"
        );
        let count = payload
            .pointer("/generationConfig/candidateCount")
            .and_then(Value::as_u64)
            .unwrap_or(1);
        assert!(u64::from(tokens) >= expected * count);
    }
    sql(&f, "INSERT INTO api_key_profiles(id,project_id,name,budget_micros,created_at,updated_at) VALUES('budget-profile',?,'budget-profile',100000,0,0)", vec![db::DEFAULT_PROJECT_ID.into()]).await;
    sql(
        &f,
        "UPDATE api_keys SET budget_micros=NULL,profile_id='budget-profile'",
        vec![],
    )
    .await;
    let key = db::authenticate_api_key(&f.state.db, &f.token, None)
        .await
        .unwrap()
        .unwrap();
    let plan = orchestration::prepare(
        &f.state.db,
        &f.state.orchestrator,
        &key,
        orchestration::load_profile(&f.state.db, &key)
            .await
            .unwrap(),
        chat(),
        &HeaderMap::new(),
        "/v1/chat/completions",
    )
    .await
    .unwrap();
    assert_eq!(
        plan.attempt_payload(&plan.candidates[0]).unwrap().0["max_tokens"],
        4096
    );
}

#[tokio::test]
async fn task5_review_project_retention_removes_derived_payloads_and_requests() {
    let f = fixture(Router::new().route("/v1/chat/completions", post(|| async {Json(json!({"choices":[{"message":{"content":"private response"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}))}))).await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            default_level: ops::logging::Level::FullBody,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let response = request(&f, "/v1/chat/completions", chat()).await;
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    f.state.observations.flush().await;
    let original = f.state.observations.get(id.clone()).await.unwrap().unwrap();
    assert!(original.request_json.is_some());
    let mut expired = original.clone();
    expired.started_at = db::now() - 2 * 86400;
    f.state.observations.record(expired.clone());
    let mut other = expired.clone();
    other.id = "internal-other".into();
    other.request_id = "other-project-event".into();
    other.project_id = "other-project".into();
    f.state.observations.record(other);
    let mut recent = original.clone();
    recent.id = "internal-recent".into();
    recent.request_id = "recent-event".into();
    f.state.observations.record(recent);
    f.state.observations.flush().await;
    sql(&f, "UPDATE requests SET started_at=0", vec![]).await;
    sql(&f, "INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES('payload-expiry',?,'payloads',1,0)", vec![db::DEFAULT_PROJECT_ID.into()]).await;
    ops::runtime::gc(&f.state).await.unwrap();
    let retained = f.state.observations.get(id.clone()).await.unwrap().unwrap();
    assert_eq!(retained.cost_micros, original.cost_micros);
    assert!(retained.request_json.is_none() && retained.response_json.is_none());
    assert!(!retained.payload_captured);
    // A delayed terminal projection must not resurrect already-expired payloads.
    f.state.observations.record(expired.clone());
    f.state.observations.flush().await;
    assert!(
        f.state
            .observations
            .get(id.clone())
            .await
            .unwrap()
            .unwrap()
            .request_json
            .is_none()
    );
    sql(&f, "INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES('request-expiry',?,'requests',1,0)", vec![db::DEFAULT_PROJECT_ID.into()]).await;
    ops::runtime::gc(&f.state).await.unwrap();
    assert!(
        f.state
            .observations
            .get(id.clone())
            .await
            .unwrap()
            .is_none()
    );
    f.state.observations.record(expired);
    f.state.observations.flush().await;
    assert!(f.state.observations.get(id).await.unwrap().is_none());
    for retained in ["other-project-event", "recent-event"] {
        assert!(
            f.state
                .observations
                .get(retained.into())
                .await
                .unwrap()
                .unwrap()
                .request_json
                .is_some()
        );
    }
    assert_eq!(number(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 1);
}

#[tokio::test]
async fn task5_review_selective_restore_settles_only_new_interrupted_executions() {
    let f = fixture(Router::new()).await;
    let key = db::authenticate_api_key(&f.state.db, &f.token, None)
        .await
        .unwrap()
        .unwrap();
    for (request_id, execution_id, contacted) in [
        ("restore-request", "restore-execution", 1),
        ("uncontacted-request", "uncontacted-execution", 0),
    ] {
        sql(&f, "INSERT INTO request_facts(id,project_id,api_key_id,log_level,started_at) VALUES(?,?,?,'off',0)", vec![request_id.into(), db::DEFAULT_PROJECT_ID.into(), key.id.clone().into()]).await;
        sql(&f, "INSERT INTO execution_facts(id,request_id,attempt,contacted,price_json,reserved_micros,started_at) VALUES(?,?,1,?,'{}',99,0)", vec![execution_id.into(), request_id.into(), contacted.into()]).await;
    }
    let artifact = ops::backup::export(
        &f.state,
        db::DEFAULT_PROJECT_ID,
        &Selection {
            resources: vec!["request_facts".into(), "execution_facts".into()],
        },
    )
    .await
    .unwrap();
    sql(&f, "DELETE FROM request_facts", vec![]).await;
    sql(&f, "INSERT INTO request_facts(id,project_id,api_key_id,log_level,started_at) VALUES('live-request',?,?,'off',0)", vec![db::DEFAULT_PROJECT_ID.into(), key.id.into()]).await;
    sql(&f, "INSERT INTO execution_facts(id,request_id,attempt,contacted,price_json,reserved_micros,started_at) VALUES('live-execution','live-request',1,1,'{}',77,0)", vec![]).await;
    sql(&f, "CREATE TRIGGER reject_recovered_usage BEFORE INSERT ON usage_logs BEGIN SELECT RAISE(ABORT,'injected recovery failure'); END", vec![]).await;
    assert!(
        ops::backup::restore(&f.state, db::DEFAULT_PROJECT_ID, &artifact, Conflict::Fail)
            .await
            .is_err()
    );
    assert_eq!(
        number(&f, "SELECT COUNT(*) AS n FROM request_facts").await,
        1
    );
    assert_eq!(
        number(&f, "SELECT COUNT(*) AS n FROM backup_restores").await,
        0
    );
    sql(&f, "DROP TRIGGER reject_recovered_usage", vec![]).await;
    assert_eq!(
        ops::backup::restore(&f.state, db::DEFAULT_PROJECT_ID, &artifact, Conflict::Fail)
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        number(
            &f,
            "SELECT COUNT(*) AS n FROM execution_facts WHERE status='interrupted'"
        )
        .await,
        2
    );
    assert_eq!(
        number(
            &f,
            "SELECT SUM(reserved_micros) AS n FROM execution_facts WHERE status='running'"
        )
        .await,
        77
    );
    assert_eq!(
        number(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        99
    );
    assert_eq!(
        number(
            &f,
            "SELECT COUNT(*) AS n FROM usage_logs WHERE settlement_kind='interrupted'"
        )
        .await,
        2
    );
    assert_eq!(
        number(
            &f,
            "SELECT COUNT(*) AS n FROM request_facts WHERE id='live-request' AND status='running'"
        )
        .await,
        1
    );
    assert_eq!(
        ops::backup::restore(&f.state, db::DEFAULT_PROJECT_ID, &artifact, Conflict::Fail)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        number(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        99
    );
}
