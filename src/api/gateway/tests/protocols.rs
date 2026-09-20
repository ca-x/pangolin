//! Behavioral fixtures derived from AxonHub routes.go and transformer/API tests.
//! These are independent JSON contracts, not copies of the Go implementation.
use super::*;
use axum::extract::OriginalUri;

async fn all_capabilities(f: &Fixture) {
    sql(
        f,
        "UPDATE models SET capabilities=?",
        vec![
            json!([
                "chat",
                "completions",
                "responses",
                "messages",
                "embeddings",
                "moderations",
                "search",
                "images",
                "videos",
                "speech",
                "transcriptions",
                "translations",
                "rerank",
                "gemini"
            ])
            .to_string()
            .into(),
        ],
    )
    .await;
}

async fn bytes(response: Response) -> Bytes {
    to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .unwrap()
}

#[tokio::test]
async fn native_json_endpoint_contract_matrix_preserves_fields_auth_and_model() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/axonhub_protocols.json")).unwrap();
    let responses = Arc::new(fixtures.clone());
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let mock=Router::new().fallback(move|OriginalUri(uri):OriginalUri,headers:HeaderMap,Json(value):Json<Value>|{let seen=copy.clone();let fixtures=responses.clone();async move{
        seen.lock().await.push((uri.path().to_owned(),headers,value.clone()));
        Json(fixtures[value["vendor_extension"]["fixture"].as_u64().unwrap() as usize]["response"].clone())
    }});
    let f = fixture(mock).await;
    all_capabilities(&f).await;
    for (index, fixture) in fixtures.iter().enumerate() {
        let path = fixture["route"].as_str().unwrap();
        let mut payload = fixture["request"].clone();
        payload["vendor_extension"] = json!({"keep":true,"fixture":index});
        let response = request(&f, path, payload).await;
        let status = response.status();
        let body: Value = serde_json::from_slice(&bytes(response).await).unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body, fixture["response"], "{path}");
    }
    let seen = seen.lock().await;
    assert_eq!(seen.len(), 11);
    for (index, (path, headers, body)) in seen.iter().enumerate() {
        assert_eq!(body["model"], "a-model");
        assert_eq!(path, fixtures[index]["upstream"].as_str().unwrap());
        let mut expected = fixtures[index]["request"].clone();
        expected["model"] = json!("a-model");
        expected["vendor_extension"] = json!({"keep":true,"fixture":index});
        assert_eq!(body, &expected);
        assert_eq!(headers["authorization"], "Bearer a-secret");
        assert_eq!(headers["x-trace-id"], "trace-test");
    }
    assert_eq!(seen.last().unwrap().0, "/api/v3/contents/generations/tasks");
}

#[tokio::test]
async fn multipart_image_audio_and_video_preserve_binary_metadata_and_rewrite_model() {
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let mock = Router::new().fallback(move |headers: HeaderMap, body: Bytes| {
        let seen = copy.clone();
        async move {
            let boundary =
                multer::parse_boundary(headers[header::CONTENT_TYPE].to_str().unwrap()).unwrap();
            let mut form = multer::Multipart::new(
                futures_util::stream::once(async move { Ok::<_, std::io::Error>(body) }),
                boundary,
            );
            let mut fields = vec![];
            while let Some(field) = form.next_field().await.unwrap() {
                fields.push((
                    field.name().unwrap().to_owned(),
                    field.file_name().map(str::to_owned),
                    field.bytes().await.unwrap(),
                ));
            }
            seen.lock().await.push(fields);
            Json(json!({"id":"video-1","text":"hello","data":[]}))
        }
    });
    let f = fixture(mock).await;
    all_capabilities(&f).await;
    for path in [
        "/v1/images/edits",
        "/v1/audio/transcriptions",
        "/v1/audio/translations",
        "/v1/videos",
    ] {
        let mut body=b"--boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\npublic\r\n--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"sample.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
        body.extend_from_slice(&[0, 255, 128, 3]);
        body.extend_from_slice(b"\r\n--boundary--\r\n");
        let response = router(f.state.clone())
            .oneshot(
                Request::post(path)
                    .header("authorization", format!("Bearer {}", f.token))
                    .header("content-type", "multipart/form-data; boundary=boundary")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{path}: {:?}",
            bytes(response).await
        );
    }
    let seen = seen.lock().await;
    assert_eq!(seen.len(), 4);
    for fields in seen.iter() {
        assert_eq!(fields[0].2, "a-model");
        assert_eq!(fields[1].1.as_deref(), Some("sample.bin"));
        assert_eq!(fields[1].2.as_ref(), &[0, 255, 128, 3]);
    }
}

