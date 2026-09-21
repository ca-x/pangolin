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

/// Authentication is the backbone of an audit trail: the first administrator, every successful
/// login and every rejected login must leave a durable record.
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
