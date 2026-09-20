use super::*;
use crate::operations::{
    self as ops,
    backup::{Conflict, Selection},
    logging::{Level, Policy},
};

async fn count(f: &Fixture, query: &str) -> i64 {
    f.state
        .db
        .query_one(ops::sql(query, vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}
fn success() -> Router {
    Router::new().route("/v1/chat/completions",post(||async {Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":100,"completion_tokens":10,"prompt_tokens_details":{"cached_tokens":20}}}))}))
}

#[tokio::test]
async fn task5_authoritative_lifecycle_survives_degraded_duckdb_and_logs_off() {
    let mut f = fixture(success()).await;
    f.state.observations =
        ObservationStore::degraded(f._directory.path().join("unavailable"), "injected fault");
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=2000000",
        vec![],
    )
    .await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            enabled: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM request_facts").await,
        1
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM execution_facts WHERE status='succeeded'"
        )
        .await,
        1
    );
    assert_eq!(
        count(&f, "SELECT SUM(total_cost_micros) AS n FROM usage_logs").await,
        120
    );
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        120
    );
    for table in [
        "traces",
        "threads",
        "requests",
        "request_executions",
        "request_contents",
    ] {
        assert_eq!(
            count(&f, &format!("SELECT COUNT(*) AS n FROM {table}")).await,
            0
        );
    }
    assert_eq!(f.state.observations.dropped_events(), 0);
    let response = router(f.state.clone())
        .oneshot(
            Request::get("/v1/models")
                .header("authorization", format!("Bearer {}", f.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(f.state.observations.dropped_events(), 0);
}
#[tokio::test]
async fn task5_each_retry_has_a_terminal_usage_fact_and_shared_trace() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(|Json(body):Json<Value>|async move {if body["model"]=="a-model" {(StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"retry"}))).into_response()}else{Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}})).into_response()}}))).await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM execution_facts").await,
        2
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 2);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM request_executions WHERE status='failed'"
        )
        .await,
        1
    );
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM traces").await, 1);
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM requests").await, 2);
}
#[tokio::test]
async fn task5_policy_and_price_are_frozen_during_upstream_work() {
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let r = reached.clone();
    let go = release.clone();
    let f=fixture(Router::new().route("/v1/chat/completions",post(move||{let r=r.clone();let go=go.clone();async move {r.notify_one();go.notified().await;Json(json!({"choices":[{"message":{"content":"kept"}}],"usage":{"prompt_tokens":10,"completion_tokens":5},"authorization":"never-stored"}))}}))).await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            default_level: Level::FullBody,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    let run = request(&f, "/v1/chat/completions", chat());
    tokio::pin!(run);
    tokio::select! {_=&mut run=>panic!("request completed before release"),_=reached.notified()=>{}}
    sql(
        &f,
        "UPDATE models SET input_price_micros=9000000,output_price_micros=9000000",
        vec![],
    )
    .await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            enabled: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    release.notify_one();
    assert_eq!(run.await.status(), StatusCode::OK);
    assert_eq!(
        count(&f, "SELECT SUM(total_cost_micros) AS n FROM usage_logs").await,
        15
    );
    let row = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT request_json,response_json FROM request_contents",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    let response: String = row.try_get("", "response_json").unwrap();
    assert!(response.contains("kept"));
    assert!(!response.contains("never-stored"));
    assert!(
        row.try_get::<String>("", "request_json")
            .unwrap()
            .contains("hello")
    );
}
#[tokio::test]
async fn task5_stream_usage_settlement_and_restart_are_idempotent() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(||async {([(header::CONTENT_TYPE,"text/event-stream")],"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":20}}\n\ndata: [DONE]\n\n")}))).await;
    sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=2000000",
        vec![],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("[DONE]"));
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        140
    );
    ops::runtime::recover(&f.state).await.unwrap();
    ops::runtime::recover(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        140
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 1);
}
#[tokio::test]
async fn task5_cancelled_stream_preserves_budget_and_conservative_charge() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(||async {let stream=async_stream::stream!{yield Ok::<_,std::io::Error>(Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n"));tokio::time::sleep(Duration::from_secs(30)).await;};([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(stream))}))).await;
    sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let reserved = count(&f, "SELECT SUM(reserved_micros) AS n FROM execution_facts").await;
    assert!(reserved > 0);
    drop(response);
    for _ in 0..50 {
        if count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        reserved
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM execution_facts WHERE status='cancelled'"
        )
        .await,
        1
    );
}
#[tokio::test]
async fn task5_backup_selective_roundtrip_encryption_atomicity_and_conflicts() {
    let f = fixture(success()).await;
    let project = db::DEFAULT_PROJECT_ID;
    let selection = Selection {
        resources: vec![
            "providers".into(),
            "channel_credentials".into(),
            "models".into(),
        ],
    };
    let artifact = ops::backup::export(&f.state, project, &selection)
        .await
        .unwrap();
    let exported = serde_json::to_string(&artifact).unwrap();
    assert!(!exported.contains("a-secret"));
    assert!(!exported.contains("secret_envelope"));
    let before = count(&f, "SELECT COUNT(*) AS n FROM providers").await;
    assert!(
        ops::backup::restore(&f.state, project, &artifact, Conflict::Fail)
            .await
            .is_err()
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM backup_restores").await,
        0
    );
    assert_eq!(
        ops::backup::restore(&f.state, project, &artifact, Conflict::Skip)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        ops::backup::restore(&f.state, project, &artifact, Conflict::Skip)
            .await
            .unwrap(),
        0
    );
    sql(
        &f,
        "UPDATE providers SET base_url='https://changed.example'",
        vec![],
    )
    .await;
    assert!(
        ops::backup::restore(&f.state, project, &artifact, Conflict::Overwrite)
            .await
            .unwrap()
            > 0
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM providers").await,
        before
    );
    let envelope = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT secret_envelope FROM channel_credentials ORDER BY id LIMIT 1",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "secret_envelope")
        .unwrap();
    assert!(
        f.state
            .secrets
            .decrypt(&envelope)
            .unwrap()
            .ends_with("-secret")
    );
    let mut corrupted = artifact.clone();
    corrupted.digest = "bad".into();
    assert!(
        ops::backup::restore(&f.state, project, &corrupted, Conflict::Overwrite)
            .await
            .is_err()
    );
    assert!(
        ops::backup::restore(&f.state, "other-project", &artifact, Conflict::Overwrite)
            .await
            .is_err()
    );
}
async fn local_target(f: &Fixture, name: &str) -> String {
    let target = ops::id();
    sql(f,"INSERT INTO data_storage_configs(id,project_id,name,kind,config_json,created_at,updated_at) VALUES(?,?,?,'local',?,0,0)",vec![target.clone().into(),db::DEFAULT_PROJECT_ID.into(),name.into(),json!({"kind":"local","directory":name}).to_string().into()]).await;
    target
}
#[tokio::test]
async fn task5_backup_target_revision_fencing_retry_and_owned_retention() {
    let f = fixture(success()).await;
    let project = db::DEFAULT_PROJECT_ID;
    let target = local_target(&f, "local").await;
    let selection = Selection {
        resources: vec!["providers".into()],
    };
    let job = ops::backup::enqueue(
        &f.state,
        project,
        std::slice::from_ref(&target),
        &selection,
        "manual-one",
    )
    .await
    .unwrap();
    sql(
        &f,
        "UPDATE data_storage_configs SET revision=revision+1 WHERE id=?",
        vec![target.clone().into()],
    )
    .await;
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.id, job);
    assert!(ops::backup::execute(&f.state, &claim).await.is_err());
    ops::jobs::finish(&f.state.db, &claim, db::now(), false)
        .await
        .unwrap();
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM backup_job_targets WHERE error_code='target_changed'"
        )
        .await,
        1
    );
    let retry = ops::backup::retry(&f.state, project, &job).await.unwrap();
    assert_ne!(retry, job);
    sql(
        &f,
        "UPDATE operation_jobs SET status='failed' WHERE id=?",
        vec![job.into()],
    )
    .await;
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.id, retry);
    ops::backup::execute(&f.state, &claim).await.unwrap();
    ops::backup::execute(&f.state, &claim).await.unwrap();
    assert!(
        ops::jobs::finish(&f.state.db, &claim, db::now(), true)
            .await
            .unwrap()
    );
    let config = ops::storage::Config::Local {
        directory: "local".into(),
    };
    let store = ops::storage::open(&f.state, project, &target, &config, None)
        .await
        .unwrap();
    ops::storage::put(store.as_ref(), "unowned.json", b"keep this".to_vec())
        .await
        .unwrap();
    ops::backup::enqueue(
        &f.state,
        project,
        std::slice::from_ref(&target),
        &selection,
        "manual-two",
    )
    .await
    .unwrap();
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    ops::backup::execute(&f.state, &claim).await.unwrap();
    ops::jobs::finish(&f.state.db, &claim, db::now(), true)
        .await
        .unwrap();
    assert_eq!(
        ops::backup::retain(&f.state, project, &target, 1)
            .await
            .unwrap(),
        1
    );
    use object_store::ObjectStoreExt;
    assert_eq!(
        store
            .get(&object_store::path::Path::from("unowned.json"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        b"keep this"
    );
}
#[tokio::test]
async fn task5_provider_quota_disable_and_compaction_tool_order() {
    let f = fixture(success()).await;
    sql(
        &f,
        "UPDATE channel_settings SET auto_disable_policy_json=?",
        vec![
            json!({"version":1,"enabled":true,"threshold":2,"duration_secs":30})
                .to_string()
                .into(),
        ],
    )
    .await;
    for _ in 0..2 {
        ops::runtime::health(
            &f.state,
            db::DEFAULT_PROJECT_ID,
            &f.providers[0],
            false,
            Some(500),
            "upstream_failed",
        )
        .await
        .unwrap();
    }
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM channel_health_state WHERE disabled_until>unixepoch()"
        )
        .await,
        1
    );
    assert_eq!(
        ops::runtime::decimal_micros("1.234567", 1000000).unwrap(),
        1234567
    );
    assert_eq!(
        ops::runtime::decimal_micros("-0.000001", 1000000).unwrap(),
        -1
    );
    use crate::orchestration::compaction::safe_boundary;
    assert!(!safe_boundary(&[
        json!({"type":"function_call","call_id":"a"})
    ]));
    assert!(safe_boundary(&[
        json!({"type":"function_call","call_id":"a"}),
        json!({"type":"function_call_output","call_id":"a","output":"done"})
    ]));
}

