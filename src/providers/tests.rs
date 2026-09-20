use super::*;
use serde_json::json;

fn target(kind: &str) -> RouteTarget {
    RouteTarget {
        public_name: "public".into(),
        upstream_name: "model".into(),
        provider_name: "provider".into(),
        provider_kind: kind.into(),
        base_url: "http://localhost:8080".into(),
        secret_envelope: String::new(),
        input_price_micros: 0,
        output_price_micros: 0,
    }
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
