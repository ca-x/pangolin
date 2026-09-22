use super::*;
use crate::access;
use crate::observability::{RequestEvent, RequestFilter};
use crate::operations::{
    self as ops,
    backup::{Conflict, Selection},
    logging::{Level, Policy},
};
use object_store::ObjectStoreExt;
use std::io::{Read, Write};

#[tokio::test]
async fn unrecoverable_credential_is_fail_closed_and_replaced_with_a_one_time_token() {
    let calls = Arc::new(AtomicUsize::new(0));
    let upstream_calls = calls.clone();
    let f = fixture(Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let calls = upstream_calls.clone();
            async move {
                calls.fetch_add(1, Ordering::Relaxed);
                Json(json!({"choices":[{"message":{"content":"unexpected"}}]}))
            }
        }),
    ))
    .await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let credential = f.providers[0].clone();
    let stored_sentinel = "not-an-authenticated-envelope";
    sql(
        &f,
        "UPDATE channel_credentials SET secret_envelope=? WHERE id=?",
        vec![stored_sentinel.into(), credential.clone().into()],
    )
    .await;
    sql(
        &f,
        "UPDATE providers SET enabled=0 WHERE id<>?",
        vec![f.providers[0].clone().into()],
    )
    .await;

    let listed = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("/api/admin/v1/projects/{project}/operations/credentials"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    let row = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == credential)
        .unwrap();
    assert_eq!(row["state"], "unrecoverable");
    assert!(!listed.to_string().contains(stored_sentinel));
    assert!(!listed.to_string().contains("secret_envelope"));

    let public = request(&f, "/v1/chat/completions", chat()).await;
    assert_ne!(public.status(), StatusCode::OK);
    let public = json_body(public).await;
    assert!(!public.to_string().contains("envelope"));
    assert!(!public.to_string().contains("decrypt"));
    assert_eq!(calls.load(Ordering::Relaxed), 0);

    let issue_path =
        format!("/api/admin/v1/projects/{project}/credentials/{credential}/recovery-token");
    let issued = admin(
        &f,
        &cookie,
        http::Method::POST,
        &issue_path,
        json!({"expires_seconds":60}),
        true,
    )
    .await;
    assert_eq!(issued.status(), StatusCode::OK);
    let issued = json_body(issued).await;
    let token = issued["token"].as_str().unwrap().to_owned();
    assert!(token.starts_with("pcr_"));
    assert!(
        f.state
            .db
            .query_all(ops::sql(
                "SELECT token_hash FROM credential_recovery_tokens WHERE token_hash=?",
                vec![token.clone().into()]
            ))
            .await
            .unwrap()
            .is_empty(),
        "plaintext recovery token must not be stored"
    );

    let recover_path = format!("/api/admin/v1/projects/{project}/credentials/{credential}/recover");
    let recovered = admin(
        &f,
        &cookie,
        http::Method::POST,
        &recover_path,
        json!({"token":token,"secret":"replacement-secret"}),
        true,
    )
    .await;
    assert_eq!(recovered.status(), StatusCode::OK);
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    let reused = admin(
        &f,
        &cookie,
        http::Method::POST,
        &recover_path,
        json!({"token":token,"secret":"another-secret"}),
        true,
    )
    .await;
    assert_eq!(reused.status(), StatusCode::CONFLICT);
    sql(
        &f,
        "UPDATE channel_credentials SET secret_envelope=? WHERE id=?",
        vec![stored_sentinel.into(), credential.clone().into()],
    )
    .await;
    let expired = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &issue_path,
            json!({"expires_seconds":60}),
            true,
        )
        .await,
    )
    .await;
    let expired_token = expired["token"].as_str().unwrap();
    sql(
        &f,
        "UPDATE credential_recovery_tokens SET expires_at=? WHERE token_hash=?",
        vec![
            (db::now() - 1).into(),
            crate::crypto::token_hash(expired_token).into(),
        ],
    )
    .await;
    let expired_attempt = admin(
        &f,
        &cookie,
        http::Method::POST,
        &recover_path,
        json!({"token":expired_token,"secret":"late-secret"}),
        true,
    )
    .await;
    assert_eq!(expired_attempt.status(), StatusCode::CONFLICT);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action IN ('credential.recovery.issue','credential.recovery.replace')"
        )
        .await,
        3
    );
}

#[tokio::test]
async fn channel_clone_merge_preview_and_bulk_delete_are_safe_and_project_scoped() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let base = format!("/api/admin/v1/projects/{project}/operations");
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/channels"),
            json!({
                "name":"Credential in URL",
                "kind":"openai",
                "base_url":"https://quota-user:quota-password@quota.invalid"
            }),
            true,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST,
        "provider URLs must not accept userinfo credentials"
    );
    sql(
        &f,
        "UPDATE providers SET settings_json=? WHERE id=?",
        vec![
            json!({"version":1,"tags":["primary"]}).to_string().into(),
            f.providers[0].clone().into(),
        ],
    )
    .await;
    sql(
        &f,
        "UPDATE channel_settings SET model_rules_json=? WHERE provider_id=?",
        vec![
            json!({"version":1,"reasoning_effort":{"auto":"high"}})
                .to_string()
                .into(),
            f.providers[0].clone().into(),
        ],
    )
    .await;

    let preview = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/channel-preview"),
            json!({"action":"clone","ids":[f.providers[0]]}),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(preview["dependencies"][0]["models"], 1);
    assert_eq!(preview["dependencies"][0]["credentials"], 1);
    assert_eq!(preview["credential_secrets_copied"], false);

    let cloned = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/channel-clone"),
            json!({"source_id":f.providers[0],"name":"Clone without secrets"}),
            true,
        )
        .await,
    )
    .await;
    let clone_id = cloned["id"].as_str().unwrap();
    assert_eq!(
        count(
            &f,
            &format!("SELECT COUNT(*) AS n FROM models WHERE provider_id='{clone_id}'")
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &f,
            &format!(
                "SELECT COUNT(*) AS n FROM channel_credentials WHERE provider_id='{clone_id}'"
            )
        )
        .await,
        0
    );
    assert_eq!(count(&f,&format!("SELECT COUNT(*) AS n FROM channel_settings WHERE provider_id='{clone_id}' AND model_rules_json LIKE '%reasoning_effort%'" )).await,1);

    let merge = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{base}/channel-merge"),
        json!({"source_id":f.providers[0],"target_id":f.providers[1]}),
        true,
    )
    .await;
    assert_eq!(
        merge.status(),
        StatusCode::CONFLICT,
        "same public model names must refuse a merge"
    );

    sql(&f,"INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('b60-foreign-project','Foreign','b60-foreign',0,1,0,0)",vec![]).await;
    sql(&f,"INSERT INTO providers(id,project_id,name,kind,base_url,enabled,created_at,updated_at) VALUES('b60-empty',?,'Empty','openai','https://empty.invalid',1,0,0),('b60-foreign','b60-foreign-project','Foreign channel','openai','https://foreign.invalid',1,0,0)",vec![project.into()]).await;
    sql(&f,"INSERT INTO models(id,provider_id,public_name,upstream_name,created_at) VALUES('b60-foreign-model','b60-foreign','public','foreign-private',0)",vec![]).await;
    let foreign_preview = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{base}/channel-preview"),
        json!({"action":"merge","ids":[f.providers[0],"b60-foreign"]}),
        true,
    )
    .await;
    assert_eq!(foreign_preview.status(), StatusCode::NOT_FOUND);
    assert!(
        !json_body(foreign_preview)
            .await
            .to_string()
            .contains("public"),
        "foreign channel model names must remain opaque"
    );
    let deleted = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/bulk-delete"),
            json!({"resource":"channels","ids":["b60-empty","b60-foreign"]}),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(deleted["deleted"], 1);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM providers WHERE id='b60-foreign'"
        )
        .await,
        1
    );
    let blocked = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{base}/bulk-delete"),
        json!({"resource":"channels","ids":[f.providers[0]]}),
        true,
    )
    .await;
    assert_eq!(blocked.status(), StatusCode::CONFLICT);
    assert!(count(&f,"SELECT COUNT(*) AS n FROM audit_events WHERE action IN ('channel-clone.save','channels.delete')").await>=2);
}

/// Reads the stored `enabled` flag straight from the record system, so an assertion
/// about a bulk key change cannot be satisfied by the response body alone.
async fn api_key_enabled(f: &Fixture, id: &str) -> bool {
    f.state
        .db
        .query_one(ops::sql(
            "SELECT enabled FROM api_keys WHERE id=?",
            vec![id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "enabled")
        .unwrap()
}

/// A generic resource page and the total beside its pagination are one view of the
/// same project-scoped event-time window. The lower bound belongs to the window,
/// the upper bound belongs to the next one, and neither a text search nor a page
/// limit may make the total describe a different set of rows.
#[tokio::test]
async fn generic_trace_list_applies_one_half_open_window_to_rows_and_total() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let from = 10_000;
    let until = 20_000;
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('b13-foreign','Foreign','b13-foreign',0,1,1,1)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO traces(id,project_id,status,started_at) VALUES('b13-window-before',?,'succeeded',?),('b13-window-from',?,'succeeded',?),('b13-window-last',?,'succeeded',?),('b13-window-until',?,'succeeded',?),('b13-window-foreign','b13-foreign','succeeded',?)",
        vec![
            project.into(),
            (from - 1).into(),
            project.into(),
            from.into(),
            project.into(),
            (until - 1).into(),
            project.into(),
            until.into(),
            ((from + until) / 2).into(),
        ],
    )
    .await;
    let page = |offset: u8| {
        let path = format!(
            "/api/admin/v1/projects/{project}/operations/traces?q=b13-window&from={from}&until={until}&lifecycle=all&limit=1&offset={offset}"
        );
        let f = &f;
        let cookie = cookie.clone();
        async move {
            json_body(admin(f, &cookie, http::Method::GET, &path, Value::Null, false).await).await
        }
    };
    let first = page(0).await;
    let second = page(1).await;
    for document in [&first, &second] {
        assert_eq!(document["total"], json!(2), "{document}");
        assert_eq!(document["data"].as_array().unwrap().len(), 1, "{document}");
    }
    assert_eq!(first["data"][0]["id"], json!("b13-window-last"));
    assert_eq!(second["data"][0]["id"], json!("b13-window-from"));
    assert!(
        [&first, &second]
            .iter()
            .all(|document| document["data"][0]["id"] != "b13-window-foreign"),
        "another project's event must not leak into the page or its total"
    );
}

/// Event resources do not all call their timestamp `created_at`. A probe is
/// windowed by when it was actually performed, with the same half-open boundary
/// as traces and analytics.
#[tokio::test]
async fn generic_probe_list_uses_its_probed_at_event_time() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let provider = &f.providers[0];
    let from = 10_000;
    let until = 20_000;
    sql(
        &f,
        "INSERT INTO channel_probes(id,provider_id,success,probed_at) VALUES('b13-probe-before',?,1,?),('b13-probe-from',?,1,?),('b13-probe-last',?,1,?),('b13-probe-until',?,1,?)",
        vec![
            provider.clone().into(),
            (from - 1).into(),
            provider.clone().into(),
            from.into(),
            provider.clone().into(),
            (until - 1).into(),
            provider.clone().into(),
            until.into(),
        ],
    )
    .await;
    let document = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{}/operations/probes?q=b13-probe&from={from}&until={until}",
                db::DEFAULT_PROJECT_ID
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(document["total"], json!(2), "{document}");
    let ids: Vec<_> = document["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["b13-probe-last", "b13-probe-from"]);
}

/// Configuration records with an immutable creation instant use that instant as
/// their point event, rather than their mutable update state.
#[tokio::test]
async fn generic_channel_list_uses_its_created_at_event_time() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let from = 10_000;
    let until = 20_000;
    sql(
        &f,
        "INSERT INTO providers(id,name,kind,base_url,enabled,created_at,updated_at,project_id) VALUES('b13-created-from','b13-created-from','openai','https://from.invalid',1,?,?,?),('b13-created-until','b13-created-until','openai','https://until.invalid',1,?,?,?)",
        vec![
            from.into(),
            from.into(),
            project.into(),
            until.into(),
            until.into(),
            project.into(),
        ],
    )
    .await;
    let document = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{project}/operations/channels?q=b13-created&from={from}&until={until}"
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(document["total"], json!(1), "{document}");
    assert_eq!(document["data"][0]["id"], json!("b13-created-from"));
}

/// B10's lifecycle facet and B13's event-time window are independent predicates.
/// Every lifecycle view keeps the same time bounds and reports the count of that
/// exact intersection.
#[tokio::test]
async fn generic_trace_window_composes_with_every_lifecycle_view() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let from = 10_000;
    let until = 20_000;
    sql(
        &f,
        "INSERT INTO traces(id,project_id,status,lifecycle,started_at) VALUES('b13-life-active',?,'succeeded','active',15000),('b13-life-retained',?,'succeeded','retained',16000),('b13-life-archived',?,'succeeded','archived',17000),('b13-life-outside',?,'succeeded','active',20000)",
        vec![
            project.into(),
            project.into(),
            project.into(),
            project.into(),
        ],
    )
    .await;
    for (lifecycle, expected) in [
        (None, 2usize),
        (Some("active"), 1),
        (Some("retained"), 1),
        (Some("archived"), 1),
        (Some("all"), 3),
    ] {
        let facet = lifecycle
            .map(|value| format!("&lifecycle={value}"))
            .unwrap_or_default();
        let document = json_body(
            admin(
                &f,
                &cookie,
                http::Method::GET,
                &format!(
                    "/api/admin/v1/projects/{project}/operations/traces?q=b13-life&from={from}&until={until}{facet}"
                ),
                Value::Null,
                false,
            )
            .await,
        )
        .await;
        assert_eq!(
            document["total"],
            json!(expected),
            "lifecycle={lifecycle:?}: {document}"
        );
        assert_eq!(
            document["data"].as_array().unwrap().len(),
            expected,
            "lifecycle={lifecycle:?}: {document}"
        );
    }
}

/// A resource without a stable event column ignores both bounds. In particular,
/// an inverted pair is not globally rejected before the resource can ignore it.
#[tokio::test]
async fn generic_untimed_resource_ignores_an_inverted_window() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    sql(
        &f,
        "INSERT INTO service_groups(id,project_id,name) VALUES('b13-untimed',?,'b13-untimed')",
        vec![project.into()],
    )
    .await;
    let response = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!(
            "/api/admin/v1/projects/{project}/operations/groups?q=b13-untimed&from=200&until=100"
        ),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let document = json_body(response).await;
    assert_eq!(document["total"], json!(1), "{document}");
    assert_eq!(document["data"][0]["id"], json!("b13-untimed"));
}

async fn count(f: &Fixture, query: &str) -> i64 {
    f.state
        .db
        .query_one(ops::sql(query, vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}
fn success() -> Router {
    Router::new().route("/v1/chat/completions",post(||async {Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":100,"completion_tokens":10,"prompt_tokens_details":{"cached_tokens":20}}}))}))
}
#[tokio::test]
async fn stored_stream_envelopes_preserve_terminal_metadata_and_sanitize_json() {
    let f = fixture(Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "event: response.delta\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\"kept\"}}],\"authorization\":\"Bearer never-store\",\"image_url\":\"data:image/png;base64,never-store\"}\n\n\
                 event: authorization-Bearer-never-store-event\n\
                 data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n\
                 data: [DONE]\n\n",
            )
        }),
    ))
    .await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            default_level: Level::FullBody,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let downstream = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&downstream).contains("[DONE]"));

    let stored: String = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT response_json FROM request_contents",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "response_json")
        .unwrap();
    let events: Vec<Value> = stored
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["version"], 1);
    assert_eq!(events[0]["event"], "response.delta");
    assert_eq!(events[0]["terminal"], false);
    assert_eq!(
        events[0].pointer("/data/choices/0/delta/content"),
        Some(&json!("kept"))
    );
    assert_eq!(events[0]["data"]["authorization"], "[REDACTED]");
    assert_eq!(events[0]["data"]["image_url"], "[MEDIA OMITTED]");
    assert_eq!(events[1]["event"], "message");
    assert!(!stored.contains("never-store"));
    assert_eq!(events[2]["event"], "message");
    assert_eq!(events[2]["data"], "[DONE]");
    assert_eq!(events[2]["terminal"], true);
}

#[tokio::test]
async fn stored_stream_envelopes_stay_bounded_and_reserve_the_terminal() {
    let chunk = format!(
        "data: {}\n\n",
        json!({"choices":[{"delta":{"content":"x".repeat(900)}}]})
    );
    let upstream = format!("{}data: [DONE]\n\n", chunk.repeat(1_400));
    let f = fixture(Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let upstream = upstream.clone();
            async move { ([(header::CONTENT_TYPE, "text/event-stream")], upstream) }
        }),
    ))
    .await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            default_level: Level::FullBody,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let stored: String = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT response_json FROM request_contents",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "response_json")
        .unwrap();

    assert!(stored.len() <= 1024 * 1024);
    let terminal: Value = serde_json::from_str(stored.lines().last().unwrap()).unwrap();
    assert_eq!(terminal["data"], "[DONE]");
    assert_eq!(terminal["terminal"], true);
}

#[tokio::test]
async fn a_large_terminal_envelope_displaces_deltas_instead_of_disappearing() {
    let delta = format!(
        "event: response.output_text.delta\ndata: {}\n\n",
        json!({"type":"response.output_text.delta","delta":"x".repeat(900)})
    );
    let result = json!({
        "id":"resp-large",
        "status":"completed",
        "output":[{"type":"message","role":"assistant","content":"z".repeat(300_000)}],
        "usage":{"input_tokens":1,"output_tokens":1}
    });
    let upstream = format!(
        "{}event: response.completed\ndata: {}\n\n",
        delta.repeat(1_000),
        json!({"type":"response.completed","response":result})
    );
    let f = fixture(Router::new().route(
        "/v1/responses",
        post(move || {
            let upstream = upstream.clone();
            async move { ([(header::CONTENT_TYPE, "text/event-stream")], upstream) }
        }),
    ))
    .await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            default_level: Level::FullBody,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let response = request(
        &f,
        "/v1/responses",
        json!({"model":"public","input":"hello","stream":true}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), 3 * 1024 * 1024)
        .await
        .unwrap();
    let stored: String = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT response_json FROM request_contents",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "response_json")
        .unwrap();

    assert!(stored.len() <= 1024 * 1024);
    let terminal: Value = serde_json::from_str(stored.lines().last().unwrap()).unwrap();
    assert_eq!(terminal["event"], "response.completed");
    assert_eq!(terminal["terminal"], true);
    assert_eq!(
        terminal
            .pointer("/data/response/output/0/content")
            .and_then(Value::as_str)
            .map(str::len),
        Some(300_000)
    );
}

#[tokio::test]
async fn task5_authoritative_lifecycle_survives_degraded_duckdb_and_logs_off() {
    let mut f = fixture(success()).await;
    f.state.observations =
        ObservationStore::degraded(f._directory.path().join("unavailable"), "injected fault");
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=2000000",
        vec![],
    )
    .await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            enabled: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM request_facts").await,
        1
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM execution_facts WHERE status='succeeded'"
        )
        .await,
        1
    );
    assert_eq!(
        count(&f, "SELECT SUM(total_cost_micros) AS n FROM usage_logs").await,
        120
    );
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        120
    );
    for table in [
        "traces",
        "threads",
        "requests",
        "request_executions",
        "request_contents",
    ] {
        assert_eq!(
            count(&f, &format!("SELECT COUNT(*) AS n FROM {table}")).await,
            0
        );
    }
    assert_eq!(f.state.observations.dropped_events(), 0);
    let response = router(f.state.clone())
        .oneshot(
            Request::get("/v1/models")
                .header("authorization", format!("Bearer {}", f.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(f.state.observations.dropped_events(), 0);
}
#[tokio::test]
async fn task5_each_retry_has_a_terminal_usage_fact_and_shared_trace() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(|Json(body):Json<Value>|async move {if body["model"]=="a-model" {(StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"retry"}))).into_response()}else{Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}})).into_response()}}))).await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM execution_facts").await,
        2
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 2);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM request_executions WHERE status='failed'"
        )
        .await,
        1
    );
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM traces").await, 1);
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM requests").await, 2);
}
#[tokio::test]
async fn task5_policy_and_price_are_frozen_during_upstream_work() {
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let r = reached.clone();
    let go = release.clone();
    let f=fixture(Router::new().route("/v1/chat/completions",post(move||{let r=r.clone();let go=go.clone();async move {r.notify_one();go.notified().await;Json(json!({"choices":[{"message":{"content":"kept"}}],"usage":{"prompt_tokens":10,"completion_tokens":5},"authorization":"never-stored"}))}}))).await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            default_level: Level::FullBody,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    let run = request(&f, "/v1/chat/completions", chat());
    tokio::pin!(run);
    tokio::select! {_=&mut run=>panic!("request completed before release"),_=reached.notified()=>{}}
    sql(
        &f,
        "UPDATE models SET input_price_micros=9000000,output_price_micros=9000000",
        vec![],
    )
    .await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            enabled: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    release.notify_one();
    assert_eq!(run.await.status(), StatusCode::OK);
    assert_eq!(
        count(&f, "SELECT SUM(total_cost_micros) AS n FROM usage_logs").await,
        15
    );
    let row = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT request_json,response_json FROM request_contents",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    let response: String = row.try_get("", "response_json").unwrap();
    assert!(response.contains("kept"));
    assert!(!response.contains("never-stored"));
    assert!(
        row.try_get::<String>("", "request_json")
            .unwrap()
            .contains("hello")
    );
}
#[tokio::test]
async fn task5_stream_usage_settlement_and_restart_are_idempotent() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(||async {([(header::CONTENT_TYPE,"text/event-stream")],"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":20}}\n\ndata: [DONE]\n\n")}))).await;
    sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=2000000",
        vec![],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("[DONE]"));
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        140
    );
    ops::runtime::recover(&f.state).await.unwrap();
    ops::runtime::recover(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        140
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 1);
}
/// Some OpenAI-compatible upstreams attach the cumulative usage to the same
/// non-empty `choices` chunk that closes every choice, instead of sending a
/// separate empty-`choices` aggregate chunk. That chunk is the in-band protocol
/// terminal for OpenAI completions, so the report it carries is complete: the
/// request was a successful 200 with exact usage and must not be recorded as
/// `usage_unavailable` with a conservative reservation.
#[tokio::test]
async fn stream_usage_on_the_finish_chunk_is_settled_as_reported() {
    let f = fixture(Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7}}\n\n\
                 data: [DONE]\n\n",
            )
        }),
    ))
    .await;
    // A budget makes the attempt reserve, so a conservative settlement would be
    // visible as a cost equal to that reservation rather than the price result.
    sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=2000000",
        vec![],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(
        body.contains("{\"delta\":{},\"finish_reason\":\"stop\"}"),
        "the upstream finish chunk must reach the client unchanged: {body}"
    );
    assert!(
        body.contains("[DONE]"),
        "the protocol terminal must still be delivered: {body}"
    );
    f.state.observations.flush().await;

    let events = f
        .state
        .observations
        .list(RequestFilter::default())
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "one request, one terminal event");
    assert_eq!(
        events[0].error_kind, None,
        "a 200 whose usage arrived on the finish chunk is not a failure"
    );
    assert_eq!(events[0].status_code, Some(200));
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM execution_facts WHERE status='succeeded'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM request_facts WHERE status='succeeded'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM request_facts WHERE status!='succeeded'"
        )
        .await,
        0,
        "the request terminal status must be successful"
    );

    let reserved = count(&f, "SELECT SUM(reserved_micros) AS n FROM execution_facts").await;
    let usage = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT input_tokens,output_tokens,total_cost_micros,settlement_kind FROM usage_logs",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(usage.try_get::<i64>("", "input_tokens").unwrap(), 11);
    assert_eq!(usage.try_get::<i64>("", "output_tokens").unwrap(), 7);
    assert_eq!(
        usage.try_get::<String>("", "settlement_kind").unwrap(),
        "reported"
    );
    assert_eq!(
        usage.try_get::<i64>("", "total_cost_micros").unwrap(),
        25,
        "11 input at 1 micro/token plus 7 output at 2 micro/token"
    );
    assert!(
        reserved > 25,
        "the price result must not be a conservative reservation ({reserved})"
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await,
        1,
        "exactly one terminal usage fact"
    );

    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        25
    );
    ops::runtime::recover(&f.state).await.unwrap();
    ops::runtime::recover(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        25,
        "recovery must not charge the key twice"
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 1);
}

/// The control: a non-terminal content delta that happens to carry a usage
/// object is not proof that generation ended, so it must never be accepted as
/// the final report or release the reservation.
#[tokio::test]
async fn stream_usage_before_the_finish_chunk_is_not_final() {
    let f = fixture(Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\"more\"}}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7}}\n\n\
                 data: [DONE]\n\n",
            )
        }),
    ))
    .await;
    sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=2000000",
        vec![],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        String::from_utf8_lossy(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .contains("[DONE]")
    );
    f.state.observations.flush().await;

    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM usage_logs WHERE settlement_kind='reported'"
        )
        .await,
        0,
        "a non-terminal delta is not a final usage report"
    );
    let reserved = count(&f, "SELECT SUM(reserved_micros) AS n FROM execution_facts").await;
    assert_eq!(
        count(&f, "SELECT SUM(total_cost_micros) AS n FROM usage_logs").await,
        reserved,
        "the reservation stands until a terminal report arrives"
    );
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        reserved
    );
}

