use super::*;

#[tokio::test]
async fn review_bridge_reserves_the_actual_output_ceiling_under_key_and_channel_tpm() {
    let observed = Arc::new(Mutex::new(vec![]));
    let captured = observed.clone();
    let mock=Router::new().route("/v1/messages",post(move|Json(body):Json<Value>|{let captured=captured.clone();async move{
        captured.lock().await.push(body.clone());Json(json!({"id":"message","content":[{"type":"text","text":"answer"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}))
    }}));
    let f = fixture(mock).await;
    sql(
        &f,
        "UPDATE providers SET kind='anthropic',settings_json=?",
        vec![
            json!({"version":1,"limits":{"tpm":1000}})
                .to_string()
                .into(),
        ],
    )
    .await;
    sql(
        &f,
        "UPDATE projects SET settings_json=?",
        vec![
            json!({"version":1,"routing":{"version":1,"limits":{"tpm":1000}}})
                .to_string()
                .into(),
        ],
    )
    .await;
    let mut body = chat();
    body["max_completion_tokens"] = json!(10);
    assert_eq!(
        request(&f, "/v1/chat/completions", body).await.status(),
        StatusCode::OK
    );
    assert_eq!(observed.lock().await[0]["max_tokens"], 10);
    let mut body = chat();
    body["max_completion_tokens"] = json!(12);
    body["max_tokens"] = json!(99999);
    assert_eq!(
        request(&f, "/v1/chat/completions", body).await.status(),
        StatusCode::OK
    );
    assert_eq!(observed.lock().await[1]["max_tokens"], 12);
    let mut body = chat();
    body["max_tokens"] = json!(10);
    body["n"] = json!(2);
    assert_eq!(
        request(&f, "/v1/chat/completions", body).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(observed.lock().await.len(), 2);
}

#[tokio::test]
async fn review_chat_multiple_choices_must_fit_key_and_channel_tpm() {
    for key_limit in [true, false] {
        let calls = Arc::new(AtomicUsize::new(0));
        let hits = calls.clone();
        let f = fixture(Router::new().route(
            "/v1/chat/completions",
            post(move || {
                hits.fetch_add(1, Ordering::Relaxed);
                async { Json(json!({"choices":[]})) }
            }),
        ))
        .await;
        if key_limit {
            sql(
                &f,
                "UPDATE projects SET settings_json=?",
                vec![
                    json!({"version":1,"routing":{"version":1,"limits":{"tpm":1000}}})
                        .to_string()
                        .into(),
                ],
            )
            .await;
        } else {
            sql(
                &f,
                "UPDATE providers SET settings_json=?",
                vec![
                    json!({"version":1,"limits":{"tpm":1000}})
                        .to_string()
                        .into(),
                ],
            )
            .await;
        }
        let mut body = chat();
        body["max_tokens"] = json!(500);
        body["n"] = json!(3);
        assert_eq!(
            request(&f, "/v1/chat/completions", body).await.status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }
}

#[tokio::test]
async fn review_legacy_tool_override_is_rejected_before_upstream() {
    let calls = Arc::new(AtomicUsize::new(0));
    let hits = calls.clone();
    let f = fixture(Router::new().route(
        "/v1/chat/completions",
        post(move || {
            hits.fetch_add(1, Ordering::Relaxed);
            async { Json(json!({"choices":[]})) }
        }),
    ))
    .await;
    sql(
        &f,
        "UPDATE projects SET settings_json=?",
        vec![
            json!({"version":1,"routing":{"version":1,"allowed_tools":[]}})
                .to_string()
                .into(),
        ],
    )
    .await;
    sql(&f,"UPDATE channel_settings SET parameter_overrides_json=?",vec![json!({"version":1,"operations":[{"merge":{"functions":[{"name":"forbidden"}],"function_call":{"name":"forbidden"}}}]}).to_string().into()]).await;
    assert!(
        request(&f, "/v1/chat/completions", chat())
            .await
            .status()
            .is_client_error()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn review_responses_apply_only_current_prompt_once_across_turns() {
    let observed = Arc::new(Mutex::new(vec![]));
    let captured = observed.clone();
    let f=fixture(Router::new().route("/v1/responses",post(move|Json(body):Json<Value>|{let captured=captured.clone();async move{
        let mut history=captured.lock().await;history.push(body.clone());let response=json!({"id":format!("r{}",history.len()),"status":"completed","output":[{"type":"message","role":"assistant","content":"answer"}]});
        if body["stream"]==true {([(header::CONTENT_TYPE,"text/event-stream")],format!("event: response.completed\ndata: {}\n\n",json!({"type":"response.completed","response":response}))).into_response()}else{Json(response).into_response()}
    }}))).await;
    sql(&f,"INSERT INTO prompts(id,project_id,name,role,content,created_at,updated_at) VALUES('prompt',?,'Directive','system','first directive',0,0)",vec![db::DEFAULT_PROJECT_ID.into()]).await;
    for turn in 1..=4 {
        if turn == 3 {
            sql(&f, "UPDATE prompts SET content='new directive'", vec![]).await;
        }
        if turn == 4 {
            sql(&f, "UPDATE prompts SET enabled=0", vec![]).await;
        }
        let mut body = json!({"model":"public","input":format!("turn {turn}"),"stream":turn%2==0});
        if turn > 1 {
            body["previous_response_id"] = json!(format!("r{}", turn - 1));
        }
        let response = request(&f, "/v1/responses", body).await;
        assert_eq!(response.status(), StatusCode::OK);
        to_bytes(response.into_body(), 16384).await.unwrap();
    }
    let bodies = observed.lock().await;
    for (index, body) in bodies.iter().enumerate() {
        let system = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["role"] == "system")
            .collect::<Vec<_>>();
        assert_eq!(system.len(), usize::from(index < 3), "turn {}", index + 1);
        if index < 3 {
            assert_eq!(
                system[0]["content"],
                if index < 2 {
                    "first directive"
                } else {
                    "new directive"
                }
            );
        }
    }
}

fn held_response_stream(terminal: Value) -> Response {
    let output = async_stream::stream! {
        yield Ok::<_,std::io::Error>(Bytes::from_static(b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\n"));
        yield Ok(Bytes::from(format!("event: {}\ndata: {}\n\n",terminal["type"].as_str().unwrap(),terminal)));
        futures_util::future::pending::<()>().await;
    };
    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(output),
    )
        .into_response()
}

fn truncated_response_stream() -> Response {
    let output = async_stream::stream! {
        yield Ok::<_,std::io::Error>(Bytes::from_static(b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\n"));
    };
    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(output),
    )
        .into_response()
}

#[tokio::test]
async fn review_terminal_success_persists_and_releases_before_consumer_drop() {
    let f=fixture(Router::new().route("/v1/responses",post(||async{held_response_stream(json!({"type":"response.completed","response":{"id":"terminal","status":"completed","output":[]}}))}))).await;
    let response = request(
        &f,
        "/v1/responses",
        json!({"model":"public","input":"hi","stream":true}),
    )
    .await;
    let mut stream = response.into_body().into_data_stream();
    stream.next().await.unwrap().unwrap();
    let terminal = stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&terminal).contains("response.completed"));
    assert!(
        f.state
            .db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM response_sessions WHERE response_id='terminal'"
            ))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        f.state
            .orchestrator
            .metrics()
            .contains("pangolin_orchestration_inflight{resource=\"channel\"} 0")
    );
    assert!(
        f.state
            .orchestrator
            .metrics()
            .contains("pangolin_orchestration_completed_total{resource=\"channel\"} 1")
    );
    drop(stream);
}

#[tokio::test]
async fn review_post_commit_errors_obey_each_error_policy_without_retry() {
    for mode in ["normalized", "custom", "pass_through"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let hits = calls.clone();
        let f=fixture(Router::new().route("/v1/responses",post(move||{hits.fetch_add(1,Ordering::Relaxed);async{held_response_stream(json!({"type":"response.failed","response":{"status":"failed","error":{"message":"private-upstream-secret"}}}))}}))).await;
        sql(
            &f,
            "UPDATE channel_settings SET retry_statuses_json=?",
            vec![
                json!({"version":1,"error_mode":mode,"error_message":"configured public error"})
                    .to_string()
                    .into(),
            ],
        )
        .await;
        let response = request(
            &f,
            "/v1/responses",
            json!({"model":"public","input":"hi","stream":true}),
        )
        .await;
        let bytes = tokio::time::timeout(
            Duration::from_millis(100),
            to_bytes(response.into_body(), 4096),
        )
        .await
        .unwrap()
        .unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(
            text.contains("private-upstream-secret"),
            mode == "pass_through"
        );
        if mode == "custom" {
            assert!(text.contains("configured public error"));
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn review_post_commit_stream_interruptions_obey_each_error_policy() {
    for mode in ["normalized", "custom", "pass_through"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let hits = calls.clone();
        let f = fixture(Router::new().route(
            "/v1/responses",
            post(move || {
                hits.fetch_add(1, Ordering::Relaxed);
                async { truncated_response_stream() }
            }),
        ))
        .await;
        sql(
            &f,
            "UPDATE channel_settings SET retry_statuses_json=?",
            vec![
                json!({"version":1,"error_mode":mode,"error_message":"configured public error"})
                    .to_string()
                    .into(),
            ],
        )
        .await;
        let response = request(
            &f,
            "/v1/responses",
            json!({"model":"public","input":"hi","stream":true}),
        )
        .await;
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("response.created"));
        assert!(text.contains("response.failed"));
        match mode {
            "custom" => assert!(text.contains("configured public error")),
            "normalized" => assert!(text.contains("upstream request failed")),
            "pass_through" => assert!(text.contains("upstream stream interrupted")),
            _ => unreachable!(),
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(
            f.state
                .orchestrator
                .metrics()
                .contains("pangolin_orchestration_failed_total{resource=\"channel\"} 1")
        );
    }
}

#[tokio::test]
async fn review_response_terminal_failures_close_held_upstream_without_persisting() {
    for (kind, status) in [
        ("response.cancelled", "cancelled"),
        ("response.canceled", "canceled"),
        ("response.completed", "failed"),
        ("response.completed", "incomplete"),
        ("response.completed", "canceled"),
        ("response.completed", "cancelled"),
    ] {
        let terminal =
            json!({"type":kind,"response":{"id":"failed-response","status":status,"output":[]}});
        let calls = Arc::new(AtomicUsize::new(0));
        let hits = calls.clone();
        let f = fixture(Router::new().route(
            "/v1/responses",
            post(move || {
                let terminal = terminal.clone();
                hits.fetch_add(1, Ordering::Relaxed);
                async move { held_response_stream(terminal) }
            }),
        ))
        .await;
        let response = request(
            &f,
            "/v1/responses",
            json!({"model":"public","input":"hi","stream":true}),
        )
        .await;
        assert!(
            tokio::time::timeout(
                Duration::from_millis(100),
                to_bytes(response.into_body(), 4096)
            )
            .await
            .is_ok(),
            "terminal {kind}/{status} was not recognized"
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(
            f.state
                .db
                .query_one(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT id FROM response_sessions"
                ))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            f.state
                .orchestrator
                .metrics()
                .contains("pangolin_orchestration_failed_total{resource=\"channel\"} 1")
        );
    }
}

#[tokio::test]
async fn review_nonretryable_http_errors_fail_counters_sticky_and_half_open_probes() {
    for kind in ["openai", "anthropic"] {
        for status in [StatusCode::INTERNAL_SERVER_ERROR, StatusCode::UNAUTHORIZED] {
            let mock = Router::new()
                .route(
                    "/v1/chat/completions",
                    post(move || async move { (status, Json(json!({"error":"failure"}))) }),
                )
                .route(
                    "/v1/messages",
                    post(move || async move { (status, Json(json!({"error":"failure"}))) }),
                );
            let f = fixture(mock).await;
            sql(&f,"UPDATE providers SET kind=?,settings_json=?",vec![kind.into(),json!({"version":1,"circuit":{"enabled":true,"failures":1,"window_ms":1000,"recovery_ms":10}}).to_string().into()]).await;
            sql(
                &f,
                "UPDATE projects SET settings_json=?",
                vec![
                    json!({"version":1,"routing":{"version":1,"sticky":"trace"}})
                        .to_string()
                        .into(),
                ],
            )
            .await;
            sql(
                &f,
                "UPDATE channel_settings SET retry_statuses_json=?",
                vec![json!({"version":1,"statuses":[]}).to_string().into()],
            )
            .await;
            let key = db::authenticate_api_key(&f.state.db, &f.token, None)
                .await
                .unwrap()
                .unwrap();
            let headers = HeaderMap::from_iter([(
                http::HeaderName::from_static("x-trace-id"),
                HeaderValue::from_static("trace-test"),
            )]);
            let mut plan = orchestration::prepare(
                &f.state.db,
                &f.state.orchestrator,
                &key,
                orchestration::load_profile(&f.state.db, &key)
                    .await
                    .unwrap(),
                chat(),
                &headers,
                "/v1/chat/completions",
            )
            .await
            .unwrap();
            let first = plan.candidates[0].clone();
            f.state
                .orchestrator
                .enter_circuit(&first.circuit_id(), &first.circuit)
                .unwrap()
                .finish(false);
            tokio::time::sleep(Duration::from_millis(15)).await;
            assert_eq!(
                request(&f, "/v1/chat/completions", chat()).await.status(),
                status
            );
            assert!(
                f.state
                    .orchestrator
                    .metrics()
                    .contains("pangolin_orchestration_failed_total{resource=\"channel\"} 1"),
                "{kind}/{status}"
            );
            assert!(
                f.state
                    .orchestrator
                    .metrics()
                    .contains("pangolin_orchestration_failed_total{resource=\"key\"} 1")
            );
            assert_eq!(
                f.state.orchestrator.measured_latency(&first.resource_id()),
                0
            );
            assert!(
                !f.state
                    .orchestrator
                    .circuit_available(&first.circuit_id(), &first.circuit)
            );
            plan.candidates[1].priority = -1;
            f.state.orchestrator.order(
                "test",
                &mut plan.candidates,
                orchestration::policy::Strategy::Failover,
                plan.sticky.as_deref(),
            );
            assert_ne!(plan.candidates[0].provider_id, first.provider_id);
        }
    }
}
