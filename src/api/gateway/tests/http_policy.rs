use super::*;

async fn set_policy(f: &Fixture, origins: &[&str], timeout_ms: u64) {
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO settings(key,value,updated_at) VALUES('system',?,unixepoch()) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            vec![json!({
                "instance_name":"Pangolin",
                "branding_name":"Pangolin / 鲮鲤",
                "favicon_url":"/logo.webp",
                "onboarding_complete":false,
                "cors_allowed_origins":origins,
                "request_timeout_ms":timeout_ms,
            }).to_string().into()],
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn gateway_http_policy_defaults_to_same_origin_and_echoes_only_allowed_origins() {
    let f = fixture(Router::new()).await;
    let default = router(f.state.clone())
        .oneshot(
            Request::get("/api/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        default
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );

    set_policy(&f, &["https://console.example.test"], 1_000).await;
    let allowed = router(f.state.clone())
        .oneshot(
            Request::get("/api/health/live")
                .header(header::ORIGIN, "https://console.example.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        allowed.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static("https://console.example.test"))
    );
    let denied = router(f.state.clone())
        .oneshot(
            Request::get("/api/health/live")
                .header(header::ORIGIN, "https://attacker.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        denied
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
    let bootstrap = router(f.state.clone())
        .oneshot(
            Request::get("/api/v1/bootstrap")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bootstrap: Value =
        serde_json::from_slice(&to_bytes(bootstrap.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert!(bootstrap["branding"].get("cors_allowed_origins").is_none());
    assert!(bootstrap["branding"].get("request_timeout_ms").is_none());
}

#[tokio::test]
async fn gateway_http_policy_timeout_is_a_complete_error_envelope() {
    let slow = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            Json(json!({"choices":[]}))
        }),
    );
    let f = fixture(slow).await;
    set_policy(&f, &[], 100).await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"error":{"type":"timeout_error","message":"request timed out"}})
    );
}

#[tokio::test]
async fn gateway_http_policy_cors_does_not_weaken_browser_csrf() {
    let f = fixture(Router::new()).await;
    set_policy(&f, &["https://console.example.test"], 1_000).await;
    let user = db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: "cors-owner@example.test".into(),
            password: "a secure cors password".into(),
            instance_name: None,
            language: None,
        },
    )
    .await
    .unwrap();
    let cookie = db::create_session(&f.state.db, &user.id).await.unwrap();
    let response = router(f.state.clone())
        .oneshot(
            Request::put("/api/admin/v1/settings/system")
                .header("cookie", format!("pangolin_session={cookie}"))
                .header(header::ORIGIN, "https://console.example.test")
                .header("sec-fetch-site", "cross-site")
                .header("x-pangolin-csrf", "1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
