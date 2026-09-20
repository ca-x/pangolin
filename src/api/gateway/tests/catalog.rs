use super::*;

async fn session(f: &Fixture) -> String {
    let user = db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: "catalog-owner@example.com".into(),
            password: "catalog-password-123456".into(),
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
        .oneshot(
            request
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
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

#[tokio::test]
async fn catalog_api_is_offline_filterable_versioned_and_authorized() {
    let f = fixture(Router::new()).await;
    let cookie = session(&f).await;
    let response = admin(
        &f,
        &cookie,
        http::Method::GET,
        "/api/admin/v1/catalog/providers?adapter_available=true&q=openai",
        Value::Null,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let list = json_body(response).await;
    assert!(list["total"].as_u64().unwrap() >= 2);
    assert!(list["version"].as_str().unwrap().starts_with("effective-"));
    let response = admin(
        &f,
        &cookie,
        http::Method::GET,
        "/api/admin/v1/catalog/models?developer=google&modality=image&capability=reasoning",
        Value::Null,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json_body(response).await["total"].as_u64().unwrap() > 0);
    let exported = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            "/api/admin/v1/catalog/export",
            Value::Null,
        )
        .await,
    )
    .await;
    assert!(exported["providers"].as_array().unwrap().len() >= 50);
    assert!(exported["models"].as_array().unwrap().len() >= 400);
    let denied = router(f.state.clone())
        .oneshot(
            Request::post("/api/admin/v1/catalog/import")
                .header("authorization", format!("Bearer {}", f.token))
                .header("content-type", "application/json")
                .body(Body::from(exported.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        "/api/admin/v1/catalog/import",
        exported,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bad=admin(&f,&cookie,http::Method::POST,"/api/admin/v1/catalog/sources",json!({"name":"unsafe","url":"https://127.0.0.1/catalog","priority":10,"refresh_interval_secs":3600,"enabled":true,"signature_policy":"optional","public_key":null})).await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let created=admin(&f,&cookie,http::Method::POST,"/api/admin/v1/catalog/sources",json!({"name":"public","url":"https://catalog.example.com/catalog.json","priority":10,"refresh_interval_secs":3600,"enabled":true,"signature_policy":"optional","public_key":null})).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json_body(created).await;
    assert_eq!(created["revision"], 1);
    assert!(created["active_snapshot_id"].is_null());
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::DELETE,
            &format!(
                "/api/admin/v1/catalog/sources/{}",
                created["id"].as_str().unwrap()
            ),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn provider_presets_install_endpoint_mapping_and_keep_tpm_policy() {
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let f=fixture(Router::new().route("/chat/completions",post(move|Json(body):Json<Value>|{let seen=copy.clone();async move{seen.lock().await.push(body);Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}))}}))).await;
    let base = db::list_providers(&f.state.db).await.unwrap()[0]
        .base_url
        .clone();
    let provider = db::create_provider(
        &f.state.db,
        &ProviderInput {
            name: "Github preset".into(),
            kind: "github".into(),
            base_url: base.clone(),
            api_key: String::new(),
        },
        f.state.secrets.encrypt("mock-github-key").unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(provider.kind, "openai_compatible");
    assert_eq!(provider.base_url, base);
    db::create_model(
        &f.state.db,
        &ModelInput {
            provider_id: provider.id.clone(),
            public_name: "preset-model".into(),
            upstream_name: "model".into(),
            capabilities: None,
            input_price_micros: Some(0),
            output_price_micros: Some(0),
            priority: None,
        },
    )
    .await
    .unwrap();
    sql(
        &f,
        "UPDATE providers SET settings_json=? WHERE id=?",
        vec![
            json!({"version":1,"limits":{"tpm":10000}})
                .to_string()
                .into(),
            provider.id.into(),
        ],
    )
    .await;
    let response = request(
        &f,
        "/v1/chat/completions",
        json!({"model":"preset-model","messages":[{"role":"user","content":"hello"}]}),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        json_body(response).await
    );
    assert_eq!(seen.lock().await[0]["max_tokens"], 4096);
}

#[tokio::test]
async fn model_catalog_defaults_preserve_explicit_admin_prices_and_capabilities() {
    let f = fixture(Router::new()).await;
    let card = crate::catalog::builtin()
        .models
        .iter()
        .find(|model| model.model_type == "embedding")
        .unwrap();
    let model = db::create_model(
        &f.state.db,
        &ModelInput {
            provider_id: f.providers[0].clone(),
            public_name: "catalog-embedding".into(),
            upstream_name: card.upstream_id.clone(),
            capabilities: None,
            input_price_micros: Some(17),
            output_price_micros: Some(0),
            priority: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&model.capabilities).unwrap(),
        json!(["embeddings"])
    );
    assert_eq!(model.input_price_micros, 17);
    assert_eq!(model.output_price_micros, 0);
    let metadata: Value = serde_json::from_str(&model.catalog_metadata_json).unwrap();
    assert_eq!(metadata["card"]["id"], card.id);
    let explicit = db::create_model(
        &f.state.db,
        &ModelInput {
            provider_id: f.providers[0].clone(),
            public_name: "explicit".into(),
            upstream_name: card.upstream_id.clone(),
            capabilities: Some(vec!["rerank".into()]),
            input_price_micros: Some(0),
            output_price_micros: Some(0),
            priority: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(explicit.capabilities, "[\"rerank\"]");
    assert_eq!(explicit.input_price_micros, 0);
}
