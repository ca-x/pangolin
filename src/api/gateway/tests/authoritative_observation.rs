use super::*;
use crate::observability::RequestFilter;
use crate::operations::{self as ops, instance_backup as full};

fn success() -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":100,"completion_tokens":10}}))
        }),
    )
}
fn rate_limited() -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error":{"message":"slow down"}})),
            )
        }),
    )
}
fn created() -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                StatusCode::CREATED,
                Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}})),
            )
        }),
    )
}

/// The durable outcome snapshot of the newest attempt, plus the provider id that
/// attempt referenced — so a test can prove the stored label is the *name* and not
/// the id it was joined from.
async fn newest_execution(
    f: &Fixture,
) -> (Option<i64>, Option<String>, Option<String>, Option<String>) {
    let row = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT http_status,provider_name,error_kind,provider_id FROM request_executions ORDER BY started_at DESC, attempt DESC LIMIT 1",
            vec![],
        ))
        .await
        .unwrap()
        .expect("the request must have written an execution row");
    (
        row.try_get::<Option<i64>>("", "http_status").unwrap(),
        row.try_get::<Option<String>>("", "provider_name").unwrap(),
        row.try_get::<Option<String>>("", "error_kind").unwrap(),
        row.try_get::<Option<String>>("", "provider_id").unwrap(),
    )
}

async fn projected(f: &Fixture) -> Vec<crate::observability::RequestListItem> {
    f.state.observations.flush().await;
    f.state
        .observations
        .list(RequestFilter::default())
        .await
        .unwrap()
}

async fn legacy_row(f: &Fixture, status: &str) {
    let now = db::now();
    sql(f, "INSERT INTO traces(id,project_id,status,started_at) VALUES('legacy-trace',?,'succeeded',?)", vec![db::DEFAULT_PROJECT_ID.into(), now.into()]).await;
    sql(f, "INSERT INTO requests(id,trace_id,protocol,endpoint,status,started_at) VALUES('legacy-request','legacy-trace','openai','/v1/chat/completions',?,?)", vec![status.into(), now.into()]).await;
    sql(f, "INSERT INTO request_facts(id,project_id,log_level,status,started_at) VALUES('legacy-request',?,'metadata',?,?)", vec![db::DEFAULT_PROJECT_ID.into(), status.into(), now.into()]).await;
    sql(f, "INSERT INTO request_executions(id,request_id,provider_id,attempt,model,status,started_at) VALUES('legacy-execution','legacy-request',?,1,'a-model',?,?)", vec![f.providers[0].clone().into(), status.into(), now.into()]).await;
}

/// Reports the same token facts whether the client asked for a stream or not, so
/// one fixture can prove the list carries them in both shapes.
fn measured_usage() -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(|Json(body): Json<Value>| async move {
            let usage = json!({
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_tokens_details": {"cached_tokens": 30, "cache_write_tokens": 5},
                "completion_tokens_details": {"reasoning_tokens": 7}
            });
            if body["stream"] == true {
                let chunk =
                    json!({"choices": [{"delta": {}, "finish_reason": "stop"}], "usage": usage});
                (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    format!(
                        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"ok\"}}}}]}}\n\n\
                         data: {chunk}\n\n\
                         data: [DONE]\n\n"
                    ),
                )
                    .into_response()
            } else {
                Json(json!({"choices":[{"message":{"content":"ok"}}],"usage":usage}))
                    .into_response()
            }
        }),
    )
}

