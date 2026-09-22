use super::*;
use serde_json::json;
use std::io::{Read, Write};
use std::sync::{Arc, Barrier};
use std::time::Duration;

fn target(kind: &str) -> RouteTarget {
    RouteTarget {
        public_name: "public".into(),
        upstream_name: "model".into(),
        provider_name: "provider".into(),
        provider_kind: kind.into(),
        base_url: "http://localhost:8080".into(),
        credential_type: "api_key".into(),
        secret_envelope: String::new(),
        proxy_url: None,
        proxy_username: None,
        proxy_secret_envelope: None,
        proxy_reuse_connections: true,
        proxy_preset_id: None,
        input_price_micros: 0,
        output_price_micros: 0,
    }
}

#[tokio::test]
async fn a_channel_proxy_is_used_with_write_only_credentials() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let (mut socket, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "proxy was not contacted"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("proxy accept failed: {error}"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = [0_u8; 4096];
        let count = socket.read(&mut bytes).unwrap();
        let request = String::from_utf8_lossy(&bytes[..count]).to_string();
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
            )
            .unwrap();
        request
    });

    let client = upstream::http_client_with_proxy(
        Duration::from_secs(3),
        Some(upstream::ProxySettings {
            url: &format!("http://{address}"),
            username: Some("operator"),
            password: Some("proxy-password-sentinel"),
            reuse_connections: false,
        }),
    )
    .unwrap();
    let body: serde_json::Value = client
        .get("http://upstream.invalid/v1/models")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body, json!({"ok":true}));

    let request = proxy.join().unwrap();
    assert!(request.starts_with("GET http://upstream.invalid/v1/models HTTP/1.1"));
    assert!(request.lines().any(|line| line.eq_ignore_ascii_case(
        "Proxy-Authorization: Basic b3BlcmF0b3I6cHJveHktcGFzc3dvcmQtc2VudGluZWw="
    )));
}

