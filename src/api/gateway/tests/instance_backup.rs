use super::*;
use crate::{
    access,
    operations::{self as ops, instance_backup as full},
};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};

const PASSPHRASE: &str = "portable-backup-contract-passphrase";
async fn disk_fixture(name: &str) -> (Fixture, access::Principal, String) {
    let mock=Router::new().route("/v1/chat/completions",post(|headers:HeaderMap|async move{assert_eq!(headers["authorization"],"Bearer full-instance-provider-sentinel");Json(json!({"id":"full-response-identity","choices":[{"message":{"content":"restored"}}],"usage":{"prompt_tokens":3,"completion_tokens":2}}))}));
    let mut f = fixture(mock).await;
    let upstream = f
        .state
        .db
        .query_one(ops::sql("SELECT base_url FROM providers LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "base_url")
        .unwrap();
    let path = f._directory.path().join("instance.sqlite");
    let mut config = (*f.state.config).clone();
    config.database_url = format!("sqlite://{}?mode=rwc", path.display());
    config.public_url = Some(upstream);
    f.state.db = db::connect(&config.database_url).await.unwrap();
    f.state.config = Arc::new(config);
    let user = db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: format!("{name}@example.com"),
            password: "instance-owner-password-123".into(),
            instance_name: Some(name.into()),
            language: None,
        },
    )
    .await
    .unwrap();
    let cookie = db::create_session(&f.state.db, &user.id).await.unwrap();
    (f, access::Principal::session(user.id), cookie)
}
async fn fill(f: &mut Fixture, owner: &access::Principal) {
    let provider = db::create_provider(
        &f.state.db,
        &ProviderInput {
            name: "full-provider".into(),
            kind: "openai".into(),
            base_url: f.state.config.public_url.clone().unwrap(),
            api_key: String::new(),
        },
        f.state
            .secrets
            .encrypt("full-instance-provider-sentinel")
            .unwrap(),
    )
    .await
    .unwrap();
    f.providers = vec![provider.id.clone()];
    db::create_model(
        &f.state.db,
        &ModelInput {
            provider_id: provider.id,
            public_name: "public".into(),
            upstream_name: "upstream".into(),
            capabilities: None,
            input_price_micros: Some(1000000),
            output_price_micros: Some(2000000),
            priority: None,
        },
    )
    .await
    .unwrap();
    let (_, token) = db::create_api_key(
        &f.state.db,
        &ApiKeyInput {
            name: "full-key".into(),
            budget_micros: Some(100000),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    f.token = token;
    let member = access::create_user(
        &f.state.db,
        owner,
        &access::UserInput {
            email: "restored-member@example.com".into(),
            password: "member-password-123456".into(),
            display_name: None,
            language: None,
        },
    )
    .await
    .unwrap();
    let project = access::create_project(
        &f.state.db,
        owner,
        &access::ProjectInput {
            name: "secondary".into(),
            slug: "secondary".into(),
            owner_user_id: Some(owner.subject_id.clone()),
        },
    )
    .await
    .unwrap();
    let role = access::create_role(
        &f.state.db,
        owner,
        &project.id,
        &access::RoleInput {
            name: "reader".into(),
            permissions: vec!["project:read".into()],
        },
    )
    .await
    .unwrap();
    access::upsert_membership(
        &f.state.db,
        owner,
        &project.id,
        &access::MembershipInput {
            user_id: member.id,
            role_id: role.id,
            status: "active".into(),
        },
    )
    .await
    .unwrap();
    sql(f,"INSERT INTO settings(key,value,updated_at) VALUES('request_logging',?,0)",vec![json!({"enabled":true,"default_level":"metadata","key_override_enabled":true,"key_disable_allowed":true}).to_string().into()]).await;
    sql(f,"INSERT INTO oidc_providers(id,name,issuer_url,client_id,client_secret_envelope,created_at,updated_at) VALUES('oidc','oidc','https://id.example','client',?,0,0)",vec![f.state.secrets.encrypt("oidc-secret-sentinel").unwrap().into()]).await;
    sql(f,"INSERT INTO webhooks(id,project_id,name,url,secret_envelope,created_at,updated_at) VALUES('hook',?,'hook','https://hook.example',?,0,0)",vec![db::DEFAULT_PROJECT_ID.into(),f.state.secrets.encrypt(&json!({"project_id":db::DEFAULT_PROJECT_ID,"webhook_id":"hook","headers":{"authorization":"webhook-secret-sentinel"}}).to_string()).unwrap().into()]).await;
    let secret = ops::storage::envelope(
        &f.state.secrets,
        db::DEFAULT_PROJECT_ID,
        "s3-target",
        &json!({"access_key_id":"s3-access-sentinel","secret_access_key":"s3-secret-sentinel"}),
    )
    .unwrap();
    sql(f,"INSERT INTO data_storage_configs(id,project_id,name,kind,config_json,secret_envelope,created_at,updated_at) VALUES('s3-target',?,'cloud','s3',?,?,0,0)",vec![db::DEFAULT_PROJECT_ID.into(),json!({"kind":"s3","endpoint":"https://objects.example","bucket":"backups","region":"us-east-1","prefix":"test"}).to_string().into(),secret.into()]).await;
    sql(f,"INSERT INTO catalog_sources(id,name,url,refresh_interval_secs,signature_policy) VALUES('catalog','catalog','https://catalog.example/index.json',3600,'none')",vec![]).await;
    let builtin: Value =
        serde_json::from_str(include_str!("../../../catalog/data/builtin.json")).unwrap();
    let entry = &builtin["providers"][0];
    sql(f,"INSERT INTO catalog_overrides(kind,entry_id,entry_json,updated_at) VALUES('provider',?,?,0)",vec![entry["id"].as_str().unwrap().into(),entry.to_string().into()]).await;
    sql(f,"INSERT INTO api_key_profiles(id,project_id,name,created_at,updated_at) VALUES('profile',?,'profile',0,0)",vec![db::DEFAULT_PROJECT_ID.into()]).await;
    sql(f,"INSERT INTO prompts(id,project_id,name,role,content,created_at,updated_at) VALUES('prompt',?,'prompt','system','retained prompt',0,0)",vec![db::DEFAULT_PROJECT_ID.into()]).await;
    sql(f,"INSERT INTO prompt_protection_rules(id,project_id,name,content_pattern,action,created_at,updated_at) VALUES('rule',?,'rule','never-match','deny',0,0)",vec![db::DEFAULT_PROJECT_ID.into()]).await;
}
fn restore(artifact: full::Artifact, portable: bool, mode: full::Mode) -> full::Restore {
    full::Restore {
        artifact,
        passphrase: portable.then(|| PASSPHRASE.into()),
        mode,
        force: false,
    }
}
async fn n(f: &Fixture, table: &str) -> i64 {
    f.state
        .db
        .query_one(ops::sql(
            format!("SELECT COUNT(*) AS n FROM {table}"),
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}
async fn request_api(f: &Fixture, cookie: &str, path: &str, value: Value, csrf: bool) -> Response {
    let mut request = Request::post(path)
        .header("content-type", "application/json")
        .header("cookie", format!("pangolin_session={cookie}"));
    if csrf {
        request = request.header("x-pangolin-csrf", "1")
    };
    router(f.state.clone())
        .oneshot(request.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn full_instance_portable_restore_rebuilds_identities_configuration_and_secrets() {
    let (mut source, actor, _) = disk_fixture("source").await;
    fill(&mut source, &actor).await;
    let response = request(&source, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let artifact = full::export(
        &source.state,
        &actor,
        full::Export {
            include_history: true,
            passphrase: Some(PASSPHRASE.into()),
        },
    )
    .await
    .unwrap();
    let public = serde_json::to_string(&artifact).unwrap();
    assert!(!public.contains("sentinel"));
    assert!(!public.contains(PASSPHRASE));
    let (mut target, target_actor, _) = disk_fixture("fresh").await;
    let check = full::preflight(
        &target.state,
        &target_actor,
        restore(artifact.clone(), true, full::Mode::Fail),
    )
    .await
    .unwrap();
    assert!(!check.destination_has_data);
    assert_eq!(check.tables["users"], 2);
    assert_eq!(check.tables["projects"], 2);
    assert_eq!(check.tables["catalog_sources"], 1);
    full::restore(
        &target.state,
        &target_actor,
        restore(artifact, true, full::Mode::Fail),
    )
    .await
    .unwrap();
    for table in [
        "users",
        "projects",
        "project_memberships",
        "roles",
        "role_permissions",
        "user_role_bindings",
        "settings",
        "catalog_sources",
        "catalog_overrides",
        "providers",
        "channel_credentials",
        "models",
        "api_key_profiles",
        "api_keys",
        "prompts",
        "prompt_protection_rules",
        "webhooks",
        "data_storage_configs",
    ] {
        assert_eq!(n(&source, table).await, n(&target, table).await, "{table}")
    }
    assert_eq!(n(&target, "sessions").await, 0);
    assert_eq!(n(&target, "instance_restore_receipts").await, 1);
    assert_eq!(n(&target, "usage_logs").await, 1);
    assert!(
        db::authenticate_api_key(&target.state.db, &source.token, None)
            .await
            .unwrap()
            .is_some()
    );
    let envelope = target
        .state
        .db
        .query_one(ops::sql(
            "SELECT secret_envelope FROM channel_credentials LIMIT 1",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "secret_envelope")
        .unwrap();
    assert_eq!(
        &*target.state.secrets.decrypt(&envelope).unwrap(),
        "full-instance-provider-sentinel"
    );
    assert!(source.state.secrets.decrypt(&envelope).is_err());
    let restarted = SecretBox::load(target._directory.path(), None).unwrap();
    assert_eq!(
        &*restarted.decrypt(&envelope).unwrap(),
        "full-instance-provider-sentinel"
    );
    let user = db::find_user_by_email(&target.state.db, "source@example.com")
        .await
        .unwrap()
        .unwrap();
    assert!(crypto::verify_password(
        "instance-owner-password-123",
        &user.password_hash
    ));
    target.token = source.token.clone();
    let response = request(&target, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(n(&target, "usage_logs").await, 2);
}

#[tokio::test]
async fn full_instance_authorization_preflight_wrong_keys_and_replace_are_explicit() {
    let (mut source, owner, _) = disk_fixture("source").await;
    fill(&mut source, &owner).await;
    let artifact = full::export(&source.state, &owner, full::Export::default())
        .await
        .unwrap();
    let (mut target, target_owner, cookie) = disk_fixture("destination").await;
    assert!(
        full::preflight(
            &target.state,
            &target_owner,
            restore(artifact.clone(), false, full::Mode::Fail)
        )
        .await
        .is_err()
    );
    assert_eq!(n(&target, "users").await, 1);
    let response = request_api(
        &target,
        &cookie,
        "/api/admin/v1/instance/backup",
        json!({}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    drop(response);
    let response = router(target.state.clone())
        .oneshot(
            Request::post("/api/admin/v1/instance/restore")
                .header("content-type", "application/json")
                .body(Body::from("malformed-json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    target.state.secrets = source.state.secrets.clone();
    fill(&mut target, &target_owner).await;
    assert!(matches!(
        full::restore(
            &target.state,
            &target_owner,
            restore(artifact.clone(), false, full::Mode::Fail)
        )
        .await,
        Err(ApiError::Conflict(_))
    ));
    let held = target.state.maintenance.clone().read_owned().await;
    assert!(matches!(
        full::restore(
            &target.state,
            &target_owner,
            restore(artifact.clone(), false, full::Mode::Replace)
        )
        .await,
        Err(ApiError::Conflict(_))
    ));
    drop(held);
    full::restore(
        &target.state,
        &target_owner,
        restore(artifact.clone(), false, full::Mode::Replace),
    )
    .await
    .unwrap();
    full::restore(
        &target.state,
        &owner,
        restore(artifact, false, full::Mode::Replace),
    )
    .await
    .unwrap();
    assert_eq!(n(&target, "instance_restore_receipts").await, 1);
    let outsider = access::Principal::api_key(
        "service",
        db::DEFAULT_PROJECT_ID,
        None,
        ["project:manage".into()],
    );
    assert!(matches!(
        full::export(&target.state, &outsider, full::Export::default()).await,
        Err(ApiError::Forbidden)
    ));
}

#[tokio::test]
async fn full_instance_semantically_invalid_encrypted_snapshot_rolls_back_before_mutation() {
    let (mut source, owner, _) = disk_fixture("source").await;
    fill(&mut source, &owner).await;
    let mut artifact = full::export(
        &source.state,
        &owner,
        full::Export {
            include_history: false,
            passphrase: Some(PASSPHRASE.into()),
        },
    )
    .await
    .unwrap();
    let salt = STANDARD_NO_PAD
        .decode(artifact.salt.as_ref().unwrap())
        .unwrap();
    let key = SecretBox::passphrase(PASSPHRASE, &salt).unwrap();
    let mut payload: Value =
        serde_json::from_str(&key.decrypt(&artifact.envelope).unwrap()).unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        file.path(),
        STANDARD_NO_PAD
            .decode(payload["database"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    {
        let conn = rusqlite::Connection::open(file.path()).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys=OFF; UPDATE projects SET owner_user_id='nonexistent-owner';",
        )
        .unwrap();
    }
    let bytes = std::fs::read(file.path()).unwrap();
    artifact.database_digest = blake3::hash(&bytes).to_hex().to_string();
    payload["database_digest"] = json!(artifact.database_digest);
    payload["database"] = json!(STANDARD_NO_PAD.encode(bytes));
    artifact.envelope = key.encrypt(&payload.to_string()).unwrap();
    let (target, target_owner, _) = disk_fixture("destination").await;
    assert!(
        full::restore(
            &target.state,
            &target_owner,
            restore(artifact, true, full::Mode::Replace)
        )
        .await
        .is_err()
    );
    assert_eq!(n(&target, "users").await, 1);
    assert_eq!(n(&target, "providers").await, 0);
    assert_eq!(n(&target, "instance_restore_receipts").await, 0);
    access::authorize(&target.state.db, &target_owner, None, "*")
        .await
        .unwrap();
}

#[test]
fn full_instance_sqlite_backup_abort_retains_original_database() {
    let source = rusqlite::Connection::open_in_memory().unwrap();
    source
        .execute_batch(
            "CREATE TABLE value(data TEXT); INSERT INTO value VALUES(hex(randomblob(100000)));",
        )
        .unwrap();
    let mut target = rusqlite::Connection::open_in_memory().unwrap();
    target
        .execute_batch("CREATE TABLE value(data TEXT); INSERT INTO value VALUES('original');")
        .unwrap();
    {
        let backup = rusqlite::backup::Backup::new(&source, &mut target).unwrap();
        assert_eq!(backup.step(1).unwrap(), rusqlite::backup::StepResult::More);
    }
    assert_eq!(
        target
            .query_row("SELECT data FROM value", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "original"
    );
}

#[tokio::test]
async fn full_instance_without_history_preserves_spend_dedup_and_survives_derived_failure() {
    let (mut source, owner, _) = disk_fixture("source").await;
    fill(&mut source, &owner).await;
    let response = request(&source, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let artifact = full::export(
        &source.state,
        &owner,
        full::Export {
            include_history: false,
            passphrase: Some(PASSPHRASE.into()),
        },
    )
    .await
    .unwrap();
    let (mut target, target_owner, _) = disk_fixture("target").await;
    target.state.observations = ObservationStore::degraded(
        target._directory.path().join("unavailable.duckdb"),
        "injected derived failure",
    );
    let result = full::restore(
        &target.state,
        &target_owner,
        restore(artifact, true, full::Mode::Fail),
    )
    .await
    .unwrap();
    assert!(!result.derived_available);
    assert_eq!(n(&target, "usage_logs").await, 0);
    assert_eq!(n(&target, "request_facts").await, 0);
    assert_eq!(n(&target, "provider_response_settlements").await, 1);
    target.token = source.token.clone();
    let response = request(&target, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let row = target
        .state
        .db
        .query_one(ops::sql("SELECT spent_micros FROM api_keys", vec![]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "spent_micros").unwrap(), 7);
    let row = target
        .state
        .db
        .query_one(ops::sql("SELECT settlement_kind FROM usage_logs", vec![]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<String>("", "settlement_kind").unwrap(),
        "duplicate"
    );
}