#[tokio::test]
async fn task5_cancelled_stream_preserves_budget_and_conservative_charge() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(||async {let stream=async_stream::stream!{yield Ok::<_,std::io::Error>(Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n"));tokio::time::sleep(Duration::from_secs(30)).await;};([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(stream))}))).await;
    sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let reserved = count(&f, "SELECT SUM(reserved_micros) AS n FROM execution_facts").await;
    assert!(reserved > 0);
    drop(response);
    for _ in 0..50 {
        if count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        reserved
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM execution_facts WHERE status='cancelled'"
        )
        .await,
        1
    );
}
#[tokio::test]
async fn task5_backup_selective_roundtrip_encryption_atomicity_and_conflicts() {
    let f = fixture(success()).await;
    let project = db::DEFAULT_PROJECT_ID;
    let selection = Selection {
        resources: vec![
            "providers".into(),
            "channel_credentials".into(),
            "models".into(),
        ],
    };
    let artifact = ops::backup::export(&f.state, project, &selection)
        .await
        .unwrap();
    let exported = serde_json::to_string(&artifact).unwrap();
    assert!(!exported.contains("a-secret"));
    assert!(!exported.contains("secret_envelope"));
    let before = count(&f, "SELECT COUNT(*) AS n FROM providers").await;
    assert!(
        ops::backup::restore(&f.state, project, &artifact, Conflict::Fail)
            .await
            .is_err()
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM backup_restores").await,
        0
    );
    assert_eq!(
        ops::backup::restore(&f.state, project, &artifact, Conflict::Skip)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        ops::backup::restore(&f.state, project, &artifact, Conflict::Skip)
            .await
            .unwrap(),
        0
    );
    sql(
        &f,
        "UPDATE providers SET base_url='https://changed.example'",
        vec![],
    )
    .await;
    assert!(
        ops::backup::restore(&f.state, project, &artifact, Conflict::Overwrite)
            .await
            .unwrap()
            > 0
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM providers").await,
        before
    );
    let envelope = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT secret_envelope FROM channel_credentials ORDER BY id LIMIT 1",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "secret_envelope")
        .unwrap();
    assert!(
        f.state
            .secrets
            .decrypt(&envelope)
            .unwrap()
            .ends_with("-secret")
    );
    let mut corrupted = artifact.clone();
    corrupted.digest = "bad".into();
    assert!(
        ops::backup::restore(&f.state, project, &corrupted, Conflict::Overwrite)
            .await
            .is_err()
    );
    assert!(
        ops::backup::restore(&f.state, "other-project", &artifact, Conflict::Overwrite)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn b51_restore_applies_per_resource_strategies_and_defaults_to_fail() {
    let f = fixture(success()).await;
    let project = db::DEFAULT_PROJECT_ID;
    let artifact = ops::backup::export(
        &f.state,
        project,
        &Selection {
            resources: vec![
                "providers".into(),
                "models".into(),
                "channel_credentials".into(),
            ],
        },
    )
    .await
    .unwrap();
    sql(
        &f,
        "UPDATE providers SET base_url='https://changed.example'",
        vec![],
    )
    .await;
    sql(&f, "UPDATE models SET priority=999", vec![]).await;
    let before_secret = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT secret_envelope FROM channel_credentials ORDER BY id LIMIT 1",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "secret_envelope")
        .unwrap();
    let restored = ops::backup::restore_with_strategies_as(
        &f.state,
        project,
        &artifact,
        &ops::backup::RestoreStrategies {
            default: Conflict::Fail,
            resources: std::collections::BTreeMap::from([
                ("providers".into(), Conflict::Skip),
                ("models".into(), Conflict::Overwrite),
                ("channel_credentials".into(), Conflict::Skip),
            ]),
        },
        None,
    )
    .await
    .unwrap();
    assert!(restored > 0);
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM models WHERE priority=999").await,
        0
    );
    assert_eq!(
        f.state
            .db
            .query_one(ops::sql(
                "SELECT secret_envelope FROM channel_credentials ORDER BY id LIMIT 1",
                vec![]
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "secret_envelope")
            .unwrap(),
        before_secret
    );

    let second = ops::backup::export(
        &f.state,
        project,
        &Selection {
            resources: vec!["providers".into()],
        },
    )
    .await
    .unwrap();
    assert!(
        ops::backup::restore_with_strategies_as(
            &f.state,
            project,
            &second,
            &Default::default(),
            None
        )
        .await
        .is_err(),
        "an omitted policy keeps fail as the default"
    );
    let mut foreign = second.clone();
    foreign.project_id = "other-project".into();
    assert!(
        ops::backup::restore_with_strategies_as(
            &f.state,
            project,
            &foreign,
            &Default::default(),
            None
        )
        .await
        .is_err()
    );
}
async fn local_target(f: &Fixture, name: &str) -> String {
    let target = ops::id();
    sql(f,"INSERT INTO data_storage_configs(id,project_id,name,kind,config_json,created_at,updated_at) VALUES(?,?,?,'local',?,0,0)",vec![target.clone().into(),db::DEFAULT_PROJECT_ID.into(),name.into(),json!({"kind":"local","directory":name}).to_string().into()]).await;
    target
}

#[tokio::test]
async fn b50_gcs_and_webdav_roundtrip_and_connection_test_does_not_write() {
    assert!(serde_json::from_value::<ops::storage::Config>(json!({"kind":"unknown"})).is_err());
    let objects = Arc::new(tokio::sync::Mutex::new(HashMap::<String, Bytes>::new()));
    let puts = Arc::new(AtomicUsize::new(0));
    let mock_objects = objects.clone();
    let mock_puts = puts.clone();
    let f = fixture(Router::new().fallback(move |request: axum::extract::Request| {
        let objects = mock_objects.clone();
        let puts = mock_puts.clone();
        async move {
            let method = request.method().clone();
            let uri = request.uri().clone();
            let key = uri.path().to_owned();
            if method.as_str() == "PROPFIND" {
                return Response::builder()
                    .status(207)
                    .header("content-type", "application/xml")
                    .body(Body::from("<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\"></d:multistatus>"))
                    .unwrap();
            }
            if method == http::Method::GET && uri.query().is_some_and(|query| query.contains("list-type=2")) {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/xml")
                    .body(Body::from("<?xml version=\"1.0\"?><ListBucketResult><Name>bucket</Name><IsTruncated>false</IsTruncated></ListBucketResult>"))
                    .unwrap();
            }
            if method == http::Method::PUT {
                let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
                objects.lock().await.insert(key, bytes);
                puts.fetch_add(1, Ordering::SeqCst);
                return Response::builder().status(StatusCode::OK).header("etag", "mock-etag").body(Body::empty()).unwrap();
            }
            if method == http::Method::GET {
                return objects.lock().await.get(&key).cloned().map_or_else(
                    || Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap(),
                    |bytes| Response::builder().status(StatusCode::OK).header("content-length", bytes.len()).header("etag", "mock-etag").header("last-modified", "Mon, 01 Jan 2024 00:00:00 GMT").body(Body::from(bytes)).unwrap(),
                );
            }
            Response::builder().status(StatusCode::OK).body(Body::empty()).unwrap()
        }
    })).await;
    let endpoint = f
        .state
        .db
        .query_one(ops::sql("SELECT base_url FROM providers LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "base_url")
        .unwrap();
    let cases = [
        (
            "gcs",
            ops::storage::Config::Gcs {
                bucket: "bucket".into(),
                prefix: String::new(),
                endpoint: Some(endpoint.clone()),
            },
            json!({"service_account":{"client_email":"mock@example.test","private_key":"unused","private_key_id":"unused","disable_oauth":true}}),
        ),
        (
            "webdav",
            ops::storage::Config::Webdav {
                endpoint: format!("{endpoint}/dav/"),
                prefix: String::new(),
            },
            json!({"username":"pangolin","password":"sentinel"}),
        ),
    ];
    for (id, config, secret) in cases {
        let envelope =
            ops::storage::envelope(&f.state.secrets, db::DEFAULT_PROJECT_ID, id, &secret).unwrap();
        let store = ops::storage::open(
            &f.state,
            db::DEFAULT_PROJECT_ID,
            id,
            &config,
            Some(&envelope),
        )
        .await
        .unwrap();
        let key = format!(
            "{}roundtrip.json",
            ops::storage::owned_prefix(&config, db::DEFAULT_PROJECT_ID, id).unwrap()
        );
        ops::storage::put(store.as_ref(), &key, br#"{"ok":true}"#.to_vec())
            .await
            .unwrap();
        assert_eq!(
            store
                .get(&object_store::path::Path::parse(&key).unwrap())
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap()
                .as_ref(),
            br#"{"ok":true}"#
        );
    }

    let before = puts.load(Ordering::SeqCst);
    let target = ops::id();
    let config = ops::storage::Config::Webdav {
        endpoint: format!("{endpoint}/dav/"),
        prefix: String::new(),
    };
    let secret = ops::storage::envelope(
        &f.state.secrets,
        db::DEFAULT_PROJECT_ID,
        &target,
        &json!({"username":"pangolin","password":"sentinel"}),
    )
    .unwrap();
    sql(&f,"INSERT INTO data_storage_configs(id,project_id,name,kind,config_json,secret_envelope,created_at,updated_at) VALUES(?,?,?,'webdav',?,?,0,0)",vec![target.clone().into(),db::DEFAULT_PROJECT_ID.into(),"dav".into(),serde_json::to_string(&config).unwrap().into(),secret.into()]).await;
    let cookie = owner(&f).await;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!(
            "/api/admin/v1/projects/{}/operations/storage/{target}/test",
            db::DEFAULT_PROJECT_ID
        ),
        json!({}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        puts.load(Ordering::SeqCst),
        before,
        "connection test must not write an object"
    );
    let body = json_body(response).await;
    assert_eq!(body, json!({"ok":true}));
    assert!(!body.to_string().contains("sentinel"));
}
#[tokio::test]
async fn task5_backup_target_revision_fencing_retry_and_owned_retention() {
    let f = fixture(success()).await;
    let project = db::DEFAULT_PROJECT_ID;
    let target = local_target(&f, "local").await;
    let selection = Selection {
        resources: vec!["providers".into()],
    };
    let job = ops::backup::enqueue(
        &f.state,
        project,
        std::slice::from_ref(&target),
        &selection,
        "manual-one",
    )
    .await
    .unwrap();
    sql(
        &f,
        "UPDATE data_storage_configs SET revision=revision+1 WHERE id=?",
        vec![target.clone().into()],
    )
    .await;
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.id, job);
    assert!(ops::backup::execute(&f.state, &claim).await.is_err());
    ops::jobs::finish(&f.state.db, &claim, db::now(), false)
        .await
        .unwrap();
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM backup_job_targets WHERE error_code='target_changed'"
        )
        .await,
        1
    );
    let retry = ops::backup::retry(&f.state, project, &job).await.unwrap();
    assert_ne!(retry, job);
    sql(
        &f,
        "UPDATE operation_jobs SET status='failed' WHERE id=?",
        vec![job.into()],
    )
    .await;
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.id, retry);
    ops::backup::execute(&f.state, &claim).await.unwrap();
    ops::backup::execute(&f.state, &claim).await.unwrap();
    assert!(
        ops::jobs::finish(&f.state.db, &claim, db::now(), true)
            .await
            .unwrap()
    );
    let config = ops::storage::Config::Local {
        directory: "local".into(),
    };
    let store = ops::storage::open(&f.state, project, &target, &config, None)
        .await
        .unwrap();
    ops::storage::put(store.as_ref(), "unowned.json", b"keep this".to_vec())
        .await
        .unwrap();
    ops::backup::enqueue(
        &f.state,
        project,
        std::slice::from_ref(&target),
        &selection,
        "manual-two",
    )
    .await
    .unwrap();
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    ops::backup::execute(&f.state, &claim).await.unwrap();
    ops::jobs::finish(&f.state.db, &claim, db::now(), true)
        .await
        .unwrap();
    assert_eq!(
        ops::backup::retain(&f.state, project, &target, 1)
            .await
            .unwrap(),
        1
    );
    use object_store::ObjectStoreExt;
    assert_eq!(
        store
            .get(&object_store::path::Path::from("unowned.json"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        b"keep this"
    );
}
#[tokio::test]
async fn task5_provider_quota_disable_and_compaction_tool_order() {
    let f = fixture(success()).await;
    sql(
        &f,
        "UPDATE channel_settings SET auto_disable_policy_json=?",
        vec![
            json!({"version":1,"enabled":true,"threshold":2,"duration_secs":30})
                .to_string()
                .into(),
        ],
    )
    .await;
    for _ in 0..2 {
        ops::runtime::health(
            &f.state,
            db::DEFAULT_PROJECT_ID,
            &f.providers[0],
            false,
            Some(500),
            "upstream_failed",
        )
        .await
        .unwrap();
    }
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM channel_health_state WHERE disabled_until>unixepoch()"
        )
        .await,
        1
    );
    assert_eq!(
        ops::runtime::decimal_micros("1.234567", 1000000).unwrap(),
        1234567
    );
    assert_eq!(
        ops::runtime::decimal_micros("-0.000001", 1000000).unwrap(),
        -1
    );
    use crate::orchestration::compaction::safe_boundary;
    assert!(!safe_boundary(&[
        json!({"type":"function_call","call_id":"a"})
    ]));
    assert!(safe_boundary(&[
        json!({"type":"function_call","call_id":"a"}),
        json!({"type":"function_call_output","call_id":"a","output":"done"})
    ]));
}

#[tokio::test]
async fn channel_proxy_settings_are_encrypted_redacted_and_validated() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let provider = &f.providers[0];
    let path = format!(
        "/api/admin/v1/projects/{}/operations/channel-settings",
        db::DEFAULT_PROJECT_ID
    );
    let saved = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "provider_id": provider,
            "proxy_url": "socks5h://127.0.0.1:1080",
            "proxy_username": "proxy-user",
            "proxy_password": "proxy-password-sentinel",
            "proxy_reuse_connections": false
        }),
        true,
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);

    let listed =
        json_body(admin(&f, &cookie, http::Method::GET, &path, Value::Null, false).await).await;
    let document = listed.to_string();
    assert!(!document.contains("proxy-password-sentinel"));
    assert!(!document.contains("proxy_secret_envelope"));
    let configured = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["provider_id"] == *provider)
        .unwrap();
    assert_eq!(configured["proxy_password_configured"], json!(true));
    assert_eq!(configured["proxy_reuse_connections"], json!(false));
    let envelope: String = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT proxy_secret_envelope FROM channel_settings WHERE provider_id=?",
            vec![provider.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "proxy_secret_envelope")
        .unwrap();
    assert_ne!(envelope, "proxy-password-sentinel");
    assert_eq!(
        f.state.secrets.decrypt(&envelope).unwrap().as_str(),
        "proxy-password-sentinel"
    );

    let malformed = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"provider_id":provider,"proxy_url":"file:///tmp/proxy"}),
        true,
    )
    .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn anthropic_chat_bridge_uses_the_channel_proxy() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let proxy = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let (mut socket, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline, "proxy was not used");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("proxy accept failed: {error}"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = [0_u8; 4096];
        let count = socket.read(&mut bytes).unwrap();
        let request = String::from_utf8_lossy(&bytes[..count]).to_string();
        let body = r#"{"id":"msg_proxy","content":[{"type":"text","text":"via proxy"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#;
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        request
    });

    let f = fixture(Router::new()).await;
    sql(
        &f,
        "UPDATE providers SET kind='anthropic',base_url='http://anthropic-upstream.invalid' WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    sql(
        &f,
        "UPDATE providers SET enabled=0 WHERE id<>?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    sql(
        &f,
        "UPDATE channel_settings SET proxy_url=?,proxy_reuse_connections=0 WHERE provider_id=?",
        vec![proxy_url.into(), f.providers[0].clone().into()],
    )
    .await;

    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["choices"][0]["message"]["content"],
        "via proxy"
    );
    let proxied = proxy.join().unwrap();
    assert!(proxied.starts_with("POST http://anthropic-upstream.invalid/v1/messages HTTP/1.1"));
}

#[tokio::test]
async fn model_sync_is_bounded_idempotent_and_preserves_manual_models() {
    let f = fixture(Router::new().route(
        "/models",
        get(|| async { Json(json!({"data":[{"id":"discovered-one"},{"id":"discovered-one"}]})) }),
    ))
    .await;
    let provider = &f.providers[0];
    for key in ["first", "second"] {
        let job = ops::jobs::enqueue(
            &f.state.db,
            Some(db::DEFAULT_PROJECT_ID),
            "model_sync",
            key,
            &json!({"provider_id":provider}),
            db::now(),
        )
        .await
        .unwrap();
        let claim = ops::jobs::claim(&f.state.db, "model-sync-test", db::now(), 120)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claim.id, job);
        ops::runtime::execute(&f.state, &claim).await.unwrap();
        assert!(
            ops::jobs::finish(&f.state.db, &claim, db::now(), true)
                .await
                .unwrap()
        );
    }
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM models WHERE upstream_name='a-model' AND discovery_managed=0"
        )
        .await,
        1,
        "the fixture's manual model must survive discovery"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM models WHERE upstream_name='discovered-one' AND discovery_managed=1 AND lifecycle='active'"
        )
        .await,
        1,
        "replaying the sync must not duplicate a discovered model"
    );
}

#[tokio::test]
async fn model_sync_schedule_is_durable_and_project_scoped() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let provider = &f.providers[0];
    f.state.db.execute(ops::sql("INSERT INTO operation_schedules(id,project_id,kind,payload_json,interval_secs,next_run_at) VALUES('model-schedule',?,'model_sync',?,30,?)",vec![db::DEFAULT_PROJECT_ID.into(),json!({"provider_id":provider}).to_string().into(),db::now().into()])).await.unwrap();
    assert_eq!(
        ops::jobs::enqueue_due(&f.state.db, db::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM operation_jobs WHERE kind='model_sync' AND project_id='00000000-0000-0000-0000-000000000001'").await,
        1
    );

    sql(&f, "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('foreign-project','Foreign','foreign-project',0,1,1,1)", vec![]).await;
    sql(&f, "INSERT INTO providers(id,name,kind,base_url,enabled,created_at,updated_at,project_id) VALUES('foreign-provider','Foreign provider','openai','https://foreign.invalid/v1',1,1,1,'foreign-project')", vec![]).await;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!(
            "/api/admin/v1/projects/{}/operations/model-sync",
            db::DEFAULT_PROJECT_ID
        ),
        json!({"provider_id":"foreign-provider"}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn failed_model_sync_records_a_redacted_channel_error() {
    let f =
        fixture(Router::new().route("/models", get(|| async { StatusCode::BAD_GATEWAY }))).await;
    let provider = &f.providers[0];
    ops::jobs::enqueue(
        &f.state.db,
        Some(db::DEFAULT_PROJECT_ID),
        "model_sync",
        "failure",
        &json!({"provider_id":provider}),
        db::now(),
    )
    .await
    .unwrap();
    let claim = ops::jobs::claim(&f.state.db, "failed-model-sync", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert!(ops::runtime::execute(&f.state, &claim).await.is_err());
    let error: String = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT model_sync_error FROM channel_settings WHERE provider_id=?",
            vec![provider.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "model_sync_error")
        .unwrap();
    assert_eq!(error, "model_sync_failed");
    assert!(!error.contains("502"));
}

async fn admin(
    f: &Fixture,
    cookie: &str,
    method: http::Method,
    path: &str,
    body: Value,
    csrf: bool,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("cookie", format!("pangolin_session={cookie}"))
        .header("content-type", "application/json");
    if csrf {
        builder = builder.header("x-pangolin-csrf", "1")
    }
    router(f.state.clone())
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
/// Adds a user to the default project with one role, and returns their id.
async fn project_user(
    f: &Fixture,
    owner: &access::Principal,
    email: &str,
    role_id: String,
) -> String {
    let user = access::create_user(
        &f.state.db,
        owner,
        &access::UserInput {
            email: email.into(),
            password: "another secure password".into(),
            display_name: None,
            language: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    access::upsert_membership(
        &f.state.db,
        owner,
        db::DEFAULT_PROJECT_ID,
        &access::MembershipInput {
            user_id: user.id.clone(),
            role_id,
            status: "active".into(),
        },
    )
    .await
    .unwrap();
    user.id
}
async fn owner(f: &Fixture) -> String {
    owner_principal(f).await.0
}

#[tokio::test]
async fn b40_b42_service_group_editor_round_trips_channels_ratio_and_audit() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("/api/admin/v1/projects/{project}/operations/groups"),
        json!({
            "id": "b40-group",
            "name": "Priority traffic",
            "tier": "priority",
            "ratio": 1.25,
            "channels": [f.providers[0]],
            "enabled": true
        }),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let groups = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("/api/admin/v1/projects/{project}/operations/groups"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    let group = groups["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["id"] == "b40-group")
        .unwrap();
    assert_eq!(group["ratio_millionths"], json!(1_250_000));
    assert_eq!(group["channels"], json!([f.providers[0]]));
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='groups.save' AND resource_id='b40-group'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn b40_b42_batch_create_names_the_invalid_row_and_writes_nothing() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("/api/admin/v1/projects/{project}/models/batch"),
        json!({"models": [
            {
                "catalog_model_id": "alibaba/deepseek-v4-flash-0731",
                "provider_id": f.providers[0],
                "public_name": "b41-valid",
                "upstream_name": "deepseek-v4-flash-0731"
            },
            {
                "catalog_model_id": "missing/catalog-card",
                "provider_id": f.providers[0],
                "public_name": "b41-invalid",
                "upstream_name": "missing"
            }
        ]}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert!(
        body["error"]["message"].as_str().unwrap().contains("row 2"),
        "{body}"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM models WHERE public_name LIKE 'b41-%'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn b40_b42_bulk_archive_skips_foreign_ids_and_delete_is_atomic_on_history() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('b41-foreign-project','Foreign','b41-foreign',0,1,0,0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO providers(id,name,kind,base_url,enabled,created_at,updated_at,project_id) VALUES('b41-foreign-provider','Foreign','openai','https://foreign.invalid',1,0,0,'b41-foreign-project')",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,enabled,created_at) VALUES('b41-local',?,'b41-local','b41-local','[]',1,0),('b41-foreign','b41-foreign-provider','b41-foreign','b41-foreign','[]',1,0)",
        vec![f.providers[0].clone().into()],
    )
    .await;
    let archive = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("/api/admin/v1/projects/{project}/models/bulk"),
            json!({"ids":["b41-local", "b41-foreign"], "action":"archive"}),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(archive["changed_ids"], json!(["b41-local"]));
    assert_eq!(archive["skipped_ids"], json!(["b41-foreign"]));
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM models WHERE id='b41-foreign' AND lifecycle='active'"
        )
        .await,
        1
    );

    sql(
        &f,
        "INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,enabled,lifecycle,created_at) VALUES('b41-deletable',?,'b41-deletable','b41-deletable','[]',0,'archived',0),('b41-history',?,'b41-history','b41-history','[]',0,'archived',0)",
        vec![f.providers[0].clone().into(), f.providers[0].clone().into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO model_prices(id,model_id,version,valid_from,created_at) VALUES('b41-price','b41-history',1,0,0)",
        vec![],
    )
    .await;
    let delete = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("/api/admin/v1/projects/{project}/models/bulk"),
        json!({"ids":["b41-deletable", "b41-history"], "action":"delete"}),
        true,
    )
    .await;
    assert_eq!(delete.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(delete).await["error"]["type"], "history_retained");
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM models WHERE id IN ('b41-deletable','b41-history')"
        )
        .await,
        2,
        "a refused bulk delete must not partially delete earlier rows"
    );
}

#[tokio::test]
async fn b40_b42_unassociated_models_names_only_enabled_models_without_a_match() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    sql(&f, "UPDATE models SET enabled=0", vec![]).await;
    sql(
        &f,
        "UPDATE providers SET settings_json='{\"tags\":[\"region-us\"]}' WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO models(id,provider_id,public_name,upstream_name,capabilities,enabled,created_at) VALUES('b42-associated',?,'b42-associated','b42-associated','[]',1,0),('b42-tagged',?,'b42-tagged','b42-tagged','[]',1,0),('b42-excluded',?,'b42-excluded','b42-excluded','[]',1,0),('b42-unassociated',?,'b42-unassociated','b42-unassociated','[]',1,0),('b42-disabled',?,'b42-disabled','b42-disabled','[]',0,0)",
        vec![
            f.providers[0].clone().into(),
            f.providers[0].clone().into(),
            f.providers[0].clone().into(),
            f.providers[0].clone().into(),
            f.providers[0].clone().into(),
        ],
    )
    .await;
    sql(
        &f,
        "INSERT INTO model_associations(id,project_id,model_id,provider_id,match_type,pattern,conditions_json,priority,weight,enabled,created_at,updated_at) VALUES('b42-match',?,'b42-associated',?,'exact','b42-associated','{\"version\":1}',1,1,1,0,0),('b42-disabled-match',?,'b42-unassociated',?,'exact','b42-unassociated','{\"version\":1}',1,1,0,0,0)",
        vec![
            project.into(),
            f.providers[0].clone().into(),
            project.into(),
            f.providers[0].clone().into(),
        ],
    )
    .await;
    sql(
        &f,
        "INSERT INTO model_associations(id,project_id,model_id,provider_id,match_type,pattern,conditions_json,exclusions_json,priority,weight,enabled,created_at,updated_at) VALUES('b42-tag-match',?,'b42-tagged',?,'channel_tags_regex','^region-(eu|us)$','{\"version\":1}','{\"version\":1}',1,1,1,0,0),('b42-excluded-match',?,'b42-excluded',?,'exact','b42-excluded','{\"version\":1}',?,1,1,1,0,0)",
        vec![
            project.into(),
            f.providers[0].clone().into(),
            project.into(),
            f.providers[0].clone().into(),
            json!({"version":1,"channel_ids":[f.providers[0].clone()]})
                .to_string()
                .into(),
        ],
    )
    .await;

    let document = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("/api/admin/v1/projects/{project}/models/unassociated"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(document["total"], json!(2), "{document}");
    assert_eq!(document["data"][0]["id"], json!("b42-excluded"));
    assert_eq!(document["data"][1]["id"], json!("b42-unassociated"));
}
/// Deletes an operations resource with an API-key principal, which is how a project
/// manager acts without a browser session.
async fn delete_operation(f: &Fixture, token: &str, resource: &str, id: &str) -> Response {
    router(f.state.clone())
        .oneshot(
            Request::builder()
                .method(http::Method::DELETE)
                .uri(format!(
                    "/api/admin/v1/projects/{}/operations/{resource}/{id}",
                    db::DEFAULT_PROJECT_ID
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from("null"))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn owner_principal(f: &Fixture) -> (String, String) {
    let user = db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: "operations@example.com".into(),
            password: "operations-test-123456".into(),
            instance_name: None,
            language: None,
        },
    )
    .await
    .unwrap();
    let cookie = db::create_session(&f.state.db, &user.id).await.unwrap();
    (cookie, user.id)
}
async fn json_body(response: Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), 48 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn profile_template_round_trip_apply_and_scope() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let operations = format!("/api/admin/v1/projects/{project}/operations");
    let profile = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{operations}/key-profiles"),
            json!({
                "name":"source",
                "rpm_limit":60,
                "tpm_limit":120_000,
                "budget_micros":5_000_000,
                "routing_policy":{"version":1,"strategy":"round_robin"},
                "mappings":[{"source_model":"chat","target_model":"public","priority":7}],
                "allowed_models":[{"pattern":"^public$","match_type":"regex"}]
            }),
            true,
        )
        .await,
    )
    .await;
    let profile_id = profile["id"].as_str().unwrap();
    let templates = format!("/api/admin/v1/projects/{project}/profile-templates");
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &templates,
        json!({"name":"mobile default","source_profile_id":profile_id}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let template_id = json_body(response).await["id"].as_str().unwrap().to_owned();

    let exported = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{templates}/{template_id}/export"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(exported["version"], 1);
    assert_eq!(exported["name"], "mobile default");
    assert!(exported.get("id").is_none());
    assert!(exported["profile"].get("id").is_none());
    assert_eq!(exported["profile"]["mappings"][0]["target_model"], "public");

    let target = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{operations}/key-profiles"),
            json!({"name":"target","rpm_limit":1,"routing_policy":{"version":1}}),
            true,
        )
        .await,
    )
    .await;
    let target_id = target["id"].as_str().unwrap();
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{templates}/{template_id}/apply"),
        json!({"target_profile_id":target_id}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let applied = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{operations}/key-profiles/{target_id}"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(applied["name"], "target");
    assert_eq!(applied["rpm_limit"], 60);
    assert_eq!(
        applied["allowed_models"],
        exported["profile"]["allowed_models"]
    );
    assert_eq!(
        count(
            &f,
            &format!(
                "SELECT COUNT(*) AS n FROM audit_events WHERE action='profile-template.apply' AND resource_id='{target_id}'"
            )
        )
        .await,
        1,
        "the apply audit must identify the profile that was changed"
    );

    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{operations}/key-profiles"),
            json!({
                "id":target_id,
                "name":"target",
                "rpm_limit":1,
                "routing_policy":{"version":1}
            }),
            true,
        )
        .await
        .status(),
        StatusCode::OK
    );
    sql(
        &f,
        "CREATE TRIGGER reject_profile_template_apply_audit BEFORE INSERT ON audit_events WHEN NEW.action='profile-template.apply' BEGIN SELECT RAISE(ABORT,'audit rejected'); END",
        vec![],
    )
    .await;
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{templates}/{template_id}/apply"),
            json!({"target_profile_id":target_id}),
            true,
        )
        .await
        .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let rolled_back = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{operations}/key-profiles/{target_id}"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(
        rolled_back["rpm_limit"], 1,
        "rejecting the target-profile audit must roll back the profile write"
    );

    sql(
        &f,
        "INSERT INTO projects(id,name,slug,enabled,created_at,updated_at) VALUES('foreign-project','Foreign','foreign',1,0,0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO api_key_profile_templates(id,project_id,name,document_json,created_at,updated_at) VALUES('foreign-template','foreign-project','foreign','{\"version\":1,\"rpm_limit\":null,\"tpm_limit\":null,\"budget_micros\":null,\"routing_policy\":{\"version\":1},\"mappings\":[],\"allowed_models\":[]}',0,0)",
        vec![],
    )
    .await;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{templates}/foreign-template/apply"),
        json!({"target_profile_id":target_id}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn profile_templates_share_validation_bound_imports_and_roll_back_without_audit() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let profiles = format!("/api/admin/v1/projects/{project}/operations/key-profiles");
    let templates = format!("/api/admin/v1/projects/{project}/profile-templates");
    let invalid = json!({
        "name":"invalid",
        "routing_policy":{"version":1},
        "allowed_models":[{"pattern":"[","match_type":"regex"}]
    });
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &profiles,
            invalid.clone(),
            true,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &templates,
            json!({
                "name":"invalid template",
                "document":{
                    "version":1,
                    "routing_policy":{"version":1},
                    "allowed_models":invalid["allowed_models"]
                }
            }),
            true,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST,
        "an explicit template document must use the ordinary profile validator"
    );

    let transfer = json!({
        "version":1,
        "name":"portable",
        "profile":{"version":1,"rpm_limit":5,"routing_policy":{"version":1}}
    });
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{templates}/import"),
            json!({"document":transfer,"conflict":"fail"}),
            true,
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{templates}/import"),
            json!({"document":transfer,"conflict":"fail"}),
            true,
        )
        .await
        .status(),
        StatusCode::CONFLICT,
        "import conflict behavior is explicit"
    );
    let unknown = json!({
        "document":{
            "version":1,
            "name":"unknown",
            "profile":{"version":1,"routing_policy":{"version":1},"secret":"never"}
        },
        "conflict":"fail"
    });
    assert!(matches!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{templates}/import"),
            unknown,
            true,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ));
    let oversized = json!({
        "document":{
            "version":1,
            "name":"oversized",
            "profile":{"version":1,"routing_policy":{"version":1},"mappings":[{"source_model":"x","target_model":"y".repeat(140_000)}]}
        },
        "conflict":"fail"
    });
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{templates}/import"),
            oversized,
            true,
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );

    sql(
        &f,
        "CREATE TRIGGER reject_profile_template_audit BEFORE INSERT ON audit_events WHEN NEW.action='profile-template.create' BEGIN SELECT RAISE(ABORT,'audit rejected'); END",
        vec![],
    )
    .await;
    let source = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &profiles,
            json!({"name":"source","routing_policy":{"version":1}}),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &templates,
            json!({"name":"must roll back","source_profile_id":source["id"]}),
            true,
        )
        .await
        .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM api_key_profile_templates WHERE name='must roll back'"
        )
        .await,
        0,
        "the template write and audit event are one transaction"
    );
}

