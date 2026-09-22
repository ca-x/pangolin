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
async fn antigravity_oauth_resolves_project_encrypts_and_refreshes_before_gateway_use() {
    let initial_access = Arc::new(format!("google-access-{}", Uuid::new_v4()));
    let refreshed_access = Arc::new(format!("google-refreshed-{}", Uuid::new_v4()));
    let refresh_token = Arc::new(format!("google-refresh-{}", Uuid::new_v4()));
    let client_secret = Arc::new(format!("google-client-secret-{}", Uuid::new_v4()));
    let exchanges = Arc::new(AtomicUsize::new(0));
    let exchange_counter = Arc::clone(&exchanges);
    let exchange_initial = Arc::clone(&initial_access);
    let exchange_refreshed = Arc::clone(&refreshed_access);
    let exchange_refresh = Arc::clone(&refresh_token);
    let expected_secret = Arc::clone(&client_secret);
    let project_access = Arc::clone(&initial_access);
    let upstream_access = Arc::clone(&refreshed_access);
    let mock = Router::new()
        .route(
            "/google-token",
            post(move |body: Bytes| {
                let initial = Arc::clone(&exchange_initial);
                let refreshed = Arc::clone(&exchange_refreshed);
                let refresh = Arc::clone(&exchange_refresh);
                let secret = Arc::clone(&expected_secret);
                let count = exchange_counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    let form = String::from_utf8(body.to_vec()).unwrap();
                    assert!(form.contains(&format!("client_secret={secret}")), "{form}");
                    if count == 0 {
                        assert!(form.contains("grant_type=authorization_code"), "{form}");
                        Json(json!({
                            "access_token":initial.as_str(),
                            "refresh_token":refresh.as_str(),
                            "expires_in":0
                        }))
                    } else {
                        assert!(form.contains("grant_type=refresh_token"), "{form}");
                        assert!(form.contains(&format!("refresh_token={refresh}")), "{form}");
                        Json(json!({"access_token":refreshed.as_str(),"expires_in":3600}))
                    }
                }
            }),
        )
        .route(
            "/load-code-assist",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let access = Arc::clone(&project_access);
                async move {
                    assert_eq!(
                        headers[header::AUTHORIZATION],
                        format!("Bearer {access}")
                    );
                    assert_eq!(body["metadata"]["pluginType"], "GEMINI");
                    Json(json!({
                        "cloudaicompanionProject":{"id":"resolved-code-assist-project"}
                    }))
                }
            }),
        )
        .route(
            "/v1internal:generateContent",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let access = Arc::clone(&upstream_access);
                async move {
                    assert_eq!(
                        headers[header::AUTHORIZATION],
                        format!("Bearer {access}")
                    );
                    assert!(headers.get("x-goog-api-key").is_none());
                    assert_eq!(body["project"], "resolved-code-assist-project");
                    assert_eq!(body["model"], "a-model");
                    Json(json!({"response":{
                        "responseId":"antigravity-response",
                        "candidates":[{"content":{"role":"model","parts":[{"text":"antigravity works"}]},"finishReason":"STOP"}],
                        "usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2}
                    }}))
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
            "UPDATE providers SET kind='gemini',settings_json=?,base_url=? WHERE id=?",
            vec![
                json!({"version":1,"oauth_test":{
                    "authorization_endpoint":"https://accounts.example.test/authorize",
                    "token_endpoint":format!("http://{address}/google-token"),
                    "provider_token_endpoint":format!("http://{address}/load-code-assist")
                }})
                .to_string()
                .into(),
                format!("http://{address}").into(),
                provider.clone().into(),
            ],
        ))
        .await
        .unwrap();
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE providers SET enabled=0 WHERE id=?",
            vec![f.providers[1].clone().into()],
        ))
        .await
        .unwrap();
    let cookie = oauth_owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/providers/{provider}/oauth/antigravity",
        db::DEFAULT_PROJECT_ID
    );
    let started = oauth_request(
        &f,
        &cookie,
        &format!("{base}/start"),
        json!({
            "client_id":"antigravity-client",
            "client_secret":client_secret.as_str(),
            "redirect_uri":"https://console.example.test/channels"
        }),
    )
    .await;
    assert_eq!(started.status(), StatusCode::OK);
    let started: Value =
        serde_json::from_slice(&to_bytes(started.into_body(), 64 * 1024).await.unwrap()).unwrap();
    let completed = oauth_request(
        &f,
        &cookie,
        &format!("{base}/complete"),
        json!({"state":started["state"],"code":"google-code"}),
    )
    .await;
    assert_eq!(completed.status(), StatusCode::OK);
    let bytes = to_bytes(completed.into_body(), 64 * 1024).await.unwrap();
    let public = String::from_utf8_lossy(&bytes);
    for secret in [
        initial_access.as_str(),
        refresh_token.as_str(),
        client_secret.as_str(),
    ] {
        assert!(!public.contains(secret));
    }
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
    assert!(!envelope.contains(initial_access.as_str()));
    let decrypted = f.state.secrets.decrypt(&envelope).unwrap();
    assert!(decrypted.contains(initial_access.as_str()));
    assert!(decrypted.contains(refresh_token.as_str()));
    assert!(decrypted.contains("resolved-code-assist-project"));

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
    let status = gateway.status();
    let body = to_bytes(gateway.into_body(), 64 * 1024).await.unwrap();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["choices"][0]["message"]["content"],
        "antigravity works"
    );
    assert_eq!(exchanges.load(Ordering::SeqCst), 2);
    let refreshed_envelope: String = f
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
    assert!(
        f.state
            .secrets
            .decrypt(&refreshed_envelope)
            .unwrap()
            .contains(refreshed_access.as_str())
    );
    let audit: i64 = f
        .state
        .db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='credentials.oauth.refresh'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(audit, 1);
    server.abort();
}