async fn admin(
    f: &Fixture,
    cookie: &str,
    method: http::Method,
    path: &str,
    body: Value,
    csrf: bool,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("cookie", format!("pangolin_session={cookie}"))
        .header("content-type", "application/json");
    if csrf {
        builder = builder.header("x-pangolin-csrf", "1")
    }
    router(f.state.clone())
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
async fn owner(f: &Fixture) -> String {
    let user = db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: "operations@example.com".into(),
            password: "operations-test-123456".into(),
            instance_name: None,
            language: None,
        },
    )
    .await
    .unwrap();
    db::create_session(&f.state.db, &user.id).await.unwrap()
}
async fn json_body(response: Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), 48 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn task6_control_plane_crud_redacts_credentials_and_audits_changes() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/operations",
        db::DEFAULT_PROJECT_ID
    );
    let credential = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{base}/credentials"),
        json!({"provider_id":f.providers[0],"credential_type":"api_key","secret":"sk-control-plane-credential","priority":5}),
        true,
    )
    .await;
    assert_eq!(credential.status(), StatusCode::OK);
    let credentials = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/credentials"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert!(credentials["data"].as_array().unwrap().len() >= 2);
    assert!(credentials["total"].as_i64().unwrap() >= 2);
    assert!(!credentials.to_string().contains("control-plane-credential"));
    assert!(!credentials.to_string().contains("secret_envelope"));

    for (resource, document) in [
        (
            "key-profiles",
            json!({"name":"mobile clients","rpm_limit":60,"tpm_limit":120000,"budget_micros":5000000,"routing_policy":{"version":1}}),
        ),
        (
            "prompts",
            json!({"name":"support policy","role":"system","content":"Answer with cited facts.","activation":{"version":1},"enabled":true}),
        ),
        (
            "protection",
            json!({"name":"credential guard","content_pattern":"(?i)api[_ -]?key","action":"redact","replacement":"[redacted]","scopes":{"version":1},"enabled":true}),
        ),
    ] {
        assert_eq!(
            admin(
                &f,
                &cookie,
                http::Method::POST,
                &format!("{base}/{resource}"),
                document,
                true,
            )
            .await
            .status(),
            StatusCode::OK,
            "{resource}"
        );
        assert_eq!(
            json_body(
                admin(
                    &f,
                    &cookie,
                    http::Method::GET,
                    &format!("{base}/{resource}"),
                    Value::Null,
                    false,
                )
                .await
            )
            .await["data"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{resource}"
        );
    }
    let key = db::authenticate_api_key(&f.state.db, &f.token, None)
        .await
        .unwrap()
        .unwrap();
    let preview = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!(
                "/api/admin/v1/projects/{}/routing-preview",
                db::DEFAULT_PROJECT_ID
            ),
            json!({"api_key_id":key.id,"model":"public","endpoint":"/v1/chat/completions"}),
            true,
        )
        .await,
    )
    .await;
    assert!(!preview["candidates"].as_array().unwrap().is_empty());
    assert_eq!(preview["decisions"][0]["stage"], "access");
    assert_eq!(admin(&f,&cookie,http::Method::PUT,"/api/admin/v1/settings/system",json!({"instance_name":"Operations","branding_name":"Pangolin / 鲮鲤","favicon_url":"/logo.webp","onboarding_complete":true}),true).await.status(),StatusCode::OK);
    assert_eq!(
        json_body(
            admin(
                &f,
                &cookie,
                http::Method::GET,
                "/api/admin/v1/settings/system",
                Value::Null,
                false
            )
            .await
        )
        .await["onboarding_complete"],
        true
    );
    assert!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action LIKE '%.save'"
        )
        .await
            >= 4
    );
}