/// The stream choice is a fact about the *request*, decided when the request was
/// admitted — not something to be inferred later from a response or a captured
/// payload. It is written to the authoritative request metadata so a rebuild can
/// read it back instead of guessing.
async fn recorded_stream(f: &Fixture, id: &str) -> Option<bool> {
    let row = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT json_type(request_metadata_json,'$.stream') AS kind FROM requests WHERE id=?",
            vec![id.into()],
        ))
        .await
        .unwrap()
        .expect("the request must have an authoritative metadata row");
    match row
        .try_get::<Option<String>>("", "kind")
        .unwrap()
        .as_deref()
    {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

/// The list is the scan surface: it has to carry the token facts, the TTFT and
/// whether the provider request was streamed, without opening every detail.
#[tokio::test]
async fn the_list_carries_the_token_facts_ttft_and_the_stream_decision() {
    let f = fixture(measured_usage()).await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("[DONE]"));

    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.input_tokens, 100);
    assert_eq!(row.output_tokens, 20);
    assert_eq!(
        row.cached_tokens, 30,
        "cache-read tokens are a recorded fact the list must return"
    );
    assert_eq!(row.cache_write_tokens, 5);
    assert_eq!(row.reasoning_tokens, 7);
    assert_eq!(
        row.stream,
        Some(true),
        "the client asked for a stream, so the row must say so"
    );
    assert!(
        row.ttft_ms.is_some(),
        "a streamed response measured a first byte"
    );
    assert_eq!(row.requested_model.as_deref(), Some("public"));
    assert_eq!(
        row.resolved_model.as_deref(),
        Some("a-model"),
        "the list must expose requested and resolved separately"
    );
    assert_eq!(row.provider.as_deref(), Some("a"));
    assert_eq!(
        recorded_stream(&f, &row.internal_id).await,
        Some(true),
        "the admission decision is authoritative, not inferred from the response"
    );
}

/// A client that did not ask for a stream is a different fact from a request whose
/// stream choice was never recorded: the first is `false`, the second is NULL, and
/// a non-stream response legitimately has no TTFT.
#[tokio::test]
async fn a_non_streaming_request_records_false_and_no_ttft() {
    let f = fixture(measured_usage()).await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );

    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].stream, Some(false));
    assert_eq!(
        rows[0].ttft_ms, None,
        "a non-stream response may have no first byte to measure"
    );
    assert_eq!(recorded_stream(&f, &rows[0].internal_id).await, Some(false));
}

/// The Gemini protocol selects streaming by the action in the URL rather than by a
/// body field, so the endpoint shape is part of the admission decision.
#[tokio::test]
async fn the_gemini_streaming_endpoint_shape_is_recorded_as_streamed() {
    let f = fixture(Router::new().route(
        "/v1beta/models/{*action}",
        post(|| async {
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"candidates\":[{\"index\":0,\"content\":{\"parts\":[{\"text\":\"ok\"}]}}]}\n\n\
                 data: {\"candidates\":[{\"index\":0,\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":100,\"candidatesTokenCount\":20,\"thoughtsTokenCount\":7}}\n\n",
            )
                .into_response()
        }),
    ))
    .await;
    let base_url = f
        .state
        .db
        .query_one(ops::sql(
            "SELECT base_url FROM providers WHERE id=?",
            vec![f.providers[0].clone().into()],
        ))
        .await
        .unwrap()
        .expect("the fixture's mock upstream address")
        .try_get::<String>("", "base_url")
        .unwrap();
    let provider = db::create_provider(
        &f.state.db,
        &ProviderInput {
            name: "gemini".into(),
            kind: "gemini".into(),
            base_url,
            api_key: String::new(),
        },
        f.state.secrets.encrypt("gemini-secret").unwrap(),
    )
    .await
    .unwrap();
    db::create_model(
        &f.state.db,
        &ModelInput {
            provider_id: provider.id.clone(),
            public_name: "gemini-public".into(),
            upstream_name: "gemini-model".into(),
            capabilities: None,
            input_price_micros: None,
            output_price_micros: None,
            priority: Some(0),
        },
        db::DEFAULT_PROJECT_ID,
    )
    .await
    .unwrap();

    let response = request(
        &f,
        "/v1beta/models/gemini-public:streamGenerateContent",
        json!({"contents":[{"parts":[{"text":"hi"}]}]}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("finishReason"));

    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].endpoint, "/v1beta/models:streamGenerateContent",
        "the endpoint shape is the streaming decision"
    );
    assert_eq!(
        rows[0].stream,
        Some(true),
        "the streaming action in the URL is a stream decision with no body field"
    );
    assert!(rows[0].ttft_ms.is_some());
    assert_eq!(recorded_stream(&f, &rows[0].internal_id).await, Some(true));
}

