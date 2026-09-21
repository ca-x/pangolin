use super::*;

async fn session(f: &Fixture) -> String {
    let user = db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: "audit-admin@example.com".into(),
            password: "audit-password-123456".into(),
            instance_name: None,
            language: Some("en".into()),
        },
    )
    .await
    .unwrap();
    db::create_session(&f.state.db, &user.id).await.unwrap()
}
async fn admin(
    f: &Fixture,
    cookie: &str,
    method: http::Method,
    path: &str,
    body: Value,
) -> Response {
    let unsafe_method = method != http::Method::GET
        && method != http::Method::HEAD
        && method != http::Method::OPTIONS;
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("cookie", format!("pangolin_session={cookie}"))
        .header("content-type", "application/json");
    if unsafe_method {
        request = request.header("x-pangolin-csrf", "1");
    }
    router(f.state.clone())
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
async fn count(f: &Fixture, query: &str) -> i64 {
    f.state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            query,
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}

/// A successful control-plane mutation must leave exactly one audit row, and
/// the business resource must exist. An unauthorised or invalid mutation must
/// leave zero audit rows and leave no side-effect.
#[tokio::test]
async fn control_plane_mutations_are_transactionally_audited() {
    let f = fixture(Router::new()).await;
    let cookie = session(&f).await;

    // ---- create provider (success) ----
    let before = count(
        &f,
        "SELECT COUNT(*) AS n FROM audit_events WHERE action='create' AND resource_type='provider'",
    )
    .await;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        "/api/admin/v1/providers",
        json!({"name":"audit-provider","kind":"openai","base_url":"https://api.openai.com","api_key":"sk-test-key"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='create' AND resource_type='provider'",
        )
        .await,
        before + 1
    );

    // ---- delete provider (not found) ----
    let before = count(
        &f,
        "SELECT COUNT(*) AS n FROM audit_events WHERE action='delete' AND resource_type='provider'",
    )
    .await;
    let response = admin(
        &f,
        &cookie,
        http::Method::DELETE,
        "/api/admin/v1/providers/nonexistent-id",
        Value::Null,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='delete' AND resource_type='provider'",
        )
        .await,
        before
    );

    // ---- create model (success) ----
    // Find the provider we created above (its id is in the audit details).
    let provider_id: String = f
        .state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT resource_id FROM audit_events WHERE action='create' AND resource_type='provider' ORDER BY created_at DESC LIMIT 1",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "resource_id")
        .unwrap();
    let before = count(
        &f,
        "SELECT COUNT(*) AS n FROM audit_events WHERE action='create' AND resource_type='model'",
    )
    .await;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        "/api/admin/v1/models",
        json!({"provider_id":provider_id,"public_name":"audit-model","upstream_name":"gpt-4o"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='create' AND resource_type='model'",
        )
        .await,
        before + 1
    );

    // ---- create model with invalid provider (failure) ----
    let before = count(
        &f,
        "SELECT COUNT(*) AS n FROM audit_events WHERE action='create' AND resource_type='model'",
    )
    .await;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        "/api/admin/v1/models",
        json!({"provider_id":"nonexistent-provider","public_name":"bad-model","upstream_name":"gpt-4o"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='create' AND resource_type='model'",
        )
        .await,
        before
    );

    // ---- create api-key (success) ----
    let before = count(
        &f,
        "SELECT COUNT(*) AS n FROM audit_events WHERE action='create' AND resource_type='api_key'",
    )
    .await;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        "/api/admin/v1/api-keys",
        json!({"name":"audit-key"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='create' AND resource_type='api_key'",
        )
        .await,
        before + 1
    );
}