#[test]
fn proxy_client_cache_stays_within_its_hard_limit_under_concurrency() {
    let pool = Arc::new(upstream::ClientPool::with_capacity(2));
    let default = reqwest::Client::new();
    let barrier = Arc::new(Barrier::new(16));
    let threads = (0..16)
        .map(|index| {
            let pool = Arc::clone(&pool);
            let default = default.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                pool.for_proxy(
                    &default,
                    Duration::from_secs(3),
                    Some(upstream::ProxySettings {
                        url: &format!("http://127.0.0.1:{}", 10_000 + index),
                        username: None,
                        password: None,
                        reuse_connections: true,
                    }),
                )
                .unwrap();
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    assert!(pool.len() <= 2);
}

#[tokio::test]
async fn compatible_presets_preserve_json_and_own_auth() {
    for kind in KINDS.iter().filter(|kind| {
        !matches!(
            **kind,
            "anthropic" | "gemini" | "azure" | "bedrock" | "vertex" | "gcp"
        )
    }) {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer cannot-win"),
        );
        let body = json!({"model":"model","input":"test","vendor":{"keep":true}});
        let request = prepare(
            &target(kind),
            "/v1/embeddings",
            &body,
            "channel-key",
            headers,
            &HeaderMap::new(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(request.payload, body);
        assert_eq!(request.headers["authorization"], "Bearer channel-key");
        assert_eq!(request.url, "http://localhost:8080/v1/embeddings");
    }
}

#[tokio::test]
async fn oauth_credentials_use_only_their_completed_provider_auth_contract() {
    let body = json!({"model":"model","messages":[{"role":"user","content":"hi"}]});
    for (kind, credential_type) in [
        ("openai", "oauth_codex"),
        ("xai", "oauth_xai"),
        ("anthropic", "oauth_claude_code"),
    ] {
        let mut target = target(kind);
        target.credential_type = credential_type.into();
        let request = prepare(
            &target,
            if kind == "anthropic" {
                "/v1/messages"
            } else {
                "/v1/chat/completions"
            },
            &body,
            "oauth-access-token",
            HeaderMap::new(),
            &HeaderMap::new(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            request.headers["authorization"],
            "Bearer oauth-access-token"
        );
        assert!(!request.headers.contains_key("x-api-key"));
        assert!(!request.headers.contains_key("x-goog-api-key"));
    }

    for (kind, credential_type) in [
        ("gemini", "oauth_antigravity"),
        ("openai", "oauth_github_copilot"),
        ("gemini", "oauth_codex"),
    ] {
        let mut target = target(kind);
        target.credential_type = credential_type.into();
        assert!(
            prepare(
                &target,
                "/v1/chat/completions",
                &body,
                "oauth-access-token",
                HeaderMap::new(),
                &HeaderMap::new(),
                None,
            )
            .await
            .is_err(),
            "{credential_type} must fail closed for {kind}"
        );
    }
}

#[tokio::test]
async fn azure_api_key_and_entra_token_use_deployment_and_version() {
    let body = json!({"model":"model","messages":[{"role":"user","content":"hi"}]});
    let key = prepare(
        &target("azure"),
        "/v1/chat/completions",
        &body,
        "azure-key",
        HeaderMap::new(),
        &HeaderMap::new(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        key.url,
        "http://localhost:8080/openai/deployments/model/chat/completions?api-version=2024-10-21"
    );
    assert_eq!(key.headers["api-key"], "azure-key");
    assert!(!key.headers.contains_key("authorization"));
    let token = prepare(
        &target("azure"),
        "/v1/chat/completions",
        &body,
        &json!({"azure_ad_token":"mock-token","api_version":"2025-01-01-preview"}).to_string(),
        HeaderMap::new(),
        &HeaderMap::new(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(token.headers["authorization"], "Bearer mock-token");
    assert!(token.url.ends_with("api-version=2025-01-01-preview"));
}

#[tokio::test]
async fn bedrock_uses_litellm_converse_and_signed_payload() {
    let credentials=json!({"access_key_id":"AKIDEXAMPLE","secret_access_key":"mock-secret","session_token":"mock-session","region":"us-east-1"}).to_string();
    let request=prepare(&target("bedrock"),"/v1/chat/completions",&json!({"model":"model","messages":[{"role":"system","content":"brief"},{"role":"user","content":"hello"}],"max_tokens":15}),&credentials,HeaderMap::new(),&HeaderMap::new(),None).await.unwrap();
    assert!(request.url.ends_with("/model/model/converse"));
    assert_eq!(
        request.payload["messages"][0]["content"][0]["text"],
        "hello"
    );
    assert_eq!(request.payload["inferenceConfig"]["maxTokens"], 15);
    assert!(
        request.headers["authorization"]
            .to_str()
            .unwrap()
            .starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/")
    );
    assert_eq!(request.headers["x-amz-security-token"], "mock-session");
    assert!(request.headers.contains_key("x-amz-date"));
    let response=request.response.apply(json!({"output":{"message":{"role":"assistant","content":[{"text":"ok"}]}},"stopReason":"end_turn","usage":{"inputTokens":2,"outputTokens":1,"totalTokens":3}})).unwrap();
    assert_eq!(response["choices"][0]["message"]["content"], "ok");
}

#[tokio::test]
async fn vertex_uses_litellm_gcp_token_and_project_url() {
    let request=prepare(&target("vertex"),"/v1beta/models:generateContent",&json!({"model":"model","contents":[{"parts":[{"text":"hi"}]}]}),&json!({"access_token":"mock-gcp-token","vertex_project":"test-project","vertex_location":"europe-west1"}).to_string(),HeaderMap::new(),&HeaderMap::new(),None).await.unwrap();
    assert_eq!(request.headers["authorization"], "Bearer mock-gcp-token");
    assert_eq!(
        request.url,
        "http://localhost:8080/v1/projects/test-project/locations/europe-west1/publishers/google/models/model:generateContent"
    );
    assert!(request.payload.get("model").is_none());
}

#[tokio::test]
async fn cross_protocol_transformations_preserve_semantics_or_reject_unknown_fields() {
    let chat = json!({"model":"model","messages":[{"role":"system","content":"brief"},{"role":"user","content":"hello"}],"max_tokens":20,"temperature":0.4});
    let request = prepare(
        &target("gemini"),
        "/v1/chat/completions",
        &chat,
        "key",
        HeaderMap::new(),
        &HeaderMap::new(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        request.payload["systemInstruction"]["parts"][0]["text"],
        "brief"
    );
    assert_eq!(request.payload["generationConfig"]["maxOutputTokens"], 20);
    let response=request.response.apply(json!({"candidates":[{"content":{"parts":[{"text":"reply"}]},"finishReason":"MAX_TOKENS"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":20,"totalTokenCount":23}})).unwrap();
    assert_eq!(response["choices"][0]["finish_reason"], "length");
    assert_eq!(response["usage"]["total_tokens"], 23);
    let mut unsupported = chat.clone();
    unsupported["tools"] = json!([]);
    assert!(
        prepare(
            &target("gemini"),
            "/v1/chat/completions",
            &unsupported,
            "key",
            HeaderMap::new(),
            &HeaderMap::new(),
            None
        )
        .await
        .is_err()
    );
    assert!(
        transforms::messages_to_chat(
            &json!({"model":"x","messages":[],"thinking":{"type":"enabled"}})
        )
        .is_err()
    );
    assert!(
        transforms::gemini_to_chat(&json!({"model":"x","contents":[],"safetySettings":[]}))
            .is_err()
    );
    assert!(transforms::embedding_to_gemini(&json!({"model":"x","input":[1,2]})).is_err());
    assert!(
        ResponseTransform::GeminiChat("x".into())
            .apply(json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"tool"}}]}}]}))
            .is_err()
    );
}

#[test]
fn join_handles_version_prefixes_without_losing_model_slashes() {
    assert_eq!(
        join("https://api.example/v1/", "/v1/embeddings"),
        "https://api.example/v1/embeddings"
    );
    assert_eq!(segment("org/model?secret#"), "org%2Fmodel%3Fsecret%23");
    assert!(anthropic_text_request("claude-test",&json!({"model":"claude-test","messages":[{"role":"user","content":"hello"}],"max_tokens":32})).is_some());
}

/// The stream decision is the protocol's own, taken once at admission. A client
/// flag is an answer only on the endpoints that accept one; the Gemini streaming
/// action is an answer by URL shape; an endpoint with no stream choice at all has
/// no answer to record, and `None` is how the record says so.
#[test]
fn the_stream_decision_is_the_protocol_shape_not_a_guess() {
    assert_eq!(
        streamed("/v1/chat/completions", &json!({"stream": true})),
        Some(true)
    );
    assert_eq!(
        streamed("/v1/chat/completions", &json!({"stream": false})),
        Some(false)
    );
    assert_eq!(
        streamed("/v1/responses", &json!({"model": "public"})),
        Some(false),
        "the endpoints that accept `stream` decide it even when the client omits it"
    );
    assert_eq!(
        streamed("/v1/messages", &json!({"stream": true})),
        Some(true)
    );
    assert_eq!(
        streamed(
            "/v1beta/models:streamGenerateContent",
            &json!({"contents": []})
        ),
        Some(true),
        "the streaming action is chosen by the URL, not by a body field"
    );
    assert_eq!(
        streamed("/v1beta/models:generateContent", &json!({"contents": []})),
        Some(false)
    );
    assert_eq!(
        streamed("/v1/embeddings", &json!({"model": "public", "input": "hi"})),
        None,
        "an endpoint with no stream choice has none to record"
    );
    assert_eq!(
        streamed("/v1/models", &json!({})),
        None,
        "a locally answered request never decided to stream anything"
    );
    assert_eq!(
        streamed("/v1/chat/completions", &json!({"stream": "yes"})),
        None,
        "a value that is not a boolean is not a decision"
    );
}

#[tokio::test]
async fn task4_sigv4_matches_the_botocore_canonical_signature_fixture() {
    // Public AWS example credentials and LiteLLM's independently generated
    // botocore golden vector, exercised through Pangolin's signing boundary.
    let credentials=json!({"region":"us-east-1","access_key_id":"AKIDEXAMPLE","secret_access_key":"wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY","session_token":"session-token"}).to_string();
    let mut url =
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/amazon.titan-text-express-v1/invoke"
            .to_owned();
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    cloud::authenticate_at(
        &target("bedrock"),
        &credentials,
        &json!({"input":"hello"}),
        &mut url,
        &mut headers,
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_704_164_645),
    )
    .await
    .unwrap();
    assert_eq!(headers["x-amz-date"], "20240102T030405Z");
    assert_eq!(
        headers["authorization"],
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20240102/us-east-1/bedrock/aws4_request, SignedHeaders=content-type;host;x-amz-date;x-amz-security-token, Signature=55c027ef47527d3ad63f1735f9d099efdbc99f296ff914bd94e727e24ec0e464"
    );
}
