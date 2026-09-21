use super::*;

mod auth_audit;
mod catalog;
mod instance_backup;
mod isolation;
mod operations;
mod protocols;
mod retention;
mod review;
mod task4_review;
mod task5_review;
use axum::{
    body::{Bytes, to_bytes},
    http::Request,
};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

struct Fixture {
    state: AppState,
    token: String,
    providers: Vec<String>,
    server: tokio::task::JoinHandle<()>,
    _directory: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn fixture(mock: Router) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let directory = tempfile::tempdir().unwrap();
    let database = db::connect("sqlite::memory:").await.unwrap();
    let secrets = SecretBox::load(directory.path(), None).unwrap();
    let (_, token) = db::create_api_key(
        &database,
        &ApiKeyInput {
            name: "test".into(),
            budget_micros: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut providers = vec![];
    for (index, name) in ["a", "b"].iter().enumerate() {
        let provider = db::create_provider(
            &database,
            &ProviderInput {
                name: name.to_string(),
                kind: "openai".into(),
                base_url: format!("http://{address}"),
                api_key: String::new(),
            },
            secrets.encrypt(&format!("{name}-secret")).unwrap(),
        )
        .await
        .unwrap();
        db::create_model(
            &database,
            &ModelInput {
                provider_id: provider.id.clone(),
                public_name: "public".into(),
                upstream_name: format!("{name}-model"),
                capabilities: None,
                input_price_micros: None,
                output_price_micros: None,
                priority: Some(index as i32),
            },
            db::DEFAULT_PROJECT_ID,
        )
        .await
        .unwrap();
        providers.push(provider.id);
    }
    let observations = ObservationStore::open(directory.path().join("events.duckdb"), 30).unwrap();
    Fixture {
        state: AppState {
            db: database,
            config: Arc::new(Config {
                bind: "127.0.0.1:0".parse().unwrap(),
                data_dir: directory.path().into(),
                database_url: "sqlite::memory:".into(),
                observation_path: directory.path().join("events.duckdb"),
                observation_retention_days: 30,
                public_url: None,
                session_secure: false,
                capture_payloads: false,
                upstream_timeout: Duration::from_secs(3),
                admin_email: None,
                admin_password: None,
                master_key: None,
            }),
            secrets,
            observations,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            oidc_client: reqwest::Client::new(),
            budget_locks: Arc::new(Mutex::new(HashMap::new())),
            maintenance: Arc::new(tokio::sync::RwLock::new(())),
            orchestrator: Arc::new(orchestration::Runtime::default()),
        },
        token,
        providers,
        server,
        _directory: directory,
    }
}

async fn request(f: &Fixture, endpoint: &str, payload: Value) -> Response {
    router(f.state.clone())
        .oneshot(
            Request::post(endpoint)
                .header("authorization", format!("Bearer {}", f.token))
                .header("content-type", "application/json")
                .header("x-trace-id", "trace-test")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn sql(f: &Fixture, query: &str, values: Vec<sea_orm::Value>) {
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            query,
            values,
        ))
        .await
        .unwrap();
}

fn chat() -> Value {
    json!({"model":"public","messages":[{"role":"user","content":"hello"}]})
}
fn streaming() -> Value {
    let mut body = chat();
    body["stream"] = json!(true);
    body
}
fn ok_stream() -> Response {
    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
    )
        .into_response()
}

#[tokio::test]
async fn retries_configured_status_and_preserves_fresh_override_payload() {
    let seen = Arc::new(Mutex::new(vec![]));
    let captured = seen.clone();
    let mock=Router::new().route("/v1/chat/completions",post(move|Json(body):Json<Value>|{let seen=captured.clone();async move{
        seen.lock().await.push(body.clone());if body["model"]=="a-model"{(StatusCode::TOO_MANY_REQUESTS,Json(json!({"error":"busy"}))).into_response()}else{Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}})).into_response()}
    }}));
    let f = fixture(mock).await;
    sql(
        &f,
        "UPDATE channel_settings SET parameter_overrides_json=? WHERE provider_id=?",
        vec![
            json!({"version":1,"operations":[{"merge":{"only_first":true}}]})
                .to_string()
                .into(),
            f.providers[0].clone().into(),
        ],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-trace-id"], "trace-test");
    let bodies = seen.lock().await;
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0]["only_first"], true);
    assert!(bodies[1].get("only_first").is_none());
}

#[tokio::test]
async fn nonretryable_status_stops_and_normalizes_provider_secrets() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mock = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            count.fetch_add(1, Ordering::Relaxed);
            async { (StatusCode::BAD_REQUEST, Json(json!({"error":"a-secret"}))) }
        }),
    );
    let f = fixture(mock).await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("a-secret"));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn empty_first_stream_retries_before_downstream_commit() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mock = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(body): Json<Value>| {
            count.fetch_add(1, Ordering::Relaxed);
            async move {
                if body["model"] == "a-model" {
                    ([(header::CONTENT_TYPE, "text/event-stream")], "").into_response()
                } else {
                    ok_stream()
                }
            }
        }),
    );
    let f = fixture(mock).await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("[DONE]"));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn no_retry_after_first_event_and_eof_emits_a_terminal_error() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mock = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            count.fetch_add(1, Ordering::Relaxed);
            async {
                (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
                )
            }
        }),
    );
    let f = fixture(mock).await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    let mut stream = response.into_body().into_data_stream();
    let first = stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first).contains("partial"));
    let terminal = stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&terminal).contains("upstream request failed"));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    f.state.observations.flush().await;
    assert_eq!(f.state.observations.summary().await.unwrap().errors, 1);
}

