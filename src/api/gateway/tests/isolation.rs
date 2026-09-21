use super::*;

async fn session(f: &Fixture) -> (String, String) {
    let user = db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: "isolation-owner@example.com".into(),
            password: "isolation-password-123456".into(),
            instance_name: None,
            language: Some("en".into()),
        },
    )
    .await
    .unwrap();
    let cookie = db::create_session(&f.state.db, &user.id).await.unwrap();
    (cookie, user.id)
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

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap(),
    )
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

/// The legacy non-project admin routes authorize against the default project, so their
/// queries must be scoped to it: a default-project administrator must not be able to read,
/// delete or attach models to another project's channels.
#[tokio::test]
async fn legacy_admin_routes_stay_inside_the_default_project() {
    let f = fixture(Router::new()).await;
    let (cookie, owner) = session(&f).await;
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,owner_user_id,is_default,enabled,created_at,updated_at) VALUES('other-project','Other','other',?,0,1,0,0)",
        vec![owner.into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO providers(id,name,kind,base_url,enabled,created_at,updated_at,project_id,settings_json) VALUES('other-provider','other-channel','openai','https://example.test',1,0,0,'other-project','{\"version\":1}')",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,input_price_micros,output_price_micros,priority,enabled,created_at,catalog_metadata_json) VALUES('other-model','other-provider','other-public','other-upstream','[\"chat\"]',0,0,1,1,0,'{\"version\":1}')",
        vec![],
    )
    .await;

    let providers = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            "/api/admin/v1/providers",
            json!(null),
        )
        .await,
    )
    .await;
    assert!(
        !providers
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "other-channel"),
        "provider list leaked another project's channel: {providers}"
    );

    let models = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            "/api/admin/v1/models",
            json!(null),
        )
        .await,
    )
    .await;
    assert!(
        !models
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["public_name"] == "other-public"),
        "model list leaked another project's model: {models}"
    );

    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::DELETE,
            "/api/admin/v1/providers/other-provider",
            json!(null)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::DELETE,
            "/api/admin/v1/models/other-model",
            json!(null)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM providers WHERE id='other-provider'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM models WHERE id='other-model'"
        )
        .await,
        1
    );

    let attached = admin(
        &f,
        &cookie,
        http::Method::POST,
        "/api/admin/v1/models",
        json!({"provider_id": "other-provider", "public_name": "leak", "upstream_name": "leak"}),
    )
    .await;
    assert_eq!(attached.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM models WHERE public_name='leak'"
        )
        .await,
        0
    );
}