/// A request Pangolin answers itself has no stream choice to report. `false` would
/// be a claim about a provider request that never happened.
#[tokio::test]
async fn a_locally_answered_request_records_no_stream_decision() {
    let f = fixture(measured_usage()).await;
    let response = router(f.state.clone())
        .oneshot(
            Request::get("/v1beta/models")
                .header("authorization", format!("Bearer {}", f.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].endpoint, "/v1beta/models");
    assert_eq!(
        rows[0].stream, None,
        "a locally answered request never decided to stream anything"
    );
}

/// After a restore or a projection schema change the console re-derives every row
/// from the record system. The token facts and the stream decision have to come
/// back verbatim, or the list would contradict the request that actually ran.
#[tokio::test]
async fn rebuilding_the_projection_reproduces_the_token_facts_and_the_stream_decision() {
    let f = fixture(measured_usage()).await;
    let response = request(&f, "/v1/chat/completions", streaming()).await;
    assert_eq!(response.status(), StatusCode::OK);
    // The terminal usage chunk only arrives if the stream is read to its end.
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("[DONE]"));
    f.state
        .db
        .execute(crate::operations::sql(
            "UPDATE requests SET source_ip='203.0.113.9'",
            vec![],
        ))
        .await
        .unwrap();
    let before = projected(&f).await;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].cache_write_tokens, 5);

    assert!(f.state.observations.clear_for_restore().await);
    assert_eq!(full::rebuild_projection(&f.state).await.unwrap(), 1);
    let after = projected(&f).await;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].input_tokens, before[0].input_tokens);
    assert_eq!(after[0].output_tokens, before[0].output_tokens);
    assert_eq!(after[0].cached_tokens, before[0].cached_tokens);
    assert_eq!(
        after[0].cache_write_tokens, before[0].cache_write_tokens,
        "cache-write tokens are settled usage, not a cost component"
    );
    assert_eq!(
        after[0].reasoning_tokens, before[0].reasoning_tokens,
        "reasoning tokens are settled usage, not a cost component"
    );
    assert_eq!(after[0].ttft_ms, before[0].ttft_ms);
    assert_eq!(
        after[0].stream, before[0].stream,
        "the rebuild reads the admission decision, it does not infer one"
    );
    assert_eq!(after[0].stream, Some(true));
    let detail = f
        .state
        .observations
        .get_for(db::DEFAULT_PROJECT_ID.into(), after[0].internal_id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        detail.source_ip.as_deref(),
        Some("203.0.113.9"),
        "the rebuild copies the authoritative direct peer instead of reparsing headers"
    );
}

/// A row written before the stream fact existed carries none. `false` would claim
/// the client asked for a non-streamed response, which nothing recorded.
#[tokio::test]
async fn a_legacy_row_rebuilds_without_a_stream_guess() {
    let f = fixture(measured_usage()).await;
    legacy_row(&f, "succeeded").await;
    assert!(f.state.observations.clear_for_restore().await);
    assert_eq!(full::rebuild_projection(&f.state).await.unwrap(), 1);

    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].stream, None,
        "an unrecorded stream choice is NULL, never a guessed false"
    );
    assert_eq!(rows[0].ttft_ms, None);
    let detail = f
        .state
        .observations
        .get_for(db::DEFAULT_PROJECT_ID.into(), rows[0].internal_id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.source_ip, None);
    assert_eq!(
        rows[0].cache_write_tokens, 0,
        "no settled usage row means no cache-write tokens were recorded"
    );
    assert_eq!(rows[0].reasoning_tokens, 0);
}

/// A `2xx` that is not `200` is still a success, and the log must say which one it
/// was: the live event knew `201` while the record system stored nothing, so a
/// rebuild could only answer `200`.
#[tokio::test]
async fn a_successful_non_200_response_keeps_its_exact_status_and_provider_name() {
    let f = fixture(created()).await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    to_bytes(response.into_body(), 1024 * 1024).await.unwrap();

    let (status, provider, error, provider_id) = newest_execution(&f).await;
    assert_eq!(
        status,
        Some(201),
        "the exact upstream status must be durable; a synthesized 200 hides it"
    );
    assert_eq!(
        provider.as_deref(),
        Some("a"),
        "the attempt must freeze the provider display name"
    );
    assert_ne!(
        provider, provider_id,
        "a provider id is not a provider name"
    );
    assert_eq!(
        error, None,
        "a settled success has no failure classification"
    );

    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status_code, Some(201));
    assert_eq!(rows[0].provider.as_deref(), Some("a"));
}