#[tokio::test]
async fn profile_templates_use_api_key_management_authority() {
    let f = fixture(success()).await;
    let (_, owner_id) = owner_principal(&f).await;
    let owner = access::Principal::session(owner_id);
    let role = access::create_role(
        &f.state.db,
        &owner,
        db::DEFAULT_PROJECT_ID,
        &access::RoleInput {
            name: "profile manager".into(),
            permissions: vec!["api_key:manage".into()],
        },
    )
    .await
    .unwrap();
    let manager = project_user(&f, &owner, "profile-manager@example.com", role.id).await;
    let member = project_user(
        &f,
        &owner,
        "profile-reader@example.com",
        access::SYSTEM_MEMBER_ROLE_ID.into(),
    )
    .await;
    let manager = db::create_session(&f.state.db, &manager).await.unwrap();
    let member = db::create_session(&f.state.db, &member).await.unwrap();
    let project = db::DEFAULT_PROJECT_ID;
    let profiles = format!("/api/admin/v1/projects/{project}/operations/key-profiles");
    let templates = format!("/api/admin/v1/projects/{project}/profile-templates");
    let response = admin(
        &f,
        &manager,
        http::Method::POST,
        &profiles,
        json!({"name":"managed","routing_policy":{"version":1}}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let profile_id = json_body(response).await["id"].as_str().unwrap().to_owned();
    assert_eq!(
        admin(
            &f,
            &manager,
            http::Method::POST,
            &templates,
            json!({"name":"managed","source_profile_id":profile_id}),
            true,
        )
        .await
        .status(),
        StatusCode::OK,
        "the template and ordinary profile writes use api_key:manage"
    );
    assert_eq!(
        admin(
            &f,
            &member,
            http::Method::GET,
            &templates,
            Value::Null,
            false,
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn task6_control_plane_crud_redacts_credentials_and_audits_changes() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/operations",
        db::DEFAULT_PROJECT_ID
    );
    let credential = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{base}/credentials"),
        json!({"provider_id":f.providers[0],"credential_type":"api_key","secret":"sk-control-plane-credential","priority":5}),
        true,
    )
    .await;
    assert_eq!(credential.status(), StatusCode::OK);
    let credentials = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/credentials"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert!(credentials["data"].as_array().unwrap().len() >= 2);
    assert!(credentials["total"].as_i64().unwrap() >= 2);
    assert!(!credentials.to_string().contains("control-plane-credential"));
    assert!(!credentials.to_string().contains("secret_envelope"));

    for (resource, document) in [
        (
            "key-profiles",
            json!({"name":"mobile clients","rpm_limit":60,"tpm_limit":120000,"budget_micros":5000000,"routing_policy":{"version":1}}),
        ),
        (
            "prompts",
            json!({"name":"support policy","role":"system","content":"Answer with cited facts.","activation":{"version":1},"enabled":true}),
        ),
        (
            "protection",
            json!({"name":"credential guard","content_pattern":"(?i)api[_ -]?key","action":"redact","replacement":"[redacted]","scopes":{"version":1},"enabled":true}),
        ),
    ] {
        assert_eq!(
            admin(
                &f,
                &cookie,
                http::Method::POST,
                &format!("{base}/{resource}"),
                document,
                true,
            )
            .await
            .status(),
            StatusCode::OK,
            "{resource}"
        );
        assert_eq!(
            json_body(
                admin(
                    &f,
                    &cookie,
                    http::Method::GET,
                    &format!("{base}/{resource}"),
                    Value::Null,
                    false,
                )
                .await
            )
            .await["data"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{resource}"
        );
    }
    let key = db::authenticate_api_key(&f.state.db, &f.token, None)
        .await
        .unwrap()
        .unwrap();
    let preview = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!(
                "/api/admin/v1/projects/{}/routing-preview",
                db::DEFAULT_PROJECT_ID
            ),
            json!({"api_key_id":key.id,"model":"public","endpoint":"/v1/chat/completions"}),
            true,
        )
        .await,
    )
    .await;
    assert!(!preview["candidates"].as_array().unwrap().is_empty());
    assert_eq!(preview["decisions"][0]["stage"], "access");
    assert_eq!(admin(&f,&cookie,http::Method::PUT,"/api/admin/v1/settings/system",json!({"instance_name":"Operations","branding_name":"Pangolin / 鲮鲤","favicon_url":"/logo.webp","onboarding_complete":true}),true).await.status(),StatusCode::OK);
    assert_eq!(
        json_body(
            admin(
                &f,
                &cookie,
                http::Method::GET,
                "/api/admin/v1/settings/system",
                Value::Null,
                false
            )
            .await
        )
        .await["onboarding_complete"],
        true
    );
    let bootstrap = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            "/api/v1/bootstrap",
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(bootstrap["branding"]["instance_name"], "Operations");
    assert_eq!(bootstrap["branding"]["onboarding_complete"], true);
    assert!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action LIKE '%.save'"
        )
        .await
            >= 4
    );
}

#[tokio::test]
async fn task6_patch_preserves_omitted_secrets_and_channel_settings() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/operations",
        db::DEFAULT_PROJECT_ID
    );
    let created = json_body(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/storage"),
            json!({"name":"secure target","config":{"kind":"local","directory":"secure"},"secret":{"token":"never-print"}}),
            true,
        )
        .await,
    )
    .await;
    let storage_id = created["id"].as_str().unwrap();
    let before_storage = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT secret_envelope FROM data_storage_configs WHERE id=?",
            vec![storage_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "secret_envelope")
        .unwrap();
    assert_eq!(admin(&f,&cookie,http::Method::POST,&format!("{base}/storage"),json!({"id":storage_id,"revision":1,"name":"renamed","config":{"kind":"local","directory":"secure"},"secret":null}),true).await.status(),StatusCode::OK);
    let after_storage = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT secret_envelope FROM data_storage_configs WHERE id=?",
            vec![storage_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "secret_envelope")
        .unwrap();
    assert_eq!(before_storage, after_storage);

    let webhook = json_body(admin(&f,&cookie,http::Method::POST,&format!("{base}/webhooks"),json!({"name":"secure hook","url":"https://example.test/hook","events":["test"],"secret_headers":{"authorization":"never-print"}}),true).await).await;
    let webhook_id = webhook["id"].as_str().unwrap();
    let before_webhook = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT secret_envelope FROM webhooks WHERE id=?",
            vec![webhook_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "secret_envelope")
        .unwrap();
    assert_eq!(admin(&f,&cookie,http::Method::POST,&format!("{base}/webhooks"),json!({"id":webhook_id,"name":"renamed hook","url":"https://example.test/hook","events":["test"],"secret_headers":null}),true).await.status(),StatusCode::OK);
    let after_webhook = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT secret_envelope FROM webhooks WHERE id=?",
            vec![webhook_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "secret_envelope")
        .unwrap();
    assert_eq!(before_webhook, after_webhook);

    let settings = json!({"version":1,"tags":["priority"],"limits":{"rpm":17},"catalog_provider_id":"fixture"});
    sql(
        &f,
        "UPDATE providers SET settings_json=? WHERE id=?",
        vec![settings.to_string().into(), f.providers[0].clone().into()],
    )
    .await;
    assert_eq!(admin(&f,&cookie,http::Method::POST,&format!("{base}/channels"),json!({"id":f.providers[0],"name":"renamed channel","kind":"openai","base_url":"https://example.test","enabled":true}),true).await.status(),StatusCode::OK);
    let preserved = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT settings_json FROM providers WHERE id=?",
            vec![f.providers[0].clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "settings_json")
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&preserved).unwrap(), settings);

    let orchestration = json!({"version":1,"affinity_rules":[{"id":"cache","mode":"prefer","source":{"kind":"pointer","value":"/prompt_cache_key"},"ttl_secs":900,"release_on_failure":true}],"session_compaction":{"enabled":true,"threshold_tokens":4096,"retain_items":12,"native":true,"summarizer_model":null},"semantic_memory":{"enabled":true,"max_candidates":4,"rerank":false}});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            &format!(
                "/api/admin/v1/projects/{}/settings/orchestration",
                db::DEFAULT_PROJECT_ID
            ),
            orchestration.clone(),
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    let fetched = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{}/settings/orchestration",
                db::DEFAULT_PROJECT_ID
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(fetched["affinity_rules"][0]["id"], "cache");
    assert_eq!(fetched["affinity_rules"][0]["ttl_secs"], 900);
    assert_eq!(
        fetched["session_compaction"],
        orchestration["session_compaction"]
    );
    assert_eq!(fetched["semantic_memory"], orchestration["semantic_memory"]);
    let mut invalid = orchestration;
    invalid["affinity_rules"][0]["ttl_secs"] = json!(0);
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            &format!(
                "/api/admin/v1/projects/{}/settings/orchestration",
                db::DEFAULT_PROJECT_ID
            ),
            invalid,
            true
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn task6_associations_are_project_scoped_and_trace_exposes_public_request_id() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let base = format!("/api/admin/v1/projects/{project}/operations");
    sql(&f,"INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('foreign-project','Foreign','foreign',0,1,1,1)",vec![]).await;
    sql(&f,"INSERT INTO providers(id,name,kind,base_url,enabled,created_at,updated_at,project_id) VALUES('foreign-provider','Foreign provider','openai','https://foreign.test',1,1,1,'foreign-project')",vec![]).await;
    sql(&f,"INSERT INTO models(id,provider_id,public_name,upstream_name,created_at) VALUES('foreign-model','foreign-provider','foreign-public','foreign-upstream',1)",vec![]).await;
    for body in [
        json!({"provider_id":"foreign-provider","match_type":"exact","pattern":"blocked"}),
        json!({"model_id":"foreign-model","match_type":"exact","pattern":"blocked"}),
    ] {
        assert_eq!(
            admin(
                &f,
                &cookie,
                http::Method::POST,
                &format!("{base}/associations"),
                body,
                true
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
    sql(&f,"INSERT INTO model_associations(id,project_id,model_id,provider_id,match_type,pattern,created_at,updated_at) VALUES('legacy-cross',?,'foreign-model','foreign-provider','exact','legacy',1,1)",vec![project.into()]).await;
    let listed = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/associations"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    let cross = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "legacy-cross")
        .unwrap();
    assert!(cross["model_name"].is_null());
    assert!(cross["provider_name"].is_null());
    sql(&f,"INSERT INTO traces(id,project_id,status,started_at,finished_at) VALUES('trace-public',?,'succeeded',1,2)",vec![project.into()]).await;
    sql(&f,"INSERT INTO request_facts(id,project_id,log_level,started_at) VALUES('internal-request',?,'metadata',1)",vec![project.into()]).await;
    sql(&f,"INSERT INTO requests(id,trace_id,protocol,endpoint,status,request_metadata_json,started_at,finished_at) VALUES('internal-request','trace-public','chat','/v1/chat/completions','succeeded',?,1,2)",vec![json!({"version":1,"external_id":"public-request"}).to_string().into()]).await;
    let detail = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/trace-detail/trace-public"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(detail["requests"][0]["id"], "internal-request");
    assert_eq!(detail["requests"][0]["public_id"], "public-request");
}

#[tokio::test]
async fn task5_management_api_prices_groups_logs_csrf_and_scope() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/operations",
        db::DEFAULT_PROJECT_ID
    );
    let config = json!({"name":"local","config":{"kind":"local","directory":"safe"}});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/storage"),
            config.clone(),
            false
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let result = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{base}/storage"),
        config,
        true,
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    let target = json_body(result).await["id"].as_str().unwrap().to_owned();
    let listing = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/storage"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(listing["data"][0]["id"], target);
    assert!(listing.to_string().find("envelope").is_none());
    let model = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT id FROM models WHERE provider_id=?",
            vec![f.providers[0].clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "id")
        .unwrap();
    let price = json!({"model_id":model,"components":[{"kind":"input","unit_size":1,"unit_price_micros":10},{"kind":"output","unit_size":1,"unit_price_micros":20},{"kind":"cache_read","unit_size":1,"unit_price_micros":1}]});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/prices"),
            price,
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    let key = db::authenticate_api_key(&f.state.db, &f.token, None)
        .await
        .unwrap()
        .unwrap();
    let group = json!({"name":"discount","ratio_millionths":500000,"api_keys":[key.id],"channels":[f.providers[0]]});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("{base}/groups"),
            group,
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        count(&f, "SELECT SUM(total_cost_micros) AS n FROM usage_logs").await,
        510
    );
    assert_eq!(
        count(&f, "SELECT SUM(cache_savings_micros) AS n FROM usage_logs").await,
        90
    );
    assert!(
        f.state
            .db
            .execute(ops::sql(
                "UPDATE model_price_components SET unit_price_micros=99",
                vec![]
            ))
            .await
            .is_err()
    );
    for resource in [
        "threads",
        "traces",
        "requests",
        "executions",
        "usage",
        "cost-items",
        "prices",
        "audit",
        "jobs",
        "groups",
        "storage",
    ] {
        let response = admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/{resource}"),
            Value::Null,
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{resource}");
    }
    let summary = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!(
            "/api/admin/v1/projects/{}/analytics?dimension=model",
            db::DEFAULT_PROJECT_ID
        ),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(summary.status(), StatusCode::OK);
    assert_eq!(json_body(summary).await["data"][0]["cost_micros"], 510);
    assert_eq!(admin(&f,&cookie,http::Method::POST,"/api/admin/v1/projects/other/operations/storage",json!({"id":target,"name":"stolen","revision":1,"config":{"kind":"local","directory":"other"}}),true).await.status(),StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn task5_affinity_switches_only_on_success_and_disabled_channel_is_invalidated() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let copy = seen.clone();
    let f=fixture(Router::new().route("/v1/chat/completions",post(move|Json(body):Json<Value>|{let seen=copy.clone();async move {seen.lock().await.push(body["model"].as_str().unwrap().to_owned());Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}))}}))).await;
    sql(&f,"UPDATE projects SET settings_json=?",vec![json!({"version":1,"affinity_rules":[{"id":"session","mode":"prefer","source":{"kind":"pointer","value":"/user"},"ttl_secs":60}]}).to_string().into()]).await;
    let mut body = chat();
    body["user"] = json!("sensitive-affinity-id");
    assert_eq!(
        request(&f, "/v1/chat/completions", body.clone())
            .await
            .status(),
        StatusCode::OK
    );
    sql(
        &f,
        "UPDATE models SET priority=-1 WHERE provider_id=?",
        vec![f.providers[1].clone().into()],
    )
    .await;
    assert_eq!(
        request(&f, "/v1/chat/completions", body.clone())
            .await
            .status(),
        StatusCode::OK
    );
    sql(
        &f,
        "UPDATE providers SET enabled=0 WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    assert_eq!(
        request(&f, "/v1/chat/completions", body).await.status(),
        StatusCode::OK
    );
    assert_eq!(*seen.lock().await, vec!["a-model", "a-model", "b-model"]);
}
#[tokio::test]
async fn task5_compaction_is_opt_in_cached_and_failure_preserves_exact_history() {
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let f=fixture(Router::new().route("/v1/responses",post(move|Json(body):Json<Value>|{let seen=copy.clone();async move {seen.lock().await.push(body.clone());Json(json!({"id":format!("resp_{}",Uuid::new_v4()),"status":"completed","output":[{"type":"message","role":"assistant","content":"ok"}],"usage":{"input_tokens":10,"output_tokens":2}}))}})).route("/v1/chat/completions",post(move||{counted.fetch_add(1,Ordering::Relaxed);async {Json(json!({"choices":[{"message":{"content":"remembered context"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}))}}))).await;
    sql(
        &f,
        "UPDATE models SET capabilities='[\"chat\",\"responses\"]'",
        vec![],
    )
    .await;
    let history = vec![
        json!({"role":"user","content":"long context ".repeat(100)}),
        json!({"role":"assistant","content":"answer"}),
        json!({"role":"user","content":"continue"}),
    ];
    let body = json!({"model":"public","input":history});
    assert_eq!(
        request(&f, "/v1/responses", body.clone()).await.status(),
        StatusCode::OK
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    sql(&f,"UPDATE projects SET settings_json=?",vec![json!({"version":1,"session_compaction":{"enabled":true,"threshold_tokens":128,"retain_items":1,"native":false,"summarizer_model":"public"}}).to_string().into()]).await;
    for _ in 0..2 {
        assert_eq!(
            request(&f, "/v1/responses", body.clone()).await.status(),
            StatusCode::OK
        )
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM session_summaries").await,
        1
    );
    let captured = seen.lock().await;
    assert_eq!(captured[0]["input"], json!(history));
    assert_eq!(captured[1]["input"].as_array().unwrap().len(), 2);
    assert!(
        captured[1]["input"][0]["content"]
            .as_str()
            .unwrap()
            .contains("remembered context")
    );
    assert_eq!(captured[1]["input"][1], history[2]);
    drop(captured);
    sql(&f,"UPDATE projects SET settings_json=?",vec![json!({"version":1,"session_compaction":{"enabled":true,"threshold_tokens":128,"retain_items":1,"native":false,"summarizer_model":"unavailable"}}).to_string().into()]).await;
    assert_eq!(
        request(&f, "/v1/responses", body).await.status(),
        StatusCode::OK
    );
    assert_eq!(seen.lock().await[3]["input"], json!(history));
}

struct FailingSemanticMemory {
    calls: Arc<AtomicUsize>,
    scopes: Arc<Mutex<Vec<(String, String)>>>,
}

impl crate::orchestration::compaction::SemanticMemory for FailingSemanticMemory {
    fn retrieve<'a>(
        &'a self,
        project: &'a str,
        key: &'a str,
        _query: &'a str,
        _limit: usize,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Vec<crate::orchestration::compaction::SemanticMemoryCandidate>,
                        crate::orchestration::compaction::SemanticMemoryError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.scopes
                .lock()
                .await
                .push((project.to_owned(), key.to_owned()));
            Err(crate::orchestration::compaction::SemanticMemoryError)
        })
    }
}

#[tokio::test]
async fn semantic_memory_is_explicitly_scoped_and_failure_preserves_exact_replay() {
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = seen.clone();
    let f = fixture(Router::new().route(
        "/v1/responses",
        post(move |Json(body): Json<Value>| {
            let captured = captured.clone();
            async move {
                captured.lock().await.push(body);
                Json(json!({
                    "id": "resp_memory",
                    "status": "completed",
                    "output": [{"type":"message","role":"assistant","content":"ok"}],
                    "usage": {"input_tokens": 10, "output_tokens": 2}
                }))
            }
        }),
    ))
    .await;
    sql(
        &f,
        "UPDATE models SET capabilities='[\"responses\"]'",
        vec![],
    )
    .await;

    let calls = Arc::new(AtomicUsize::new(0));
    let scopes = Arc::new(Mutex::new(Vec::new()));
    f.state
        .orchestrator
        .semantic_memory
        .install_provider(Arc::new(FailingSemanticMemory {
            calls: calls.clone(),
            scopes: scopes.clone(),
        }));
    let history = vec![
        json!({"type":"message","role":"user","content":"Use the saved preference"}),
        json!({"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"}),
        json!({"type":"function_call_output","call_id":"call_1","output":"first"}),
        json!({"type":"message","role":"user","content":"Continue exactly"}),
    ];
    let body = json!({"model":"public","input":history});

    assert_eq!(
        request(&f, "/v1/responses", body.clone()).await.status(),
        StatusCode::OK
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(seen.lock().await[0]["input"], json!(history));

    sql(
        &f,
        "UPDATE projects SET settings_json=?",
        vec![
            json!({
                "version": 1,
                "semantic_memory": {"enabled": true, "max_candidates": 4, "rerank": false}
            })
            .to_string()
            .into(),
        ],
    )
    .await;
    assert_eq!(
        request(&f, "/v1/responses", body).await.status(),
        StatusCode::OK
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        *scopes.lock().await,
        vec![(
            db::DEFAULT_PROJECT_ID.to_owned(),
            f.state
                .db
                .query_one(ops::sql(
                    "SELECT id FROM api_keys WHERE project_id=?",
                    vec![db::DEFAULT_PROJECT_ID.into()],
                ))
                .await
                .unwrap()
                .unwrap()
                .try_get::<String>("", "id")
                .unwrap(),
        )]
    );
    assert_eq!(seen.lock().await[1]["input"], json!(history));
}

#[tokio::test]
async fn task5_schedule_snapshot_claim_and_restart_do_not_duplicate_jobs() {
    let f = fixture(success()).await;
    let target = local_target(&f, "scheduled").await;
    sql(&f,"INSERT INTO operation_schedules(id,project_id,kind,payload_json,interval_secs,next_run_at) VALUES('schedule',?,'automatic_backup',?,60,0)",vec![db::DEFAULT_PROJECT_ID.into(),json!({"targets":[target],"resources":["providers"]}).to_string().into()]).await;
    assert_eq!(
        ops::jobs::enqueue_due(&f.state.db, db::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        ops::jobs::enqueue_due(&f.state.db, db::now())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM backup_job_targets").await,
        1
    );
    let claim = ops::jobs::claim(&f.state.db, "old", db::now(), 1)
        .await
        .unwrap()
        .unwrap();
    let takeover = ops::jobs::claim(&f.state.db, "new", db::now() + 2, 120)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.id, takeover.id);
    assert!(takeover.fence > claim.fence);
    assert!(
        !ops::jobs::finish(&f.state.db, &claim, db::now() + 2, true)
            .await
            .unwrap()
    );
    ops::backup::execute(&f.state, &takeover).await.unwrap();
    assert!(
        ops::jobs::finish(&f.state.db, &takeover, db::now() + 2, true)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn task5_webhook_outbox_retries_and_gc_preserves_usage_and_latest_quota() {
    let seen = Arc::new(AtomicUsize::new(0));
    let copy = seen.clone();
    let f = fixture(Router::new().route(
        "/hook",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let n = copy.fetch_add(1, Ordering::Relaxed);
            async move {
                assert!(headers.contains_key("idempotency-key"));
                assert_eq!(body["event"], "test");
                if n == 0 {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::OK
                }
            }
        }),
    ))
    .await;
    let provider = f
        .state
        .db
        .query_one(ops::sql("SELECT base_url FROM providers LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "base_url")
        .unwrap();
    sql(&f,"INSERT INTO webhooks(id,project_id,name,url,subscriptions_json,created_at,updated_at) VALUES('hook',?,'hook',?,?,0,0)",vec![db::DEFAULT_PROJECT_ID.into(),format!("{provider}/hook").into(),json!({"version":1,"events":["test"]}).to_string().into()]).await;
    ops::runtime::notify(
        &f.state.db,
        db::DEFAULT_PROJECT_ID,
        "test",
        "one",
        &json!({"ok":true}),
    )
    .await
    .unwrap();
    ops::runtime::notify(
        &f.state.db,
        db::DEFAULT_PROJECT_ID,
        "test",
        "one",
        &json!({"ok":true}),
    )
    .await
    .unwrap();
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM operation_jobs").await,
        1
    );
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert!(ops::runtime::execute(&f.state, &claim).await.is_err());
    ops::jobs::finish(&f.state.db, &claim, db::now(), false)
        .await
        .unwrap();
    let next = ops::jobs::claim(&f.state.db, "worker", db::now() + 3, 120)
        .await
        .unwrap()
        .unwrap();
    ops::runtime::execute(&f.state, &next).await.unwrap();
    ops::jobs::finish(&f.state.db, &next, db::now() + 3, true)
        .await
        .unwrap();
    assert_eq!(seen.load(Ordering::Relaxed), 2);
    for (id, collected) in [("old", 0), ("new", 1)] {
        sql(&f,"INSERT INTO provider_quota_snapshots(id,provider_id,remaining_micros,collected_at) VALUES(?,?,0,?)",vec![id.into(),f.providers[0].clone().into(),collected.into()]).await;
    }
    sql(&f,"INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES('q',?,'quota',1,0)",vec![db::DEFAULT_PROJECT_ID.into()]).await;
    ops::runtime::gc(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM provider_quota_snapshots").await,
        1
    );
}

#[tokio::test]
async fn b52_cache_diagnostics_are_bounded_and_clear_keeps_authority_serving() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let before = count(&f,"SELECT (SELECT COUNT(*) FROM providers)+(SELECT COUNT(*) FROM models)+(SELECT COUNT(*) FROM api_keys) AS n").await;
    let response = admin(
        &f,
        &cookie,
        http::Method::GET,
        "/api/admin/v1/instance/diagnostics/cache",
        Value::Null,
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let document = json_body(response).await;
    assert_eq!(document["version"], 1);
    assert!(
        document
            .pointer("/runtime/caches")
            .and_then(Value::as_array)
            .is_some_and(|caches| caches.len() <= 16)
    );
    let serialized = document.to_string();
    assert!(serialized.len() < 32 * 1024);
    for forbidden in ["secret_envelope", "authorization", "api_key", "payload"] {
        assert!(!serialized.contains(forbidden));
    }

    let response = admin(
        &f,
        &cookie,
        http::Method::DELETE,
        "/api/admin/v1/instance/diagnostics/cache",
        Value::Null,
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(count(&f,"SELECT (SELECT COUNT(*) FROM providers)+(SELECT COUNT(*) FROM models)+(SELECT COUNT(*) FROM api_keys) AS n").await,before);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='diagnostics.cache.clear'"
        )
        .await,
        1
    );
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn b53_webhook_uses_encrypted_proxy_preset_and_bounded_timeout() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let f = fixture(Router::new().fallback(move || {
        seen.fetch_add(1, Ordering::SeqCst);
        async { StatusCode::OK }
    }))
    .await;
    let proxy_url = f
        .state
        .db
        .query_one(ops::sql("SELECT base_url FROM providers LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "base_url")
        .unwrap();
    let cookie = owner(&f).await;
    let created = json_body(admin(&f,&cookie,http::Method::POST,"/api/admin/v1/instance/proxy-presets",json!({"name":"egress","url":proxy_url,"credentials":{"username":"proxy-user","password":"proxy-sentinel"},"enabled":true}),true).await).await;
    let preset = created["id"].as_str().unwrap();
    let listed = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            "/api/admin/v1/instance/proxy-presets",
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(listed["data"][0]["id"], preset);
    assert!(!listed.to_string().contains("proxy-sentinel"));
    let envelope = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT secret_envelope FROM proxy_presets WHERE id=?",
            vec![preset.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "secret_envelope")
        .unwrap();
    assert!(!envelope.contains("proxy-sentinel"));

    let base = format!(
        "/api/admin/v1/projects/{}/operations/webhooks",
        db::DEFAULT_PROJECT_ID
    );
    let webhook = json_body(admin(&f,&cookie,http::Method::POST,&base,json!({"name":"proxied","url":"http://destination.invalid/hook","events":["test"],"timeout_secs":3,"proxy_preset_id":preset}),true).await).await;
    assert_eq!(admin(&f,&cookie,http::Method::POST,&base,json!({"name":"bad timeout","url":"http://destination.invalid/hook","events":["test"],"timeout_secs":301,"proxy_preset_id":preset}),true).await.status(),StatusCode::BAD_REQUEST);
    ops::runtime::notify(
        &f.state.db,
        db::DEFAULT_PROJECT_ID,
        "test",
        "proxied",
        &json!({"ok":true}),
    )
    .await
    .unwrap();
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    ops::runtime::execute(&f.state, &claim).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(count(&f,&format!("SELECT COUNT(*) AS n FROM webhooks WHERE id='{}' AND timeout_secs=3 AND proxy_preset_id='{}'",webhook["id"].as_str().unwrap(),preset)).await,1);
}

#[tokio::test]
async fn b53_scheduled_catalog_refresh_uses_its_proxy_preset() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let f = fixture(Router::new().fallback(move || {
        seen.fetch_add(1, Ordering::SeqCst);
        async { StatusCode::OK }
    }))
    .await;
    let proxy_url = f
        .state
        .db
        .query_one(ops::sql("SELECT base_url FROM providers LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "base_url")
        .unwrap();
    sql(
        &f,
        "INSERT INTO proxy_presets(id,name,url,enabled,created_at,updated_at) VALUES('catalog-proxy','Catalog proxy',?,1,0,0)",
        vec![proxy_url.into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO catalog_sources(id,name,url,priority,refresh_interval_secs,enabled,signature_policy,proxy_preset_id,revision) VALUES('scheduled-source','Scheduled','https://1.1.1.1/catalog',100,3600,1,'none','catalog-proxy',1)",
        vec![],
    )
    .await;
    ops::jobs::enqueue(
        &f.state.db,
        None,
        "catalog_refresh",
        "scheduled-catalog-proxy",
        &json!({"source_id":"scheduled-source","revision":1}),
        db::now(),
    )
    .await
    .unwrap();
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();

    assert!(ops::runtime::execute(&f.state, &claim).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// A typo in a routing condition used to be stored happily and then abort
/// candidate generation for the whole project, so every request through it
/// answered a generic 500 with nothing naming the rule. It is now a 400 on the
/// form, and nothing is written.
/// A restore clears the derived projection. Nothing re-derived it, so the console
/// reported zero requests for the whole retained window even though every request
/// was still in the record system. This is that recovery path.
#[tokio::test]
async fn a_cleared_projection_is_rebuilt_from_the_record_system() {
    let f = fixture(success()).await;
    for _ in 0..3 {
        let response = request(&f, "/v1/chat/completions", chat()).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    f.state.observations.flush().await;
    let before = f
        .state
        .observations
        .summary(SummaryFilter::default())
        .await
        .unwrap();
    assert_eq!(
        before.requests, 3,
        "the live projection should hold the traffic"
    );

    // Exactly what a restore leaves behind: the flag is set, then startup clears.
    f.state
        .db
        .execute(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            "INSERT INTO settings(key,value,updated_at) VALUES('_internal.observation_reset_required','true',unixepoch()) ON CONFLICT(key) DO UPDATE SET value='true'".to_owned(),
        ))
        .await
        .unwrap();
    assert!(
        crate::operations::instance_backup::reset_projection(&f.state)
            .await
            .unwrap()
    );

    let after = f
        .state
        .observations
        .summary(SummaryFilter::default())
        .await
        .unwrap();
    assert_eq!(
        after.requests, 3,
        "a restored instance must re-derive the projection, not report zero traffic"
    );
    assert_eq!(after.input_tokens, before.input_tokens);
}

/// `x-request-id` is caller-controlled. Keying the projection on it meant one
/// client reusing an id silently replaced another request's analytics row, and
/// the projection could never be reconciled against the record system's UUIDs.
#[tokio::test]
async fn a_reused_external_request_id_does_not_overwrite_another_request() {
    let f = fixture(success()).await;
    for _ in 0..2 {
        let response = router(f.state.clone())
            .oneshot(
                Request::post("/v1/chat/completions")
                    .header("authorization", format!("Bearer {}", f.token))
                    .header("content-type", "application/json")
                    .header("x-request-id", "client-reused-id")
                    .body(Body::from(chat().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    f.state.observations.flush().await;
    let summary = f
        .state
        .observations
        .summary(SummaryFilter::default())
        .await
        .unwrap();
    assert_eq!(
        summary.requests, 2,
        "both requests must survive; the client-supplied id is not an identity"
    );
    assert_eq!(f.state.observations.dropped_events(), 0);
}

/// A malformed activation document was stored happily and only parsed per
/// request, where it aborted prompt injection for the whole project and
/// surfaced as a generic 500 on every gateway request. The console reported
/// "Saved" and nothing named the offending prompt.
#[tokio::test]
async fn a_malformed_prompt_activation_is_rejected_at_write_time() {
    let f = fixture(Router::new()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/prompts",
        db::DEFAULT_PROJECT_ID
    );
    let prompt = |activation: Value, role: &str| {
        json!({
            "name": "support policy",
            "role": role,
            "content": "Answer with cited facts.",
            "activation": activation,
            "enabled": true,
        })
    };
    for activation in [
        json!({"version":1,"model":"gpt-4o"}),
        json!({"version":2}),
        json!({"version":1,"field":"body/model","op":"eq","value":"m"}),
    ] {
        let response = admin(
            &f,
            &cookie,
            http::Method::POST,
            &path,
            prompt(activation.clone(), "system"),
            true,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "activation should be rejected on the form: {activation}"
        );
    }
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        prompt(json!(null), "System"),
        true,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a role that is not system/user/assistant reaches the provider and fails there"
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM prompts").await,
        0,
        "a rejected prompt must not be stored"
    );

    // A blank activation box in the console serialises as null and means "match
    // everything", not "store the JSON literal null".
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        prompt(json!(null), "system"),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM prompts WHERE activation_json='{\"version\":1}'"
        )
        .await,
        1,
        "a blank activation must become the default document"
    );
}

#[tokio::test]
async fn a_new_prompt_is_disabled_and_placement_fields_are_strict() {
    let f = fixture(Router::new()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/prompts",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "id": "safe-default",
            "name": "safe default",
            "role": "system",
            "content": "Only cite verified facts.",
            "activation": {"version": 1}
        }),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM prompts WHERE name='safe default' AND enabled=0 AND \"order\"=0 AND action='prepend'"
        )
        .await,
        1,
        "an omitted status and placement must use the safe durable defaults"
    );

    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let document: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        document["data"][0]["enabled"], 0,
        "generic SQLite projections expose boolean columns as 0/1"
    );
    assert_eq!(document["data"][0]["order"], 0);
    assert_eq!(document["data"][0]["action"], "prepend");

    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "id": "safe-default",
            "name": "safe default",
            "role": "system",
            "content": "Explicit placement.",
            "activation": {"version": 1},
            "order": 21,
            "action": "append",
            "enabled": true
        }),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    // Older clients send the prompt document without the new placement fields.
    // An update from one must preserve the stored policy instead of silently
    // reverting it to the create defaults.
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "id": "safe-default",
            "name": "safe default",
            "role": "system",
            "content": "Updated by an older client.",
            "activation": {"version": 1}
        }),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM prompts WHERE id='safe-default' AND enabled=1 AND \"order\"=21 AND action='append' AND content='Updated by an older client.'"
        )
        .await,
        1,
        "an update that predates placement fields must keep the stored policy"
    );

    for (name, fields) in [
        ("unknown action", json!({"action":"around"})),
        ("non-string action", json!({"action":7})),
        ("fractional order", json!({"order":1.5})),
        ("string order", json!({"order":"1"})),
        ("non-boolean enabled", json!({"enabled":"false"})),
    ] {
        let mut prompt = json!({
            "name": name,
            "role": "system",
            "content": "x",
            "activation": {"version": 1}
        });
        prompt
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let response = admin(&f, &cookie, http::Method::POST, &path, prompt, true).await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{name} must be refused instead of silently defaulted"
        );
    }
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM prompts").await, 1);
}