#[tokio::test]
async fn first_event_timeout_fails_over_and_drop_releases_inflight_slot() {
    let mock=Router::new().route("/v1/chat/completions",post(|Json(body):Json<Value>|async move{if body["model"]=="a-model" {
        let stream=async_stream::stream! {tokio::time::sleep(Duration::from_secs(1)).await;yield Ok::<_,std::io::Error>(Bytes::from_static(b"data: [DONE]\n\n"));};
        ([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(stream)).into_response()
    }else{ok_stream()}}));
    let f = fixture(mock).await;
    sql(
        &f,
        "UPDATE channel_settings SET retry_statuses_json=? WHERE provider_id=?",
        vec![
            json!({"version":1,"first_event_timeout_ms":10})
                .to_string()
                .into(),
            f.providers[0].clone().into(),
        ],
    )
    .await;
    sql(
        &f,
        "UPDATE providers SET settings_json=? WHERE id=?",
        vec![
            json!({"version":1,"limits":{"concurrent":1}})
                .to_string()
                .into(),
            f.providers[1].clone().into(),
        ],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    // Drop before polling the downstream body: the captured guards must still release.
    drop(response);
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(to_bytes(response.into_body(), 4096).await.is_ok());
}

#[tokio::test]
async fn same_channel_retries_cannot_bypass_rpm_and_global_attempt_limit() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mock = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            count.fetch_add(1, Ordering::Relaxed);
            async { StatusCode::SERVICE_UNAVAILABLE }
        }),
    );
    let f = fixture(mock).await;
    sql(
        &f,
        "UPDATE channel_settings SET retry_statuses_json=?",
        vec![json!({"version":1,"attempts":10}).to_string().into()],
    )
    .await;
    sql(
        &f,
        "UPDATE providers SET settings_json=?",
        vec![json!({"version":1,"limits":{"rpm":1}}).to_string().into()],
    )
    .await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn responses_session_records_stream_terminal_before_disconnect_and_survives_restart() {
    let inputs = Arc::new(Mutex::new(vec![]));
    let captured = inputs.clone();
    let mock=Router::new().route("/v1/responses",post(move|Json(body):Json<Value>|{let captured=captured.clone();async move{
        captured.lock().await.push(body.clone());
        let result=json!({"id":"resp_saved","status":"completed","output":[{"type":"message","role":"assistant","content":"answer"}]});
        if body["stream"]==true {([(header::CONTENT_TYPE,"text/event-stream")],format!("event: response.completed\ndata: {}\n\n",json!({"type":"response.completed","response":result}))).into_response()}else{Json(result).into_response()}
    }}));
    let mut f = fixture(mock).await;
    let response = request(
        &f,
        "/v1/responses",
        json!({"model":"public","input":"one","stream":true}),
    )
    .await;
    let mut stream = response.into_body().into_data_stream();
    let terminal = stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&terminal).contains("response.completed"));
    drop(stream);
    f.state.orchestrator = Arc::new(orchestration::Runtime::default());
    let response = request(
        &f,
        "/v1/responses",
        json!({"model":"public","previous_response_id":"resp_saved","input":"two"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let inputs = inputs.lock().await;
    assert_eq!(inputs[1]["input"].as_array().unwrap().len(), 3);
    assert!(inputs[1].get("previous_response_id").is_none());
}

#[tokio::test]
async fn global_retry_budget_stops_high_per_channel_attempt_counts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mock = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            count.fetch_add(1, Ordering::Relaxed);
            async { StatusCode::SERVICE_UNAVAILABLE }
        }),
    );
    let f = fixture(mock).await;
    sql(
        &f,
        "UPDATE channel_settings SET retry_statuses_json=?",
        vec![json!({"version":1,"attempts":10}).to_string().into()],
    )
    .await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(calls.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn empty_success_policy_retries_content_but_accepts_tool_calls() {
    let mock=Router::new().route("/v1/chat/completions",post(|Json(body):Json<Value>|async move{
        if body["model"]=="a-model"{Json(json!({"choices":[{"message":{"content":""}}]}))}
        else{Json(json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"call","function":{"name":"lookup","arguments":"{}"}}]}}]}))}
    }));
    let f = fixture(mock).await;
    sql(
        &f,
        "UPDATE channel_settings SET retry_statuses_json=?",
        vec![json!({"version":1,"empty_success":true}).to_string().into()],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["choices"][0]["message"]["tool_calls"][0]["id"], "call");
}

#[tokio::test]
async fn anthropic_bridge_respects_endpoint_and_sanitized_header_overrides() {
    let mock=Router::new().route("/custom/messages",post(|headers:HeaderMap,Json(body):Json<Value>|async move{
        assert_eq!(headers["x-api-key"],"a-secret");assert_eq!(headers["x-model"],"public");assert_eq!(headers["x-trace-id"],"trace-test");assert_eq!(body["model"],"a-model");
        Json(json!({"id":"message","content":[{"type":"text","text":"answer"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}))
    }));
    let f = fixture(mock).await;
    sql(
        &f,
        "UPDATE providers SET kind='anthropic' WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    sql(&f,"UPDATE channel_settings SET endpoint_mappings_json=?,parameter_overrides_json=? WHERE provider_id=?",vec![json!({"version":1,"paths":{"/v1/chat/completions":"/custom/messages"}}).to_string().into(),json!({"version":1,"operations":[{"headers":{"x-model":{"$request":"/body/model"}}}]}).to_string().into(),f.providers[0].clone().into()]).await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "answer");
}
