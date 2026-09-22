use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

async fn oauth_owner(f: &Fixture) -> String {
    let user = db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: "oauth-owner@example.test".into(),
            password: "a secure oauth password".into(),
            instance_name: None,
            language: None,
        },
    )
    .await
    .unwrap();
    db::create_session(&f.state.db, &user.id).await.unwrap()
}

async fn oauth_request(f: &Fixture, cookie: &str, path: &str, body: Value) -> Response {
    router(f.state.clone())
        .oneshot(
            Request::post(path)
                .header("cookie", format!("pangolin_session={cookie}"))
                .header("x-pangolin-csrf", "1")
                .header(header::ORIGIN, "https://console.example.test")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn oauth_api_encrypts_the_token_and_never_returns_it() {
    let token = Arc::new(format!("oauth-{}", Uuid::new_v4()));
    let refresh_token = Arc::new(format!("refresh-{}", Uuid::new_v4()));
    let exchange_token = Arc::clone(&token);
    let exchange_refresh_token = Arc::clone(&refresh_token);
    let upstream_token = Arc::clone(&token);
    let mock = Router::new()
        .route(
            "/token",
            post(move |body: Bytes| {
                let token = Arc::clone(&exchange_token);
                let refresh_token = Arc::clone(&exchange_refresh_token);
                async move {
                    let body = String::from_utf8(body.to_vec()).unwrap();
                    if body.contains("code=bad-code") {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error":"invalid_grant"})),
                        );
                    }
                    (
                        StatusCode::OK,
                        Json(json!({"access_token":token.as_str(),"refresh_token":refresh_token.as_str(),"token_type":"bearer"})),
                    )
                }
            }),
        )
        .route(
            "/v1/chat/completions",
            post(move |headers: HeaderMap| {
                let token = Arc::clone(&upstream_token);
                async move {
                    assert_eq!(
                        headers.get(header::AUTHORIZATION).unwrap(),
                        &HeaderValue::from_str(&format!("Bearer {token}")).unwrap()
                    );
                    Json(json!({
                        "choices":[{"message":{"role":"assistant","content":"oauth works"}}],
                        "usage":{"prompt_tokens":1,"completion_tokens":1}
                    }))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let f = fixture(Router::new()).await;
    let provider = &f.providers[0];
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE providers SET settings_json=?,base_url=? WHERE id=?",
            vec![
                json!({"version":1,"oauth_test":{
                    "authorization_endpoint":"https://idp.example.test/authorize",
                    "token_endpoint":format!("http://{address}/token")
                }})
                .to_string()
                .into(),
                format!("http://{address}").into(),
                provider.clone().into(),
            ],
        ))
        .await
        .unwrap();
    let cookie = oauth_owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/providers/{provider}/oauth/codex",
        db::DEFAULT_PROJECT_ID
    );
    let started = oauth_request(
        &f,
        &cookie,
        &format!("{base}/start"),
        json!({"client_id":"test-client","redirect_uri":"https://console.example.test/channels"}),
    )
    .await;
    assert_eq!(started.status(), StatusCode::OK);
    let started: Value =
        serde_json::from_slice(&to_bytes(started.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert!(
        started["authorization_url"]
            .as_str()
            .unwrap()
            .contains("code_challenge=")
    );
    assert!(started.get("verifier").is_none());

    let completed = oauth_request(
        &f,
        &cookie,
        &format!("{base}/complete"),
        json!({"state":started["state"],"code":"good-code"}),
    )
    .await;
    assert_eq!(completed.status(), StatusCode::OK);
    let bytes = to_bytes(completed.into_body(), 64 * 1024).await.unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(token.as_str()));
    assert!(!String::from_utf8_lossy(&bytes).contains(refresh_token.as_str()));
    let completed: Value = serde_json::from_slice(&bytes).unwrap();
    let credential = completed["credential_id"].as_str().unwrap();
    let envelope: String = f
        .state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT secret_envelope FROM channel_credentials WHERE id=?",
            vec![credential.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "secret_envelope")
        .unwrap();
    assert!(!envelope.contains(token.as_str()));
    let decrypted = f.state.secrets.decrypt(&envelope).unwrap();
    assert!(decrypted.contains(token.as_str()));
    assert!(decrypted.contains(refresh_token.as_str()));
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE channel_credentials SET enabled=0 WHERE provider_id=? AND id<>?",
            vec![provider.clone().into(), credential.into()],
        ))
        .await
        .unwrap();
    let gateway = request(&f, "/v1/chat/completions", chat()).await;
    let gateway_status = gateway.status();
    let gateway_body = to_bytes(gateway.into_body(), 64 * 1024).await.unwrap();
    assert_eq!(
        gateway_status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&gateway_body)
    );

    let before: i64 = f
        .state
        .db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM channel_credentials",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    let retry = oauth_request(
        &f,
        &cookie,
        &format!("{base}/start"),
        json!({"client_id":"test-client","redirect_uri":"https://console.example.test/channels"}),
    )
    .await;
    let retry: Value =
        serde_json::from_slice(&to_bytes(retry.into_body(), 64 * 1024).await.unwrap()).unwrap();
    let failed = oauth_request(
        &f,
        &cookie,
        &format!("{base}/complete"),
        json!({"state":retry["state"],"code":"bad-code"}),
    )
    .await;
    assert_eq!(failed.status(), StatusCode::BAD_GATEWAY);
    let after: i64 = f
        .state
        .db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM channel_credentials",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(after, before);
    server.abort();
}

#[tokio::test]
async fn oauth_api_refuses_flows_without_complete_provider_adapters() {
    let polls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&polls);
    let mock = Router::new()
        .route(
            "/device",
            post(move || async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Json(json!({"unexpected":true}))
            }),
        )
        .route(
            "/token",
            post(|| async { Json(json!({"unexpected":true})) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let f = fixture(Router::new()).await;
    let provider = &f.providers[0];
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE providers SET settings_json=? WHERE id=?",
            vec![
                json!({"version":1,"oauth_test":{
                    "device_endpoint":format!("http://{address}/device"),
                    "token_endpoint":format!("http://{address}/token")
                }})
                .to_string()
                .into(),
                provider.clone().into(),
            ],
        ))
        .await
        .unwrap();
    let cookie = oauth_owner(&f).await;
    for flow in ["github_copilot", "antigravity"] {
        let path = format!(
            "/api/admin/v1/projects/{}/providers/{provider}/oauth/{flow}/start",
            db::DEFAULT_PROJECT_ID
        );
        let response = oauth_request(&f, &cookie, &path, json!({"client_id":"test-client"})).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{flow}");
    }
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    server.abort();
}