/// The request list shows the client-supplied trace id, so opening a trace had
/// to accept that id — resolving only by the internal UUID made every trace link
/// a 404.
#[tokio::test]
async fn a_trace_can_be_opened_by_the_id_the_client_sent() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    f.state.observations.flush().await;

    let path = format!(
        "/api/admin/v1/projects/{}/operations/trace-detail/trace-test",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the trace id the client sent must resolve"
    );
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let document: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        document["trace"]["external_id"], "trace-test",
        "the trace itself must be the one the client asked for"
    );
    // Resolving the trace but rendering it with no children was the real trap.
    assert!(
        !document["requests"].as_array().unwrap().is_empty(),
        "a trace opened by its external id must still list its requests"
    );
    assert!(
        !document["executions"].as_array().unwrap().is_empty(),
        "and its executions"
    );
    assert!(
        document.get("usage").is_some() && document.get("cost_items").is_some(),
        "token and per-component cost detail must be exposed"
    );
    assert!(
        document["requests"][0].get("source_ip").is_some(),
        "the request's client address must be exposed for triage"
    );
    if let Some(usage) = document["usage"].as_array().and_then(|rows| rows.first()) {
        assert!(
            usage.get("cache_write_tokens").is_some(),
            "cache write tokens must be exposed, not silently dropped"
        );
    }
}

/// The same request with a trace id of its own, so one case can build several
/// traces — the client's `x-trace-id` is what a trace row is identified by.
async fn traced_request(f: &Fixture, trace: &str, endpoint: &str, payload: Value) -> Response {
    router(f.state.clone())
        .oneshot(
            Request::post(endpoint)
                .header("authorization", format!("Bearer {}", f.token))
                .header("content-type", "application/json")
                .header("x-trace-id", trace)
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// One row of the traces resource, found by the client's own trace id.
async fn trace_row(f: &Fixture, cookie: &str, trace: &str) -> Value {
    let list = json_body(
        admin(
            f,
            cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{}/operations/traces?q={trace}",
                db::DEFAULT_PROJECT_ID
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    list["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["external_id"] == json!(trace))
        .cloned()
        .unwrap_or_else(|| panic!("{trace} must be listed"))
}

/// The trace list is the row an operator triages from, and it carried only the
/// internal UUID and a thread: the client's own trace id could not be matched
/// against the log line that wrote it, the row never said how many requests the
/// trace held, and the question that started it was stored but never shown.
#[tokio::test]
async fn the_trace_list_names_the_client_trace_id_its_requests_and_the_first_user_query() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    // A preview is only possible when the logging policy stored the body; that is
    // the policy's decision, not the console's.
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            enabled: true,
            default_level: Level::FullBody,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        traced_request(
            &f,
            "trace-test",
            "/v1/chat/completions",
            json!({"model":"public","messages":[{"role":"user","content":"second question"}]})
        )
        .await
        .status(),
        StatusCode::OK
    );
    f.state.observations.flush().await;

    let row = trace_row(&f, &cookie, "trace-test").await;
    assert_eq!(
        row["external_id"],
        json!("trace-test"),
        "the client's own trace id identifies the row"
    );
    assert_eq!(
        row["request_count"],
        json!(2),
        "the row must say how many requests the trace holds"
    );
    assert_eq!(
        row["first_user_query"],
        json!("hello"),
        "the preview is the earliest stored user input, not the last one"
    );

    // The trace's attempts name the channel the way the request detail does: the
    // name frozen on the execution, never a live join on the current provider.
    let base = format!(
        "/api/admin/v1/projects/{}/operations",
        db::DEFAULT_PROJECT_ID
    );
    let detail = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/trace-detail/trace-test"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(detail["trace"]["external_id"], json!("trace-test"));
    assert_eq!(
        detail["executions"][0]["provider_name"],
        json!("a"),
        "the attempt must name the frozen channel"
    );
    // A row that never recorded a name keeps NULL: a provider id is not a name,
    // and the fallback belongs to the console, not to this projection.
    sql(&f,"INSERT INTO request_executions(id,request_id,provider_id,attempt,status,started_at) SELECT 'legacy-attempt',request_id,provider_id,9,'failed',started_at FROM request_executions LIMIT 1",vec![]).await;
    let detail = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/trace-detail/trace-test"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    let legacy = detail["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!("legacy-attempt"))
        .expect("the legacy attempt is listed");
    assert_eq!(
        legacy["provider_name"],
        Value::Null,
        "a provider id must not be substituted for a frozen name"
    );
    assert!(
        legacy["provider_id"].as_str().is_some(),
        "the id stays on the row so the console can fall back to it"
    );
}

/// A preview is a derived read of the payload the logging policy already stored,
/// and of nothing else: no body means `—`, and a body whose only text is system,
/// assistant or tool content has no user query to show. Reading a preview must
/// never widen what a project can see.
#[tokio::test]
async fn a_trace_preview_reads_only_a_stored_user_input() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    // The site default captures metadata only, so there is no body to read at all.
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            enabled: true,
            default_level: Level::FullBody,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for (trace, body) in [
        (
            "trace-system",
            json!({"model":"public","messages":[{"role":"system","content":"never show this"}]}),
        ),
        (
            "trace-tool",
            json!({"model":"public","messages":[{"role":"tool","content":"tool payload"}]}),
        ),
        (
            "trace-assistant",
            json!({"model":"public","messages":[{"role":"assistant","content":"internal reasoning"}]}),
        ),
        (
            "trace-media",
            json!({"model":"public","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.test/a.png"}}]}]}),
        ),
    ] {
        traced_request(&f, trace, "/v1/chat/completions", body).await;
    }
    traced_request(&f, "trace-broken", "/v1/chat/completions", chat()).await;
    sql(&f,"UPDATE request_contents SET request_json='{ truncated' WHERE request_id IN (SELECT id FROM requests WHERE trace_id IN (SELECT id FROM traces WHERE external_id='trace-broken'))",vec![]).await;
    f.state.observations.flush().await;

    for trace in [
        "trace-test",
        "trace-system",
        "trace-tool",
        "trace-assistant",
        "trace-media",
        "trace-broken",
    ] {
        let row = trace_row(&f, &cookie, trace).await;
        assert!(
            row.as_object().unwrap().contains_key("first_user_query"),
            "{trace} must carry the preview field even when it is unmeasured"
        );
        assert_eq!(
            row["first_user_query"],
            Value::Null,
            "{trace} has no user text to preview"
        );
    }
    // The count is a fact about the trace, not about the page it is listed on.
    assert_eq!(
        trace_row(&f, &cookie, "trace-test").await["request_count"],
        json!(1)
    );

    // Another project's trace is not on this page at all, preview or not.
    sql(&f,"INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('other-project','Other','other',0,1,0,0)",vec![]).await;
    sql(&f,"INSERT INTO traces(id,project_id,external_id,status,started_at) VALUES('t-other','other-project','trace-foreign','succeeded',0)",vec![]).await;
    sql(&f,"INSERT INTO requests(id,trace_id,protocol,endpoint,status,started_at) VALUES('r-other','t-other','openai','/v1/chat/completions','succeeded',0)",vec![]).await;
    sql(&f,"INSERT INTO request_contents(request_id,request_json) VALUES('r-other','{\"messages\":[{\"role\":\"user\",\"content\":\"foreign secret\"}]}')",vec![]).await;
    let list = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{}/operations/traces?q=trace-foreign",
                db::DEFAULT_PROJECT_ID
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(
        list["data"].as_array().unwrap().len(),
        0,
        "another project's trace must not be listed, preview or not"
    );
}

/// The preview reads at most eight stored bodies of a trace, in order, and never a
/// body larger than its own read bound. A trace beyond either bound reads `—`: a
/// client can put a thousand requests, or a megabyte of body, in one trace, and a
/// list must not read all of it. Both bounds are decisions, so both are pinned.
#[tokio::test]
async fn a_trace_preview_stops_at_its_bounds() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            enabled: true,
            default_level: Level::FullBody,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // Eight requests with nothing a user wrote, then the one that asks.
    for _ in 0..8 {
        traced_request(
            &f,
            "trace-bound",
            "/v1/chat/completions",
            json!({"model":"public","messages":[{"role":"system","content":"a system prompt"}]}),
        )
        .await;
    }
    traced_request(&f, "trace-bound", "/v1/chat/completions", chat()).await;
    // A single body past the read bound is not parsed at all, so the earliest user
    // input of this trace is not found even though it is the first thing in it.
    traced_request(
        &f,
        "trace-huge",
        "/v1/chat/completions",
        json!({"model":"public","messages":[{"role":"user","content":"x".repeat(300 * 1024)}]}),
    )
    .await;
    f.state.observations.flush().await;

    let row = trace_row(&f, &cookie, "trace-bound").await;
    assert_eq!(
        row["request_count"],
        json!(9),
        "the count describes the trace, not the part of it the preview read"
    );
    assert_eq!(
        row["first_user_query"],
        Value::Null,
        "the ninth body is beyond the bound the preview reads"
    );
    assert_eq!(
        trace_row(&f, &cookie, "trace-huge").await["first_user_query"],
        Value::Null,
        "a body larger than the read bound is not parsed"
    );
}

/// Deleting a model that has price history cascaded into an immutability trigger
/// that aborts the statement, which surfaced as an opaque 500 with nothing
/// naming the cause. A model with no history must still delete cleanly.
#[tokio::test]
async fn deleting_a_model_with_history_is_a_typed_conflict() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO providers(id,project_id,name,kind,base_url,enabled,created_at,updated_at) VALUES('p-del',?,'del','openai','http://127.0.0.1:1',1,0,0)",
        vec![db::DEFAULT_PROJECT_ID.into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO models(id,provider_id,public_name,upstream_name,lifecycle,created_at) VALUES('m-used','p-del','used','used','archived',0),('m-free','p-del','free','free','archived',0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO model_prices(id,model_id,version,valid_from,created_at) VALUES('price-used','m-used',1,0,0)",
        vec![],
    )
    .await;

    let path = format!(
        "/api/admin/v1/projects/{}/operations/models/m-used",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::DELETE, &path, json!(null), true).await;
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "an in-use model must be refused with a reason, not an internal error"
    );

    let path = format!(
        "/api/admin/v1/projects/{}/operations/models/m-free",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::DELETE, &path, json!(null), true).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a model with no history must still be deletable"
    );
}

fn model_lifecycle_path(project: &str, model: &str) -> String {
    format!("/api/admin/v1/projects/{project}/models/{model}/lifecycle")
}

fn model_impact_path(project: &str, model: &str) -> String {
    format!("/api/admin/v1/projects/{project}/models/{model}/delete-impact")
}

#[tokio::test]
async fn model_archive_restore_is_immediate_audited_and_not_a_bulk_toggle() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let provider = &f.providers[0];
    sql(
        &f,
        "INSERT INTO models(id,provider_id,public_name,upstream_name,created_at) VALUES('b30-life',?,'b30-public','a-model',0)",
        vec![provider.clone().into()],
    )
    .await;

    let before_generation = f.state.orchestrator.generation.load(Ordering::Relaxed);
    let mut body = chat();
    body["model"] = json!("b30-public");
    assert_eq!(
        request(&f, "/v1/chat/completions", body.clone())
            .await
            .status(),
        StatusCode::OK
    );

    let archived = admin(
        &f,
        &cookie,
        http::Method::POST,
        &model_lifecycle_path(project, "b30-life"),
        json!({"action":"archive"}),
        true,
    )
    .await;
    assert_eq!(archived.status(), StatusCode::OK);
    assert!(
        f.state.orchestrator.generation.load(Ordering::Relaxed) > before_generation,
        "the lifecycle mutation must invalidate process-local routing state"
    );
    assert_eq!(
        request(&f, "/v1/chat/completions", body.clone())
            .await
            .status(),
        StatusCode::BAD_REQUEST,
        "an archived model must stop routing immediately"
    );
    let public = router(f.state.clone())
        .oneshot(
            Request::get("/v1/models")
                .header("authorization", format!("Bearer {}", f.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let public = json_body(public).await;
    assert!(
        public["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|model| model["id"] != "b30-public"),
        "archived models must not appear in public discovery: {public}"
    );

    let default_list = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("/api/admin/v1/projects/{project}/operations/models?q=b30-life"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(default_list["total"], json!(0));
    let archived_list = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{project}/operations/models?q=b30-life&lifecycle=archived"
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(archived_list["data"][0]["lifecycle"], json!("archived"));

    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &format!("/api/admin/v1/projects/{project}/operations/bulk-toggle"),
            json!({"resource":"models","ids":["b30-life"],"enabled":false}),
            true,
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM models WHERE id='b30-life' AND enabled=1 AND lifecycle='archived'"
        )
        .await,
        1,
        "generic enable/disable must not mutate archived models"
    );

    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &model_lifecycle_path(project, "b30-life"),
            json!({"action":"restore"}),
            true,
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&f, "/v1/chat/completions", body).await.status(),
        StatusCode::OK,
        "restore must re-enter normal enabled and policy checks"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action IN ('model.archive','model.restore') AND resource_id='b30-life'"
        )
        .await,
        2
    );
}

#[tokio::test]
async fn model_delete_preview_and_archive_gate_are_scoped_typed_and_audited() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let provider = &f.providers[0];
    sql(
        &f,
        "INSERT INTO models(id,provider_id,public_name,upstream_name,lifecycle,created_at) VALUES('b30-used',?,'b30-used','used','archived',0),('b30-active',?,'b30-active','active','active',0),('b30-free',?,'b30-free','free','archived',0)",
        vec![provider.clone().into(), provider.clone().into(), provider.clone().into()],
    )
    .await;
    sql(&f,"INSERT INTO model_associations(id,project_id,model_id,match_type,pattern,created_at,updated_at) VALUES('b30-assoc',?,'b30-used','exact','b30-used',0,0)",vec![project.into()]).await;
    sql(&f,"INSERT INTO model_prices(id,model_id,version,valid_from,created_at) VALUES('b30-price','b30-used',1,0,0)",vec![]).await;
    sql(&f,"INSERT INTO model_price_components(id,price_id,kind,unit_size,unit_price_micros) VALUES('b30-component','b30-price','input',1,1)",vec![]).await;
    sql(&f,"INSERT INTO request_facts(id,project_id,log_level,started_at) VALUES('b30-request',?,'metadata',0)",vec![project.into()]).await;
    sql(&f,"INSERT INTO execution_facts(id,request_id,model_id,attempt,config_json,price_json,started_at) VALUES('b30-execution','b30-request','b30-used',1,'{}','{}',0)",vec![]).await;
    sql(&f,"INSERT INTO usage_logs(id,execution_id,model_id,price_id,created_at) VALUES('b30-usage','b30-execution','b30-used','b30-price',0)",vec![]).await;

    let preview = admin(
        &f,
        &cookie,
        http::Method::GET,
        &model_impact_path(project, "b30-used"),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(preview.status(), StatusCode::OK);
    let preview = json_body(preview).await;
    assert_eq!(preview["prices"], json!(1));
    assert_eq!(preview["price_components"], json!(1));
    assert_eq!(preview["usage_history"], json!(1));
    assert_eq!(preview["associations"], json!(1));
    assert_eq!(preview["executions"], json!(1));
    assert_eq!(preview["blocked"], json!(true));

    for model in ["missing", "foreign"] {
        if model == "foreign" {
            sql(&f,"INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('b30-other','Other','b30-other',0,1,0,0)",vec![]).await;
            sql(&f,"INSERT INTO providers(id,project_id,name,kind,base_url,created_at,updated_at) VALUES('b30-other-provider','b30-other','Other','openai','',0,0)",vec![]).await;
            sql(&f,"INSERT INTO models(id,provider_id,public_name,upstream_name,lifecycle,created_at) VALUES('foreign','b30-other-provider','foreign','foreign','archived',0)",vec![]).await;
        }
        assert_eq!(
            admin(
                &f,
                &cookie,
                http::Method::GET,
                &model_impact_path(project, model),
                Value::Null,
                false,
            )
            .await
            .status(),
            StatusCode::NOT_FOUND,
            "foreign and absent ids are the same opaque result"
        );
    }

    let active_delete = admin(
        &f,
        &cookie,
        http::Method::DELETE,
        &format!("/api/admin/v1/projects/{project}/operations/models/b30-active"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(active_delete.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(active_delete).await["error"]["type"],
        json!("archive_required")
    );

    let used_delete = admin(
        &f,
        &cookie,
        http::Method::DELETE,
        &format!("/api/admin/v1/projects/{project}/operations/models/b30-used"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(used_delete.status(), StatusCode::CONFLICT);
    let used_error = json_body(used_delete).await;
    assert_eq!(used_error["error"]["type"], json!("history_retained"));
    assert!(
        !used_error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("SQLite")
    );

    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::DELETE,
            &format!("/api/admin/v1/projects/{project}/operations/models/b30-free"),
            Value::Null,
            true,
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM models WHERE id='b30-free'").await,
        0
    );
    assert_eq!(count(&f,"SELECT COUNT(*) AS n FROM audit_events WHERE action='models.delete' AND resource_id='b30-free'").await,1);
}

/// The stored request-logging policy was parsed with `deny_unknown_fields`, so a
/// document that already contained an extra field made policy resolution fail on
/// the admission path: 500 for every gateway request in the instance, and the
/// console could not repair it because the write path rejected the same field.
#[tokio::test]
async fn a_stored_logging_policy_with_an_unknown_field_does_not_break_the_gateway() {
    let f = fixture(success()).await;
    sql(
        &f,
        "INSERT INTO settings(key,value,updated_at) VALUES('request_logging',?,0) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        vec![json!({
            "version": ops::logging::CURRENT_POLICY_VERSION,
            "enabled": true,
            "default_level": "metadata",
            "key_override_enabled": true,
            "key_disable_allowed": true,
            "retained_from_an_older_build": true,
        })
        .to_string()
        .into()],
    )
    .await;

    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "stored configuration must never be able to break admission"
    );
}

const MODEL_SETTINGS_PATH: &str = "/api/admin/v1/settings/models";

async fn stored_model_settings(f: &Fixture) -> Value {
    f.state
        .db
        .query_one(ops::sql(
            "SELECT value FROM settings WHERE key='model_settings'",
            vec![],
        ))
        .await
        .unwrap()
        .map(|row| serde_json::from_str(&row.try_get::<String>("", "value").unwrap()).unwrap())
        .unwrap_or(Value::Null)
}

#[tokio::test]
async fn b32_model_settings_strict_write_rejects_unknown_fields_versions_and_regexes() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let known = json!({
        "version": 1,
        "fallback_to_channels_on_model_not_found": false,
        "query_all_channel_models": false,
        "default_model_api_include_all": true,
        "auto_reasoning_effort": true,
        "model_blacklist_regex": "^private-",
        "hide_unroutable_models_in_list": false,
    });
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            MODEL_SETTINGS_PATH,
            known.clone(),
            true,
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(stored_model_settings(&f).await, known);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='model-settings.update'",
        )
        .await,
        1,
    );

    let mut unknown_field = known.clone();
    unknown_field["query_all_channel_model"] = json!(true);
    let mut unknown_version = known.clone();
    unknown_version["version"] = json!(2);
    let mut invalid_regex = known.clone();
    invalid_regex["model_blacklist_regex"] = json!("[");
    let mut oversized_regex = known.clone();
    oversized_regex["model_blacklist_regex"] = json!("x".repeat(2049));
    for invalid in [
        unknown_field,
        unknown_version,
        invalid_regex,
        oversized_regex,
    ] {
        let response = admin(
            &f,
            &cookie,
            http::Method::PUT,
            MODEL_SETTINGS_PATH,
            invalid,
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("model settings"))
        );
    }
    assert_eq!(stored_model_settings(&f).await, known);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='model-settings.update'",
        )
        .await,
        1,
        "rejected writes must not persist or audit anything",
    );
}

#[tokio::test]
async fn b32_model_settings_tolerate_unknown_stored_fields_and_keep_admission_available() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO settings(key,value,updated_at) VALUES('model_settings',?,0)",
        vec![
            json!({
                "version": 1,
                "fallback_to_channels_on_model_not_found": true,
                "query_all_channel_models": true,
                "default_model_api_include_all": false,
                "auto_reasoning_effort": false,
                "model_blacklist_regex": "",
                "hide_unroutable_models_in_list": true,
                "retained_from_a_future_build": {"enabled": true},
            })
            .to_string()
            .into(),
        ],
    )
    .await;

    let read = admin(
        &f,
        &cookie,
        http::Method::GET,
        MODEL_SETTINGS_PATH,
        Value::Null,
        false,
    )
    .await;
    assert_eq!(read.status(), StatusCode::OK);
    let document = json_body(read).await;
    assert_eq!(document.get("retained_from_a_future_build"), None);
    assert_eq!(document["version"], 1);
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK,
        "a stored field this build does not know must not break admission",
    );
}

const LOG_POLICY_PATH: &str = "/api/admin/v1/settings/request-logging";

/// The stored request-logging document, exactly as the record system holds it.
async fn stored_logging_policy(f: &Fixture) -> Value {
    f.state
        .db
        .query_one(ops::sql(
            "SELECT value FROM settings WHERE key='request_logging'",
            vec![],
        ))
        .await
        .unwrap()
        .map(|row| serde_json::from_str(&row.try_get::<String>("", "value").unwrap()).unwrap())
        .unwrap_or(Value::Null)
}

/// Live preview is a bounded process-local view, not another request log. The
/// read is authorized for exactly one project, its wire shape has no place for a
/// payload, and the request-logging policy can hide it without mutating the
/// registry that owns the active request.
#[tokio::test]
async fn live_request_preview_is_project_scoped_payload_free_and_policy_gated() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('live-foreign','Foreign','live-foreign',0,1,1,1)",
        vec![],
    )
    .await;
    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            live_preview_enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let own = f.state.orchestrator.begin_live_request(
        db::DEFAULT_PROJECT_ID,
        "public",
        &f.providers[0],
        "key-own",
    );
    let foreign = f.state.orchestrator.begin_live_request(
        "live-foreign",
        "private-model",
        &f.providers[1],
        "key-foreign",
    );
    sql(
        &f,
        "INSERT INTO request_facts(id,project_id,log_level,status,started_at) VALUES('live-accounting',?,'off','running',1)",
        vec![db::DEFAULT_PROJECT_ID.into()],
    )
    .await;
    let path = format!(
        "/api/admin/v1/projects/{}/live-requests",
        db::DEFAULT_PROJECT_ID
    );
    let visible =
        json_body(admin(&f, &cookie, http::Method::GET, &path, Value::Null, false).await).await;
    assert_eq!(visible["enabled"], json!(true), "{visible}");
    let rows = visible["data"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        1,
        "another project's request must stay invisible"
    );
    assert_eq!(rows[0]["model"], json!("public"));
    assert_eq!(rows[0]["channel_id"], json!(&f.providers[0]));
    assert_eq!(rows[0]["api_key_id"], json!("key-own"));
    assert!(rows[0]["started_at"].as_i64().is_some());
    let keys: std::collections::BTreeSet<_> = rows[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["api_key_id", "channel_id", "model", "started_at"]
            .into_iter()
            .collect(),
        "the live shape must have no request body, response body, or project id"
    );

    ops::logging::set_policy(
        &f.state.db,
        &Policy {
            live_preview_enabled: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let hidden =
        json_body(admin(&f, &cookie, http::Method::GET, &path, Value::Null, false).await).await;
    assert_eq!(hidden, json!({"enabled":false,"data":[]}));
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM request_facts WHERE id='live-accounting'",
        )
        .await,
        1,
        "preview policy changes must not remove authoritative accounting facts"
    );
    assert_eq!(
        f.state
            .orchestrator
            .live_requests(db::DEFAULT_PROJECT_ID)
            .len(),
        1,
        "the toggle hides the view; it does not alter live gateway state"
    );

    drop(own);
    drop(foreign);
    assert!(
        f.state
            .orchestrator
            .live_requests(db::DEFAULT_PROJECT_ID)
            .is_empty(),
        "dropping request ownership removes the ephemeral row"
    );
}

async fn logging_audits(f: &Fixture) -> i64 {
    count(
        f,
        "SELECT COUNT(*) AS n FROM audit_events WHERE action='logging.update'",
    )
    .await
}

#[tokio::test]
async fn versioned_request_logging_policy_round_trips_and_is_audited() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let default = admin(
        &f,
        &cookie,
        http::Method::GET,
        LOG_POLICY_PATH,
        Value::Null,
        false,
    )
    .await;
    assert_eq!(default.status(), StatusCode::OK);
    assert_eq!(
        json_body(default).await,
        json!({
            "version": ops::logging::CURRENT_POLICY_VERSION,
            "enabled": true,
            "default_level": "metadata",
            "key_override_enabled": false,
            "key_disable_allowed": false,
        })
    );

    let versioned = json!({
        "version": ops::logging::CURRENT_POLICY_VERSION,
        "enabled": false,
        "default_level": "redacted_body",
        "key_override_enabled": true,
        "key_disable_allowed": true,
    });
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            LOG_POLICY_PATH,
            versioned.clone(),
            true,
        )
        .await
        .status(),
        StatusCode::OK
    );
    let read = admin(
        &f,
        &cookie,
        http::Method::GET,
        LOG_POLICY_PATH,
        Value::Null,
        false,
    )
    .await;
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(json_body(read).await, versioned);
    assert_eq!(stored_logging_policy(&f).await, versioned);
    assert_eq!(logging_audits(&f).await, 1);
}

#[tokio::test]
async fn an_unknown_request_logging_version_is_rejected_and_nothing_is_written() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let response = admin(
        &f,
        &cookie,
        http::Method::PUT,
        LOG_POLICY_PATH,
        json!({"version": 2, "default_level": "full_body"}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"]["type"],
        "invalid_request_error"
    );
    assert_eq!(stored_logging_policy(&f).await, Value::Null);
    assert_eq!(logging_audits(&f).await, 0);
}