#[tokio::test]
async fn task5_management_api_prices_groups_logs_csrf_and_scope() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/operations",
        db::DEFAULT_PROJECT_ID
    );
    let config = json!({"name":"local","config":{"kind":"local","directory":"safe"}});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/storage"),
            config.clone(),
            false
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let result = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{base}/storage"),
        config,
        true,
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    let target = json_body(result).await["id"].as_str().unwrap().to_owned();
    let listing = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/storage"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(listing["data"][0]["id"], target);
    assert!(listing.to_string().find("envelope").is_none());
    let model = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT id FROM models WHERE provider_id=?",
            vec![f.providers[0].clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "id")
        .unwrap();
    let price = json!({"model_id":model,"components":[{"kind":"input","unit_size":1,"unit_price_micros":10},{"kind":"output","unit_size":1,"unit_price_micros":20},{"kind":"cache_read","unit_size":1,"unit_price_micros":1}]});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/prices"),
            price,
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    let key = db::authenticate_api_key(&f.state.db, &f.token, None)
        .await
        .unwrap()
        .unwrap();
    let group = json!({"name":"discount","ratio_millionths":500000,"api_keys":[key.id],"channels":[f.providers[0]]});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/groups"),
            group,
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        count(&f, "SELECT SUM(total_cost_micros) AS n FROM usage_logs").await,
        510
    );
    assert_eq!(
        count(&f, "SELECT SUM(cache_savings_micros) AS n FROM usage_logs").await,
        90
    );
    assert!(
        f.state
            .db
            .execute(ops::sql(
                "UPDATE model_price_components SET unit_price_micros=99",
                vec![]
            ))
            .await
            .is_err()
    );
    for resource in [
        "threads",
        "traces",
        "requests",
        "executions",
        "usage",
        "cost-items",
        "prices",
        "audit",
        "jobs",
        "groups",
        "storage",
    ] {
        let response = admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/{resource}"),
            Value::Null,
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{resource}");
    }
    let summary = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!(
            "/api/admin/v1/projects/{}/analytics?dimension=model",
            db::DEFAULT_PROJECT_ID
        ),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(summary.status(), StatusCode::OK);
    assert_eq!(json_body(summary).await["data"][0]["cost_micros"], 510);
    assert_eq!(admin(&f,&cookie,http::Method::POST,"/api/admin/v1/projects/other/operations/storage",json!({"id":target,"name":"stolen","revision":1,"config":{"kind":"local","directory":"other"}}),true).await.status(),StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn task5_affinity_switches_only_on_success_and_disabled_channel_is_invalidated() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let copy = seen.clone();
    let f=fixture(Router::new().route("/v1/chat/completions",post(move|Json(body):Json<Value>|{let seen=copy.clone();async move {seen.lock().await.push(body["model"].as_str().unwrap().to_owned());Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}))}}))).await;
    sql(&f,"UPDATE projects SET settings_json=?",vec![json!({"version":1,"affinity_rules":[{"id":"session","mode":"prefer","source":{"kind":"pointer","value":"/user"},"ttl_secs":60}]}).to_string().into()]).await;
    let mut body = chat();
    body["user"] = json!("sensitive-affinity-id");
    assert_eq!(
        request(&f, "/v1/chat/completions", body.clone())
            .await
            .status(),
        StatusCode::OK
    );
    sql(
        &f,
        "UPDATE models SET priority=-1 WHERE provider_id=?",
        vec![f.providers[1].clone().into()],
    )
    .await;
    assert_eq!(
        request(&f, "/v1/chat/completions", body.clone())
            .await
            .status(),
        StatusCode::OK
    );
    sql(
        &f,
        "UPDATE providers SET enabled=0 WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    assert_eq!(
        request(&f, "/v1/chat/completions", body).await.status(),
        StatusCode::OK
    );
    assert_eq!(*seen.lock().await, vec!["a-model", "a-model", "b-model"]);
}
#[tokio::test]
async fn task5_compaction_is_opt_in_cached_and_failure_preserves_exact_history() {
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let f=fixture(Router::new().route("/v1/responses",post(move|Json(body):Json<Value>|{let seen=copy.clone();async move {seen.lock().await.push(body.clone());Json(json!({"id":format!("resp_{}",Uuid::new_v4()),"status":"completed","output":[{"type":"message","role":"assistant","content":"ok"}],"usage":{"input_tokens":10,"output_tokens":2}}))}})).route("/v1/chat/completions",post(move||{counted.fetch_add(1,Ordering::Relaxed);async {Json(json!({"choices":[{"message":{"content":"remembered context"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}))}}))).await;
    sql(
        &f,
        "UPDATE models SET capabilities='[\"chat\",\"responses\"]'",
        vec![],
    )
    .await;
    let history = vec![
        json!({"role":"user","content":"long context ".repeat(100)}),
        json!({"role":"assistant","content":"answer"}),
        json!({"role":"user","content":"continue"}),
    ];
    let body = json!({"model":"public","input":history});
    assert_eq!(
        request(&f, "/v1/responses", body.clone()).await.status(),
        StatusCode::OK
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    sql(&f,"UPDATE projects SET settings_json=?",vec![json!({"version":1,"session_compaction":{"enabled":true,"threshold_tokens":128,"retain_items":1,"native":false,"summarizer_model":"public"}}).to_string().into()]).await;
    for _ in 0..2 {
        assert_eq!(
            request(&f, "/v1/responses", body.clone()).await.status(),
            StatusCode::OK
        )
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM session_summaries").await,
        1
    );
    let captured = seen.lock().await;
    assert_eq!(captured[0]["input"], json!(history));
    assert_eq!(captured[1]["input"].as_array().unwrap().len(), 2);
    assert!(
        captured[1]["input"][0]["content"]
            .as_str()
            .unwrap()
            .contains("remembered context")
    );
    assert_eq!(captured[1]["input"][1], history[2]);
    drop(captured);
    sql(&f,"UPDATE projects SET settings_json=?",vec![json!({"version":1,"session_compaction":{"enabled":true,"threshold_tokens":128,"retain_items":1,"native":false,"summarizer_model":"unavailable"}}).to_string().into()]).await;
    assert_eq!(
        request(&f, "/v1/responses", body).await.status(),
        StatusCode::OK
    );
    assert_eq!(seen.lock().await[3]["input"], json!(history));
}

#[tokio::test]
async fn task5_schedule_snapshot_claim_and_restart_do_not_duplicate_jobs() {
    let f = fixture(success()).await;
    let target = local_target(&f, "scheduled").await;
    sql(&f,"INSERT INTO operation_schedules(id,project_id,kind,payload_json,interval_secs,next_run_at) VALUES('schedule',?,'automatic_backup',?,60,0)",vec![db::DEFAULT_PROJECT_ID.into(),json!({"targets":[target],"resources":["providers"]}).to_string().into()]).await;
    assert_eq!(
        ops::jobs::enqueue_due(&f.state.db, db::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        ops::jobs::enqueue_due(&f.state.db, db::now())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM backup_job_targets").await,
        1
    );
    let claim = ops::jobs::claim(&f.state.db, "old", db::now(), 1)
        .await
        .unwrap()
        .unwrap();
    let takeover = ops::jobs::claim(&f.state.db, "new", db::now() + 2, 120)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.id, takeover.id);
    assert!(takeover.fence > claim.fence);
    assert!(
        !ops::jobs::finish(&f.state.db, &claim, db::now() + 2, true)
            .await
            .unwrap()
    );
    ops::backup::execute(&f.state, &takeover).await.unwrap();
    assert!(
        ops::jobs::finish(&f.state.db, &takeover, db::now() + 2, true)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn task5_webhook_outbox_retries_and_gc_preserves_usage_and_latest_quota() {
    let seen = Arc::new(AtomicUsize::new(0));
    let copy = seen.clone();
    let f = fixture(Router::new().route(
        "/hook",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let n = copy.fetch_add(1, Ordering::Relaxed);
            async move {
                assert!(headers.contains_key("idempotency-key"));
                assert_eq!(body["event"], "test");
                if n == 0 {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::OK
                }
            }
        }),
    ))
    .await;
    let provider = f
        .state
        .db
        .query_one(ops::sql("SELECT base_url FROM providers LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "base_url")
        .unwrap();
    sql(&f,"INSERT INTO webhooks(id,project_id,name,url,subscriptions_json,created_at,updated_at) VALUES('hook',?,'hook',?,?,0,0)",vec![db::DEFAULT_PROJECT_ID.into(),format!("{provider}/hook").into(),json!({"version":1,"events":["test"]}).to_string().into()]).await;
    ops::runtime::notify(
        &f.state.db,
        db::DEFAULT_PROJECT_ID,
        "test",
        "one",
        &json!({"ok":true}),
    )
    .await
    .unwrap();
    ops::runtime::notify(
        &f.state.db,
        db::DEFAULT_PROJECT_ID,
        "test",
        "one",
        &json!({"ok":true}),
    )
    .await
    .unwrap();
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM operation_jobs").await,
        1
    );
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert!(ops::runtime::execute(&f.state, &claim).await.is_err());
    ops::jobs::finish(&f.state.db, &claim, db::now(), false)
        .await
        .unwrap();
    let next = ops::jobs::claim(&f.state.db, "worker", db::now() + 3, 120)
        .await
        .unwrap()
        .unwrap();
    ops::runtime::execute(&f.state, &next).await.unwrap();
    ops::jobs::finish(&f.state.db, &next, db::now() + 3, true)
        .await
        .unwrap();
    assert_eq!(seen.load(Ordering::Relaxed), 2);
    for (id, collected) in [("old", 0), ("new", 1)] {
        sql(&f,"INSERT INTO provider_quota_snapshots(id,provider_id,remaining_micros,collected_at) VALUES(?,?,0,?)",vec![id.into(),f.providers[0].clone().into(),collected.into()]).await;
    }
    sql(&f,"INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES('q',?,'quota',1,0)",vec![db::DEFAULT_PROJECT_ID.into()]).await;
    ops::runtime::gc(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM provider_quota_snapshots").await,
        1
    );
}

#[tokio::test]
async fn task5_concurrent_streams_cannot_oversubscribe_durable_budget() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let f=fixture(Router::new().route("/v1/chat/completions",post(move||{counted.fetch_add(1,Ordering::Relaxed);async {let stream=async_stream::stream! {yield Ok::<_,std::io::Error>(Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"pending\"}}]}\n\n"));tokio::time::sleep(Duration::from_secs(30)).await;};([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(stream))}}))).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
    let first = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(first.status(), StatusCode::OK);
    let bound = count(&f, "SELECT SUM(reserved_micros) AS n FROM execution_facts").await;
    sql(
        &f,
        "UPDATE api_keys SET budget_micros=?",
        vec![bound.into()],
    )
    .await;
    let (a, b) = tokio::join!(
        request(&f, "/v1/chat/completions", streaming()),
        request(&f, "/v1/chat/completions", streaming())
    );
    assert_eq!(a.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(b.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop(first);
}
#[tokio::test]
async fn task5_file_reopen_recovers_interrupted_execution_once() {
    let mut f = fixture(success()).await;
    let path = f._directory.path().join("reopened.sqlite");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    f.state.db = db::connect(&url).await.unwrap();
    let (key, _) = db::create_api_key(
        &f.state.db,
        &ApiKeyInput {
            name: "persisted".into(),
            budget_micros: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sql(&f,"INSERT INTO request_facts(id,project_id,api_key_id,log_level,started_at) VALUES('interrupted',?,?,'off',0)",vec![db::DEFAULT_PROJECT_ID.into(),key.id.clone().into()]).await;
    sql(&f,"INSERT INTO execution_facts(id,request_id,attempt,price_json,reserved_micros,contacted,started_at) VALUES('attempt','interrupted',1,'{}',1234,1,0)",vec![]).await;
    f.state.db.clone().close().await.unwrap();
    f.state.db = db::connect(&url).await.unwrap();
    ops::runtime::recover(&f.state).await.unwrap();
    ops::runtime::recover(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        1234
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 1);
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM requests").await, 0);
}

#[tokio::test]
async fn task5_backup_remaps_named_channels_and_never_refunds_live_spend() {
    let f = fixture(success()).await;
    let selection = Selection {
        resources: vec![
            "providers".into(),
            "channel_credentials".into(),
            "models".into(),
            "api_keys".into(),
        ],
    };
    sql(&f, "UPDATE api_keys SET spent_micros=123", vec![]).await;
    let artifact = ops::backup::export(&f.state, db::DEFAULT_PROJECT_ID, &selection)
        .await
        .unwrap();
    sql(&f, "UPDATE api_keys SET spent_micros=999", vec![]).await;
    sql(
        &f,
        "DELETE FROM providers WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    let replacement = db::create_provider(
        &f.state.db,
        &ProviderInput {
            name: "a".into(),
            kind: "openai".into(),
            base_url: "https://replacement.example".into(),
            api_key: String::new(),
        },
        f.state.secrets.encrypt("replacement-secret").unwrap(),
    )
    .await
    .unwrap();
    ops::backup::restore(
        &f.state,
        db::DEFAULT_PROJECT_ID,
        &artifact,
        Conflict::Overwrite,
    )
    .await
    .unwrap();
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM providers").await, 2);
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        999
    );
    let provider = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT provider_id FROM models WHERE upstream_name='a-model'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "provider_id")
        .unwrap();
    assert_eq!(provider, replacement.id);
}
#[tokio::test]
async fn task5_bounded_media_is_costed_and_polling_does_not_double_charge() {
    let f = fixture(
        Router::new()
            .route(
                "/v1/videos",
                post(|| async { Json(json!({"id":"video-owned","status":"queued"})) }),
            )
            .route(
                "/v1/videos/video-owned",
                get(|| async { Json(json!({"id":"video-owned","status":"completed"})) }),
            ),
    )
    .await;
    sql(&f, "UPDATE api_keys SET budget_micros=10000", vec![]).await;
    sql(&f, "UPDATE models SET capabilities='[\"videos\"]'", vec![]).await;
    let model = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT id FROM models WHERE provider_id=?",
            vec![f.providers[0].clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "id")
        .unwrap();
    sql(&f,"INSERT INTO model_prices(id,model_id,version,valid_from,created_at) VALUES('video-price',?,1,0,0)",vec![model.into()]).await;
    sql(&f,"INSERT INTO model_price_components(id,price_id,kind,unit_size,unit_price_micros) VALUES('seconds','video-price','unit',1,100)",vec![]).await;
    assert_eq!(
        request(
            &f,
            "/v1/videos",
            json!({"model":"public","prompt":"ocean","seconds":"4"})
        )
        .await
        .status(),
        StatusCode::OK
    );
    for _ in 0..2 {
        let response = router(f.state.clone())
            .oneshot(
                Request::get("/v1/videos/video-owned")
                    .header("authorization", format!("Bearer {}", f.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK)
    }
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        400
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 3);
}
#[tokio::test]
async fn task5_probe_quota_collector_and_invalid_schedule_isolation() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(||async{Json(json!({"choices":[{"message":{"content":"OK"}}],"usage":{"prompt_tokens":2,"completion_tokens":1}}))})).route("/api/v1/key",get(||async{Json(json!({"data":{"limit_remaining":0.0}}))}))).await;
    for kind in ["probe", "quota"] {
        ops::jobs::enqueue(
            &f.state.db,
            Some(db::DEFAULT_PROJECT_ID),
            kind,
            kind,
            &json!({"provider_id":f.providers[0]}),
            db::now(),
        )
        .await
        .unwrap();
        let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
            .await
            .unwrap()
            .unwrap();
        ops::runtime::execute(&f.state, &claim).await.unwrap();
        ops::jobs::finish(&f.state.db, &claim, db::now(), true)
            .await
            .unwrap();
    }
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM channel_probes WHERE success=1 AND output_tokens=1"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM provider_quota_snapshots WHERE remaining_micros=0"
        )
        .await,
        1
    );
    sql(&f,"INSERT INTO operation_schedules(id,project_id,kind,payload_json,interval_secs,next_run_at) VALUES('bad',?,'automatic_backup','{}',60,0),('good',?,'probe',?,60,0)",vec![db::DEFAULT_PROJECT_ID.into(),db::DEFAULT_PROJECT_ID.into(),json!({"provider_id":f.providers[1]}).to_string().into()]).await;
    assert_eq!(
        ops::jobs::enqueue_due(&f.state.db, db::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(count(&f,"SELECT COUNT(*) AS n FROM operation_jobs WHERE kind='invalid_schedule' AND status='failed'").await,1);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM operation_jobs WHERE kind='probe' AND status='pending'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn cc_switch_response_ids_deduplicate_settlement_without_cross_key_collisions() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(||async{Json(json!({"id":"vendor-stable-id","choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}))}))).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    for _ in 0..2 {
        assert_eq!(
            request(&f, "/v1/chat/completions", chat()).await.status(),
            StatusCode::OK
        )
    }
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        12
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM usage_logs WHERE settlement_kind='duplicate'"
        )
        .await,
        1
    );
    let (_, token) = db::create_api_key(
        &f.state.db,
        &ApiKeyInput {
            name: "other-dedup-scope".into(),
            budget_micros: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let response = router(f.state.clone())
        .oneshot(
            Request::post("/v1/chat/completions")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(chat().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        24
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM provider_response_settlements WHERE length(fingerprint)=64"
        )
        .await,
        2
    );
}
#[tokio::test]
async fn cc_switch_compressed_json_and_sse_are_decoded_before_terminal_accounting() {
    use std::io::Write;
    fn gzip(value: &str) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(value.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }
    let f=fixture(Router::new().route("/v1/chat/completions",post(|Json(body):Json<Value>|async move{if body["stream"]==true {([(header::CONTENT_ENCODING,"gzip"),(header::CONTENT_TYPE,"text/event-stream")],gzip("data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n")).into_response()}else{([(header::CONTENT_ENCODING,"gzip"),(header::CONTENT_TYPE,"application/json")],gzip(&json!({"choices":[{"message":{"content":"decoded"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}).to_string())).into_response()}}))).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["choices"][0]["message"]["content"],
        "decoded"
    );
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("[DONE]"));
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        24
    );
}

#[tokio::test]
async fn task5_usage_and_request_history_backup_roundtrip_preserves_costs() {
    let f = fixture(success()).await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    let resources = [
        "threads",
        "traces",
        "requests",
        "request_contents",
        "request_facts",
        "execution_facts",
        "request_executions",
        "usage_logs",
        "usage_cost_items",
        "provider_response_settlements",
    ];
    let artifact = ops::backup::export(
        &f.state,
        db::DEFAULT_PROJECT_ID,
        &Selection {
            resources: resources.iter().map(|v| v.to_string()).collect(),
        },
    )
    .await
    .unwrap();
    let spent = count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await;
    sql(&f, "DELETE FROM request_facts", vec![]).await;
    sql(&f, "DELETE FROM traces", vec![]).await;
    assert!(
        ops::backup::restore(&f.state, db::DEFAULT_PROJECT_ID, &artifact, Conflict::Fail)
            .await
            .unwrap()
            > 0
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 1);
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM request_executions").await,
        1
    );
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        spent
    );
}

#[tokio::test]
async fn task5_operations_share_api_key_permissions_and_audit_service_subjects() {
    let f = fixture(success()).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/storage",
        db::DEFAULT_PROJECT_ID
    );
    let send = |method, path: String, body: Value| {
        router(f.state.clone()).oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {}", f.token))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
    };
    assert_eq!(
        send(http::Method::GET, path.clone(), Value::Null)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    sql(
        &f,
        "UPDATE api_keys SET scopes='[\"project:read\"]'",
        vec![],
    )
    .await;
    assert_eq!(
        send(http::Method::GET, path.clone(), Value::Null)
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let body = json!({"name":"service-target","config":{"kind":"local","directory":"service"}});
    assert_eq!(
        send(http::Method::POST, path.clone(), body.clone())
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    sql(
        &f,
        "UPDATE api_keys SET scopes='[\"project:manage\"]'",
        vec![],
    )
    .await;
    assert_eq!(
        send(http::Method::POST, path, body).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(count(&f,"SELECT COUNT(*) AS n FROM audit_events WHERE json_extract(details,'$.principal_kind')='api_key' AND actor_user_id IS NULL").await,1);
    assert_eq!(
        send(
            http::Method::GET,
            "/api/admin/v1/projects/other/operations/storage".into(),
            Value::Null
        )
        .await
        .unwrap()
        .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn task5_quota_collection_never_forwards_structured_secrets_or_changes_origin() {
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = calls.clone();
    let f = fixture(Router::new().fallback(move || {
        captured.fetch_add(1, Ordering::Relaxed);
        async { Json(json!({"data":{"limit_remaining":10}})) }
    }))
    .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let captured = calls.clone();
    let other = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(move || {
                captured.fetch_add(1, Ordering::Relaxed);
                async { Json(json!({"data":{"limit_remaining":10}})) }
            }),
        )
        .await
        .unwrap()
    });
    ops::jobs::enqueue(
        &f.state.db,
        Some(db::DEFAULT_PROJECT_ID),
        "quota",
        "wrong-origin",
        &json!({"provider_id":f.providers[0],"path":format!("/\\{address}/key")}),
        db::now(),
    )
    .await
    .unwrap();
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert!(ops::runtime::execute(&f.state, &claim).await.is_err());
    sql(
        &f,
        "UPDATE providers SET kind='bedrock' WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    sql(&f,"UPDATE channel_credentials SET secret_envelope=? WHERE provider_id=?",vec![f.state.secrets.encrypt(&json!({"access_key_id":"cloud-access-sentinel","secret_access_key":"cloud-private-sentinel"}).to_string()).unwrap().into(),f.providers[0].clone().into()]).await;
    ops::jobs::enqueue(
        &f.state.db,
        Some(db::DEFAULT_PROJECT_ID),
        "quota",
        "cloud-credentials",
        &json!({"provider_id":f.providers[0]}),
        db::now(),
    )
    .await
    .unwrap();
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert!(ops::runtime::execute(&f.state, &claim).await.is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    other.abort();
}

#[tokio::test]
async fn task5_quota_sequence_overrides_wall_clock_order() {
    let f = fixture(success()).await;
    for (id, sequence, remaining, collected) in [("older", 1, 0, 200), ("newer", 2, 100, 100)] {
        sql(&f,"INSERT INTO provider_quota_snapshots(id,provider_id,remaining_micros,collected_at,sequence) VALUES(?,?,?,?,?)",vec![id.into(),f.providers[0].clone().into(),remaining.into(),collected.into(),sequence.into()]).await;
    }
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    let row = f
        .state
        .db
        .query_one(ops::sql("SELECT provider_id FROM execution_facts", vec![]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<String>("", "provider_id").unwrap(),
        f.providers[0]
    );
}