#[tokio::test]
async fn speech_returns_exact_binary_content_and_models_are_routable() {
    let f = fixture(Router::new().route(
        "/v1/audio/speech",
        post(|| async { ([(header::CONTENT_TYPE, "audio/mpeg")], vec![0, 255, 1, 2]) }),
    ))
    .await;
    all_capabilities(&f).await;
    let response = request(
        &f,
        "/v1/audio/speech",
        json!({"model":"public","input":"hello","voice":"alloy"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "audio/mpeg");
    assert_eq!(bytes(response).await.as_ref(), &[0, 255, 1, 2]);
    for path in [
        "/v1/models/public",
        "/anthropic/v1/models",
        "/v1beta/models",
        "/gemini/v1beta/models",
    ] {
        let response = router(f.state.clone())
            .oneshot(
                Request::get(path)
                    .header("authorization", format!("Bearer {}", f.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn async_tasks_are_key_owned_pinned_and_deleted() {
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let mock = Router::new().fallback(
        move |method: http::Method, OriginalUri(uri): OriginalUri, headers: HeaderMap| {
            let seen = copy.clone();
            async move {
                seen.lock().await.push((
                    method.clone(),
                    uri.path().to_owned(),
                    headers["authorization"].clone(),
                ));
                if method == http::Method::DELETE {
                    StatusCode::NO_CONTENT.into_response()
                } else {
                    Json(json!({"id":"owned-task","status":"queued"})).into_response()
                }
            }
        },
    );
    let f = fixture(mock).await;
    all_capabilities(&f).await;
    assert_eq!(
        request(&f, "/v1/videos", json!({"model":"public","prompt":"ocean"}))
            .await
            .status(),
        StatusCode::OK
    );
    let (_, other) = db::create_api_key(
        &f.state.db,
        &ApiKeyInput {
            name: "other".into(),
            budget_micros: None,
        },
    )
    .await
    .unwrap();
    let call = |method, token: String| {
        router(f.state.clone()).oneshot(
            Request::builder()
                .method(method)
                .uri("/v1/videos/owned-task")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
    };
    assert_eq!(
        call(http::Method::GET, other).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(http::Method::GET, f.token.clone())
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        call(http::Method::DELETE, f.token.clone())
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(http::Method::GET, f.token.clone())
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(seen.lock().await.len(), 3);
    assert!(
        seen.lock()
            .await
            .iter()
            .all(|(_, _, auth)| auth == "Bearer a-secret")
    );
}

#[tokio::test]
async fn gemini_native_alias_stream_and_key_query_contracts() {
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let mock=Router::new().fallback(move|OriginalUri(uri):OriginalUri,headers:HeaderMap,Json(body):Json<Value>|{let seen=copy.clone();async move{seen.lock().await.push((uri.to_string(),headers,body));if uri.path().ends_with("streamGenerateContent"){([(header::CONTENT_TYPE,"text/event-stream")],"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]},\"finishReason\":\"STOP\"}]}\n\n").into_response()}else{Json(json!({"candidates":[{"content":{"parts":[{"text":"ok"}]},"finishReason":"STOP"}]})).into_response()}}});
    let f = fixture(mock).await;
    all_capabilities(&f).await;
    sql(&f, "UPDATE providers SET kind='gemini'", vec![]).await;
    for path in [
        "/v1beta/models/public:generateContent",
        "/gemini/v1beta/models/public:generateContent",
        "/v1beta/models/public:streamGenerateContent",
    ] {
        let mut uri = reqwest::Url::parse(&format!("http://localhost{path}")).unwrap();
        uri.query_pairs_mut().append_pair("key", &f.token);
        let response=router(f.state.clone()).oneshot(Request::post(format!("{}?{}",uri.path(),uri.query().unwrap())).header("content-type","application/json").body(Body::from(json!({"contents":[{"role":"user","parts":[{"text":"hello"}]}],"safetySettings":[{"category":"HARM_CATEGORY_HARASSMENT","threshold":"BLOCK_NONE"}]}).to_string())).unwrap()).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{path}: {:?}",
            bytes(response).await
        );
    }
    let seen = seen.lock().await;
    assert_eq!(seen.len(), 3);
    assert!(
        seen.last()
            .unwrap()
            .0
            .ends_with(":streamGenerateContent?alt=sse")
    );
    for (_, headers, body) in seen.iter() {
        assert_eq!(headers["x-goog-api-key"], "a-secret");
        assert!(body.get("model").is_none());
        assert!(body.get("stream").is_none());
        assert!(body.get("safetySettings").is_some());
    }
}

#[tokio::test]
async fn anthropic_alias_preserves_native_tools_version_and_stream_terminal() {
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let f=fixture(Router::new().route("/v1/messages",post(move|headers:HeaderMap,Json(body):Json<Value>|{let seen=copy.clone();async move{
        seen.lock().await.push((headers,body.clone()));if body["stream"]==true{([(header::CONTENT_TYPE,"text/event-stream")],"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n").into_response()}else{Json(json!({"id":"msg_1","type":"message","content":[{"type":"tool_use","id":"call_1","name":"clock","input":{}}],"stop_reason":"tool_use"})).into_response()}
    }}))).await;
    sql(&f, "UPDATE providers SET kind='anthropic'", vec![]).await;
    for (path, stream) in [("/v1/messages", false), ("/anthropic/v1/messages", true)] {
        let response=router(f.state.clone()).oneshot(Request::post(path).header("x-api-key",&f.token).header("anthropic-version","2023-06-01").header("anthropic-beta","tools-2024-04-04").header("content-type","application/json").body(Body::from(json!({"model":"public","max_tokens":24,"messages":[{"role":"user","content":"hi"}],"stream":stream,"tools":[{"name":"clock","input_schema":{"type":"object"}}]}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let data = bytes(response).await;
        if stream {
            assert!(String::from_utf8_lossy(&data).contains("message_stop"));
        }
    }
    let seen = seen.lock().await;
    assert_eq!(seen.len(), 2);
    for (headers, body) in seen.iter() {
        assert_eq!(headers["x-api-key"], "a-secret");
        assert_eq!(headers["anthropic-beta"], "tools-2024-04-04");
        assert_eq!(body["tools"][0]["name"], "clock");
    }
}

#[tokio::test]
async fn legacy_completion_and_ai_sdk_streams_have_protocol_terminals() {
    let f = fixture(
        Router::new()
            .route(
                "/v1/completions",
                post(|| async {
                    (
                        [(header::CONTENT_TYPE, "text/event-stream")],
                        "data: {\"choices\":[{\"text\":\"ok\"}]}\n\ndata: [DONE]\n\n",
                    )
                }),
            )
            .route("/v1/chat/completions", post(|| async { ok_stream() })),
    )
    .await;
    all_capabilities(&f).await;
    let response = request(
        &f,
        "/v1/completions",
        json!({"model":"public","prompt":"hi","stream":true}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(String::from_utf8_lossy(&bytes(response).await).contains("[DONE]"));
    for ui in [false, true] {
        let mut request = Request::post("/aisdk/v1/chat/completions")
            .header("authorization", format!("Bearer {}", f.token))
            .header("content-type", "application/json");
        if ui {
            request = request.header("x-vercel-ai-ui-message-stream", "v1");
        }
        let response=router(f.state.clone()).oneshot(request.body(Body::from(json!({"model":"public","messages":[{"id":"client-message","role":"user","parts":[{"type":"text","text":"hello","state":"done"}]}]}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[if ui {
                "x-vercel-ai-ui-message-stream"
            } else {
                "x-vercel-ai-data-stream"
            }],
            "v1"
        );
        let data = String::from_utf8(bytes(response).await.to_vec()).unwrap();
        if ui {
            assert!(data.contains("\"type\":\"text-delta\""));
            assert!(data.contains("\"delta\":\"ok\""));
            assert!(data.ends_with("data: [DONE]\n\n"));
        } else {
            assert!(data.contains("0:\"ok\"\n"));
            assert!(data.contains("d:{"));
        }
    }
}

#[tokio::test]
async fn websocket_response_create_uses_orchestrator_and_durable_continuation() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let f=fixture(Router::new().route("/v1/responses",post(move|Json(body):Json<Value>|{let seen=copy.clone();async move{
        seen.lock().await.push(body);
        let completed=json!({"type":"response.completed","response":{"id":"resp-contract","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"reply"}]}]}});
        ([(header::CONTENT_TYPE,"text/event-stream")],format!("event: response.created\ndata: {{\"type\":\"response.created\",\"response\":{{\"id\":\"resp-contract\"}}}}\n\nevent: response.completed\ndata: {completed}\n\n"))
    }}))).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = router(f.state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut handshake = format!("ws://{address}/v1/responses")
        .into_client_request()
        .unwrap();
    handshake.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", f.token)).unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(handshake).await.unwrap();
    for payload in [
        json!({"type":"response.create","stream_id":"lane-a","model":"public","input":"first","stream":false,"background":true}),
        json!({"type":"response.create","stream_id":"lane-a","model":"public","input":"second","previous_response_id":"resp-contract"}),
    ] {
        socket
            .send(Message::Text(payload.to_string().into()))
            .await
            .unwrap();
        for expected in ["response.created", "response.completed"] {
            let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let value: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            assert_eq!(value["type"], expected, "{value}");
            assert_eq!(value["stream_id"], "lane-a");
        }
    }
    let seen = seen.lock().await;
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0]["stream"], true);
    assert!(seen[0].get("type").is_none());
    assert!(seen[0].get("stream_id").is_none());
    assert!(seen[0].get("background").is_none());
    assert_eq!(seen[1]["input"].as_array().unwrap().len(), 3);
    socket.close(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn all_new_routes_require_gateway_auth_and_media_limits_fail_before_contact() {
    let calls = Arc::new(AtomicUsize::new(0));
    let copy = calls.clone();
    let f = fixture(Router::new().fallback(move || {
        let calls = copy.clone();
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Json(json!({"data":[]}))
        }
    }))
    .await;
    all_capabilities(&f).await;
    for path in [
        "/v1/completions",
        "/v1/embeddings",
        "/v1/moderations",
        "/v1/alpha/search",
        "/v1/images/generations",
        "/v1/audio/speech",
        "/v1/videos",
        "/jina/v1/rerank",
        "/anthropic/v1/messages",
        "/doubao/v3/contents/generations/tasks",
    ] {
        let response = router(f.state.clone())
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"model":"public"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
    sql(&f, "UPDATE api_keys SET budget_micros=1000", vec![]).await;
    assert_eq!(
        request(
            &f,
            "/v1/images/generations",
            json!({"model":"public","prompt":"sea"})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