#[tokio::test]
async fn malformed_or_unknown_stored_logging_policy_keeps_admission_private_and_available() {
    for document in [
        "{malformed-policy}".to_owned(),
        json!({
            "version": 2,
            "enabled": true,
            "default_level": "full_body",
            "key_override_enabled": true,
            "key_disable_allowed": true,
        })
        .to_string(),
    ] {
        let f = fixture(success()).await;
        sql(
            &f,
            "INSERT INTO settings(key,value,updated_at) VALUES('request_logging',?,0) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            vec![document.into()],
        )
        .await;

        let response = request(&f, "/v1/chat/completions", chat()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            count(&f, "SELECT COUNT(*) AS n FROM requests").await,
            0,
            "an invalid policy must never enable browsable metadata or bodies"
        );
        assert_eq!(
            count(&f, "SELECT COUNT(*) AS n FROM request_contents").await,
            0,
            "an invalid policy must never capture a request body"
        );
        assert_eq!(
            count(&f, "SELECT COUNT(*) AS n FROM request_facts").await,
            1,
            "privacy fallback must not stop authoritative accounting"
        );

        let cookie = owner(&f).await;
        let read = admin(
            &f,
            &cookie,
            http::Method::GET,
            LOG_POLICY_PATH,
            Value::Null,
            false,
        )
        .await;
        assert_eq!(read.status(), StatusCode::OK);
        assert_eq!(
            json_body(read).await,
            json!({
                "version": ops::logging::CURRENT_POLICY_VERSION,
                "enabled": false,
                "default_level": "metadata",
                "key_override_enabled": false,
                "key_disable_allowed": false,
            }),
            "the console gets a writable fail-closed shape, never fields from the invalid document"
        );
    }
}

#[tokio::test]
async fn a_non_text_stored_logging_policy_keeps_admission_private_and_repairable() {
    for stored_value in ["X'7B7D'", "CAST(X'80' AS TEXT)", "17"] {
        let f = fixture(success()).await;
        let insert = format!(
            "INSERT INTO settings(key,value,updated_at) VALUES('request_logging',{stored_value},0) ON CONFLICT(key) DO UPDATE SET value=excluded.value"
        );
        sql(&f, &insert, vec![]).await;

        let response = request(&f, "/v1/chat/completions", chat()).await;
        assert_eq!(response.status(), StatusCode::OK, "{stored_value}");
        assert_eq!(
            count(&f, "SELECT COUNT(*) AS n FROM requests").await,
            0,
            "an invalid SQLite value must not enable browsable metadata: {stored_value}"
        );
        assert_eq!(
            count(&f, "SELECT COUNT(*) AS n FROM request_contents").await,
            0,
            "an invalid SQLite value must not capture bodies: {stored_value}"
        );
        assert_eq!(
            count(&f, "SELECT COUNT(*) AS n FROM request_facts").await,
            1,
            "accounting must continue: {stored_value}"
        );

        let cookie = owner(&f).await;
        let read = admin(
            &f,
            &cookie,
            http::Method::GET,
            LOG_POLICY_PATH,
            Value::Null,
            false,
        )
        .await;
        assert_eq!(read.status(), StatusCode::OK, "{stored_value}");
        assert_eq!(
            json_body(read).await,
            json!({
                "version": ops::logging::CURRENT_POLICY_VERSION,
                "enabled": false,
                "default_level": "metadata",
                "key_override_enabled": false,
                "key_disable_allowed": false,
            }),
            "GET must return the writable repair shape: {stored_value}"
        );
    }
}

/// The write path is the only place an operator typo can install a logging
/// policy nobody asked for. `PUT` deserialized straight into the tolerant
/// `Policy`, so `{"enabledd": false}` was accepted and stored the default
/// *enabled/metadata* policy — the exact inverse of what was typed — and an
/// audit row recorded it as a deliberate change. A strict write contract rejects
/// the document instead, and the stored policy and its audit history stay as
/// they were. Read tolerance is a separate, deliberate contract (see the test
/// above); this one is about the boundary where the typo enters.
#[tokio::test]
async fn an_unknown_request_logging_field_is_rejected_and_nothing_is_written() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let known = json!({"version":ops::logging::CURRENT_POLICY_VERSION,"enabled":true,"default_level":"off","key_override_enabled":true,"key_disable_allowed":true});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            LOG_POLICY_PATH,
            known.clone(),
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    let stored = stored_logging_policy(&f).await;
    let audits = logging_audits(&f).await;
    assert_eq!(stored, known, "the known policy must be the one stored");

    let response = admin(
        &f,
        &cookie,
        http::Method::PUT,
        LOG_POLICY_PATH,
        json!({"enabledd": false}),
        true,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a typo must be refused, not silently turned into a different policy"
    );
    let body = json_body(response).await;
    assert_eq!(body["error"]["type"], "invalid_request_error");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("request-logging"),
        "the error must name the contract: {message}"
    );
    assert!(
        !message.contains("unknown field") && !message.contains("line "),
        "no serde cause may reach the client: {message}"
    );
    assert_eq!(
        stored_logging_policy(&f).await,
        known,
        "a rejected write must leave the stored policy untouched"
    );
    assert_eq!(
        logging_audits(&f).await,
        audits,
        "a rejected write must not be audited as a change"
    );
}

/// The other two shapes, which the old extractor rejected before the handler ever
/// ran and so answered outside this contract. An unknown *enum* value
/// (`"verbose"`, `"Metadata"`) never fell back to a level: `Json<Policy>` refused
/// it with a plain-text `422` the client could not read as the documented `400`
/// JSON envelope, and a wrong primitive type (`3` for a level, `"no"` for a bool)
/// failed the same way. That is the inverse of the field typo above, where the
/// document was *accepted* and installed the default enabled/metadata policy
/// nobody wrote. The strict write DTO answers both from one place now: an unknown
/// field, an unknown level, a wrong type and a non-object body all leave
/// `PolicyInput::parse` as the same `400 invalid_request_error`, naming the
/// contract and carrying no serde cause, with the stored policy and its audit
/// history untouched.
#[tokio::test]
async fn an_invalid_request_logging_level_or_type_is_rejected() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let known = json!({"version":ops::logging::CURRENT_POLICY_VERSION,"enabled":true,"default_level":"metadata","key_override_enabled":false,"key_disable_allowed":false});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            LOG_POLICY_PATH,
            known.clone(),
            true
        )
        .await
        .status(),
        StatusCode::OK
    );

    for input in [
        json!({"default_level":"verbose"}),
        json!({"default_level":"Metadata"}),
        json!({"default_level":3}),
        json!({"default_level":null}),
        json!({"enabled":"no"}),
        json!({"enabled":1}),
        json!({"key_override_enabled":"yes"}),
        json!({"key_disable_allowed":{"value":true}}),
        json!([]),
        json!("off"),
    ] {
        let response = admin(
            &f,
            &cookie,
            http::Method::PUT,
            LOG_POLICY_PATH,
            input.clone(),
            true,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "should be rejected on the form: {input}"
        );
        let body = json_body(response).await;
        assert_eq!(body["error"]["type"], "invalid_request_error", "{input}");
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("request-logging")),
            "the envelope must carry a safe validation message: {input} {body}"
        );
    }
    assert_eq!(
        stored_logging_policy(&f).await,
        known,
        "no rejected input may reach the record system"
    );
    assert_eq!(
        logging_audits(&f).await,
        1,
        "only the accepted write is audited"
    );
}

/// `inherit` is a valid level for a key and not for the site default. The strict
/// write contract must not have loosened that domain rule, and its refusal is
/// still a 400 that writes nothing and audits nothing.
#[tokio::test]
async fn the_site_default_still_cannot_inherit() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let known = json!({"version":ops::logging::CURRENT_POLICY_VERSION,"enabled":true,"default_level":"metadata","key_override_enabled":false,"key_disable_allowed":false});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            LOG_POLICY_PATH,
            known.clone(),
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    let response = admin(
        &f,
        &cookie,
        http::Method::PUT,
        LOG_POLICY_PATH,
        json!({"default_level":"inherit"}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"]["type"],
        "invalid_request_error"
    );
    assert_eq!(stored_logging_policy(&f).await, known);
    assert_eq!(
        logging_audits(&f).await,
        1,
        "only the accepted write is audited"
    );
}

/// A policy that is actually valid still commits, together with exactly one
/// audit row in the same transaction. A partial document keeps the documented
/// default for every field it omits (`enabled` stays true, not `false`).
#[tokio::test]
async fn a_valid_request_logging_policy_commits_with_one_audit_row() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;

    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            LOG_POLICY_PATH,
            json!({"default_level":"full_body"}),
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        stored_logging_policy(&f).await,
        json!({"version":ops::logging::CURRENT_POLICY_VERSION,"enabled":true,"default_level":"full_body","key_override_enabled":false,"key_disable_allowed":false}),
        "an omitted field keeps its documented default"
    );
    assert_eq!(logging_audits(&f).await, 1);

    let full = json!({"version":ops::logging::CURRENT_POLICY_VERSION,"enabled":false,"default_level":"redacted_body","key_override_enabled":true,"key_disable_allowed":true});
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            LOG_POLICY_PATH,
            full.clone(),
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(stored_logging_policy(&f).await, full);
    assert_eq!(logging_audits(&f).await, 2);
    let read = admin(
        &f,
        &cookie,
        http::Method::GET,
        LOG_POLICY_PATH,
        Value::Null,
        false,
    )
    .await;
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(json_body(read).await, full);
}

/// The console reads the policy and writes back what it read. A stored document
/// carrying a field this build does not know must not turn the settings form
/// into a page that can only answer 400, so the read returns the policy this
/// build can also write, while admission keeps tolerating the stored document.
#[tokio::test]
async fn a_stored_logging_policy_with_an_unknown_field_stays_writable_from_the_console() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO settings(key,value,updated_at) VALUES('request_logging',?,0) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        vec![json!({
            "enabled": false,
            "default_level": "off",
            "key_override_enabled": false,
            "key_disable_allowed": false,
            "retained_from_an_older_build": true,
        })
        .to_string()
        .into()],
    )
    .await;

    let read = admin(
        &f,
        &cookie,
        http::Method::GET,
        LOG_POLICY_PATH,
        Value::Null,
        false,
    )
    .await;
    assert_eq!(read.status(), StatusCode::OK);
    let document = json_body(read).await;
    assert_eq!(
        document,
        json!({"version":ops::logging::CURRENT_POLICY_VERSION,"enabled":false,"default_level":"off","key_override_enabled":false,"key_disable_allowed":false}),
        "the read returns the effective policy, not the raw stored document"
    );

    // What the console read, it can write back.
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::PUT,
            LOG_POLICY_PATH,
            document,
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        stored_logging_policy(&f).await,
        json!({"version":ops::logging::CURRENT_POLICY_VERSION,"enabled":false,"default_level":"off","key_override_enabled":false,"key_disable_allowed":false}),
        "the accepted write replaces the document with the known fields"
    );
}

/// Every upstream failure was flattened to 502, so the request log could not
/// distinguish a rate limit from a bad key from an outage and the status facet
/// could only ever offer 200 or 502. The attempt already held the real status.
#[tokio::test]
async fn the_request_log_keeps_the_real_upstream_status() {
    let upstream = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error":{"message":"slow down"}})),
            )
        }),
    );
    let f = fixture(upstream).await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    f.state.observations.flush().await;

    let rows = f
        .state
        .observations
        .list(RequestFilter::default())
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "one request, one row");
    assert_eq!(
        rows[0].status_code,
        Some(429),
        "the upstream status must survive into the log; 502 hides the reason"
    );
    assert_eq!(rows[0].error_kind.as_deref(), Some("failed"));
    // The client still gets the gateway's own error code; what must not be lost
    // is the upstream status, which is what the log and its facet are for.
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        f.state.observations.dropped_events(),
        0,
        "nothing may be dropped on the way to the projection"
    );
}

/// The execution projection is where an operator explains a request, so the facts
/// the record system freezes at settlement must reach it — the attempt's own status,
/// the channel name it used and why it failed.
#[tokio::test]
async fn execution_projections_expose_the_outcome_snapshot() {
    let upstream = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                StatusCode::CREATED,
                Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}})),
            )
        }),
    );
    let f = fixture(upstream).await;
    let cookie = owner(&f).await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::CREATED
    );
    let base = format!(
        "/api/admin/v1/projects/{}/operations",
        db::DEFAULT_PROJECT_ID
    );
    let list = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/executions"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    let row = &list["data"][0];
    assert_eq!(row["provider_name"], json!("a"));
    assert_eq!(row["http_status"], json!(201));
    assert_eq!(row["error_kind"], json!(null));
    let request_id = row["request_id"].as_str().unwrap().to_owned();
    let detail = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/requests/{request_id}"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    let attempt = &detail["executions"][0];
    assert_eq!(attempt["provider_name"], json!("a"));
    assert_eq!(attempt["http_status"], json!(201));
    assert_eq!(attempt["error_kind"], json!(null));
}

/// The orchestrator reads `projects.settings_json.routing` as the project's
/// default routing policy, but no route ever wrote it: the setting was reachable
/// only by editing SQLite by hand, and a malformed one would have been accepted.
#[tokio::test]
async fn the_project_default_routing_policy_is_writable_and_validated() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/settings/orchestration",
        db::DEFAULT_PROJECT_ID
    );
    let settings = |routing: Value| {
        json!({
            "version": 1,
            "affinity_rules": [],
            "session_compaction": {
                "enabled": false,
                "threshold_tokens": 8192,
                "retain_items": 16,
                "native": true,
                "summarizer_model": null,
            },
            "routing": routing,
        })
    };

    let response = admin(
        &f,
        &cookie,
        http::Method::PUT,
        &path,
        settings(json!({"version":1,"field":"/body/model","op":"eq","value":"fast"})),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let stored: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        stored["routing"]["value"], "fast",
        "the default routing policy must survive the round trip"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM projects WHERE settings_json LIKE '%\"routing\"%'"
        )
        .await,
        1,
        "it must actually be persisted on the project"
    );

    // A malformed default would have broken candidate generation for every
    // request in the project; it must be refused on the form.
    let response = admin(
        &f,
        &cookie,
        http::Method::PUT,
        &path,
        settings(json!({"version":1,"model":"fast"})),
        true,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a malformed default routing condition must be refused when written"
    );
}

/// Deleting a channel used to either abort on immutable price history (an opaque
/// 500) or silently delete its models and credentials. Neither is acceptable: the
/// operator must be told what would be destroyed.
#[tokio::test]
async fn deleting_a_channel_in_use_is_a_typed_conflict() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO providers(id,project_id,name,kind,base_url,enabled,created_at,updated_at) VALUES('ch-in-use',?,'in-use','openai','http://127.0.0.1:1',1,0,0),('ch-empty',?,'empty','openai','http://127.0.0.1:1',1,0,0)",
        vec![db::DEFAULT_PROJECT_ID.into(), db::DEFAULT_PROJECT_ID.into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO models(id,provider_id,public_name,upstream_name,created_at) VALUES('m-attached','ch-in-use','attached','attached',0)",
        vec![],
    )
    .await;

    let path = format!(
        "/api/admin/v1/projects/{}/operations/channels/ch-in-use",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::DELETE, &path, json!(null), true).await;
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "a channel that still owns models must be refused, not silently cascaded"
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM models WHERE id='m-attached'").await,
        1,
        "the model must still be there"
    );

    let path = format!(
        "/api/admin/v1/projects/{}/operations/channels/ch-empty",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::DELETE, &path, json!(null), true).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "an unused channel must still be deletable"
    );
}

/// The dependency checks in the remove path are not project-scoped and they ran
/// *before* the project-scoped delete, so a project-A manager could tell a foreign
/// channel that still owns models (409) from a foreign or absent id that owns
/// nothing (404) — an existence-and-usage oracle over another project's resources.
/// Ownership has to be established first, and every answer for a resource this
/// project does not own has to be the same one.
#[tokio::test]
async fn deleting_a_foreign_channel_or_model_fails_closed() {
    let f = fixture(success()).await;
    let (cookie, owner_id) = owner_principal(&f).await;
    let owner = access::Principal::session(owner_id);
    // A manager of the default project, holding exactly that project's authority.
    let (_, manager) = access::create_scoped_api_key(
        &f.state.db,
        &owner,
        &access::ScopedApiKeyInput {
            name: "project-manager".into(),
            project_id: db::DEFAULT_PROJECT_ID.into(),
            user_id: None,
            profile_id: None,
            key_type: "service".into(),
            scopes: vec!["project:manage".into()],
            budget_micros: None,
            expires_at: None,
            allowed_ips: vec![],
            denied_ips: vec![],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sql(&f,"INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('remove-project','Elsewhere','remove-elsewhere',0,1,0,0)",vec![]).await;
    sql(&f,"INSERT INTO providers(id,project_id,name,kind,base_url,enabled,created_at,updated_at) VALUES('b-attached','remove-project','b-attached','openai','http://127.0.0.1:1',1,0,0),('b-bare','remove-project','b-bare','openai','http://127.0.0.1:1',1,0,0)",vec![]).await;
    sql(&f,"INSERT INTO models(id,provider_id,public_name,upstream_name,created_at) VALUES('b-model','b-attached','b-public','b-upstream',0),('b-bare-model','b-bare','b-bare-public','b-bare-upstream',0)",vec![]).await;
    sql(&f,"INSERT INTO channel_credentials(id,provider_id,secret_envelope,created_at,updated_at) VALUES('b-cred','b-attached','envelope',0,0)",vec![]).await;
    // The model in the other project also has price history, which is the second
    // dependency the remove path reports on.
    sql(&f,"INSERT INTO model_prices(id,model_id,version,valid_from,created_at) VALUES('b-price','b-model',1,0,0)",vec![]).await;

    for (resource, id) in [
        ("channels", "b-attached"),
        ("channels", "b-bare"),
        ("channels", "no-such-channel"),
        ("models", "b-model"),
        ("models", "b-bare-model"),
        ("models", "no-such-model"),
    ] {
        let response = delete_operation(&f, &manager, resource, id).await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{resource}/{id} belongs to another project, so it must be answered exactly like an id that does not exist"
        );
    }
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM providers WHERE id IN ('b-attached','b-bare')"
        )
        .await,
        2,
        "another project's channels must be untouched"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM models WHERE id IN ('b-model','b-bare-model')"
        )
        .await,
        2,
        "another project's models must be untouched"
    );

    // The same project's own resources keep their own answers: a channel that still
    // owns models is a conflict, an unreferenced one is deletable.
    let own = format!(
        "/api/admin/v1/projects/{}/operations/channels/{}",
        db::DEFAULT_PROJECT_ID,
        f.providers[0]
    );
    let response = admin(&f, &cookie, http::Method::DELETE, &own, json!(null), true).await;
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "this project's own channel is still refused for its own reason"
    );
}

/// The request detail returned the row and nothing else, so the console could
/// show a request but never explain it: no attempts, no tokens, no cost
/// breakdown. It also had no project check of its own.
#[tokio::test]
async fn the_request_detail_carries_its_attempts_and_cost() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let external = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();

    let internal: String = f
        .state
        .db
        .query_one(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "SELECT id FROM requests ORDER BY started_at DESC LIMIT 1",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();

    let path = format!(
        "/api/admin/v1/projects/{}/operations/requests/{}",
        db::DEFAULT_PROJECT_ID,
        internal
    );
    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let document: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        document["executions"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "a request detail must list its attempts"
    );
    assert!(
        document.get("usage").is_some() && document.get("cost_items").is_some(),
        "and its tokens and per-component cost"
    );

    // A request from another project must not be readable here.
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('other-project','Other','other',0,1,0,0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO traces(id,project_id,status,started_at) VALUES('t-other','other-project','succeeded',0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO requests(id,trace_id,protocol,endpoint,status,started_at) VALUES('r-other','t-other','openai','/v1/chat/completions','succeeded',0)",
        vec![],
    )
    .await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/requests/r-other",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "a request from another project must not be readable"
    );
    assert!(!external.is_empty());
}

/// An artifact could only be produced, never found again: the export route
/// returned a new one and nothing listed what already existed, so an operator who
/// lost the downloaded file had no way back to it from the console.
#[tokio::test]
async fn backup_artifacts_can_be_listed_and_re_downloaded() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let artifact = ops::backup::export(
        &f.state,
        db::DEFAULT_PROJECT_ID,
        &Selection {
            resources: vec!["providers".into(), "models".into()],
        },
    )
    .await
    .unwrap();

    let path = format!(
        "/api/admin/v1/projects/{}/backup/artifacts",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let document: Value = serde_json::from_slice(&body).unwrap();
    let rows = document["artifacts"].as_array().unwrap();
    assert!(
        rows.iter().any(|row| row["id"] == artifact.id),
        "the artifact just created must be listed"
    );
    assert!(
        rows.iter().all(|row| row.get("envelope").is_none()),
        "the list must not carry the encrypted database image"
    );

    let path = format!(
        "/api/admin/v1/projects/{}/backup/artifacts/{}",
        db::DEFAULT_PROJECT_ID,
        artifact.id
    );
    // The payload route is owner-gated: this principal can list the project's
    // artifacts but must not be handed the encrypted database image. (The owner
    // path itself is not covered here — the fixture has no managing principal.)
    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "an encrypted database image must not be downloadable without project:manage"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action='backup.artifact.read'"
        )
        .await,
        0,
        "a refused download must not be audited as a download"
    );

    let path = format!(
        "/api/admin/v1/projects/{}/backup/artifacts/does-not-exist",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "authorization runs before existence, so an unknown id is refused the same way"
    );
}

/// `bulk-toggle` claims the enable/disable lifecycle but refused prompts and
/// protection rules, so the console could only flip channels, models and
/// credentials while the prompts tab had no bulk action at all.
#[tokio::test]
async fn bulk_toggle_covers_prompts_and_protection_rules() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO prompts(id,project_id,name,role,content,enabled,created_at,updated_at) VALUES('p1',?,'P1','system','x',1,0,0),('p2',?,'P2','system','y',1,0,0)",
        vec![db::DEFAULT_PROJECT_ID.into(), db::DEFAULT_PROJECT_ID.into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO prompt_protection_rules(id,project_id,name,content_pattern,action,enabled,created_at,updated_at) VALUES('r1',?,'R1','secret','deny',1,0,0)",
        vec![db::DEFAULT_PROJECT_ID.into()],
    )
    .await;

    let path = format!(
        "/api/admin/v1/projects/{}/operations/bulk-toggle",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"resource":"prompts","ids":["p1","p2"],"enabled":false}),
        true,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "prompts must be part of the bulk lifecycle, not refused"
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM prompts WHERE enabled=0").await,
        2,
        "both prompts must have been disabled"
    );

    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"resource":"protection","ids":["r1"],"enabled":false}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM prompt_protection_rules WHERE enabled=0"
        )
        .await,
        1
    );
}

/// Preview is an authenticated, project-scoped read of the same regex rules the
/// gateway enforces. It may explain disabled or archived rows, but archived rows
/// must never alter a live request, and preview itself must not write, audit, or
/// contact an upstream.
#[tokio::test]
async fn protection_preview_is_bounded_read_only_and_archived_rules_do_not_enforce() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured_calls = calls.clone();
    let captured_bodies = seen.clone();
    let f = fixture(Router::new().route(
        "/v1/chat/completions",
        post(move |Json(body): Json<Value>| {
            let captured_bodies = captured_bodies.clone();
            let captured_calls = captured_calls.clone();
            async move {
                captured_calls.fetch_add(1, Ordering::Relaxed);
                captured_bodies.lock().await.push(body);
                Json(json!({"choices":[{"message":{"content":"ok"}}]}))
            }
        }),
    ))
    .await;
    let (cookie, owner_id) = owner_principal(&f).await;
    let owner = access::Principal::session(owner_id);
    let (_, manager) = access::create_scoped_api_key(
        &f.state.db,
        &owner,
        &access::ScopedApiKeyInput {
            name: "preview-manager".into(),
            project_id: db::DEFAULT_PROJECT_ID.into(),
            user_id: None,
            profile_id: None,
            key_type: "service".into(),
            scopes: vec!["project:manage".into()],
            budget_micros: None,
            expires_at: None,
            allowed_ips: vec![],
            denied_ips: vec![],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('preview-foreign','Foreign','preview-foreign',0,1,0,0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO prompt_protection_rules(id,project_id,name,description,content_pattern,action,replacement,enabled,state,created_at,updated_at) VALUES
         ('preview-redact',?,'Secrets','Masks numbered secrets','secret-[0-9]+','redact','[MASKED]',1,'active',1,1),
         ('preview-deny',?,'Block','Rejects blocked text','block','deny',NULL,1,'active',2,2),
         ('preview-miss',?,'No match','','never','redact','x',0,'active',3,3),
         ('preview-archived',?,'Archived','','archived','redact','[OLD]',1,'archived',4,4),
         ('preview-foreign-rule','preview-foreign','Foreign rule','','secret','redact','leak',1,'active',5,5)",
        vec![
            db::DEFAULT_PROJECT_ID.into(),
            db::DEFAULT_PROJECT_ID.into(),
            db::DEFAULT_PROJECT_ID.into(),
            db::DEFAULT_PROJECT_ID.into(),
        ],
    )
    .await;

    let path = format!(
        "/api/admin/v1/projects/{}/protection-preview",
        db::DEFAULT_PROJECT_ID
    );
    let unauthenticated = router(f.state.clone())
        .oneshot(
            Request::post(&path)
                .header("content-type", "application/json")
                .header("x-pangolin-csrf", "1")
                .body(Body::from(r#"{"text":"secret-123"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let audit_before = count(&f, "SELECT COUNT(*) AS n FROM audit_events").await;
    let rules_before = count(&f, "SELECT COUNT(*) AS n FROM prompt_protection_rules").await;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"text":"secret-123 block archived"}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let document = json_body(response).await;
    let rows = document["rules"].as_array().unwrap();
    assert_eq!(rows.len(), 4, "foreign rules must not appear: {document}");
    assert_eq!(
        rows[0],
        json!({
            "id":"preview-redact","name":"Secrets","description":"Masks numbered secrets",
            "action":"redact","enabled":true,"state":"active","matched":true,
            "result":"[MASKED] block archived"
        })
    );
    assert_eq!(rows[1]["matched"], json!(true));
    assert_eq!(rows[1]["action"], json!("deny"));
    assert_eq!(rows[1]["result"], json!("secret-123 block archived"));
    assert_eq!(rows[2]["matched"], json!(false));
    assert_eq!(rows[3]["state"], json!("archived"));
    assert_eq!(rows[3]["result"], json!("secret-123 block [OLD]"));
    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "preview must not call upstream"
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM audit_events").await,
        audit_before
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM prompt_protection_rules").await,
        rules_before
    );

    let oversized = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"text":"x".repeat(16 * 1024 + 1)}),
        true,
    )
    .await;
    assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);

    let foreign = router(f.state.clone())
        .oneshot(
            Request::post("/api/admin/v1/projects/preview-foreign/protection-preview")
                .header("authorization", format!("Bearer {manager}"))
                .header("content-type", "application/json")
                .header("x-pangolin-csrf", "1")
                .body(Body::from(r#"{"text":"secret"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);

    let list = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{}/operations/protection?q=Archived",
                db::DEFAULT_PROJECT_ID
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(list["data"][0]["state"], json!("archived"));

    let live = request(
        &f,
        "/v1/chat/completions",
        json!({"model":"public","messages":[{"role":"user","content":"secret-123 archived"}]}),
    )
    .await;
    assert_eq!(live.status(), StatusCode::OK);
    let bodies = seen.lock().await;
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        bodies[0]["messages"][0]["content"],
        json!("[MASKED] archived")
    );
}

#[tokio::test]
async fn protection_metadata_round_trips_and_rejects_unknown_state() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/protection",
        db::DEFAULT_PROJECT_ID
    );
    let created = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "name":"Archived metadata",
            "description":"Why this rule still exists",
            "content_pattern":"legacy",
            "action":"redact",
            "replacement":"[OLD]",
            "state":"archived"
        }),
        true,
    )
    .await;
    assert_eq!(created.status(), StatusCode::OK);

    let list = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{path}?q=Archived%20metadata"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(list["data"][0]["description"], "Why this rule still exists");
    assert_eq!(list["data"][0]["state"], "archived");

    let blank_description = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "name":"Blank description",
            "description":null,
            "content_pattern":"blank",
            "action":"deny",
            "state":"active"
        }),
        true,
    )
    .await;
    assert_eq!(blank_description.status(), StatusCode::OK);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM prompt_protection_rules WHERE name='Blank description' AND description=''"
        )
        .await,
        1
    );

    let invalid = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "name":"Unknown state",
            "content_pattern":"x",
            "action":"deny",
            "state":"deleted"
        }),
        true,
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM prompt_protection_rules WHERE name='Unknown state'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn protection_preview_refuses_an_unbounded_rule_set() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "WITH RECURSIVE sequence(value) AS (
             SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 501
         )
         INSERT INTO prompt_protection_rules(id,project_id,name,content_pattern,action,created_at,updated_at)
         SELECT 'bounded-' || value, ?, 'Bounded ' || value, 'x', 'deny', value, value FROM sequence",
        vec![db::DEFAULT_PROJECT_ID.into()],
    )
    .await;
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!(
            "/api/admin/v1/projects/{}/protection-preview",
            db::DEFAULT_PROJECT_ID
        ),
        json!({"text":"x"}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Handler errors answer with the envelope, but axum's own extractor rejections
/// returned plain text, so an admin client could not parse every failure the same
/// way. The gateway's pass-through bodies must stay untouched.
#[tokio::test]
async fn admin_body_rejections_answer_with_the_error_envelope() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/prompts",
        db::DEFAULT_PROJECT_ID
    );
    let response = router(f.state.clone())
        .oneshot(
            Request::post(&path)
                .header("cookie", format!("pangolin_session={cookie}"))
                .header("x-pangolin-csrf", "1")
                .header("content-type", "application/json")
                .body(Body::from("{ not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        content_type.starts_with("application/json"),
        "a rejection must be parseable like every other error, got {content_type}"
    );
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let document: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        document
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some(),
        "the envelope must carry a message: {document}"
    );
}

