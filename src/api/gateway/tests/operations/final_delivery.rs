use super::*;

async fn model_id(f: &Fixture, provider: &str) -> String {
    f.state
        .db
        .query_one(ops::sql(
            "SELECT id FROM models WHERE provider_id=? LIMIT 1",
            vec![provider.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap()
}
async fn usage_row(f: &Fixture) -> sea_orm::QueryResult {
    f.state.db.query_one(ops::sql("SELECT u.*,e.reserved_micros FROM usage_logs u JOIN execution_facts e ON e.id=u.execution_id ORDER BY u.created_at DESC LIMIT 1",vec![])).await.unwrap().unwrap()
}
fn flags(row: &sea_orm::QueryResult) -> Value {
    serde_json::from_str(&row.try_get::<String>("", "usage_measurement_json").unwrap()).unwrap()
}

#[tokio::test]
async fn final_fix_anthropic_bridge_keeps_actual_source_presence_and_budget_hold() {
    for native in [
        None,
        Some(json!({"input_tokens":3,"output_tokens":7})),
        Some(
            json!({"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}),
        ),
        Some(
            json!({"input_tokens":3,"output_tokens":7,"cache_read_input_tokens":2,"cache_creation_input_tokens":1}),
        ),
        Some(
            json!({"input_tokens":i64::MAX,"output_tokens":7,"cache_read_input_tokens":1,"cache_creation_input_tokens":1}),
        ),
    ] {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (start_tx, release_rx) = tokio::sync::oneshot::channel();
        let started = Arc::new(Mutex::new(Some(started_tx)));
        let release = Arc::new(Mutex::new(Some(release_rx)));
        let mut raw = json!({"id":"bridge-source-id","type":"message","role":"assistant","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn"});
        if let Some(value) = native.clone() {
            raw["usage"] = value;
        }
        let f = fixture(Router::new().fallback(post(move || {
            let raw = raw.clone();
            let started = started.clone();
            let release = release.clone();
            async move {
                started.lock().await.take().unwrap().send(()).unwrap();
                release.lock().await.take().unwrap().await.unwrap();
                Json(raw)
            }
        })))
        .await;
        sql(&f,"UPDATE providers SET kind='anthropic',enabled=CASE WHEN id=? THEN 1 ELSE 0 END; UPDATE models SET pricing_configured=1,input_price_micros=1000000,output_price_micros=1000000; UPDATE api_keys SET budget_micros=100000",vec![f.providers[0].clone().into()]).await;
        let requested = request(
            &f,
            "/v1/chat/completions",
            json!({"model":"public","messages":[{"role":"user","content":"hello"}],"max_tokens":16}),
        );
        let observed = async {
            started_rx.await.unwrap();
            assert!(count(&f,"SELECT SUM(reserved_micros) AS n FROM execution_facts WHERE status='running' AND contacted=1").await>0);
            assert_eq!(
                count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
                0
            );
            start_tx.send(()).unwrap();
        };
        let (response, ()) = tokio::join!(requested, observed);
        assert_eq!(response.status(), StatusCode::OK);
        let caller = json_body(response).await;
        assert_eq!(caller["choices"][0]["message"]["content"], "OK");
        assert_eq!(caller["id"], "bridge-source-id");
        let row = usage_row(&f).await;
        let measured = flags(&row);
        let full = native.as_ref().is_some_and(|v| {
            v["cache_read_input_tokens"].is_i64() && v["input_tokens"] != i64::MAX
        });
        let valid_output = native
            .as_ref()
            .is_some_and(|v| v["output_tokens"].is_i64() && v["input_tokens"] != i64::MAX);
        assert_eq!(measured["input_tokens"], full);
        assert_eq!(measured["output_tokens"], valid_output);
        assert_eq!(measured["cache_read_tokens"], full);
        assert_eq!(measured["cache_write_tokens"], full);
        assert_eq!(
            row.try_get::<String>("", "pricing_status").unwrap(),
            if full { "priced" } else { "incomplete_usage" }
        );
        assert_eq!(
            row.try_get::<String>("", "settlement_kind").unwrap(),
            if full { "reported" } else { "conservative" }
        );
        if full {
            assert_eq!(
                row.try_get::<i64>("", "total_cost_micros").unwrap(),
                if native.as_ref().unwrap()["output_tokens"] == 0 {
                    0
                } else {
                    13
                }
            );
        }
        if !full {
            assert_eq!(
                row.try_get::<i64>("", "total_cost_micros").unwrap(),
                row.try_get::<i64>("", "reserved_micros").unwrap()
            );
        }
    }
}

#[tokio::test]
async fn final_fix_known_constant_and_zero_prices_do_not_fabricate_reported_usage() {
    for case in ["flat", "zero", "mixed", "invalid"] {
        let case_owned = case.to_owned();
        let f=fixture(Router::new().fallback(post(move||{let case=case_owned.clone();async move{let mut body=json!({"id":"fixed-source-id","choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}]});if case=="invalid"{body["usage"]=json!({"completion_tokens":-1});}Json(body)}}))).await;
        sql(&f,"UPDATE providers SET enabled=CASE WHEN id=? THEN 1 ELSE 0 END; UPDATE models SET pricing_configured=1; UPDATE api_keys SET budget_micros=100000",vec![f.providers[0].clone().into()]).await;
        let cookie = owner(&f).await;
        if case != "zero" {
            let model = model_id(&f, &f.providers[0]).await;
            let mut components = vec![json!({"kind":"flat","unit_size":1,"unit_price_micros":25})];
            if case == "mixed" {
                components.push(json!({"kind":"output","unit_size":1,"unit_price_micros":1}));
            }
            let response = admin(
                &f,
                &cookie,
                http::Method::POST,
                &format!(
                    "/api/admin/v1/projects/{}/operations/prices",
                    db::DEFAULT_PROJECT_ID
                ),
                json!({"model_id":model,"provider_id":f.providers[0],"components":components}),
                true,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        assert_eq!(request(&f,"/v1/chat/completions",json!({"model":"public","messages":[{"role":"user","content":"hello"}],"max_tokens":16})).await.status(),StatusCode::OK);
        let row = usage_row(&f).await;
        let complete = matches!(case, "flat" | "zero");
        assert_eq!(
            row.try_get::<String>("", "pricing_status").unwrap(),
            if case == "zero" {
                "explicit_free"
            } else if complete {
                "priced"
            } else {
                "incomplete_usage"
            }
        );
        assert_eq!(
            row.try_get::<String>("", "settlement_kind").unwrap(),
            if complete {
                "unreported"
            } else {
                "conservative"
            }
        );
        assert_eq!(flags(&row)["input_tokens"], false);
        assert_eq!(flags(&row)["output_tokens"], false);
        if complete {
            assert_eq!(
                row.try_get::<i64>("", "total_cost_micros").unwrap(),
                if case == "flat" { 25 } else { 0 }
            );
            let analytics = json_body(
                admin(
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
                .await,
            )
            .await;
            let actual = &analytics["data"][0];
            assert_eq!(actual["cost_micros"], if case == "flat" { 25 } else { 0 });
            assert_eq!(actual["measured_cost_count"], 1);
            assert!(actual["input_tokens"].is_null());
            assert!(actual["output_tokens"].is_null());
            let expected = if case == "flat" { 25 } else { 0 };
            f.state.observations.flush().await;
            let live = f
                .state
                .observations
                .summary_for(
                    db::DEFAULT_PROJECT_ID.into(),
                    crate::observability::SummaryFilter::default(),
                )
                .await
                .unwrap();
            assert_eq!(live.cost_micros, Some(expected));
            assert_eq!(live.input_tokens, None);
            assert_eq!(live.output_tokens, None);
            assert_eq!(live.measured_cost_count, 1);
            assert!(f.state.observations.clear_for_restore().await);
            assert_eq!(
                crate::operations::instance_backup::rebuild_projection(&f.state)
                    .await
                    .unwrap(),
                1
            );
            let rebuilt = f
                .state
                .observations
                .summary_for(
                    db::DEFAULT_PROJECT_ID.into(),
                    crate::observability::SummaryFilter::default(),
                )
                .await
                .unwrap();
            assert_eq!(rebuilt.cost_micros, live.cost_micros);
            assert_eq!(rebuilt.input_tokens, None);
            assert_eq!(rebuilt.output_tokens, None);
        }
    }
}

#[tokio::test]
async fn final_fix_probe_provider_filters_scope_page_and_total_before_selection() {
    let f = fixture(success()).await;
    sql(
        &f,
        "INSERT OR IGNORE INTO channel_settings(provider_id) SELECT id FROM providers",
        vec![],
    )
    .await;
    let cookie = owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/operations",
        db::DEFAULT_PROJECT_ID
    );
    for resource in ["credentials", "channel-settings"] {
        let response = admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/{resource}?provider_id={}&limit=1", f.providers[0]),
            Value::Null,
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["total"], 1);
        assert_eq!(body["data"][0]["provider_id"], f.providers[0]);
        let next = json_body(
            admin(
                &f,
                &cookie,
                http::Method::GET,
                &format!(
                    "{base}/{resource}?provider_id={}&limit=1&offset=1",
                    f.providers[0]
                ),
                Value::Null,
                false,
            )
            .await,
        )
        .await;
        assert_eq!(next["total"], 1);
        assert_eq!(next["data"], json!([]));
    }
    let selected = model_id(&f, &f.providers[0]).await;
    let queued = admin(&f, &cookie, http::Method::POST, &format!("{base}/probe"), json!({"provider_id":f.providers[0],"model_id":selected,"credential_id":f.providers[0],"endpoint":"/v1/chat/completions","stream":false}), true).await;
    assert_eq!(queued.status(), StatusCode::OK);
    let job = ops::jobs::claim(&f.state.db, "picker-contract", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    let submitted: Value = serde_json::from_str(&job.payload_json).unwrap();
    assert_eq!(submitted["credential_id"], f.providers[0]);
    assert_eq!(submitted["model_id"], selected);
    assert_eq!(submitted["endpoint"], "/v1/chat/completions");
    assert_eq!(submitted["stream"], false);
    ops::runtime::execute(&f.state, &job).await.unwrap();
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM channel_probes WHERE success=1"
        )
        .await,
        1
    );
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/models?provider_id={}", f.providers[0]),
            Value::Null,
            false
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let created = admin(
        &f,
        &cookie,
        http::Method::POST,
        "/api/admin/v1/projects",
        json!({"name":"Foreign","slug":"foreign-filter"}),
        true,
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let foreign = json_body(created).await;
    sql(
        &f,
        "UPDATE providers SET project_id=? WHERE id=?",
        vec![
            foreign["id"].as_str().unwrap().into(),
            f.providers[1].clone().into(),
        ],
    )
    .await;
    for resource in ["credentials", "channel-settings"] {
        assert_eq!(
            admin(
                &f,
                &cookie,
                http::Method::GET,
                &format!("{base}/{resource}?provider_id={}", f.providers[1]),
                Value::Null,
                false
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn final_fix_foreign_chat_text_does_not_establish_native_probe_output_or_gateway_timing() {
    for visible in [false, true] {
        let native = if visible {
            json!({"content":"OK"})
        } else {
            json!({"role":"assistant"})
        };
        let stream = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":native}],"candidates":[{"content":{"parts":[{"text":"foreign"}]}}]}),
            json!({"choices":[],"usage":{"prompt_tokens":0,"completion_tokens":0}})
        );
        let f = fixture(Router::new().fallback(post(move || {
            let stream = stream.clone();
            async move { ([(header::CONTENT_TYPE, "text/event-stream")], stream) }
        })))
        .await;
        let model = model_id(&f, &f.providers[0]).await;
        ops::jobs::enqueue(&f.state.db,Some(db::DEFAULT_PROJECT_ID),"probe","final-source-text",&json!({"provider_id":f.providers[0],"model_id":model,"endpoint":"/v1/chat/completions","stream":true}),db::now()).await.unwrap();
        let claim = ops::jobs::claim(&f.state.db, "final-worker", db::now(), 120)
            .await
            .unwrap()
            .unwrap();
        let execution = ops::runtime::execute(&f.state, &claim).await;
        assert_eq!(execution.is_ok(), visible);
        let probe = f
            .state
            .db
            .query_one(ops::sql(
                "SELECT success,first_text_ms FROM channel_probes",
                vec![],
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(probe.try_get::<bool>("", "success").unwrap(), visible);
        assert_eq!(
            probe
                .try_get::<Option<i64>>("", "first_text_ms")
                .unwrap()
                .is_some(),
            visible
        );
        let response = request(
            &f,
            "/v1/chat/completions",
            json!({"model":"public","messages":[{"role":"user","content":"hello"}],"stream":true}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("foreign"));
        let row = f
            .state
            .db
            .query_one(ops::sql(
                "SELECT first_text_ms FROM request_executions LIMIT 1",
                vec![],
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.try_get::<Option<i64>>("", "first_text_ms")
                .unwrap()
                .is_some(),
            visible
        );
    }
}

#[tokio::test]
async fn final_fix_context_uses_only_a_configured_supported_estimator() {
    let configured = std::env::var_os("PANGOLIN_TOKENIZER_CL100K").is_some();
    let payload = json!({"model":"gpt-4","messages":[{"role":"user","content":"hello ".repeat(40)}],"max_tokens":16});
    let estimate = crate::providers::tokens::input_tokens("openai", "gpt-4", &payload);
    if configured {
        assert_eq!(estimate, Some(48));
    } else {
        assert_eq!(estimate, None);
    }
    for context in [32, 64, 65] {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let f=fixture(Router::new().fallback(post(move||{let seen=seen.clone();async move{seen.fetch_add(1,Ordering::Relaxed);Json(json!({"choices":[{"message":{"content":"OK"}}],"usage":{"prompt_tokens":48,"completion_tokens":1}}))}}))).await;
        sql(&f,"UPDATE providers SET enabled=CASE WHEN id=? THEN 1 ELSE 0 END; UPDATE models SET upstream_name='gpt-4',catalog_metadata_json=?",vec![f.providers[0].clone().into(),json!({"catalog_version":"test","card":{"limits":{"context":context,"output":16}}}).to_string().into()]).await;
        let mut body = payload.clone();
        body["model"] = json!("public");
        let response = request(&f, "/v1/chat/completions", body).await;
        let rejected = configured && context < 64;
        assert_eq!(calls.load(Ordering::Relaxed), if rejected { 0 } else { 1 });
        assert_eq!(response.status() == StatusCode::OK, !rejected);
    }
}

#[tokio::test]
async fn final_fix_constant_fee_missing_protocol_completion_stays_conservative() {
    for streamed in [false, true] {
        let f = fixture(Router::new().fallback(post(move || async move {
            if streamed {
                (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n".to_owned(),
                )
            } else {
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    json!({"foreign":"not a completed native reply"}).to_string(),
                )
            }
        })))
        .await;
        sql(&f,"UPDATE providers SET enabled=CASE WHEN id=? THEN 1 ELSE 0 END; UPDATE api_keys SET budget_micros=100000",vec![f.providers[0].clone().into()]).await;
        let cookie = owner(&f).await;
        let model = model_id(&f, &f.providers[0]).await;
        assert_eq!(admin(&f,&cookie,http::Method::POST,&format!("/api/admin/v1/projects/{}/operations/prices",db::DEFAULT_PROJECT_ID),json!({"provider_id":f.providers[0],"model_id":model,"components":[{"kind":"flat","unit_size":1,"unit_price_micros":25}]}),true).await.status(),StatusCode::OK);
        let reply = request(
            &f,
            "/v1/chat/completions",
            json!({"model":"public","messages":[{"role":"user","content":"hi"}],"stream":streamed}),
        )
        .await;
        assert_eq!(reply.status(), StatusCode::OK);
        let body = to_bytes(reply.into_body(), usize::MAX).await;
        if streamed {
            if let Ok(body) = body {
                assert!(String::from_utf8_lossy(&body).contains("OK"));
            }
        } else {
            assert!(String::from_utf8_lossy(&body.unwrap()).contains("foreign"))
        };
        let row = usage_row(&f).await;
        assert_eq!(
            row.try_get::<String>("", "pricing_status").unwrap(),
            "incomplete_usage"
        );
        assert_eq!(
            row.try_get::<String>("", "settlement_kind").unwrap(),
            "conservative"
        );
        assert_eq!(row.try_get::<i64>("", "total_cost_micros").unwrap(), 25);
        assert_eq!(flags(&row)["output_tokens"], false);
    }
}

#[tokio::test]
async fn final_fix_context_wider_candidate_and_no_requested_output_preserve_eligibility() {
    let configured = std::env::var_os("PANGOLIN_TOKENIZER_CL100K").is_some();
    let text = json!({"model":"public","messages":[{"role":"user","content":"hello ".repeat(40)}]});
    for alternative in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let f=fixture(Router::new().fallback(post(move||{let seen=seen.clone();async move{seen.fetch_add(1,Ordering::Relaxed);Json(json!({"choices":[{"message":{"content":"OK"}}],"usage":{"prompt_tokens":48,"completion_tokens":0}}))}}))).await;
        for (index, provider) in f.providers.iter().enumerate() {
            sql(&f,"UPDATE models SET upstream_name='gpt-4',catalog_metadata_json=? WHERE provider_id=?",vec![json!({"card":{"limits":{"context":if alternative&&index==0{32}else if alternative{64}else{48},"output":16}}}).to_string().into(),provider.clone().into()]).await;
        }
        if !alternative {
            sql(
                &f,
                "UPDATE providers SET enabled=CASE WHEN id=? THEN 1 ELSE 0 END",
                vec![f.providers[0].clone().into()],
            )
            .await;
        }
        let mut body = text.clone();
        if alternative {
            body["max_tokens"] = json!(16);
        }
        assert_eq!(
            request(&f, "/v1/chat/completions", body).await.status(),
            StatusCode::OK
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let actual = f
            .state
            .db
            .query_one(ops::sql("SELECT provider_id FROM execution_facts", vec![]))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "provider_id")
            .unwrap();
        assert_eq!(actual, f.providers[usize::from(alternative && configured)]);
    }
    // A known context cannot convert unknown or multimedia input estimates into refusals.
    for (upstream, content) in [
        ("unrecognized-model", json!("hello ".repeat(40))),
        (
            "gpt-4",
            json!([{"type":"image_url","image_url":{"url":"data:image/png;base64,AQ=="}}]),
        ),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let f = fixture(Router::new().fallback(post(move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::Relaxed);
                Json(json!({"choices":[{"message":{"content":"OK"}}]}))
            }
        })))
        .await;
        sql(&f,"UPDATE providers SET enabled=CASE WHEN id=? THEN 1 ELSE 0 END; UPDATE models SET upstream_name=?,catalog_metadata_json=?",vec![f.providers[0].clone().into(),upstream.into(),json!({"card":{"limits":{"context":1},"capabilities":{"vision":true}}}).to_string().into()]).await;
        assert_eq!(request(&f,"/v1/chat/completions",json!({"model":"public","messages":[{"role":"user","content":content}],"max_tokens":16})).await.status(),StatusCode::OK);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn final_fix_special_bridge_native_envelope_excludes_foreign_counter_aliases() {
    for actual in [false, true] {
        let raw = if actual {
            json!({"id":"native-foreign-control","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":7,"cache_read_input_tokens":2,"cache_creation_input_tokens":1,"completion_tokens":-1},"usageMetadata":{"totalTokenCount":999}})
        } else {
            json!({"id":"native-foreign-control","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"prompt_tokens":3,"completion_tokens":7}})
        };
        let f = fixture(Router::new().fallback(post(move || {
            let raw = raw.clone();
            async move { Json(raw) }
        })))
        .await;
        sql(&f,"UPDATE providers SET kind='anthropic',enabled=CASE WHEN id=? THEN 1 ELSE 0 END; UPDATE models SET pricing_configured=1,input_price_micros=0,output_price_micros=1000000; UPDATE api_keys SET budget_micros=100000",vec![f.providers[0].clone().into()]).await;
        let response = request(
            &f,
            "/v1/chat/completions",
            json!({"model":"public","messages":[{"role":"user","content":"hi"}],"max_tokens":16}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["choices"][0]["message"]["content"], "OK");
        let row = usage_row(&f).await;
        assert_eq!(flags(&row)["output_tokens"], actual);
        assert_eq!(flags(&row)["cache_read_tokens"], actual);
        assert_eq!(
            row.try_get::<String>("", "pricing_status").unwrap(),
            if actual { "priced" } else { "incomplete_usage" }
        );
        assert_eq!(
            row.try_get::<i64>("", "total_cost_micros").unwrap(),
            if actual {
                7
            } else {
                row.try_get::<i64>("", "reserved_micros").unwrap()
            }
        );
    }
}

#[tokio::test]
async fn final_fix_bridge_total_overflow_keeps_independent_quantities_and_conservative_money() {
    let f=fixture(Router::new().fallback(post(||async{Json(json!({"id":"large-native-counts","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":i64::MAX,"output_tokens":7,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}))}))).await;
    sql(&f,"UPDATE providers SET kind='anthropic',enabled=CASE WHEN id=? THEN 1 ELSE 0 END; UPDATE models SET pricing_configured=1,input_price_micros=1000000,output_price_micros=1000000; UPDATE api_keys SET budget_micros=100000",vec![f.providers[0].clone().into()]).await;
    let response = request(
        &f,
        "/v1/chat/completions",
        json!({"model":"public","messages":[{"role":"user","content":"hi"}],"max_tokens":16}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["choices"][0]["message"]["content"], "OK");
    assert_eq!(
        body["usage"]["total_tokens"].as_u64(),
        Some(i64::MAX as u64 + 7)
    );
    let row = usage_row(&f).await;
    assert_eq!(flags(&row)["input_tokens"], true);
    assert_eq!(flags(&row)["output_tokens"], true);
    assert_eq!(
        row.try_get::<String>("", "pricing_status").unwrap(),
        "incomplete_usage"
    );
    assert_eq!(
        row.try_get::<String>("", "settlement_kind").unwrap(),
        "conservative"
    );
    assert_eq!(
        row.try_get::<i64>("", "total_cost_micros").unwrap(),
        row.try_get::<i64>("", "reserved_micros").unwrap()
    );
}

#[tokio::test]
async fn final_fix_constant_stream_requires_successful_terminal_without_fabricated_counters() {
    for success in [false, true] {
        let end = if success {
            "data: [DONE]\n\n"
        } else {
            "event: error\ndata: {\"error\":{\"message\":\"synthetic failure\"}}\n\n"
        };
        let wire = format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"OK\"}}}}]}}\n\n{end}");
        let f = fixture(Router::new().fallback(post(move || {
            let wire = wire.clone();
            async move { ([(header::CONTENT_TYPE, "text/event-stream")], wire) }
        })))
        .await;
        sql(&f,"UPDATE providers SET enabled=CASE WHEN id=? THEN 1 ELSE 0 END; UPDATE api_keys SET budget_micros=100000",vec![f.providers[0].clone().into()]).await;
        let cookie = owner(&f).await;
        let model = model_id(&f, &f.providers[0]).await;
        assert_eq!(admin(&f,&cookie,http::Method::POST,&format!("/api/admin/v1/projects/{}/operations/prices",db::DEFAULT_PROJECT_ID),json!({"provider_id":f.providers[0],"model_id":model,"components":[{"kind":"flat","unit_size":1,"unit_price_micros":25}]}),true).await.status(),StatusCode::OK);
        let response = request(
            &f,
            "/v1/chat/completions",
            json!({"model":"public","messages":[{"role":"user","content":"hi"}],"stream":true}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("OK"));
        let row = usage_row(&f).await;
        assert_eq!(
            row.try_get::<String>("", "pricing_status").unwrap(),
            if success {
                "priced"
            } else {
                "incomplete_usage"
            }
        );
        assert_eq!(
            row.try_get::<String>("", "settlement_kind").unwrap(),
            if success {
                "unreported"
            } else {
                "conservative"
            }
        );
        assert_eq!(row.try_get::<i64>("", "total_cost_micros").unwrap(), 25);
        assert_eq!(flags(&row)["input_tokens"], false);
        assert_eq!(flags(&row)["output_tokens"], false);
        assert_eq!(
            count(&f, "SELECT COUNT(*) AS n FROM execution_facts").await,
            1
        );
    }
}
