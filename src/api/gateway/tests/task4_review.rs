use super::*;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};

const SENTINEL: &str = "task4_private_database_decryption_session_sentinel";
struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn websocket(
    f: &Fixture,
) -> (
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    Server,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = router(f.state.clone());
    let server = Server(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let mut handshake = format!("ws://{address}/v1/responses")
        .into_client_request()
        .unwrap();
    handshake.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", f.token)).unwrap(),
    );
    (
        tokio_tungstenite::connect_async(handshake).await.unwrap().0,
        server,
    )
}
async fn body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}
async fn ws_event(socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

#[tokio::test]
async fn task4_internal_errors_are_redacted_in_every_http_and_websocket_wrapper() {
    let f = fixture(Router::new()).await;
    let (mut socket, _server) = websocket(&f).await;
    sql(&f,&format!("CREATE TRIGGER injected_private_failure BEFORE UPDATE ON api_keys BEGIN SELECT RAISE(ABORT,'{SENTINEL}'); END"),vec![]).await;
    for path in [
        "/v1/chat/completions",
        "/v1/messages",
        "/anthropic/v1/messages",
        "/v1beta/models/public:generateContent",
        "/gemini/v1beta/models/public:generateContent",
        "/anthropic/v1/models",
        "/v1beta/models",
    ] {
        let method = if path.ends_with("/models") {
            http::Method::GET
        } else {
            http::Method::POST
        };
        let response = router(f.state.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", format!("Bearer {}", f.token))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        if path.contains(":generateContent") {
                            json!({"contents":[{"parts":[{"text":"hi"}]}]})
                        } else {
                            chat()
                        }
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{path}"
        );
        let value = body(response).await;
        assert_eq!(value["error"]["message"], "An internal error occurred");
        assert!(!value.to_string().contains(SENTINEL));
        if path.contains("gemini") || path.contains("v1beta") {
            assert_eq!(value["error"]["status"], "INTERNAL");
        } else if path.contains("messages") || path.contains("anthropic") {
            assert_eq!(value["type"], "error");
            assert_eq!(value["error"]["type"], "api_error");
        }
    }
    socket.send(Message::Text(json!({"type":"response.create","model":"public","input":"hi","stream_id":"secret-test"}).to_string().into())).await.unwrap();
    let value = ws_event(&mut socket).await;
    assert_eq!(value["status"], 500);
    assert_eq!(value["error"]["type"], "internal_error");
    assert_eq!(value["error"]["message"], "An internal error occurred");
    assert!(!value.to_string().contains(SENTINEL));
    for gemini in [false, true] {
        let response = crate::api::protocols::protocol_error(
            Err(ApiError::Internal(anyhow::anyhow!(SENTINEL))),
            gemini,
        )
        .unwrap();
        assert!(!body(response).await.to_string().contains(SENTINEL));
    }
}

