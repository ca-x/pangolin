use super::*;
use crate::operations::{self as ops, runtime as ops_runtime};

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
    Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":100,"completion_tokens":10}}))
        }),
    )
}

/// The operator-facing "requests" retention policy must reach the authoritative ledger, not only
/// the legacy request table: children cascade from `request_facts`, running rows survive, and the
/// lifetime budget counter stays authoritative.
#[tokio::test]
async fn requests_retention_prunes_the_authoritative_ledger_and_keeps_running_work() {
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
    // one settled request, plus one execution that is still in flight
    sql(
        &f,
        "INSERT INTO request_facts(id,project_id,log_level,status,started_at) VALUES('live','00000000-0000-0000-0000-000000000001','metadata','running',1)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO execution_facts(id,request_id,attempt,status,price_json,started_at) VALUES('live-exec','live',1,'running','{\"version\":1}',1)",
        vec![],
    )
    .await;
    // age the settled request beyond the retention window
    sql(
        &f,
        "UPDATE request_facts SET started_at=1, finished_at=2 WHERE status!='running'",
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
        "INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) VALUES('r','00000000-0000-0000-0000-000000000001','requests',1,0)",
        vec![],
    )
    .await;
    let spent_before = count(
        &f,
        "SELECT COALESCE(SUM(spent_micros),0) AS n FROM api_keys",
    )
    .await;
    assert!(
        spent_before > 0,
        "the settled request must have charged a budget"
    );

    ops_runtime::gc(&f.state).await.unwrap();

    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM request_facts WHERE status!='running'"
        )
        .await,
        0,
        "aged request facts must be pruned"
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM usage_logs").await,
        0,
        "usage rows must cascade with their execution"
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM usage_cost_items").await,
        0,
        "cost items must cascade with their usage row"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM request_facts WHERE id='live'"
        )
        .await,
        1,
        "in-flight work must keep its reservation"
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM execution_facts WHERE id='live-exec'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &f,
            "SELECT COALESCE(SUM(spent_micros),0) AS n FROM api_keys"
        )
        .await,
        spent_before,
        "the lifetime budget counter must survive retention"
    );
}
/// The retention form used to offer a "retain payloads" switch that nothing read:
/// the GC selects the column and deletes on resource type and days alone, and
/// `request_contents` cascades from `requests`. An operator who ticked it got the
/// bodies deleted anyway. Bodies are governed by the separate `payloads`
/// resource, and this pins both facts so the UI cannot drift from them again.
#[tokio::test]
async fn payload_retention_is_its_own_resource_and_the_projection_does_not_offer_a_dead_switch() {
    let f = fixture(success()).await;
    sql(
        &f,
        "INSERT INTO traces(id,project_id,status,started_at) VALUES('tr','00000000-0000-0000-0000-000000000001','succeeded',1)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO requests(id,trace_id,protocol,endpoint,requested_model,status,started_at,finished_at) \
         VALUES('old-req','tr','openai','/v1/chat/completions','m','succeeded',1,2)",
        vec![],
    )
    .await;
    sql(
        &f,
        "INSERT INTO request_contents(request_id,request_json,response_json) VALUES('old-req','{}','{}')",
        vec![],
    )
    .await;
    // A payloads policy expires bodies on its own schedule.
    sql(
        &f,
        "INSERT INTO data_retention_policies(id,project_id,resource_type,retention_days,updated_at) \
         VALUES('p','00000000-0000-0000-0000-000000000001','payloads',1,0)",
        vec![],
    )
    .await;
    ops_runtime::gc(&f.state).await.unwrap();
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM request_contents").await,
        0,
        "a payloads policy is what expires captured bodies"
    );
    assert_eq!(
        count(&f, "SELECT COUNT(*) AS n FROM requests WHERE id='old-req'").await,
        1,
        "expiring bodies must not delete the request itself"
    );
}

/// Anonymous password failures have a finite security-audit window. The cleanup is deliberately
/// narrow: recent failures, successful authentication, and control-plane audit history survive.
#[tokio::test]
async fn gc_expires_only_old_anonymous_password_failure_audits() {
    let f = fixture(success()).await;
    db::create_initial_admin(
        &f.state.db,
        &SetupRequest {
            email: "audit-owner@example.com".into(),
            password: "correct horse battery staple".into(),
            instance_name: None,
            language: Some("en".into()),
        },
    )
    .await
    .unwrap();
    let now = db::now();
    let old = now - 400 * 86_400;
    sql(
        &f,
        "INSERT INTO audit_events(id,action,resource_type,resource_id,details,created_at) VALUES \
         ('old-failure','login_failed','authentication','','{\"method\":\"password\",\"reason\":\"invalid_credentials\"}',?), \
         ('recent-failure','login_failed','authentication','','{\"method\":\"password\",\"reason\":\"invalid_credentials\"}',?)",
        vec![old.into(), now.into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO audit_events(id,actor_user_id,action,resource_type,resource_id,details,created_at) \
         SELECT 'old-success',id,'login','user',id,'{}',? FROM users WHERE email='audit-owner@example.com'",
        vec![old.into()],
    )
    .await;
    sql(
        &f,
        "INSERT INTO audit_events(id,actor_user_id,action,resource_type,resource_id,details,created_at) \
         SELECT 'old-control',id,'update','system','settings','{}',? FROM users WHERE email='audit-owner@example.com'",
        vec![old.into()],
    )
    .await;

    ops_runtime::gc(&f.state).await.unwrap();

    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE id='old-failure'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) AS n FROM audit_events WHERE id IN ('recent-failure','old-success','old-control')"
        )
        .await,
        3,
        "retention must preserve recent threat evidence and durable success/control-plane history"
    );
}