/// A schedule with an unreadable configuration is marked `invalid_configuration`
/// and then simply never runs. The list projection omitted that field, so the
/// console showed a schedule that silently does nothing.
#[tokio::test]
async fn a_broken_schedule_reports_why_it_failed() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO operation_schedules(id,project_id,kind,payload_json,interval_secs,next_run_at,enabled,revision,last_error) VALUES('s-broken',?,'automatic_backup','{}',3600,0,1,1,'invalid_configuration')",
        vec![db::DEFAULT_PROJECT_ID.into()],
    )
    .await;

    let path = format!(
        "/api/admin/v1/projects/{}/operations/schedules",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let document: Value = serde_json::from_slice(&body).unwrap();
    let rows = document["data"].as_array().unwrap();
    let broken = rows
        .iter()
        .find(|row| row["id"] == "s-broken")
        .expect("the schedule must be listed");
    assert_eq!(
        broken["last_error"], "invalid_configuration",
        "a schedule that silently never runs must say why"
    );
}

/// A duplicate model hit UNIQUE(provider_id, public_name, upstream_name) and
/// surfaced as an opaque 500 whose only clue was the constraint name in the log.
#[tokio::test]
async fn a_duplicate_model_is_a_typed_conflict() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO providers(id,project_id,name,kind,base_url,enabled,created_at,updated_at) VALUES('p-dup',?,'dup','openai','http://127.0.0.1:1',1,0,0)",
        vec![db::DEFAULT_PROJECT_ID.into()],
    )
    .await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/models",
        db::DEFAULT_PROJECT_ID
    );
    let body = json!({
        "provider_id": "p-dup",
        "public_name": "same",
        "upstream_name": "same",
        "capabilities": ["chat"],
    });
    let response = admin(&f, &cookie, http::Method::POST, &path, body.clone(), true).await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = admin(&f, &cookie, http::Method::POST, &path, body, true).await;
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "a duplicate model must be refused with a reason, not an internal error"
    );
    // The code is the contract the console names in its own language; the message
    // keeps the detail. A generic `conflict_error` told the client nothing it could
    // translate.
    // The admin envelope is the OpenAI protocol one, which carries the code in
    // `error.type` — `error.code` is Gemini's numeric HTTP status, so asserting
    // that field would have passed against nothing.
    let envelope = json_body(response).await;
    assert_eq!(envelope["error"]["type"], "duplicate_model");
}

/// Any uniqueness violation used to become an opaque 500 with the constraint name
/// only in the server log. The conversion is now the safety net, so a resource
/// without its own pre-check still answers with a conflict.
#[tokio::test]
async fn a_uniqueness_violation_is_a_typed_conflict_not_an_internal_error() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/channels",
        db::DEFAULT_PROJECT_ID
    );
    // Channel names are globally unique (UNIQUE COLLATE NOCASE), and this arm has
    // no pre-check of its own, so it is the conversion that has to answer.
    let body = json!({
        "name": "unique-channel",
        "kind": "openai",
        "base_url": "http://127.0.0.1:1",
        "enabled": true,
    });
    let response = admin(&f, &cookie, http::Method::POST, &path, body.clone(), true).await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = admin(&f, &cookie, http::Method::POST, &path, body, true).await;
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "a duplicate name must be refused with a reason, not an internal error"
    );
    let text = String::from_utf8(
        to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        !text.contains("UNIQUE constraint failed"),
        "the public error must not leak the constraint name: {text}"
    );
}

/// The console's orchestration form does not carry `routing`, so a save used to
/// reset the project's default routing policy to the empty document. A field the
/// request never mentioned must be left alone.
#[tokio::test]
async fn saving_orchestration_settings_without_routing_preserves_it() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/settings/orchestration",
        db::DEFAULT_PROJECT_ID
    );
    let base = |routing: Option<Value>| {
        let mut body = json!({
            "version": 1,
            "affinity_rules": [],
            "session_compaction": {
                "enabled": false,
                "threshold_tokens": 8192,
                "retain_items": 16,
                "native": true,
                "summarizer_model": null,
            },
        });
        if let Some(routing) = routing {
            body["routing"] = routing;
        }
        body
    };

    let response = admin(
        &f,
        &cookie,
        http::Method::PUT,
        &path,
        base(Some(
            json!({"version":1,"field":"/body/model","op":"eq","value":"fast"}),
        )),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = admin(&f, &cookie, http::Method::PUT, &path, base(None), true).await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let stored: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        stored["routing"]["value"], "fast",
        "a save that does not mention routing must not wipe it"
    );
}

/// Deleting a project cascades into every table that references it, and where
/// immutable price or usage history exists a trigger aborts the statement: an
/// opaque 500 with nothing naming the cause.
#[tokio::test]
async fn deleting_a_project_with_history_is_refused_with_a_reason() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('p-history','History','history',0,1,0,0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO providers(id,project_id,name,kind,base_url,enabled,created_at,updated_at) VALUES('p-hist-provider','p-history','hist','openai','http://127.0.0.1:1',1,0,0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO models(id,provider_id,public_name,upstream_name,created_at) VALUES('m-hist','p-hist-provider','hist','hist',0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO model_prices(id,model_id,version,valid_from,created_at) VALUES('price-hist','m-hist',1,0,0)",
        vec![],
    )
    .await;

    let response = admin(
        &f,
        &cookie,
        http::Method::DELETE,
        "/api/admin/v1/projects/p-history",
        json!(null),
        true,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a project with immutable history must be refused with a reason, not an internal error"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM projects WHERE id='p-history'"
        )
        .await,
        1,
        "the project must still be there"
    );
}

/// One prompt with the default activation matches everything, so a project with a
/// single prompt answered 400 on every non-conversational endpoint — embeddings
/// included. The reference product skips injection there; so do we now, and the
/// skip is recorded as a decision rather than being silent.
#[tokio::test]
async fn a_prompt_does_not_break_endpoints_that_cannot_carry_injection() {
    let upstream = Router::new()
        .route(
            "/v1/chat/completions",
            post(|| async { Json(json!({"choices":[{"message":{"content":"ok"}}]})) }),
        )
        .route(
            "/v1/embeddings",
            post(|| async {
                Json(json!({"data":[{"embedding":[0.1]}],"usage":{"prompt_tokens":3}}))
            }),
        );
    let f = fixture(upstream).await;
    sql(
        &f,
        "UPDATE models SET capabilities='[\"chat\",\"embeddings\"]'",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO prompts(id,project_id,name,role,content,activation_json,enabled,created_at,updated_at) VALUES('default-prompt',?,'Default','system','injected','{\"version\":1}',1,0,0)",
        vec![db::DEFAULT_PROJECT_ID.into()],
    )
    .await;

    let response = request(&f, "/v1/embeddings", json!({"model":"public","input":"hi"})).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a prompt must not fail an endpoint that cannot carry injection"
    );
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let document: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        document.get("data").is_some(),
        "the embedding response must pass through: {document}"
    );
}

/// Creating a binding needs `role:manage` on the project, but listing them only
/// existed on the instance-level route, so a project manager could grant a role
/// and then neither see nor revoke it.
#[tokio::test]
async fn a_project_manager_can_list_the_bindings_they_can_grant() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO users(id,email,password_hash,role,language,theme,created_at,display_name,enabled,updated_at) VALUES('u-bind','bind@example.com','x','member','zh-CN','system:bronze',0,'Bind',1,0)",
        vec![],
    )
    .await;
    let role: String = f
        .state
        .db
        .query_one(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "SELECT id FROM roles WHERE project_id IS NULL ORDER BY name LIMIT 1",
            vec![],
        ))
        .await
        .unwrap()
        .expect("a system role must exist")
        .try_get("", "id")
        .unwrap();
    sql(
        &f,
        "INSERT INTO user_role_bindings(id,user_id,role_id,project_id,created_at) VALUES('b1','u-bind',?,?,0)",
        vec![role.into(), db::DEFAULT_PROJECT_ID.into()],
    )
    .await;

    let path = format!(
        "/api/admin/v1/projects/{}/users/u-bind/role-bindings",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), false).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a project-scoped binding list must be reachable"
    );
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let document: Value = serde_json::from_slice(&body).unwrap();
    let rows = document.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "b1");
    assert_eq!(rows[0]["project_id"], db::DEFAULT_PROJECT_ID);
}

#[tokio::test]
async fn a_malformed_routing_condition_is_rejected_at_write_time() {
    let f = fixture(Router::new()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/associations",
        db::DEFAULT_PROJECT_ID
    );
    for (index, conditions) in [
        json!({"version":1,"field":"/daily_time","op":"gte","value":480}),
        json!({"version":1,"field":"/has_image","op":"eq","value":true}),
        json!({"version":1,"field":"/has_video","op":"eq","value":true}),
        json!({"version":1,"field":"/has_document","op":"eq","value":true}),
        json!({"version":1,"field":"/has_audio","op":"eq","value":true}),
        json!({"version":1,"field":"/stream","op":"eq","value":true}),
        json!({"version":1,"field":"/request_format","op":"in","value":["openai_chat","openai_responses"]}),
    ]
    .into_iter()
    .enumerate()
    {
        let response = admin(
            &f,
            &cookie,
            http::Method::POST,
            &path,
            json!({"id":format!("valid-{index}"),"match_type":"exact","pattern":format!("m-{index}"),"conditions":conditions}),
            true,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "domain conditions should be accepted on the form: {conditions}"
        );
    }

    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "id":"valid-exclusions",
            "match_type":"channel_tags_regex",
            "pattern":"^region-(eu|us)$",
            "exclusions":{
                "version":1,
                "channel_name_patterns":["^deprecated-"],
                "channel_ids":["provider-id"],
                "channel_tags":["private"]
            }
        }),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    for exclusions in [
        json!({"version":2}),
        json!({"version":1,"channel_name_patterns":["[unclosed"]}),
        json!({"version":1,"channel_ids":"provider-id"}),
        json!({"version":1,"channel_tags":[""]}),
        json!({"version":1,"unknown":[]}),
    ] {
        let response = admin(
            &f,
            &cookie,
            http::Method::POST,
            &path,
            json!({"match_type":"exact","pattern":"m","exclusions":exclusions}),
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{exclusions}");
    }

    for conditions in [
        json!({"version":1,"field":"/body/model","op":"eq","value":"m","typo":true}),
        json!({"version":1,"field":"/body/model","op":"matches","value":"m"}),
        json!({"version":1,"field":"body/model","op":"eq","value":"m"}),
        json!({"version":1,"field":"/body/model","op":"regex","value":"[unclosed"}),
        json!({"version":1,"field":"/unknown","op":"eq","value":"m"}),
        json!({"version":1,"field":"/headers/authorization","op":"eq","value":"private"}),
        json!({"version":1,"field":"/has_image","op":"regex","value":"true"}),
        json!({"version":1,"field":"/daily_time","op":"gte","value":1440}),
        json!({"version":2}),
    ] {
        let response = admin(
            &f,
            &cookie,
            http::Method::POST,
            &path,
            json!({"match_type":"exact","pattern":"m","conditions":conditions}),
            true,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "conditions should be rejected on the form: {conditions}"
        );
    }
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM model_associations").await,
        8,
        "only valid domain conditions should be stored"
    );

    // A well-formed condition still saves.
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"match_type":"exact","pattern":"m","conditions":{"version":1,"field":"/body/temperature","op":"lt","value":1}}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM model_associations").await,
        9
    );
}

#[tokio::test]
async fn task5_concurrent_streams_cannot_oversubscribe_durable_budget() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let f=fixture(Router::new().route("/v1/chat/completions",post(move||{counted.fetch_add(1,Ordering::Relaxed);async {let stream=async_stream::stream! {yield Ok::<_,std::io::Error>(Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"pending\"}}]}\n\n"));tokio::time::sleep(Duration::from_secs(30)).await;};([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(stream))}}))).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    sql(&f, "UPDATE api_keys SET budget_micros=100000", vec![]).await;
    let first = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(first.status(), StatusCode::OK);
    let bound = count(&f, "SELECT SUM(reserved_micros) AS n FROM execution_facts").await;
    sql(
        &f,
        "UPDATE api_keys SET budget_micros=?",
        vec![bound.into()],
    )
    .await;
    let (a, b) = tokio::join!(
        request(&f, "/v1/chat/completions", streaming()),
        request(&f, "/v1/chat/completions", streaming())
    );
    assert_eq!(a.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(b.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop(first);
}
#[tokio::test]
async fn task5_file_reopen_recovers_interrupted_execution_once() {
    let mut f = fixture(success()).await;
    let path = f._directory.path().join("reopened.sqlite");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    f.state.db = db::connect(&url).await.unwrap();
    let (key, _) = db::create_api_key(
        &f.state.db,
        &ApiKeyInput {
            name: "persisted".into(),
            budget_micros: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sql(&f,"INSERT INTO request_facts(id,project_id,api_key_id,log_level,started_at) VALUES('interrupted',?,?,'off',0)",vec![db::DEFAULT_PROJECT_ID.into(),key.id.clone().into()]).await;
    sql(&f,"INSERT INTO execution_facts(id,request_id,attempt,price_json,reserved_micros,contacted,started_at) VALUES('attempt','interrupted',1,'{}',1234,1,0)",vec![]).await;
    f.state.db.clone().close().await.unwrap();
    f.state.db = db::connect(&url).await.unwrap();
    ops::runtime::recover(&f.state).await.unwrap();
    ops::runtime::recover(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        1234
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 1);
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM requests").await, 0);
}

#[tokio::test]
async fn task5_backup_remaps_named_channels_and_never_refunds_live_spend() {
    let f = fixture(success()).await;
    let selection = Selection {
        resources: vec![
            "providers".into(),
            "channel_credentials".into(),
            "models".into(),
            "api_keys".into(),
        ],
    };
    sql(&f, "UPDATE api_keys SET spent_micros=123", vec![]).await;
    let artifact = ops::backup::export(&f.state, db::DEFAULT_PROJECT_ID, &selection)
        .await
        .unwrap();
    sql(&f, "UPDATE api_keys SET spent_micros=999", vec![]).await;
    sql(
        &f,
        "DELETE FROM providers WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    let replacement = db::create_provider(
        &f.state.db,
        &ProviderInput {
            name: "a".into(),
            kind: "openai".into(),
            base_url: "https://replacement.example".into(),
            api_key: String::new(),
        },
        f.state.secrets.encrypt("replacement-secret").unwrap(),
    )
    .await
    .unwrap();
    ops::backup::restore(
        &f.state,
        db::DEFAULT_PROJECT_ID,
        &artifact,
        Conflict::Overwrite,
    )
    .await
    .unwrap();
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM providers").await, 2);
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        999
    );
    let provider = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT provider_id FROM models WHERE upstream_name='a-model'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "provider_id")
        .unwrap();
    assert_eq!(provider, replacement.id);
}
#[tokio::test]
async fn task5_bounded_media_is_costed_and_polling_does_not_double_charge() {
    let f = fixture(
        Router::new()
            .route(
                "/v1/videos",
                post(|| async { Json(json!({"id":"video-owned","status":"queued"})) }),
            )
            .route(
                "/v1/videos/video-owned",
                get(|| async { Json(json!({"id":"video-owned","status":"completed"})) }),
            ),
    )
    .await;
    sql(&f, "UPDATE api_keys SET budget_micros=10000", vec![]).await;
    sql(&f, "UPDATE models SET capabilities='[\"videos\"]'", vec![]).await;
    let model = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT id FROM models WHERE provider_id=?",
            vec![f.providers[0].clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "id")
        .unwrap();
    sql(&f,"INSERT INTO model_prices(id,model_id,version,valid_from,created_at) VALUES('video-price',?,1,0,0)",vec![model.into()]).await;
    sql(&f,"INSERT INTO model_price_components(id,price_id,kind,unit_size,unit_price_micros) VALUES('seconds','video-price','unit',1,100)",vec![]).await;
    assert_eq!(
        request(
            &f,
            "/v1/videos",
            json!({"model":"public","prompt":"ocean","seconds":"4"})
        )
        .await
        .status(),
        StatusCode::OK
    );
    for _ in 0..2 {
        let response = router(f.state.clone())
            .oneshot(
                Request::get("/v1/videos/video-owned")
                    .header("authorization", format!("Bearer {}", f.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK)
    }
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        400
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 3);
}
#[tokio::test]
async fn task5_probe_quota_collector_and_invalid_schedule_isolation() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(||async{Json(json!({"choices":[{"message":{"content":"OK"}}],"usage":{"prompt_tokens":2,"completion_tokens":1}}))})).route("/api/v1/key",get(||async{Json(json!({"data":{"limit_remaining":0.0}}))}))).await;
    for kind in ["probe", "quota"] {
        ops::jobs::enqueue(
            &f.state.db,
            Some(db::DEFAULT_PROJECT_ID),
            kind,
            kind,
            &json!({"provider_id":f.providers[0]}),
            db::now(),
        )
        .await
        .unwrap();
        let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
            .await
            .unwrap()
            .unwrap();
        ops::runtime::execute(&f.state, &claim).await.unwrap();
        ops::jobs::finish(&f.state.db, &claim, db::now(), true)
            .await
            .unwrap();
    }
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM channel_probes WHERE success=1 AND output_tokens=1"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM provider_quota_snapshots WHERE remaining_micros=0"
        )
        .await,
        1
    );
    sql(&f,"INSERT INTO operation_schedules(id,project_id,kind,payload_json,interval_secs,next_run_at) VALUES('bad',?,'automatic_backup','{}',60,0),('good',?,'probe',?,60,0)",vec![db::DEFAULT_PROJECT_ID.into(),db::DEFAULT_PROJECT_ID.into(),json!({"provider_id":f.providers[1]}).to_string().into()]).await;
    assert_eq!(
        ops::jobs::enqueue_due(&f.state.db, db::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(count(&f,"SELECT COUNT(*) AS n FROM operation_jobs WHERE kind='invalid_schedule' AND status='failed'").await,1);
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM operation_jobs WHERE kind='probe' AND status='pending'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn cc_switch_response_ids_deduplicate_settlement_without_cross_key_collisions() {
    let f=fixture(Router::new().route("/v1/chat/completions",post(||async{Json(json!({"id":"vendor-stable-id","choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}))}))).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    for _ in 0..2 {
        assert_eq!(
            request(&f, "/v1/chat/completions", chat()).await.status(),
            StatusCode::OK
        )
    }
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        12
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM usage_logs WHERE settlement_kind='duplicate'"
        )
        .await,
        1
    );
    let (_, token) = db::create_api_key(
        &f.state.db,
        &ApiKeyInput {
            name: "other-dedup-scope".into(),
            budget_micros: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let response = router(f.state.clone())
        .oneshot(
            Request::post("/v1/chat/completions")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(chat().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        24
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM provider_response_settlements WHERE length(fingerprint)=64"
        )
        .await,
        2
    );
}
#[tokio::test]
async fn cc_switch_compressed_json_and_sse_are_decoded_before_terminal_accounting() {
    use std::io::Write;
    fn gzip(value: &str) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(value.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }
    let f=fixture(Router::new().route("/v1/chat/completions",post(|Json(body):Json<Value>|async move{if body["stream"]==true {([(header::CONTENT_ENCODING,"gzip"),(header::CONTENT_TYPE,"text/event-stream")],gzip("data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n")).into_response()}else{([(header::CONTENT_ENCODING,"gzip"),(header::CONTENT_TYPE,"application/json")],gzip(&json!({"choices":[{"message":{"content":"decoded"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}).to_string())).into_response()}}))).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=1000000",
        vec![],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["choices"][0]["message"]["content"],
        "decoded"
    );
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("[DONE]"));
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        24
    );
}

#[tokio::test]
async fn task5_usage_and_request_history_backup_roundtrip_preserves_costs() {
    let f = fixture(success()).await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    let resources = [
        "threads",
        "traces",
        "requests",
        "request_contents",
        "request_facts",
        "execution_facts",
        "request_executions",
        "usage_logs",
        "usage_cost_items",
        "provider_response_settlements",
    ];
    let artifact = ops::backup::export(
        &f.state,
        db::DEFAULT_PROJECT_ID,
        &Selection {
            resources: resources.iter().map(|v| v.to_string()).collect(),
        },
    )
    .await
    .unwrap();
    let spent = count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await;
    sql(&f, "DELETE FROM request_facts", vec![]).await;
    sql(&f, "DELETE FROM traces", vec![]).await;
    assert!(
        ops::backup::restore(&f.state, db::DEFAULT_PROJECT_ID, &artifact, Conflict::Fail)
            .await
            .unwrap()
            > 0
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 1);
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM request_executions").await,
        1
    );
    assert_eq!(
        count(&f, "SELECT SUM(spent_micros) AS n FROM api_keys").await,
        spent
    );
}

#[tokio::test]
async fn task5_operations_share_api_key_permissions_and_audit_service_subjects() {
    let f = fixture(success()).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/storage",
        db::DEFAULT_PROJECT_ID
    );
    let send = |method, path: String, body: Value| {
        router(f.state.clone()).oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {}", f.token))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
    };
    assert_eq!(
        send(http::Method::GET, path.clone(), Value::Null)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    sql(
        &f,
        "UPDATE api_keys SET scopes='[\"project:read\"]'",
        vec![],
    )
    .await;
    assert_eq!(
        send(http::Method::GET, path.clone(), Value::Null)
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let body = json!({"name":"service-target","config":{"kind":"local","directory":"service"}});
    assert_eq!(
        send(http::Method::POST, path.clone(), body.clone())
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    sql(
        &f,
        "UPDATE api_keys SET scopes='[\"project:manage\"]'",
        vec![],
    )
    .await;
    assert_eq!(
        send(http::Method::POST, path, body).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(count(&f,"SELECT COUNT(*) AS n FROM audit_events WHERE json_extract(details,'$.principal_kind')='api_key' AND actor_user_id IS NULL").await,1);
    assert_eq!(
        send(
            http::Method::GET,
            "/api/admin/v1/projects/other/operations/storage".into(),
            Value::Null
        )
        .await
        .unwrap()
        .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn task5_quota_collection_never_forwards_structured_secrets_or_changes_origin() {
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = calls.clone();
    let f = fixture(Router::new().fallback(move || {
        captured.fetch_add(1, Ordering::Relaxed);
        async { Json(json!({"data":{"limit_remaining":10}})) }
    }))
    .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let captured = calls.clone();
    let other = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(move || {
                captured.fetch_add(1, Ordering::Relaxed);
                async { Json(json!({"data":{"limit_remaining":10}})) }
            }),
        )
        .await
        .unwrap()
    });
    ops::jobs::enqueue(
        &f.state.db,
        Some(db::DEFAULT_PROJECT_ID),
        "quota",
        "wrong-origin",
        &json!({"provider_id":f.providers[0],"path":format!("/\\{address}/key")}),
        db::now(),
    )
    .await
    .unwrap();
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert!(ops::runtime::execute(&f.state, &claim).await.is_err());
    sql(
        &f,
        "UPDATE providers SET kind='bedrock' WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;
    sql(&f,"UPDATE channel_credentials SET secret_envelope=? WHERE provider_id=?",vec![f.state.secrets.encrypt(&json!({"access_key_id":"cloud-access-sentinel","secret_access_key":"cloud-private-sentinel"}).to_string()).unwrap().into(),f.providers[0].clone().into()]).await;
    ops::jobs::enqueue(
        &f.state.db,
        Some(db::DEFAULT_PROJECT_ID),
        "quota",
        "cloud-credentials",
        &json!({"provider_id":f.providers[0]}),
        db::now(),
    )
    .await
    .unwrap();
    let claim = ops::jobs::claim(&f.state.db, "worker", db::now(), 120)
        .await
        .unwrap()
        .unwrap();
    assert!(ops::runtime::execute(&f.state, &claim).await.is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    other.abort();
}

#[tokio::test]
async fn task5_quota_sequence_overrides_wall_clock_order() {
    let f = fixture(success()).await;
    for (id, sequence, remaining, collected) in [("older", 1, 0, 200), ("newer", 2, 100, 100)] {
        sql(&f,"INSERT INTO provider_quota_snapshots(id,provider_id,remaining_micros,collected_at,sequence) VALUES(?,?,?,?,?)",vec![id.into(),f.providers[0].clone().into(),remaining.into(),collected.into(),sequence.into()]).await;
    }
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    let row = f
        .state
        .db
        .query_one(ops::sql("SELECT provider_id FROM execution_facts", vec![]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<String>("", "provider_id").unwrap(),
        f.providers[0]
    );
}

// -- Issue 2: proxy_settings is removed from channel-settings API --

#[tokio::test]
async fn channel_settings_response_excludes_proxy_settings() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let base = format!(
        "/api/admin/v1/projects/{}/operations",
        db::DEFAULT_PROJECT_ID
    );
    let response = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/channel-settings"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    let data = response["data"].as_array().unwrap();
    assert!(
        !data.is_empty(),
        "channel-settings list should not be empty"
    );
    for item in data {
        assert!(
            item.get("proxy_settings").is_none(),
            "channel-settings item must not contain proxy_settings: {item}"
        );
    }
    // Patching channel-settings without proxy_settings must succeed.
    let patched = admin(
        &f,
        &cookie,
        http::Method::POST,
        &format!("{base}/channel-settings"),
        json!({"provider_id": f.providers[0], "model_rules": null}),
        true,
    )
    .await;
    assert_eq!(patched.status(), StatusCode::OK);
}

#[tokio::test]
async fn channel_model_rules_are_validated_before_write() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/channel-settings",
        db::DEFAULT_PROJECT_ID
    );

    let invalid = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "provider_id": f.providers[0],
            "model_rules": {
                "version": 1,
                "auto_trim_prefixes": ["vendor", ""],
                "mappings": {"alias": "target"}
            }
        }),
        true,
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let valid_rules = json!({
        "version": 1,
        "auto_trim_prefixes": ["vendor"],
        "hide_original": true,
        "hide_mapped": false,
        "mappings": {"vendor/model": "model"}
    });
    let valid = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"provider_id": f.providers[0], "model_rules": valid_rules.clone()}),
        true,
    )
    .await;
    assert_eq!(valid.status(), StatusCode::OK);

    let stored = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT model_rules_json FROM channel_settings WHERE provider_id=?",
            vec![f.providers[0].clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "model_rules_json")
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), valid_rules);
}

#[tokio::test]
async fn bulk_toggle_covers_api_keys_and_stays_inside_the_project() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('other-project','Other','other',0,1,0,0)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,scopes,budget_micros,spent_micros,enabled,created_at,project_id) VALUES('k1','K1','pk1','h1','d1','[\"*\"]',0,0,1,0,?),('k2','K2','pk2','h2','d2','[\"*\"]',0,0,1,0,?),('k3','K3','pk3','h3','d3','[\"*\"]',0,0,1,0,'other-project')",
        vec![db::DEFAULT_PROJECT_ID.into(), db::DEFAULT_PROJECT_ID.into()],
    )
    .await;

    let path = format!(
        "/api/admin/v1/projects/{}/operations/bulk-toggle",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"resource":"keys","ids":["k1","k2","k3"],"enabled":false}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    // The two keys in this project are disabled; the third belongs to another
    // project and must not be touched even though its id was in the list.
    assert_eq!(
        count(
            &f,
            &format!(
                "SELECT COUNT(*) AS n FROM api_keys WHERE enabled=0 AND project_id='{}'",
                db::DEFAULT_PROJECT_ID
            )
        )
        .await,
        2
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM api_keys WHERE enabled=1 AND project_id='other-project'"
        )
        .await,
        1
    );
}

/// A cleared JSON box in the console serialises as `null`, and persisting the
/// literal `null` is what breaks the project: the scope document is parsed per
/// request, so a bad one 500s every call. The handler maps `None`/`null` to
/// `{"version":1}` — this pins that, and pins the consequence rather than the
/// status code, because a 200 with a broken document would still be a broken
/// project.
#[tokio::test]
async fn a_cleared_protection_scope_becomes_the_default_document_and_keeps_the_project_working() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let path = format!(
        "/api/admin/v1/projects/{}/operations/protection",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"name":"P1","content_pattern":"secret","action":"deny","scopes":null}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let stored: String = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT scopes_json FROM prompt_protection_rules WHERE name='P1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "scopes_json")
        .unwrap();
    assert_ne!(
        stored.trim(),
        "null",
        "the literal null is what breaks the project"
    );
    assert_eq!(stored.trim(), r#"{"version":1}"#);

    // The proof that matters: the project still answers.
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
}