/// A rate limit is not an outage. The live event has always known `429`; without a
/// durable snapshot the rebuild could only answer `502`.
#[tokio::test]
async fn a_rate_limited_attempt_keeps_429_and_the_kind_the_live_event_emitted() {
    let f = fixture(rate_limited()).await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert!(
        response.status().is_server_error(),
        "the client still gets the gateway's own error"
    );
    drop(response);

    let (status, provider, error, provider_id) = newest_execution(&f).await;
    assert_eq!(status, Some(429));
    assert_ne!(provider, provider_id);
    assert!(matches!(provider.as_deref(), Some("a" | "b")));
    assert_eq!(error.as_deref(), Some("failed"));

    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status_code, Some(429));
    assert_eq!(
        rows[0].error_kind, error,
        "the execution row and the live event must carry one classification"
    );
}

/// A failure before any upstream response has no status to report. `502` was the
/// gateway's own answer to the client, not something the upstream said.
#[tokio::test]
async fn a_local_failure_without_an_upstream_response_keeps_a_null_status() {
    let f = fixture(success()).await;
    sql(
        &f,
        "UPDATE providers SET base_url='http://127.0.0.1:1'",
        vec![],
    )
    .await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    assert!(response.status().is_server_error());
    drop(response);

    let attempts = f
        .state
        .db
        .query_all(ops::sql(
            "SELECT http_status,provider_name FROM request_executions",
            vec![],
        ))
        .await
        .unwrap();
    assert!(!attempts.is_empty(), "the attempts were recorded");
    for attempt in attempts {
        assert_eq!(
            attempt.try_get::<Option<i64>>("", "http_status").unwrap(),
            None,
            "no upstream response was observed, so no status may be invented"
        );
        assert!(
            attempt
                .try_get::<Option<String>>("", "provider_name")
                .unwrap()
                .is_some(),
            "the channel is still known even when the upstream never answered"
        );
    }

    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].status_code, None,
        "an unmeasured status is NULL, not 502"
    );
}

/// History is a snapshot, not a join: renaming a channel afterwards must not
/// relabel the requests it already served.
#[tokio::test]
async fn renaming_a_provider_does_not_rewrite_the_stored_label() {
    let f = fixture(success()).await;
    assert_eq!(
        request(&f, "/v1/chat/completions", chat()).await.status(),
        StatusCode::OK
    );
    sql(
        &f,
        "UPDATE providers SET name='renamed-after-the-fact' WHERE id=?",
        vec![f.providers[0].clone().into()],
    )
    .await;

    let (_, provider, _, provider_id) = newest_execution(&f).await;
    assert_eq!(
        provider.as_deref(),
        Some("a"),
        "the execution keeps the name frozen at attempt creation"
    );
    assert_ne!(provider, provider_id);

    assert!(f.state.observations.clear_for_restore().await);
    assert_eq!(full::rebuild_projection(&f.state).await.unwrap(), 1);
    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].provider.as_deref(),
        Some("a"),
        "a rebuild reads the frozen label, not the current provider row"
    );
}

/// After a restore or a projection schema change the console re-derives every row
/// from the record system. The facts it shows must be the ones that were recorded.
#[tokio::test]
async fn rebuilding_the_projection_re_derives_the_exact_snapshots() {
    let f = fixture(rate_limited()).await;
    let response = request(&f, "/v1/chat/completions", chat()).await;
    drop(response);
    let before = projected(&f).await;
    assert_eq!(before.len(), 1);

    assert!(f.state.observations.clear_for_restore().await);
    assert_eq!(full::rebuild_projection(&f.state).await.unwrap(), 1);
    let after = projected(&f).await;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].status_code, Some(429));
    assert_eq!(after[0].status_code, before[0].status_code);
    assert_eq!(after[0].error_kind, before[0].error_kind);
    assert_eq!(after[0].error_kind.as_deref(), Some("failed"));
    assert_eq!(after[0].provider, before[0].provider);

    let (_, _, _, provider_id) = newest_execution(&f).await;
    assert_ne!(
        after[0].provider, provider_id,
        "the rebuild must not substitute the provider id for its display name"
    );
    assert!(matches!(after[0].provider.as_deref(), Some("a" | "b")));
}

