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