#[tokio::test]
async fn task4_native_error_envelopes_cover_local_validation_auth_access_and_upstream_statuses() {
    let f = fixture(
        Router::new().fallback(|Json(payload): Json<Value>| async move {
            (
                StatusCode::from_u16(payload["test_status"].as_u64().unwrap_or(200) as u16)
                    .unwrap(),
                Json(json!({"error":{"message":"upstream-private-sentinel"}})),
            )
        }),
    )
    .await;
    sql(
        &f,
        "UPDATE channel_settings SET retry_statuses_json=?",
        vec![
            json!({"version":1,"statuses":[],"error_mode":"normalized"})
                .to_string()
                .into(),
        ],
    )
    .await;
    for (kind, path) in [
        ("anthropic", "/anthropic/v1/messages"),
        ("gemini", "/v1beta/models/public:generateContent"),
    ] {
        sql(&f, "UPDATE providers SET kind=?", vec![kind.into()]).await;
        for (status, expected) in [
            (
                400,
                if kind == "gemini" {
                    "INVALID_ARGUMENT"
                } else {
                    "invalid_request_error"
                },
            ),
            (
                401,
                if kind == "gemini" {
                    "UNAUTHENTICATED"
                } else {
                    "authentication_error"
                },
            ),
            (
                403,
                if kind == "gemini" {
                    "PERMISSION_DENIED"
                } else {
                    "permission_error"
                },
            ),
            (
                429,
                if kind == "gemini" {
                    "RESOURCE_EXHAUSTED"
                } else {
                    "rate_limit_error"
                },
            ),
            (
                500,
                if kind == "gemini" {
                    "INTERNAL"
                } else {
                    "api_error"
                },
            ),
        ] {
            let payload = if kind == "gemini" {
                json!({"contents":[{"parts":[{"text":"hi"}]}],"test_status":status})
            } else {
                json!({"model":"public","messages":[{"role":"user","content":"hi"}],"test_status":status})
            };
            let response = request(&f, path, payload).await;
            assert_eq!(response.status().as_u16(), status);
            let value = body(response).await;
            assert_eq!(
                value["error"][if kind == "gemini" { "status" } else { "type" }],
                expected
            );
            assert!(!value.to_string().contains("upstream-private-sentinel"));
        }
        let unauth = router(f.state.clone())
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        if kind == "gemini" {
                            json!({"contents":[]})
                        } else {
                            chat()
                        }
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
        let value = body(unauth).await;
        assert_eq!(
            value["error"][if kind == "gemini" { "status" } else { "type" }],
            if kind == "gemini" {
                "UNAUTHENTICATED"
            } else {
                "authentication_error"
            }
        );
        let malformed = router(f.state.clone())
            .oneshot(
                Request::post(path)
                    .header("authorization", format!("Bearer {}", f.token))
                    .header("content-type", "application/json")
                    .body(Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
        let value = body(malformed).await;
        assert_eq!(
            value["error"][if kind == "gemini" { "status" } else { "type" }],
            if kind == "gemini" {
                "INVALID_ARGUMENT"
            } else {
                "invalid_request_error"
            }
        );
    }
    sql(
        &f,
        "UPDATE projects SET settings_json=?",
        vec![
            json!({"version":1,"routing":{"version":1,"allowed_endpoints":[]}})
                .to_string()
                .into(),
        ],
    )
    .await;
    let denied = request(
        &f,
        "/v1beta/models/public:generateContent",
        json!({"contents":[]}),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(body(denied).await["error"]["status"], "PERMISSION_DENIED");
    sql(
        &f,
        "UPDATE projects SET settings_json=?",
        vec![
            json!({"version":1,"routing":{"version":1,"limits":{"rpm":0}}})
                .to_string()
                .into(),
        ],
    )
    .await;
    let throttled = request(
        &f,
        "/v1beta/models/public:generateContent",
        json!({"contents":[]}),
    )
    .await;
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        body(throttled).await["error"]["status"],
        "RESOURCE_EXHAUSTED"
    );
}

#[derive(Clone)]
struct LogCapture(Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self {
        self.clone()
    }
}

#[tokio::test]
async fn task4_trace_layer_never_records_gemini_query_credentials() {
    use tracing::instrument::WithSubscriber;
    let f = fixture(Router::new()).await;
    let capture = LogCapture(Arc::new(std::sync::Mutex::new(vec![])));
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(capture.clone())
        .finish();
    let app = router(f.state.clone())
        .layer(tower_http::trace::TraceLayer::new_for_http().make_span_with(crate::http_span));
    let response = app
        .oneshot(
            Request::get(
                "/v1beta/models?key=gemini-query-secret-sentinel&other=secondary-secret-sentinel",
            )
            .body(Body::empty())
            .unwrap(),
        )
        .with_subscriber(subscriber)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("/v1beta/models"), "{logs}");
    assert!(!logs.contains("gemini-query-secret-sentinel"));
    assert!(!logs.contains("secondary-secret-sentinel"));
    assert!(!logs.contains("?key="));
}

#[tokio::test]
async fn task4_gemini_waits_for_every_candidate_terminal() {
    let mock=Router::new().fallback(||async{([(header::CONTENT_TYPE,"text/event-stream")],"data: {\"candidates\":[{\"index\":0,\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\"first\"}]}}]}\n\ndata: {\"candidates\":[{\"index\":1,\"content\":{\"parts\":[{\"text\":\"second\"}]}}]}\n\ndata: {\"candidates\":[{\"index\":1,\"finishReason\":\"STOP\"}]}\n\n")});
    let f = fixture(mock).await;
    sql(&f, "UPDATE providers SET kind='gemini'", vec![]).await;
    let response = request(
        &f,
        "/v1beta/models/public:streamGenerateContent",
        json!({"contents":[{"parts":[{"text":"hi"}]}],"generationConfig":{"candidateCount":2}}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let text =
        String::from_utf8(to_bytes(response.into_body(), 4096).await.unwrap().to_vec()).unwrap();
    assert!(text.contains("second"));
    assert_eq!(text.matches("finishReason").count(), 2);
    assert!(!text.contains("error"));
}

#[tokio::test]
async fn task4_native_stream_errors_keep_rate_limit_types_without_private_details_or_retry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let f=fixture(Router::new().fallback(move|axum::extract::OriginalUri(uri):axum::extract::OriginalUri|{let count=count.clone();async move{
        count.fetch_add(1,Ordering::SeqCst);
        let (first,error)=if uri.path()=="/v1/messages"{(json!({"type":"message_start","message":{"id":"msg_test"}}),json!({"type":"error","error":{"type":"rate_limit_error","message":SENTINEL}}))}else{(json!({"candidates":[{"index":0,"content":{"parts":[{"text":"hello"}]}}]}),json!({"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":SENTINEL}}))};
        ([(header::CONTENT_TYPE,"text/event-stream")],format!("data: {first}\n\nevent: error\ndata: {error}\n\n"))
    }})).await;
    for (kind, path, payload, expected) in [
        (
            "anthropic",
            "/v1/messages",
            json!({"model":"public","messages":[{"role":"user","content":"hi"}],"stream":true}),
            "rate_limit_error",
        ),
        (
            "gemini",
            "/v1beta/models/public:streamGenerateContent",
            json!({"contents":[{"parts":[{"text":"hi"}]}]}),
            "RESOURCE_EXHAUSTED",
        ),
    ] {
        sql(&f, "UPDATE providers SET kind=?", vec![kind.into()]).await;
        let response = request(&f, path, payload).await;
        assert_eq!(response.status(), StatusCode::OK);
        let text = String::from_utf8(to_bytes(response.into_body(), 8192).await.unwrap().to_vec())
            .unwrap();
        assert!(text.contains(expected), "{text}");
        assert!(!text.contains(SENTINEL));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn task4_websocket_lanes_are_concurrent_and_same_lane_is_fifo() {
    let gate = Arc::new(tokio::sync::Notify::new());
    let wait = gate.clone();
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel::<String>();
    let f=fixture(Router::new().route("/v1/responses",post(move|Json(payload):Json<Value>|{let wait=wait.clone();let started=started.clone();async move{
        let name=payload["input"].as_str().unwrap().to_owned();started.send(name.clone()).unwrap();if name=="a1"{wait.notified().await;}
        ([(header::CONTENT_TYPE,"text/event-stream")],format!("data: {}\n\n",json!({"type":"response.completed","response":{"id":format!("resp_{name}"),"status":"completed","output":[]}})))
    }}))).await;
    let (mut socket, _server) = websocket(&f).await;
    for (lane, input) in [("a", "a1"), ("a", "a2"), ("b", "b1")] {
        socket
            .send(Message::Text(
                json!({"type":"response.create","stream_id":lane,"model":"public","input":input})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
    }
    let first = tokio::time::timeout(Duration::from_secs(5), starts.recv())
        .await
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(5), starts.recv())
        .await
        .unwrap()
        .unwrap();
    assert!([first.as_str(), second.as_str()].contains(&"a1"));
    assert!([first.as_str(), second.as_str()].contains(&"b1"));
    assert!(starts.try_recv().is_err());
    let b = ws_event(&mut socket).await;
    assert_eq!(b["stream_id"], "b");
    assert_eq!(b["response"]["id"], "resp_b1");
    gate.notify_one();
    let a1 = ws_event(&mut socket).await;
    let a2 = ws_event(&mut socket).await;
    assert_eq!(a1["response"]["id"], "resp_a1");
    assert_eq!(a2["response"]["id"], "resp_a2");
    assert_eq!(starts.recv().await.unwrap(), "a2");
}

#[tokio::test]
async fn task4_websocket_disconnect_cancels_backpressured_stream_without_waiting_for_timeout() {
    let produced = Arc::new(AtomicUsize::new(0));
    let output = produced.clone();
    let mut f=fixture(Router::new().route("/v1/responses",post(move||{let output=output.clone();async move{
        let stream=async_stream::stream!{let frame=format!("data: {}\n\n",json!({"type":"response.output_text.delta","delta":"x".repeat(32*1024)}));loop{output.fetch_add(1,Ordering::SeqCst);yield Ok::<_,std::io::Error>(Bytes::from(frame.clone()));}};
        ([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(stream))
    }}))).await;
    Arc::make_mut(&mut f.state.config).upstream_timeout = Duration::from_secs(60);
    let (mut socket, _server) = websocket(&f).await;
    socket
        .send(Message::Text(
            json!({"type":"response.create","model":"public","input":"slow consumer"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while produced.load(Ordering::SeqCst) < 40 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(socket);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let metrics = f.state.orchestrator.metrics();
            if metrics.contains("pangolin_orchestration_inflight{resource=\"channel\"} 0")
                && metrics.contains("pangolin_orchestration_inflight{resource=\"key\"} 0")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn task4_multipart_repeated_files_mime_and_aggregate_size_are_preserved() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let f = fixture(Router::new().route(
        "/v1/images/edits",
        post(move |headers: HeaderMap, bytes: Bytes| {
            let calls = count.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                let boundary =
                    multer::parse_boundary(headers[header::CONTENT_TYPE].to_str().unwrap())
                        .unwrap();
                let mut form = multer::Multipart::new(
                    futures_util::stream::once(async move { Ok::<_, std::io::Error>(bytes) }),
                    boundary,
                );
                let mut files = vec![];
                while let Some(field) = form.next_field().await.unwrap() {
                    if field.file_name().is_some() {
                        files.push((
                            field.name().unwrap().to_owned(),
                            field.file_name().unwrap().to_owned(),
                            field.content_type().unwrap().to_string(),
                            field.bytes().await.unwrap(),
                        ));
                    }
                }
                assert_eq!(files.len(), 2);
                assert!(files.iter().all(|file| file.0 == "image[]"));
                assert_eq!(files[0].1, "one.png");
                assert_eq!(files[0].2, "image/png");
                assert_eq!(files[1].2, "image/jpeg");
                assert_eq!(files[0].3.as_ref(), &[0, 255, 1]);
                assert_eq!(files[1].3.as_ref(), &[2, 254, 3]);
                Json(json!({"data":[{"url":"fixture"}]}))
            }
        }),
    ))
    .await;
    sql(&f, "UPDATE models SET capabilities='[\"images\"]'", vec![]).await;
    let mut data =
        b"--edge\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\npublic\r\n".to_vec();
    for (filename, mime, value) in [
        ("one.png", "image/png", [0u8, 255, 1]),
        ("two.jpg", "image/jpeg", [2, 254, 3]),
    ] {
        data.extend_from_slice(format!("--edge\r\nContent-Disposition: form-data; name=\"image[]\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes());
        data.extend_from_slice(&value);
        data.extend_from_slice(b"\r\n");
    }
    data.extend_from_slice(b"--edge--\r\n");
    for (payload, status) in [
        (data, StatusCode::OK),
        (vec![0; 64 * 1024 * 1024 + 1], StatusCode::PAYLOAD_TOO_LARGE),
    ] {
        let response = router(f.state.clone())
            .oneshot(
                Request::post("/v1/images/edits")
                    .header("authorization", format!("Bearer {}", f.token))
                    .header("content-type", "multipart/form-data; boundary=edge")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            assert_eq!(
                body(response).await["error"]["type"],
                "invalid_request_error"
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