#[tokio::test]
async fn copilot_device_oauth_slows_down_encrypts_and_refreshes_provider_token() {
    let device_code = Arc::new(format!("device-{}", Uuid::new_v4()));
    let github_token = Arc::new(format!("github-{}", Uuid::new_v4()));
    let first_copilot = Arc::new(format!("copilot-first-{}", Uuid::new_v4()));
    let refreshed_copilot = Arc::new(format!("copilot-refreshed-{}", Uuid::new_v4()));
    let github_polls = Arc::new(AtomicUsize::new(0));
    let copilot_exchanges = Arc::new(AtomicUsize::new(0));
    let start_device = Arc::clone(&device_code);
    let poll_counter = Arc::clone(&github_polls);
    let poll_device = Arc::clone(&device_code);
    let poll_github = Arc::clone(&github_token);
    let copilot_counter = Arc::clone(&copilot_exchanges);
    let exchange_github = Arc::clone(&github_token);
    let exchange_first = Arc::clone(&first_copilot);
    let exchange_refreshed = Arc::clone(&refreshed_copilot);
    let upstream_copilot = Arc::clone(&refreshed_copilot);
    let first_expiry = db::now() + 300;
    let refreshed_expiry = db::now() + 3600;
    let mock = Router::new()
        .route(
            "/device",
            post(move |body: Bytes| {
                let device = Arc::clone(&start_device);
                async move {
                    let form = String::from_utf8(body.to_vec()).unwrap();
                    assert!(form.contains("client_id=copilot-client"), "{form}");
                    assert!(form.contains("scope=read%3Auser"), "{form}");
                    Json(json!({
                        "device_code":device.as_str(),
                        "user_code":"ABCD-EFGH",
                        "verification_uri":"https://github.com/login/device",
                        "expires_in":900,
                        "interval":1
                    }))
                }
            }),
        )
        .route(
            "/github-token",
            post(move |body: Bytes| {
                let device = Arc::clone(&poll_device);
                let github = Arc::clone(&poll_github);
                let count = poll_counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    let form = String::from_utf8(body.to_vec()).unwrap();
                    assert!(form.contains(&format!("device_code={device}")), "{form}");
                    match count {
                        0 => Json(json!({"error":"authorization_pending"})),
                        1 => Json(json!({"error":"slow_down"})),
                        _ => Json(json!({"access_token":github.as_str(),"token_type":"bearer"})),
                    }
                }
            }),
        )
        .route(
            "/copilot-token",
            get(move |headers: HeaderMap| {
                let github = Arc::clone(&exchange_github);
                let first = Arc::clone(&exchange_first);
                let refreshed = Arc::clone(&exchange_refreshed);
                let count = copilot_counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    assert_eq!(
                        headers[header::AUTHORIZATION],
                        format!("token {github}")
                    );
                    if count == 0 {
                        Json(json!({"token":first.as_str(),"expires_at":first_expiry}))
                    } else {
                        Json(json!({"token":refreshed.as_str(),"expires_at":refreshed_expiry}))
                    }
                }
            }),
        )
        .route(
            "/chat/completions",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let token = Arc::clone(&upstream_copilot);
                async move {
                    assert_eq!(
                        headers[header::AUTHORIZATION],
                        format!("Bearer {token}")
                    );
                    assert_eq!(headers["editor-version"], "vscode/1.95.0");
                    assert_eq!(headers["editor-plugin-version"], "copilot-chat/0.26.7");
                    assert_eq!(headers["copilot-integration-id"], "vscode-chat");
                    assert_eq!(headers["openai-intent"], "conversation-edits");
                    assert_eq!(headers["x-github-api-version"], "2025-04-01");
                    assert_eq!(
                        headers["x-vscode-user-agent-library-version"],
                        "electron-fetch"
                    );
                    assert_eq!(headers["x-initiator"], "user");
                    assert_eq!(body["model"], "a-model");
                    Json(json!({
                        "id":"copilot-response",
                        "object":"chat.completion",
                        "choices":[{"index":0,"message":{"role":"assistant","content":"copilot works"},"finish_reason":"stop"}],
                        "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}
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
                    "device_endpoint":format!("http://{address}/device"),
                    "token_endpoint":format!("http://{address}/github-token"),
                    "provider_token_endpoint":format!("http://{address}/copilot-token")
                }})
                .to_string()
                .into(),
                format!("http://{address}").into(),
                provider.clone().into(),
            ],
        ))
        .await
        .unwrap();
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE providers SET enabled=0 WHERE id=?",
            vec![f.providers[1].clone().into()],
        ))
        .await
        .unwrap();
    let cookie = oauth_owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/providers/{provider}/oauth/github_copilot",
        db::DEFAULT_PROJECT_ID
    );
    let started = oauth_request(
        &f,
        &cookie,
        &format!("{base}/start"),
        json!({"client_id":"copilot-client"}),
    )
    .await;
    assert_eq!(started.status(), StatusCode::OK);
    let started: Value =
        serde_json::from_slice(&to_bytes(started.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert_eq!(started["interval"], 5);
    assert_eq!(started["user_code"], "ABCD-EFGH");
    let complete_path = format!("{base}/complete");
    let complete = || {
        oauth_request(
            &f,
            &cookie,
            &complete_path,
            json!({"state":started["state"]}),
        )
    };

    let pending = complete().await;
    assert_eq!(pending.status(), StatusCode::OK);
    let pending: Value =
        serde_json::from_slice(&to_bytes(pending.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert_eq!(pending["status"], "pending");
    assert_eq!(pending["retry_after"], 5);
    assert_eq!(github_polls.load(Ordering::SeqCst), 1);

    f.state
        .db
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE provider_oauth_states SET last_poll_at=unixepoch()-5",
        ))
        .await
        .unwrap();
    let slowed = complete().await;
    assert_eq!(slowed.status(), StatusCode::OK);
    let slowed: Value =
        serde_json::from_slice(&to_bytes(slowed.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert_eq!(slowed["status"], "pending");
    assert_eq!(slowed["retry_after"], 10);
    assert_eq!(github_polls.load(Ordering::SeqCst), 2);

    let locally_throttled = complete().await;
    assert_eq!(locally_throttled.status(), StatusCode::OK);
    assert_eq!(github_polls.load(Ordering::SeqCst), 2);
    f.state
        .db
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE provider_oauth_states SET last_poll_at=unixepoch()-10",
        ))
        .await
        .unwrap();
    let completed = complete().await;
    assert_eq!(completed.status(), StatusCode::OK);
    let bytes = to_bytes(completed.into_body(), 64 * 1024).await.unwrap();
    let public = String::from_utf8_lossy(&bytes);
    for secret in [github_token.as_str(), first_copilot.as_str()] {
        assert!(!public.contains(secret));
    }
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
    assert!(!envelope.contains(github_token.as_str()));
    let decrypted = f.state.secrets.decrypt(&envelope).unwrap();
    assert!(decrypted.contains(github_token.as_str()));
    assert!(decrypted.contains(first_copilot.as_str()));

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
    let status = gateway.status();
    let body = to_bytes(gateway.into_body(), 64 * 1024).await.unwrap();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["choices"][0]["message"]["content"],
        "copilot works"
    );
    assert_eq!(copilot_exchanges.load(Ordering::SeqCst), 2);
    let refreshed_envelope: String = f
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
    assert!(
        f.state
            .secrets
            .decrypt(&refreshed_envelope)
            .unwrap()
            .contains(refreshed_copilot.as_str())
    );
    server.abort();
}
