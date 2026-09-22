use super::*;

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

async fn post_json(f: &Fixture, path: &str, body: Value) -> Response {
    router(f.state.clone())
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn post_login(app: &Router, email: &str, password: &str) -> Response {
    let mut request = Request::post("/api/v1/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"email": email, "password": password}).to_string(),
        ))
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "192.0.2.44:4312".parse::<SocketAddr>().unwrap(),
    ));
    app.clone().oneshot(request).await.unwrap()
}

/// Authentication is the backbone of an audit trail: the first administrator, every successful
/// login and every admitted rejected login must leave a durable record.
#[tokio::test]
async fn authentication_events_are_audited() {
    let f = fixture(Router::new()).await;

    let setup = post_json(
        &f,
        "/api/v1/setup",
        json!({
            "email": "audited-owner@example.com",
            "password": "audited-password-123456",
            "instance_name": "Audit",
            "language": "en",
        }),
    )
    .await;
    assert_eq!(setup.status(), StatusCode::OK);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='setup' AND resource_type='user'"
        )
        .await,
        1
    );

    let rejected = post_json(
        &f,
        "/api/v1/auth/login",
        json!({"email": "audited-owner@example.com", "password": "wrong-password"}),
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    let unknown = post_json(
        &f,
        "/api/v1/auth/login",
        json!({"email": "nobody@example.com", "password": "wrong-password"}),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='login_failed'"
        )
        .await,
        2
    );

    let accepted = post_json(
        &f,
        "/api/v1/auth/login",
        json!({"email": "audited-owner@example.com", "password": "audited-password-123456"}),
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='login' AND actor_user_id<>''"
        )
        .await,
        1
    );
}

/// Invalid local credentials, including a nonexistent account, have one public response. Once
/// the trusted peer exhausts its small admission budget, later attempts do no password/SQLite
/// work and therefore cannot append an unbounded audit stream.
#[tokio::test]
async fn password_login_bruteforce_is_bounded_without_account_enumeration() {
    let f = fixture(Router::new()).await;
    db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: "owner@example.com".into(),
            password: "correct horse battery staple".into(),
            instance_name: None,
            language: Some("en".into()),
        },
    )
    .await
    .unwrap();
    let app = router(f.state.clone());

    let wrong_account = post_login(&app, " OWNER@example.com ", "wrong").await;
    let unknown_account = post_login(&app, "missing@example.com", "wrong").await;
    let expected_status = wrong_account.status();
    let expected_body = to_bytes(wrong_account.into_body(), 4096).await.unwrap();
    assert_eq!(expected_status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown_account.status(), expected_status);
    assert_eq!(
        to_bytes(unknown_account.into_body(), 4096).await.unwrap(),
        expected_body
    );

    for email in [
        "owner@example.com",
        "OWNER@example.com",
        " owner@EXAMPLE.com",
        "Owner@Example.Com",
    ] {
        assert_eq!(
            post_login(&app, email, "wrong").await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let throttled = post_login(&app, "owner@example.com", "wrong").await;
    assert_eq!(throttled.status(), expected_status);
    assert_eq!(
        to_bytes(throttled.into_body(), 4096).await.unwrap(),
        expected_body,
        "throttling must not reveal whether the normalized account exists"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='login_failed'"
        )
        .await,
        6,
        "the rejected request must not consume the SQLite writer for another audit row"
    );
}