/// A crash leaves an execution with no upstream answer and no live event. The
/// recovery path is then the only writer, so it records the classification that
/// belongs to the status it already writes — otherwise a restored instance would
/// report an interrupted request as neither a success nor a failure.
#[tokio::test]
async fn a_crash_recovered_execution_records_its_classification() {
    let f = fixture(success()).await;
    let now = db::now();
    sql(
        &f,
        "INSERT INTO traces(id,project_id,status,started_at) VALUES('crashed-trace',?,'running',?)",
        vec![db::DEFAULT_PROJECT_ID.into(), now.into()],
    )
    .await;
    sql(&f, "INSERT INTO requests(id,trace_id,protocol,endpoint,status,started_at) VALUES('crashed','crashed-trace','openai','/v1/chat/completions','running',?)", vec![now.into()]).await;
    sql(&f, "INSERT INTO request_facts(id,project_id,log_level,status,started_at) VALUES('crashed',?,'metadata','running',?)", vec![db::DEFAULT_PROJECT_ID.into(), now.into()]).await;
    sql(&f, "INSERT INTO execution_facts(id,request_id,provider_id,attempt,price_json,reserved_micros,contacted,status,started_at) VALUES('crashed-execution','crashed',?,1,'{}',0,1,'running',?)", vec![f.providers[0].clone().into(), now.into()]).await;
    sql(&f, "INSERT INTO request_executions(id,request_id,provider_id,provider_name,attempt,model,status,started_at) VALUES('crashed-execution','crashed',?,'a',1,'a-model','running',?)", vec![f.providers[0].clone().into(), now.into()]).await;

    ops::runtime::recover(&f.state).await.unwrap();
    let (status, provider, error, _) = newest_execution(&f).await;
    assert_eq!(status, None, "no upstream response was ever observed");
    assert_eq!(provider.as_deref(), Some("a"));
    assert_eq!(error.as_deref(), Some("interrupted"));

    assert!(f.state.observations.clear_for_restore().await);
    assert_eq!(full::rebuild_projection(&f.state).await.unwrap(), 1);
    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status_code, None);
    assert_eq!(
        rows[0].error_kind.as_deref(),
        Some("interrupted"),
        "the recovery path's own classification must survive the rebuild"
    );
    assert_eq!(rows[0].provider.as_deref(), Some("a"));
}

/// Rows written before the snapshot columns existed carry no status, no name and no
/// classification. Rebuilding them must say so instead of inventing `200`, `502` or
/// a provider UUID.
#[tokio::test]
async fn a_legacy_row_without_snapshots_rebuilds_to_unmeasured() {
    let f = fixture(success()).await;
    legacy_row(&f, "succeeded").await;
    assert!(f.state.observations.clear_for_restore().await);
    assert_eq!(full::rebuild_projection(&f.state).await.unwrap(), 1);
    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].status_code, None,
        "a row that never recorded a status must not rebuild to 200"
    );
    assert_eq!(
        rows[0].provider, None,
        "a provider id is not a provider name"
    );
    assert_eq!(
        rows[0].error_kind, None,
        "a status is not a failure classification"
    );
}

/// A failed legacy row must not rebuild to a synthesized `502` with the execution
/// status standing in for the failure classification.
#[tokio::test]
async fn a_legacy_failure_rebuilds_without_a_synthesized_502() {
    let f = fixture(success()).await;
    legacy_row(&f, "failed").await;
    assert!(f.state.observations.clear_for_restore().await);
    assert_eq!(full::rebuild_projection(&f.state).await.unwrap(), 1);
    let rows = projected(&f).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status_code, None, "502 was never observed");
    assert_eq!(
        rows[0].error_kind, None,
        "the execution status is not a classification"
    );
    assert_eq!(rows[0].provider, None);
}

/// Logging level `off` is a deliberate refusal to keep a browsable record. The
/// projection never sees those requests, and a rebuild must not pull them back in
/// from the record system just because the tables still hold them.
#[tokio::test]
async fn a_rebuild_leaves_off_level_requests_outside_the_projection() {
    let f = fixture(measured_usage()).await;
    legacy_row(&f, "succeeded").await;
    sql(
        &f,
        "UPDATE request_facts SET log_level='off' WHERE id='legacy-request'",
        vec![],
    )
    .await;
    assert!(f.state.observations.clear_for_restore().await);
    assert_eq!(
        full::rebuild_projection(&f.state).await.unwrap(),
        0,
        "a request the site policy refuses to log is not browsable"
    );
    assert!(projected(&f).await.is_empty());
}