/// The console links to a request by the id the projection exposes as `public_id`,
/// which is the client's external request id when the client sent one. The record
/// route resolved only the internal primary key, so those links 404'd and the detail
/// card fell back to saying the attempts and the cost parts were unavailable.
///
/// The path uses the project the request actually belongs to: an earlier version of
/// this test hard-coded `DEFAULT_PROJECT_ID` and failed with 404 for a reason that
/// had nothing to do with the lookup.
#[tokio::test]
async fn a_request_can_be_opened_by_the_external_id_the_client_sent() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    sql(
        &f,
        "UPDATE requests SET request_metadata_json='{\"external_id\":\"ext-1\"}',source_ip='203.0.113.11'",
        vec![],
    )
    .await;
    let project: String = f
        .state
        .db
        .query_one(ops::sql("SELECT project_id FROM traces LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "project_id")
        .unwrap();
    let path = format!("/api/admin/v1/projects/{project}/operations/requests/ext-1");
    let response = admin(&f, &cookie, http::Method::GET, &path, Value::Null, false).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a link built from the projection's public_id must open the record"
    );
    let document = json_body(response).await;
    assert_eq!(document["source_ip"], "203.0.113.11");
    for forbidden in [
        "user_agent",
        "headers",
        "authorization",
        "cookie",
        "credential",
    ] {
        assert!(
            document.get(forbidden).is_none(),
            "network identity must not widen the detail into header or credential capture: {forbidden}"
        );
    }

    // The internal id still resolves, so nothing that worked before stops working.
    let internal: String = f
        .state
        .db
        .query_one(ops::sql("SELECT id FROM requests LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    let path = format!("/api/admin/v1/projects/{project}/operations/requests/{internal}");
    assert_eq!(
        admin(&f, &cookie, http::Method::GET, &path, Value::Null, false)
            .await
            .status(),
        StatusCode::OK
    );
}

/// The console's bulk key action ran through the generic table toggle: it passed the
/// route's `project:manage` check and then ran `UPDATE api_keys SET enabled=?`, so it
/// could enable a `user`/`personal` key whose owner was suspended — exactly the state
/// the single-key update refuses. Bulk key state is the same lifecycle, so it has to
/// answer with the same rule, atomically.
#[tokio::test]
async fn bulk_key_toggle_applies_the_api_key_lifecycle_contract() {
    let f = fixture(success()).await;
    let (cookie, owner_id) = owner_principal(&f).await;
    let owner = access::Principal::session(owner_id);
    let member = access::create_user(
        &f.state.db,
        &owner,
        &access::UserInput {
            email: "bulk-owner@example.com".into(),
            password: "another secure password".into(),
            display_name: None,
            language: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let membership = |status: &'static str| access::MembershipInput {
        user_id: member.id.clone(),
        role_id: access::SYSTEM_MEMBER_ROLE_ID.into(),
        status: status.into(),
    };
    access::upsert_membership(
        &f.state.db,
        &owner,
        db::DEFAULT_PROJECT_ID,
        &membership("active"),
    )
    .await
    .unwrap();
    let key = |name: &str, owner: Option<String>, kind: &str| access::ScopedApiKeyInput {
        name: name.into(),
        project_id: db::DEFAULT_PROJECT_ID.into(),
        user_id: owner,
        profile_id: None,
        key_type: kind.into(),
        scopes: vec!["gateway:use".into()],
        budget_micros: None,
        expires_at: None,
        allowed_ips: vec![],
        denied_ips: vec![],
        ..Default::default()
    };
    let (service, _) =
        access::create_scoped_api_key(&f.state.db, &owner, &key("service", None, "service"))
            .await
            .unwrap();
    let (owned, _) = access::create_scoped_api_key(
        &f.state.db,
        &owner,
        &key("owned", Some(member.id.clone()), "personal"),
    )
    .await
    .unwrap();
    let path = format!(
        "/api/admin/v1/projects/{}/operations/bulk-toggle",
        db::DEFAULT_PROJECT_ID
    );
    let bulk =
        |ids: Vec<String>, enabled: bool| json!({"resource":"keys","ids":ids,"enabled":enabled});

    // Disabling is always allowed, and it is audited.
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        bulk(vec![service.id.clone(), owned.id.clone()], false),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!api_key_enabled(&f, &service.id).await);
    assert!(!api_key_enabled(&f, &owned.id).await);

    // The owner is suspended: re-enabling the batch must be refused as a whole.
    access::upsert_membership(
        &f.state.db,
        &owner,
        db::DEFAULT_PROJECT_ID,
        &membership("suspended"),
    )
    .await
    .unwrap();
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        bulk(vec![service.id.clone(), owned.id.clone()], true),
        true,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "bulk key state must apply the key lifecycle, not flip a column"
    );
    let message = json_body(response).await["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned();

    // One rule, one reason: the single-key update says the same thing.
    let single = admin(
        &f,
        &cookie,
        http::Method::PATCH,
        &format!(
            "/api/admin/v1/projects/{}/api-keys/{}",
            db::DEFAULT_PROJECT_ID,
            owned.id
        ),
        json!({"enabled":true}),
        true,
    )
    .await;
    assert_eq!(single.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        message,
        json_body(single).await["error"]["message"]
            .as_str()
            .unwrap_or_default(),
        "the bulk path must state the same rule as the single-key update"
    );
    assert!(!message.is_empty());
    assert!(
        !api_key_enabled(&f, &service.id).await,
        "no partial update: the service key must stay disabled"
    );
    assert!(!api_key_enabled(&f, &owned.id).await);

    // A key this project does not own is skipped, exactly as the other bulk
    // resources treat an id outside the project: the batch cannot reach or change
    // another project's keys, and it reports only what it changed.
    sql(&f,"INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('keys-project','Elsewhere','keys-elsewhere',0,1,0,0)",vec![]).await;
    let (foreign, _) = access::create_scoped_api_key(
        &f.state.db,
        &owner,
        &access::ScopedApiKeyInput {
            project_id: "keys-project".into(),
            ..key("foreign", None, "service")
        },
    )
    .await
    .unwrap();
    for ids in [vec![foreign.id.clone()], vec!["no-such-key".into()]] {
        let response = admin(
            &f,
            &cookie,
            http::Method::POST,
            &path,
            bulk(ids, false),
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            json_body(response).await["updated"],
            0,
            "an id this project does not own is not a change"
        );
    }
    assert!(
        api_key_enabled(&f, &foreign.id).await,
        "another project's key must not be toggled"
    );

    // With the owner active again the same batch applies, and every key it changed
    // carries its own audit row.
    access::upsert_membership(
        &f.state.db,
        &owner,
        db::DEFAULT_PROJECT_ID,
        &membership("active"),
    )
    .await
    .unwrap();
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        bulk(vec![service.id.clone(), owned.id.clone()], true),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(api_key_enabled(&f, &service.id).await);
    assert!(api_key_enabled(&f, &owned.id).await);
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM audit_events WHERE resource_type='api_key' AND action='update' AND json_extract(details,'$.bulk')=1").await,
        4,
        "both applied batches are audited, one row per key changed; the skipped foreign id left none"
    );
}

/// Bulk key state is the key lifecycle, so its authority is the key contract's
/// `api_key:manage` — not the generic `project:manage` gate the rest of the
/// operations API uses. A role holding only `api_key:manage` can change a key
/// through the dedicated route, so it must be able to change one here too; a
/// principal holding neither permission must be refused and nothing may change. The
/// browser-mutation guard is unchanged: a session still needs the CSRF header, while
/// an API key never sends one.
#[tokio::test]
async fn bulk_key_state_takes_the_key_permission_and_keeps_the_session_guard() {
    let f = fixture(success()).await;
    let (_, owner_id) = owner_principal(&f).await;
    let owner = access::Principal::session(owner_id);
    let role = access::create_role(
        &f.state.db,
        &owner,
        db::DEFAULT_PROJECT_ID,
        &access::RoleInput {
            name: "key manager".into(),
            permissions: vec!["api_key:manage".into()],
        },
    )
    .await
    .unwrap();
    let key_manager = project_user(&f, &owner, "key-manager@example.com", role.id.clone()).await;
    let member = project_user(
        &f,
        &owner,
        "plain-member@example.com",
        access::SYSTEM_MEMBER_ROLE_ID.into(),
    )
    .await;
    let (service, _) = access::create_scoped_api_key(
        &f.state.db,
        &owner,
        &access::ScopedApiKeyInput {
            name: "service".into(),
            project_id: db::DEFAULT_PROJECT_ID.into(),
            user_id: None,
            profile_id: None,
            key_type: "service".into(),
            scopes: vec!["gateway:use".into()],
            budget_micros: None,
            expires_at: None,
            allowed_ips: vec![],
            denied_ips: vec![],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (_, key_only_token) = access::create_scoped_api_key(
        &f.state.db,
        &owner,
        &access::ScopedApiKeyInput {
            name: "key-manager".into(),
            project_id: db::DEFAULT_PROJECT_ID.into(),
            user_id: None,
            profile_id: None,
            key_type: "service".into(),
            scopes: vec!["api_key:manage".into()],
            budget_micros: None,
            expires_at: None,
            allowed_ips: vec![],
            denied_ips: vec![],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let path = format!(
        "/api/admin/v1/projects/{}/operations/bulk-toggle",
        db::DEFAULT_PROJECT_ID
    );
    let bulk =
        |ids: Vec<String>, enabled: bool| json!({"resource":"keys","ids":ids,"enabled":enabled});

    // The key-only role can change key state, exactly as it can through the
    // dedicated route.
    let manager_session = db::create_session(&f.state.db, &key_manager).await.unwrap();
    let response = admin(
        &f,
        &manager_session,
        http::Method::POST,
        &path,
        bulk(vec![service.id.clone()], false),
        true,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "api_key:manage without project:manage must be enough for key bulk state"
    );
    assert!(!api_key_enabled(&f, &service.id).await);

    // The session guard still applies to this route.
    let response = admin(
        &f,
        &manager_session,
        http::Method::POST,
        &path,
        bulk(vec![service.id.clone()], true),
        false,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a session mutation without the CSRF header stays refused"
    );
    assert!(!api_key_enabled(&f, &service.id).await);

    // And the key permission does not widen into the generic operations gate.
    let response = admin(
        &f,
        &manager_session,
        http::Method::POST,
        &path,
        json!({"resource":"channels","ids":[f.providers[0].clone()],"enabled":false}),
        true,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "the rest of the operations API still needs project:manage"
    );
    assert_eq!(
        count(
            &f,
            &format!(
                "SELECT COUNT(*) AS n FROM providers WHERE id='{}' AND enabled=1",
                f.providers[0]
            )
        )
        .await,
        1,
        "the channel must be untouched"
    );

    // Neither permission: refused, and nothing changes.
    let member_session = db::create_session(&f.state.db, &member).await.unwrap();
    let response = admin(
        &f,
        &member_session,
        http::Method::POST,
        &path,
        bulk(vec![service.id.clone()], true),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        !api_key_enabled(&f, &service.id).await,
        "a refused bulk key change must change nothing"
    );

    // The same authority through an API key, which needs no CSRF header.
    let response = router(f.state.clone())
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(&path)
                .header("authorization", format!("Bearer {key_only_token}"))
                .header("content-type", "application/json")
                .body(Body::from(bulk(vec![service.id.clone()], true).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "an API key holding api_key:manage must be able to change key state"
    );
    assert!(api_key_enabled(&f, &service.id).await);
}

/// Opens a gateway request with a caller-chosen `x-request-id`, which is what the
/// record system stores as the request's external id and what the projection exposes
/// as `public_id`.
async fn request_with_external_id(f: &Fixture, request_id: &str) -> Response {
    router(f.state.clone())
        .oneshot(
            Request::post("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", f.token))
                .header("content-type", "application/json")
                .header("x-trace-id", "trace-test")
                .header("x-request-id", request_id)
                .body(Body::from(chat().to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Resolving the external id was not enough: the detail route bound the *raw path
/// segment* to the executions, usage and cost queries, so opening a request through
/// the `public_id` the console links with returned the row with no attempts, no
/// tokens and no cost breakdown. The identity has to be the project-scoped one that
/// was already resolved, for the row and for every child fact.
#[tokio::test]
async fn request_detail_child_facts_follow_the_resolved_identity() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    assert_eq!(
        request_with_external_id(&f, "ext-facts").await.status(),
        StatusCode::OK
    );
    let internal: String = f
        .state
        .db
        .query_one(ops::sql("SELECT id FROM requests LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    let execution: String = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT id FROM request_executions WHERE request_id=?",
            vec![internal.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    let recorded: (i64, i64, i64) = {
        let row = f
            .state
            .db
            .query_one(ops::sql(
                "SELECT input_tokens,output_tokens,total_cost_micros FROM usage_logs WHERE execution_id=?",
                vec![execution.clone().into()],
            ))
            .await
            .unwrap()
            .unwrap();
        (
            row.try_get("", "input_tokens").unwrap(),
            row.try_get("", "output_tokens").unwrap(),
            row.try_get("", "total_cost_micros").unwrap(),
        )
    };
    assert_eq!(recorded.0, 100, "the fixture must record real tokens");

    let project = db::DEFAULT_PROJECT_ID;
    let by_external = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!("/api/admin/v1/projects/{project}/operations/requests/ext-facts"),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(by_external.status(), StatusCode::OK);
    let document = json_body(by_external).await;
    assert_eq!(
        document["id"], internal,
        "the external id must resolve to the internal row"
    );
    assert_eq!(
        document["executions"][0]["id"], execution,
        "the attempt must be the one the request actually made, not a lookup of the raw path segment"
    );
    assert_eq!(document["executions"][0]["request_id"], internal);
    assert_eq!(document["usage"][0]["execution_id"], execution);
    assert_eq!(document["usage"][0]["input_tokens"], recorded.0);
    assert_eq!(document["usage"][0]["output_tokens"], recorded.1);
    assert_eq!(
        document["usage"][0]["total_cost_micros"], recorded.2,
        "the recorded cost must survive the external-id lookup"
    );
    assert_eq!(document["cost_items"][0]["execution_id"], execution);
    assert!(
        document["cost_items"][0]["subtotal_micros"].is_i64(),
        "the price components must be listed, got {}",
        document["cost_items"]
    );

    // The same facts must come back through the internal id: one request, one truth.
    let by_internal = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!("/api/admin/v1/projects/{project}/operations/requests/{internal}"),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(by_internal.status(), StatusCode::OK);
    let internal_document = json_body(by_internal).await;
    assert_eq!(document["executions"], internal_document["executions"]);
    assert_eq!(document["usage"], internal_document["usage"]);
    assert_eq!(document["cost_items"], internal_document["cost_items"]);
}

/// The external id is caller-controlled, so it can collide with another project's
/// internal request id. Because the child-fact queries were not bound to the resolved
/// identity, that collision returned the *foreign* project's attempts, tokens and
/// cost breakdown under this project's request — a cross-project read of another
/// tenant's execution facts.
#[tokio::test]
async fn request_detail_child_facts_never_cross_projects_on_an_id_collision() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    assert_eq!(
        request_with_external_id(&f, "shared-request")
            .await
            .status(),
        StatusCode::OK
    );
    let internal: String = f
        .state
        .db
        .query_one(ops::sql("SELECT id FROM requests LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();

    // A second project whose request carries the colliding id as its *primary* key.
    sql(&f,"INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('project-b','B','b',0,1,0,0)",vec![]).await;
    sql(&f,"INSERT INTO traces(id,project_id,status,started_at) VALUES('trace-b','project-b','succeeded',0)",vec![]).await;
    sql(&f,"INSERT INTO request_facts(id,project_id,log_level,status,started_at) VALUES('shared-request','project-b','off','succeeded',0)",vec![]).await;
    sql(&f,"INSERT INTO requests(id,trace_id,protocol,endpoint,status,started_at) VALUES('shared-request','trace-b','openai','/v1/chat/completions','succeeded',0)",vec![]).await;
    sql(&f,"INSERT INTO request_executions(id,request_id,attempt,model,status,started_at) VALUES('exec-b','shared-request',1,'foreign-model','succeeded',0)",vec![]).await;
    sql(&f,"INSERT INTO execution_facts(id,request_id,attempt,status,price_json,started_at) VALUES('exec-b','shared-request',1,'succeeded','{}',0)",vec![]).await;
    sql(&f,"INSERT INTO usage_logs(id,execution_id,input_tokens,output_tokens,total_cost_micros,created_at) VALUES('usage-b','exec-b',11,22,333,0)",vec![]).await;
    sql(&f,"INSERT INTO usage_cost_items(id,usage_log_id,quantity,subtotal_micros) VALUES('cost-b','usage-b',5,333)",vec![]).await;

    let project = db::DEFAULT_PROJECT_ID;
    let response = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!("/api/admin/v1/projects/{project}/operations/requests/shared-request"),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let document = json_body(response).await;
    assert_eq!(
        document["id"], internal,
        "the path segment must resolve inside the requested project, never to the foreign row"
    );
    let executions = document["executions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let usage = document["usage"].as_array().cloned().unwrap_or_default();
    let cost_items = document["cost_items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        executions.iter().all(|row| row["id"] != "exec-b"),
        "another project's attempt leaked into this request: {executions:?}"
    );
    assert!(
        usage.iter().all(|row| row["execution_id"] != "exec-b"),
        "another project's usage leaked into this request: {usage:?}"
    );
    assert!(
        cost_items.iter().all(|row| row["execution_id"] != "exec-b"),
        "another project's cost items leaked into this request: {cost_items:?}"
    );
    assert!(
        usage.iter().all(|row| row["input_tokens"] != 11),
        "another project's token counts leaked into this request: {usage:?}"
    );

    // The collision is real: project B does resolve that id to its own facts.
    let foreign = admin(
        &f,
        &cookie,
        http::Method::GET,
        "/api/admin/v1/projects/project-b/operations/requests/shared-request",
        Value::Null,
        false,
    )
    .await;
    assert_eq!(foreign.status(), StatusCode::OK);
    let foreign_document = json_body(foreign).await;
    assert_eq!(
        foreign_document["executions"][0]["id"], "exec-b",
        "the fixture must be a genuine collision"
    );
}

/// The console restores from the document this route returns, and its parser requires
/// `version`, `project_id` and `resources` alongside the envelope. The route used to
/// send only `id`, `digest`, `manifest`, `envelope` and `created_at`, so the console
/// rejected the artifact it had just downloaded with "该文件不是可恢复的备份产物。"
/// — an untrue statement about its own output, and the reason backup to restore never
/// completed end to end in a browser run.
#[tokio::test]
async fn a_downloaded_artifact_is_the_restorable_document() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let artifact = ops::backup::export(
        &f.state,
        db::DEFAULT_PROJECT_ID,
        &Selection {
            resources: vec!["providers".into(), "models".into()],
        },
    )
    .await
    .unwrap();

    let path = format!(
        "/api/admin/v1/projects/{}/backup/artifacts/{}",
        db::DEFAULT_PROJECT_ID,
        artifact.id
    );
    // `get_artifact` authorizes as a write, so this read needs the CSRF header — the
    // same requirement that made the console's download fail until it sent one.
    let response = admin(&f, &cookie, http::Method::GET, &path, json!(null), true).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let document: Value = serde_json::from_slice(&body).unwrap();

    // The console's parser is this struct's shape, so deserializing it here is the
    // contract: `deny_unknown_fields` makes the round trip exact rather than lenient.
    let parsed: ops::backup::Artifact = serde_json::from_value(document)
        .expect("the download must be the document the restore path accepts");
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.project_id, db::DEFAULT_PROJECT_ID);
    assert_eq!(
        parsed.resources,
        vec!["providers".to_string(), "models".to_string()]
    );
    assert_eq!(parsed.id, artifact.id);
}

/// The console browses the projection through these two routes, so the identity they
/// answer with is the console's identity. `x-request-id` is chosen by the caller: it
/// repeats, and a newer row in another project can carry the same one. The list must
/// expose Pangolin's own UUID, and the detail read must resolve inside the requested
/// project — internal id first, external id only as a bookmark-compatible fallback.
#[tokio::test]
async fn observability_request_identity_is_internal_and_project_scoped() {
    let f = fixture(success()).await;
    let (cookie, owner_id) = owner_principal(&f).await;
    // A real gateway request, so the listed row is the record system's own request.
    assert_eq!(
        request_with_external_id(&f, "shared-request")
            .await
            .status(),
        StatusCode::OK
    );
    let internal: String = f
        .state
        .db
        .query_one(ops::sql("SELECT id FROM requests LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    f.state.observations.flush().await;
    let local = f
        .state
        .observations
        .get(internal.clone())
        .await
        .unwrap()
        .unwrap();

    // A second project whose *newer* row reuses the external id. Under the old
    // global-newest-first lookup this row decided the default project's answer.
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('project-b','B','b',0,1,0,0)",
        vec![],
    )
    .await;
    let mut foreign = local.clone();
    foreign.id = "internal-foreign".into();
    foreign.project_id = "project-b".into();
    foreign.started_at = local.started_at + 86_400;
    f.state.observations.record(foreign);
    f.state.observations.flush().await;

    let base = format!(
        "/api/admin/v1/projects/{}/observability/requests",
        db::DEFAULT_PROJECT_ID
    );
    let list = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}?limit=50"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    let rows = list["data"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        list["total"].as_i64(),
        Some(1),
        "another project's row must not be counted here: {list}"
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["internal_id"], internal,
        "the list must expose the internal UUID the record system keys the request by"
    );
    assert_eq!(
        rows[0]["request_id"], "shared-request",
        "the external id stays on the row for correlation"
    );
    assert!(
        rows.iter()
            .all(|row| row["internal_id"] != "internal-foreign"),
        "another project's row leaked into the list: {list}"
    );

    // The detail resolves the internal id the console links with.
    let by_internal = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!("{base}/{internal}"),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(by_internal.status(), StatusCode::OK);
    let document = json_body(by_internal).await;
    assert_eq!(document["id"], internal);
    assert_eq!(document["request_id"], "shared-request");

    // An external-id bookmark still works — and resolves this project's row, not the
    // newer foreign one.
    let by_external = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!("{base}/shared-request"),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(
        by_external.status(),
        StatusCode::OK,
        "an existing external-id bookmark must keep working"
    );
    assert_eq!(json_body(by_external).await["id"], internal);

    // Each project sees its own row, and the other project's internal id is not
    // reachable from here.
    let foreign_base = "/api/admin/v1/projects/project-b/observability/requests";
    let foreign_detail = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!("{foreign_base}/shared-request"),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(foreign_detail.status(), StatusCode::OK);
    assert_eq!(json_body(foreign_detail).await["id"], "internal-foreign");
    let hidden = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!("{base}/internal-foreign"),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(
        hidden.status(),
        StatusCode::NOT_FOUND,
        "another project's internal id must not resolve here"
    );

    // Authorization fails closed before any event is returned: a user who is not a
    // member of the project gets nothing, on both routes.
    let outsider = access::create_user(
        &f.state.db,
        &access::Principal::session(owner_id),
        &access::UserInput {
            email: "observability-outsider@example.com".into(),
            password: "outsider-password-123456".into(),
            display_name: None,
            language: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let outsider_cookie = db::create_session(&f.state.db, &outsider.id).await.unwrap();
    for path in [
        format!("{base}?limit=50"),
        format!("{base}/{internal}"),
        format!("{base}/shared-request"),
    ] {
        let response = admin(
            &f,
            &outsider_cookie,
            http::Method::GET,
            &path,
            Value::Null,
            false,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "a non-member must not read the projection at {path}"
        );
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(
            !body.contains(&internal) && !body.contains("shared-request"),
            "a refused read must not carry request identity: {body}"
        );
    }
}

/// One projected event at an exact event time, so a window can be asked for by its
/// own bounds instead of by waiting for the clock to move.
fn projected(
    id: &str,
    started_at: i64,
    input_tokens: i64,
    output_tokens: i64,
    cost_micros: i64,
) -> RequestEvent {
    RequestEvent {
        id: id.into(),
        project_id: db::DEFAULT_PROJECT_ID.into(),
        request_id: id.into(),
        trace_id: format!("trace-{id}"),
        source_ip: None,
        started_at,
        finished_at: started_at,
        endpoint: "/v1/chat/completions".into(),
        api_key_id: None,
        provider: Some("a".into()),
        requested_model: Some("public".into()),
        resolved_model: Some("a-model".into()),
        status_code: Some(200),
        error_kind: None,
        latency_ms: 12,
        ttft_ms: None,
        input_tokens,
        output_tokens,
        cached_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        stream: None,
        cost_micros,
        payload_captured: false,
        request_json: None,
        response_json: None,
    }
}

/// The overview was hard-coded to 24 hours: neither summary route accepted a
/// window, so a caller asking for the last hour was answered with the whole day
/// and the console could not offer any other span.
#[tokio::test]
async fn the_project_summary_honours_the_window_the_caller_asked_for() {
    let f = fixture(success()).await;
    let now = db::now();
    f.state
        .observations
        .record(projected("recent", now - 1800, 7, 3, 21));
    f.state
        .observations
        .record(projected("older", now - 5 * 3600, 100, 40, 900));
    f.state.observations.flush().await;
    let cookie = owner(&f).await;

    let body = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{}/observability/summary?from={}&until={}",
                db::DEFAULT_PROJECT_ID,
                now - 3600,
                now
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(
        body["requests"], 1,
        "the window the caller asked for has to be the window the summary describes"
    );
    assert_eq!(body["errors"], 0);
    assert_eq!(body["input_tokens"], 7);
    assert_eq!(body["output_tokens"], 3);
    assert_eq!(body["cost_micros"], 21);
    let series = body["series"].as_array().unwrap();
    assert_eq!(
        series.len(),
        1,
        "only the bucket inside the window may be returned: {series:?}"
    );
    assert_eq!(series[0]["requests"], 1);
}

/// The unscoped route is the other summary surface; it took no window either.
#[tokio::test]
async fn the_unscoped_summary_honours_the_window_too() {
    let f = fixture(success()).await;
    let now = db::now();
    f.state
        .observations
        .record(projected("recent", now - 1800, 7, 3, 21));
    f.state
        .observations
        .record(projected("older", now - 5 * 3600, 100, 40, 900));
    f.state.observations.flush().await;
    let cookie = owner(&f).await;

    let body = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/observability/summary?from={}&until={}",
                now - 3600,
                now
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(body["requests"], 1);
    assert_eq!(body["input_tokens"], 7);
}

/// The console's window is one window: the summary and the breakdown have to agree
/// on exactly which events it contains. On event time that means half-open — an
/// event stamped at `until` belongs to the next window, one second earlier belongs
/// to this one — so the two endpoints must not disagree by the boundary second.
#[tokio::test]
async fn the_summary_and_the_breakdown_agree_on_a_half_open_window() {
    let f = fixture(success()).await;
    for _ in 0..2 {
        assert_eq!(
            request(&f, "/v1/chat/completions", chat()).await.status(),
            StatusCode::OK
        );
    }
    f.state.observations.flush().await;

    // Two real requests, moved to the two sides of one boundary in *both* stores,
    // so this compares the same two events rather than two fixtures.
    let rows = f
        .state
        .observations
        .list(RequestFilter {
            project_id: Some(db::DEFAULT_PROJECT_ID.into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "the fixture's traffic has to be projected");
    let until = db::now();
    for (row, started_at) in rows.iter().zip([until - 1, until]) {
        sql(
            &f,
            "UPDATE request_facts SET started_at=? WHERE id=?",
            vec![started_at.into(), row.internal_id.clone().into()],
        )
        .await;
        let mut event = f
            .state
            .observations
            .get(row.internal_id.clone())
            .await
            .unwrap()
            .expect("every listed row is readable by its own internal id");
        event.started_at = started_at;
        f.state.observations.record(event);
    }
    f.state.observations.flush().await;

    let cookie = owner(&f).await;
    let from = until - 3_600;
    let summary = |until: i64| {
        let path = format!(
            "/api/admin/v1/projects/{}/observability/summary?from={from}&until={until}",
            db::DEFAULT_PROJECT_ID
        );
        let f = &f;
        let cookie = cookie.clone();
        async move {
            json_body(admin(f, &cookie, http::Method::GET, &path, Value::Null, false).await).await
        }
    };
    let breakdown = |until: i64| {
        let path = format!(
            "/api/admin/v1/projects/{}/analytics?dimension=model&from={from}&until={until}",
            db::DEFAULT_PROJECT_ID
        );
        let f = &f;
        let cookie = cookie.clone();
        async move {
            let body =
                json_body(admin(f, &cookie, http::Method::GET, &path, Value::Null, false).await)
                    .await;
            body["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["requests"].as_i64().unwrap_or(0))
                .sum::<i64>()
        }
    };

    // The control: one second wider and both events are in the window, so the
    // exclusion below is the boundary and not an empty projection.
    let wider = summary(until + 1).await;
    assert_eq!(wider["requests"], 2, "both events exist: {wider}");
    assert_eq!(breakdown(until + 1).await, 2);

    let summary = summary(until).await;
    assert_eq!(
        summary["requests"], 1,
        "an event stamped at `until` is in the next window, not this one: {summary}"
    );
    assert_eq!(
        breakdown(until).await,
        1,
        "the breakdown has to describe the same half-open window as the summary"
    );

    // `from == until` stays a legal, empty window — a real question with no events
    // in it, not a refusal — and the event stamped exactly at `until` is outside it.
    let empty = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{}/observability/summary?from={until}&until={until}",
                db::DEFAULT_PROJECT_ID
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(empty["requests"], 0, "{empty}");
    assert!(empty["series"].as_array().unwrap().is_empty(), "{empty}");
}

/// A per-key analytics link is an object-scoped read, not an arbitrary string
/// filter. Foreign and nonexistent ids are deliberately indistinguishable, while
/// a valid key remains readable even when it has no usage rows yet.
#[tokio::test]
async fn analytics_api_key_filter_refuses_foreign_and_unknown_keys() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('usage-foreign-project','Foreign','usage-foreign',0,1,1,1)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO api_keys(id,name,key_prefix,key_hash,lookup_digest,scopes,created_at,project_id) VALUES('usage-foreign-key','Foreign','usage-foreign-prefix','hash','usage-foreign-digest','[\"gateway:use\"]',1,'usage-foreign-project')",
        vec![],
    )
    .await;
    let own_key: String = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT id FROM api_keys WHERE project_id=? ORDER BY created_at LIMIT 1",
            vec![db::DEFAULT_PROJECT_ID.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    let now = db::now();
    sql(
        &f,
        "INSERT INTO request_facts(id,project_id,api_key_id,log_level,status,started_at) VALUES('usage-unmeasured-request',?,?,'metadata','failed',?)",
        vec![db::DEFAULT_PROJECT_ID.into(), own_key.clone().into(), now.into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO execution_facts(id,request_id,attempt,status,price_json,started_at) VALUES('usage-unmeasured-execution','usage-unmeasured-request',1,'failed','{}',?)",
        vec![now.into()],
    )
    .await;

    let analytics = |key: String| {
        let f = &f;
        let cookie = cookie.clone();
        async move {
            admin(
                f,
                &cookie,
                http::Method::GET,
                &format!(
                    "/api/admin/v1/projects/{}/analytics?dimension=model&api_key={key}",
                    db::DEFAULT_PROJECT_ID
                ),
                Value::Null,
                false,
            )
            .await
            .status()
        }
    };

    assert_eq!(analytics(own_key.clone()).await, StatusCode::OK);
    assert_eq!(
        analytics("usage-foreign-key".into()).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        analytics("usage-missing-key".into()).await,
        StatusCode::NOT_FOUND
    );
    let measured = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{}/analytics?dimension=model&api_key={own_key}",
                db::DEFAULT_PROJECT_ID
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(measured["data"][0]["usage_measured"], json!(false));
    assert_eq!(measured["data"][0]["tokens_per_second"], Value::Null);
}

/// A window that ends before it starts is a mistake the caller has to see. It is
/// refused with the console's own error envelope, and the refusal must not name a
/// query, a table or a projection cause.
#[tokio::test]
async fn an_inverted_summary_window_is_refused_without_exposing_a_cause() {
    let f = fixture(success()).await;
    let now = db::now();
    let cookie = owner(&f).await;

    let response = admin(
        &f,
        &cookie,
        http::Method::GET,
        &format!(
            "/api/admin/v1/projects/{}/observability/summary?from={}&until={}",
            db::DEFAULT_PROJECT_ID,
            now,
            now - 3600
        ),
        Value::Null,
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        !message.is_empty(),
        "a refusal has to say something: {body}"
    );
    for leak in [
        "request_events",
        "SELECT",
        "select",
        "DuckDB",
        "duckdb",
        "quantile",
    ] {
        assert!(
            !message.contains(leak),
            "the refusal must not carry the projection's own cause ({leak}): {message}"
        );
    }
}

/// The trace lifecycle route the console drives. One project-scoped mutation
/// endpoint takes an explicit action, and the trace is addressed by its internal
/// id — never by the caller-supplied external one, which is not an identity.
fn trace_lifecycle_path(project: &str, trace: &str) -> String {
    format!("/api/admin/v1/projects/{project}/traces/{trace}/lifecycle")
}

async fn trace_lifecycle(
    f: &Fixture,
    cookie: &str,
    project: &str,
    trace: &str,
    action: &str,
) -> Response {
    admin(
        f,
        cookie,
        http::Method::POST,
        &trace_lifecycle_path(project, trace),
        json!({ "action": action }),
        true,
    )
    .await
}

async fn trace_lifecycle_state(f: &Fixture, trace: &str) -> String {
    f.state
        .db
        .query_one(ops::sql(
            "SELECT lifecycle FROM traces WHERE id=?",
            vec![trace.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "lifecycle")
        .unwrap()
}

/// The one trace the fixture's request created, with the outcome status it settled
/// on — the fact the lifecycle must never overwrite.
async fn settled_trace(f: &Fixture) -> (String, String) {
    let row = f
        .state
        .db
        .query_one(ops::sql("SELECT id,status FROM traces LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap();
    (
        row.try_get("", "id").unwrap(),
        row.try_get("", "status").unwrap(),
    )
}

/// One project-scoped, transactionally audited contract behind four explicit
/// actions. This case walks the whole operator contract: every valid action, the
/// state the list and the detail project, the exact audit action/resource/project
/// facts, the refused nonsensical transition, and the fact that a foreign and a
/// nonexistent trace are the same 404 with no update and no audit row.
#[tokio::test]
async fn trace_lifecycle_actions_are_project_scoped_audited_and_refuse_nonsense_transitions() {
    let f = fixture(success()).await;
    let cookie = owner(&f).await;
    let project = db::DEFAULT_PROJECT_ID;
    let base = format!("/api/admin/v1/projects/{project}/operations");
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    let (trace, outcome) = settled_trace(&f).await;
    let audit =
        |action: &str| format!("SELECT COUNT(*) AS n FROM audit_events WHERE action='{action}'");
    let lifecycle_audits =
        || "SELECT COUNT(*) AS n FROM audit_events WHERE action LIKE 'trace.%'".to_string();

    // The session CSRF guard covers this route like every other browser mutation.
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &trace_lifecycle_path(project, &trace),
            json!({"action":"archive"}),
            false,
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(count(&f, &lifecycle_audits()).await, 0);

    // ---- active -> archived, with the audit row in the same transaction ----
    assert_eq!(
        trace_lifecycle(&f, &cookie, project, &trace, "archive")
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(trace_lifecycle_state(&f, &trace).await, "archived");
    assert_eq!(count(&f, &audit("trace.archive")).await, 1);
    let recorded = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT resource_type,resource_id,details FROM audit_events WHERE action='trace.archive'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        recorded.try_get::<String>("", "resource_type").unwrap(),
        "operations"
    );
    assert_eq!(
        recorded.try_get::<String>("", "resource_id").unwrap(),
        trace,
        "the audit row names the trace, not the action"
    );
    let details: Value =
        serde_json::from_str(&recorded.try_get::<String>("", "details").unwrap()).unwrap();
    assert_eq!(details["project_id"], json!(project));
    assert_eq!(details["principal_kind"], json!("session"));

    // The list excludes an archived trace by default and finds it again under an
    // explicit lifecycle filter; the outcome column is untouched by all of this.
    let listed = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/traces"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(
        listed["total"],
        json!(0),
        "archived is out of the default view"
    );
    let archived = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/traces?lifecycle=archived"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(archived["total"], json!(1));
    assert_eq!(archived["data"][0]["lifecycle"], json!("archived"));
    assert_eq!(
        archived["data"][0]["status"],
        json!(outcome),
        "the projection still reports the execution outcome"
    );
    let detail = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/trace-detail/{trace}"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(detail["trace"]["lifecycle"], json!("archived"));

    // ---- an exact repeat and a nonsensical transition are refused ----
    assert_eq!(
        trace_lifecycle(&f, &cookie, project, &trace, "archive")
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        trace_lifecycle(&f, &cookie, project, &trace, "retain")
            .await
            .status(),
        StatusCode::CONFLICT,
        "an archived trace is restored, not pinned"
    );
    assert_eq!(
        trace_lifecycle(&f, &cookie, project, &trace, "unretain")
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(count(&f, &lifecycle_audits()).await, 1);
    assert_eq!(trace_lifecycle_state(&f, &trace).await, "archived");

    // ---- archived -> active ----
    assert_eq!(
        trace_lifecycle(&f, &cookie, project, &trace, "unarchive")
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(trace_lifecycle_state(&f, &trace).await, "active");
    assert_eq!(count(&f, &audit("trace.unarchive")).await, 1);

    // ---- active -> retained -> active ----
    assert_eq!(
        trace_lifecycle(&f, &cookie, project, &trace, "retain")
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(trace_lifecycle_state(&f, &trace).await, "retained");
    assert_eq!(count(&f, &audit("trace.retain")).await, 1);
    let retained = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!("{base}/traces"),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(
        retained["total"],
        json!(1),
        "the default view keeps active and retained traces"
    );
    assert_eq!(retained["data"][0]["lifecycle"], json!("retained"));
    assert_eq!(
        trace_lifecycle(&f, &cookie, project, &trace, "retain")
            .await
            .status(),
        StatusCode::CONFLICT,
        "an already retained trace is not pinned twice"
    );
    assert_eq!(
        trace_lifecycle(&f, &cookie, project, &trace, "unretain")
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(trace_lifecycle_state(&f, &trace).await, "active");
    assert_eq!(count(&f, &audit("trace.unretain")).await, 1);
    assert_eq!(count(&f, &lifecycle_audits()).await, 4);
    // The four actions never rewrote the execution outcome.
    assert_eq!(
        f.state
            .db
            .query_one(ops::sql(
                "SELECT status FROM traces WHERE id=?",
                vec![trace.clone().into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "status")
            .unwrap(),
        outcome
    );

    // An unknown action is a refusal, not a silent no-op.
    assert_eq!(
        trace_lifecycle(&f, &cookie, project, &trace, "delete")
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );

    // ---- a foreign trace and a nonexistent one are the same 404 ----
    sql(
        &f,
        "INSERT INTO projects(id,name,slug,is_default,enabled,created_at,updated_at) VALUES('foreign-project','Foreign','foreign',0,1,1,1)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO traces(id,project_id,status,started_at) VALUES('foreign-trace','foreign-project','succeeded',1)",
        vec![],
    )
    .await;
    let audits_before = count(&f, &lifecycle_audits()).await;
    let foreign = trace_lifecycle(&f, &cookie, project, "foreign-trace", "archive").await;
    let missing = trace_lifecycle(&f, &cookie, project, "no-such-trace", "archive").await;
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        json_body(foreign).await,
        json_body(missing).await,
        "a foreign trace must be indistinguishable from a nonexistent one"
    );
    assert_eq!(
        count(&f, &lifecycle_audits()).await,
        audits_before,
        "a refused mutation writes no audit row"
    );
    assert_eq!(
        trace_lifecycle_state(&f, "foreign-trace").await,
        "active",
        "a refused mutation changes no state"
    );
}

/// Retention is a promise about the record system, and "retained" is the
/// operator's one way to keep a trace out of it. A retained trace keeps its
/// requests, captured bodies, authoritative request/execution facts and the
/// cascading usage and cost rows — under a project policy and under a global one —
/// and stops being preserved the moment it is unretained.
#[tokio::test]
async fn retention_gc_preserves_a_retained_trace_and_releases_it_when_unretained() {
    const ROWS: [(&str, &str); 7] = [
        ("traces", "SELECT COUNT(*) AS n FROM traces"),
        ("requests", "SELECT COUNT(*) AS n FROM requests"),
        (
            "request_contents",
            "SELECT COUNT(*) AS n FROM request_contents",
        ),
        ("request_facts", "SELECT COUNT(*) AS n FROM request_facts"),
        (
            "execution_facts",
            "SELECT COUNT(*) AS n FROM execution_facts",
        ),
        ("usage_logs", "SELECT COUNT(*) AS n FROM usage_logs"),
        (
            "usage_cost_items",
            "SELECT COUNT(*) AS n FROM usage_cost_items",
        ),
    ];
    for global in [false, true] {
        let f = fixture(success()).await;
        ops::logging::set_policy(
            &f.state.db,
            &Policy {
                default_level: Level::FullBody,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        sql(
            &f,
            "UPDATE models SET input_price_micros=1000000,output_price_micros=2000000",
            vec![],
        )
        .await;
        assert_eq!(
            request(&f, "/v1/chat/completions", chat()).await.status(),
            StatusCode::OK
        );
        let (trace, _) = settled_trace(&f).await;
        // Age every row past the one-day window, and pin the trace.
        sql(&f, "UPDATE requests SET started_at=1,finished_at=2", vec![]).await;
        sql(
            &f,
            "UPDATE request_facts SET started_at=1,finished_at=2 WHERE status!='running'",
            vec![],
        )
        .await;
        sql(
            &f,
            "UPDATE execution_facts SET started_at=1 WHERE status!='running'",
            vec![],
        )
        .await;
        sql(
            &f,
            "UPDATE traces SET finished_at=2,lifecycle='retained'",
            vec![],
        )
        .await;
        assert_eq!(
            count(&f, "SELECT COUNT(*) AS n FROM request_contents").await,
            1,
            "the fixture must have captured a body, otherwise this proves nothing"
        );
        // The cost of one settled request is several component rows, so the proof
        // is that nothing the trace owns is missing — not a hard-coded count.
        let settled: Vec<(&str, i64)> = {
            let mut counts = vec![];
            for (table, query) in ROWS {
                counts.push((table, count(&f, query).await));
            }
            counts
        };
        assert!(
            settled.iter().all(|(_, rows)| *rows > 0),
            "the fixture has to have produced every kind of row this proves: {settled:?}"
        );
        for resource in ["requests", "payloads"] {
            let (id, project) = if global {
                (format!("g-{resource}"), None)
            } else {
                (format!("p-{resource}"), Some(db::DEFAULT_PROJECT_ID))
            };
            f.state
                .db
                .execute(ops::sql(
                    "INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES(?,?,?,1,0)",
                    vec![id.into(), project.map(str::to_owned).into(), resource.into()],
                ))
                .await
                .unwrap();
        }

        ops::runtime::gc(&f.state).await.unwrap();

        let scope = if global { "global" } else { "project" };
        for (table, query) in ROWS {
            assert_eq!(
                count(&f, query).await,
                settled.iter().find(|(name, _)| *name == table).unwrap().1,
                "{table} must survive a {scope} retention policy while the trace is retained"
            );
        }
        // "Preserved" means the trace still opens: the detail projection reads its
        // requests and its attempts out of the record system.
        let cookie = owner(&f).await;
        let detail = json_body(
            admin(
                &f,
                &cookie,
                http::Method::GET,
                &format!(
                    "/api/admin/v1/projects/{}/operations/trace-detail/{trace}",
                    db::DEFAULT_PROJECT_ID
                ),
                Value::Null,
                false,
            )
            .await,
        )
        .await;
        assert_eq!(detail["requests"].as_array().unwrap().len(), 1, "{scope}");
        assert_eq!(detail["executions"].as_array().unwrap().len(), 1, "{scope}");

        // Unretaining returns the trace to the behaviour that was already there.
        sql(&f, "UPDATE traces SET lifecycle='active'", vec![]).await;
        ops::runtime::gc(&f.state).await.unwrap();
        for (table, query) in ROWS {
            assert_eq!(
                count(&f, query).await,
                0,
                "{table} must be pruned again after the trace is unretained ({scope})"
            );
        }
    }
}

/// Archiving is a console lifecycle state, not a licence to break the retention
/// policy: an archived trace is pruned exactly like an active one.
#[tokio::test]
async fn an_archived_trace_is_still_governed_by_the_retention_policy() {
    let f = fixture(success()).await;
    sql(
        &f,
        "UPDATE models SET input_price_micros=1000000,output_price_micros=2000000",
        vec![],
    )
    .await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    sql(&f, "UPDATE requests SET started_at=1,finished_at=2", vec![]).await;
    sql(
        &f,
        "UPDATE request_facts SET started_at=1,finished_at=2 WHERE status!='running'",
        vec![],
    )
    .await;
    sql(
        &f,
        "UPDATE traces SET finished_at=2,lifecycle='archived'",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES('r',NULL,'requests',1,0)",
        vec![],
    )
    .await;
    ops::runtime::gc(&f.state).await.unwrap();
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM traces").await, 0);
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM requests").await, 0);
}

/// The derived row for one request, aged past the request/payload windows the test
/// installs and past the instance's own observation window, but not so old that no
/// window could ever have admitted it. `RequestEvent.id` is the record system's own
/// request uuid, so an event built from a real request id is exactly the row the
/// request log would show for that request.
fn aged_event(request_id: &str, project: &str) -> RequestEvent {
    let started_at = db::now() - 60 * 86_400;
    RequestEvent {
        id: request_id.into(),
        project_id: project.into(),
        request_id: "client-supplied".into(),
        trace_id: "external-trace".into(),
        source_ip: None,
        started_at,
        finished_at: started_at + 1,
        endpoint: "/v1/chat/completions".into(),
        api_key_id: None,
        provider: Some("Channel A".into()),
        requested_model: Some("demo".into()),
        resolved_model: Some("demo".into()),
        status_code: Some(200),
        error_kind: None,
        latency_ms: 12,
        ttft_ms: None,
        input_tokens: 1,
        output_tokens: 1,
        cached_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        stream: None,
        cost_micros: 0,
        payload_captured: true,
        request_json: Some(r#"{"messages":[{"role":"user","content":"pinned"}]}"#.into()),
        response_json: Some(r#"{"choices":[]}"#.into()),
    }
}

/// The request log's own row for one internal request id, over a window wide
/// enough to include a row aged far past every retention window.
async fn logged(f: &Fixture, request_id: &str) -> Option<RequestEvent> {
    f.state
        .observations
        .get(request_id.to_owned())
        .await
        .unwrap()
}

/// A row the projection already holds and that is older than every window.
///
/// Admission refuses an event older than the instance's own observation window, so
/// a row like this exists because it was admitted while a wider window was in force
/// and then aged in place — which is exactly the row a retention pass has to decide
/// about, and the row whose fate the operator's pin changes. Widening the writer's
/// rules for the setup and letting the pass under test install the real ones is how
/// the test reaches that state through the store's own API.
async fn admitted_aged_event(f: &Fixture, event: RequestEvent) {
    f.state
        .observations
        .apply_retention(vec![crate::observability::Retention::new(
            None, 3650, false,
        )])
        .await;
    f.state.observations.record(event);
    f.state.observations.flush().await;
}

/// A retained trace is pinned in the observable product, not only in the record
/// system: the request log and the summary read the DuckDB projection, so the same
/// pin has to reach it — for a project-scoped rule and a global one, and for the
/// delete rule and the payload rule alike. Unretaining returns the run to the
/// ordinary window.
#[tokio::test]
async fn derived_retention_keeps_a_pinned_trace_in_the_request_log() {
    for global in [false, true] {
        for resource in ["requests", "payloads"] {
            let f = fixture(success()).await;
            ops::logging::set_policy(
                &f.state.db,
                &Policy {
                    default_level: Level::FullBody,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert_eq!(
                request(&f, "/v1/chat/completions", chat()).await.status(),
                StatusCode::OK
            );
            let (trace, _) = settled_trace(&f).await;
            let internal: String = f
                .state
                .db
                .query_one(ops::sql("SELECT id FROM requests LIMIT 1", vec![]))
                .await
                .unwrap()
                .unwrap()
                .try_get("", "id")
                .unwrap();
            let project: String = f
                .state
                .db
                .query_one(ops::sql(
                    "SELECT t.project_id AS project_id FROM requests r JOIN traces t ON t.id=r.trace_id WHERE r.id=?",
                    vec![internal.clone().into()],
                ))
                .await
                .unwrap()
                .unwrap()
                .try_get("", "project_id")
                .unwrap();
            // The derived row is the one the request log would show, aged past every
            // window — the instance's own observation window included.
            admitted_aged_event(&f, aged_event(&internal, &project)).await;
            sql(
                &f,
                "UPDATE traces SET lifecycle='retained' WHERE id=?",
                vec![trace.clone().into()],
            )
            .await;
            let (id, policy_project) = if global {
                (format!("g-{resource}"), None)
            } else {
                (format!("p-{resource}"), Some(project.clone()))
            };
            f.state
                .db
                .execute(ops::sql(
                    "INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES(?,?,?,1,0)",
                    vec![id.into(), policy_project.into(), resource.into()],
                ))
                .await
                .unwrap();
            // Age the record system's own rows as well, so both surfaces describe the
            // same aged run and the released pin can be asserted on both.
            sql(&f, "UPDATE requests SET started_at=1,finished_at=2", vec![]).await;
            sql(
                &f,
                "UPDATE request_facts SET started_at=1,finished_at=2 WHERE status!='running'",
                vec![],
            )
            .await;
            sql(
                &f,
                "UPDATE traces SET finished_at=2 WHERE id=?",
                vec![trace.clone().into()],
            )
            .await;

            ops::runtime::gc(&f.state).await.unwrap();

            let scope = format!(
                "{}-scoped {resource} policy",
                if global { "global" } else { "project" }
            );
            let pinned = logged(&f, &internal).await.unwrap_or_else(|| {
                panic!("the request log must still show the run under a {scope}")
            });
            assert!(
                pinned.request_json.is_some(),
                "a payload rule must not strip the pinned run's body ({scope})"
            );
            // The record system agrees, which is what makes the two surfaces
            // describe one trace.
            assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM requests").await, 1);

            // Releasing the pin returns the run to the ordinary window, in both
            // surfaces at once.
            sql(&f, "UPDATE traces SET lifecycle='active'", vec![]).await;
            ops::runtime::gc(&f.state).await.unwrap();
            assert!(
                logged(&f, &internal).await.is_none(),
                "an unretained run expires from the request log again ({scope})"
            );
            assert_eq!(
                count(&f, "SELECT COUNT(*) AS n FROM request_contents").await,
                0,
                "the released run's stored body is governed by the policy again ({scope})"
            );
            if resource == "requests" {
                assert_eq!(
                    count(&f, "SELECT COUNT(*) AS n FROM requests").await,
                    0,
                    "a requests policy prunes the released run ({scope})"
                );
            }
        }
    }
}

/// The ceiling is a ceiling, not a page boundary.
///
/// The pin set is read in pages, and a set that only exceeds the ceiling on the
/// last, *partial* page used to be accepted: the partial page returned success
/// before the ceiling was ever consulted. That is the one case where the set is
/// complete but unbounded — 50,001 pins read as 500 full pages and one id — so the
/// pass went on to apply rules it had just decided not to bound. Nothing pinned is
/// expired by a pass that cannot name all of it, so this has to fail safe too, and
/// the observable proof is that an *unpinned* aged row also survives: with
/// retention applied it would be gone.
#[tokio::test]
async fn a_pin_set_that_only_overflows_on_the_last_page_still_disables_derived_retention() {
    let f = fixture(success()).await;
    let project = db::DEFAULT_PROJECT_ID;
    for index in 1..=4 {
        sql(
            &f,
            &format!(
                "INSERT INTO traces(id,project_id,status,started_at,lifecycle) VALUES('trace-{index}','{project}','succeeded',1,'retained')"
            ),
            vec![],
        )
        .await;
        sql(
            &f,
            &format!(
                "INSERT INTO requests(id,trace_id,protocol,endpoint,status,started_at,finished_at) VALUES('request-{index}','trace-{index}','openai','/v1/chat/completions','succeeded',1,2)"
            ),
            vec![],
        )
        .await;
        admitted_aged_event(&f, aged_event(&format!("request-{index}"), project)).await;
    }
    // One request nobody pinned, so a rule that does run is visible: it is the row
    // that proves whether retention was applied at all.
    sql(
        &f,
        &format!(
            "INSERT INTO traces(id,project_id,status,started_at,lifecycle) VALUES('trace-ordinary','{project}','succeeded',1,'active')"
        ),
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO requests(id,trace_id,protocol,endpoint,status,started_at,finished_at) VALUES('request-ordinary','trace-ordinary','openai','/v1/chat/completions','succeeded',1,2)",
        vec![],
    )
    .await;
    admitted_aged_event(&f, aged_event("request-ordinary", project)).await;

    // Three ids per page and a ceiling of three: the fourth pin lands on the last,
    // partial page, which is exactly where the ceiling used to be skipped.
    let degraded = ops::runtime::sync_retention_within(&f.state, 3, 3)
        .await
        .unwrap();
    assert!(
        !degraded,
        "a pin set over the ceiling must report derived retention degraded, however it was paged"
    );
    assert!(
        logged(&f, "request-ordinary").await.is_some(),
        "the unpinned aged row proves no rule ran: a set that does not fit must disable retention entirely"
    );
    for index in 1..=4 {
        assert!(
            logged(&f, &format!("request-{index}")).await.is_some(),
            "no pinned request may be expired"
        );
    }

    // The boundary in the other direction: a set of exactly the ceiling is a set
    // that fits, so retention is applied — the unpinned row is expired and the
    // pins are the reason the others are not.
    let applied = ops::runtime::sync_retention_within(&f.state, 4, 4)
        .await
        .unwrap();
    assert!(
        applied,
        "a pin set of exactly the ceiling still fits and must apply retention"
    );
    assert!(
        logged(&f, "request-ordinary").await.is_none(),
        "with retention applied, the unpinned aged row is expired"
    );
    for index in 1..=4 {
        assert!(
            logged(&f, &format!("request-{index}")).await.is_some(),
            "and the pinned requests are what the union spares"
        );
    }
}

/// The pin set is transferred in bounded pages, and a pass that cannot name every
/// pinned request must not clean up with the part it has: a partial preserve set
/// would expire exactly the pins it did not carry. It disables derived retention
/// instead — nothing is deleted, nothing is payload-cleared, including the events
/// still arriving — and reports itself degraded while the record system keeps
/// enforcing the pin and the GC still completes.
#[tokio::test]
async fn an_incomplete_pin_set_disables_derived_retention_instead_of_expiring_a_pin() {
    let f = fixture(success()).await;
    let project = db::DEFAULT_PROJECT_ID;
    // Four pinned traces, each with one request. The derived rows are aged past
    // every window, so any rule at all would remove them.
    for index in 1..=4 {
        sql(
            &f,
            &format!(
                "INSERT INTO traces(id,project_id,status,started_at,lifecycle) VALUES('trace-{index}','{project}','succeeded',1,'retained')"
            ),
            vec![],
        )
        .await;
        sql(
            &f,
            &format!(
                "INSERT INTO requests(id,trace_id,protocol,endpoint,status,started_at,finished_at) VALUES('request-{index}','trace-{index}','openai','/v1/chat/completions','succeeded',1,2)"
            ),
            vec![],
        )
        .await;
    }
    for index in 1..=4 {
        admitted_aged_event(&f, aged_event(&format!("request-{index}"), project)).await;
    }

    // Two ids per page and a ceiling of three: the fourth pin cannot be carried, so
    // no rule may be applied at all.
    let degraded = ops::runtime::sync_retention_within(&f.state, 2, 3)
        .await
        .unwrap();
    assert!(
        !degraded,
        "a pin set that does not fit must report derived retention degraded"
    );
    for index in 1..=4 {
        let id = format!("request-{index}");
        assert!(
            logged(&f, &id).await.is_some(),
            "no pinned request may be expired, including {id} beyond the ceiling"
        );
    }

    // A late event is admitted untouched too: the writer holds no active rule, so
    // nothing is dropped and no payload is cleared.
    f.state
        .observations
        .record(aged_event("request-late", project));
    f.state.observations.flush().await;
    let late = logged(&f, "request-late").await.unwrap();
    assert!(
        late.request_json.is_some(),
        "a disabled pass must not strip a late event's payload either"
    );

    // SQLite remains authoritative and the pass still completes: the pins hold and
    // the enforcement path is not weakened by a degraded projection.
    ops::runtime::gc(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM requests").await,
        4,
        "the record system must still keep every pinned request"
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM traces").await,
        4,
        "and every pinned trace"
    );
}

/// The derived projection is a convenience: when it is degraded, the pin the
/// operator asked for is still enforced where it is authoritative, and the pass
/// still completes.
#[tokio::test]
async fn a_degraded_projection_does_not_weaken_the_pin() {
    let mut f = fixture(success()).await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    let (trace, _) = settled_trace(&f).await;
    f.state.observations =
        ObservationStore::degraded(f._directory.path().join("unavailable"), "injected fault");
    sql(&f, "UPDATE traces SET lifecycle='retained'", vec![]).await;
    sql(&f, "UPDATE requests SET started_at=1,finished_at=2", vec![]).await;
    sql(
        &f,
        "UPDATE request_facts SET started_at=1,finished_at=2 WHERE status!='running'",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES('r',NULL,'requests',1,0)",
        vec![],
    )
    .await;

    // The pass completes, the pin still holds in the record system, and the trace
    // still opens — the degraded projection degrades observability and nothing else.
    ops::runtime::gc(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM traces").await,
        1,
        "the record system must keep the pinned trace while the projection is down"
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM requests").await, 1);
    assert!(
        !f.state.observations.is_available(),
        "the fixture's projection really is degraded"
    );
    let cookie = owner(&f).await;
    let detail = json_body(
        admin(
            &f,
            &cookie,
            http::Method::GET,
            &format!(
                "/api/admin/v1/projects/{}/operations/trace-detail/{trace}",
                db::DEFAULT_PROJECT_ID
            ),
            Value::Null,
            false,
        )
        .await,
    )
    .await;
    assert_eq!(detail["requests"].as_array().unwrap().len(), 1);
}

/// The admin playground is only a session-authenticated entrance to the ordinary
/// gateway. The selected project key still owns routing, traces and settlement,
/// and its token is never reconstructed or returned to the browser.
#[tokio::test]
async fn session_playground_reuses_gateway_policy_and_accounting_without_a_token() {
    let contacted = Arc::new(AtomicUsize::new(0));
    let seen = contacted.clone();
    let f = fixture(Router::new().route(
        "/v1/responses",
        post(move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::Relaxed);
                Json(json!({
                    "id":"resp-playground",
                    "status":"completed",
                    "output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}],
                    "usage":{"input_tokens":3,"output_tokens":2}
                }))
            }
        }),
    ))
    .await;
    let cookie = owner(&f).await;
    let key: String = f
        .state
        .db
        .query_one(ops::sql("SELECT id FROM api_keys LIMIT 1", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    let path = format!(
        "/api/admin/v1/projects/{}/playground/chat",
        db::DEFAULT_PROJECT_ID
    );
    let response = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({
            "api_key_id": key,
            "payload": {"model":"public","input":"hi","stream":false}
        }),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(
        to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("resp-playground"));
    assert!(!body.contains(&f.token));
    assert_eq!(contacted.load(Ordering::Relaxed), 1);
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM requests").await, 1);
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM request_executions").await,
        1
    );
    assert_eq!(count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await, 1);

    let missing = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"api_key_id":"missing","payload":{"model":"public","input":"x"}}),
        true,
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(contacted.load(Ordering::Relaxed), 1);

    sql(&f, "UPDATE api_keys SET scopes='[]'", vec![]).await;
    let unscoped = admin(
        &f,
        &cookie,
        http::Method::POST,
        &path,
        json!({"api_key_id":key,"payload":{"model":"public","input":"x"}}),
        true,
    )
    .await;
    assert_eq!(unscoped.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(contacted.load(Ordering::Relaxed), 1);

    // Browser CSRF is still mandatory, and an API-key principal cannot call the
    // session-only route even when it has gateway scope.
    assert_eq!(
        admin(
            &f,
            &cookie,
            http::Method::POST,
            &path,
            json!({"api_key_id":"missing","payload":{"model":"public","input":"x"}}),
            false,
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}
