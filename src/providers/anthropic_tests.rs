use super::*;
use crate::{
    api::AppState,
    config::Config,
    crypto::SecretBox,
    db,
    models::{ApiKeyInput, ModelInput, ProviderInput},
    observability::ObservationStore,
};
use axum::{Json, Router, body::Body, http::Request, routing::post};
use sea_orm::ConnectionTrait;
use serde_json::json;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tower::ServiceExt;

fn function_tools() -> Value {
    json!([{
        "name": "rack_status",
        "description": "Read a rack's service state.",
        "input_schema": {
            "type": "object",
            "properties": {"rack": {"type": "string", "enum": ["a", "b"]}},
            "required": ["rack"],
            "additionalProperties": false
        }
    }])
}

fn tool_response() -> Value {
    json!({
        "id": "chatcmpl_rack_contract",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": null, "refusal": null, "annotations": [], "tool_calls": [{
                "id": "call_rack_a",
                "type": "function",
                "function": {"name": "rack_status", "arguments": "{\"rack\":\"a\"}"}
            }]},
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 21,
            "completion_tokens": 9,
            "total_tokens": 30,
            "prompt_tokens_details": {"cached_tokens": 5},
            "completion_tokens_details": {"reasoning_tokens": 3}
        }
    })
}

#[test]
fn anthropic_tool_history_keeps_text_ids_and_json() {
    let converted = transforms::messages_to_chat(&json!({
        "model": "rack-model",
        "max_tokens": 96,
        "messages": [
            {"role": "user", "content": "Check two racks."},
            {"role": "assistant", "content": [
                {"type": "text", "text": "Checking the racks."},
                {"type": "tool_use", "id": "call_rack_a", "name": "rack_status", "input": {"rack": "a", "verbose": true}},
                {"type": "tool_use", "id": "call_rack_b", "name": "rack_status", "input": {"rack": "b"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_rack_a", "content": "ready"},
                {"type": "tool_result", "tool_use_id": "call_rack_b", "content": [{"type": "text", "text": "needs "}, {"type": "text", "text": "service"}]},
                {"type": "text", "text": "Summarize."}
            ]}
        ]
    }))
    .unwrap();

    assert_eq!(converted["max_tokens"], 96);
    assert_eq!(converted["messages"].as_array().unwrap().len(), 5);
    assert_eq!(converted["messages"][1]["content"], "Checking the racks.");
    let calls = converted["messages"][1]["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["id"], "call_rack_a");
    assert_eq!(calls[0]["type"], "function");
    assert_eq!(calls[0]["function"]["name"], "rack_status");
    assert_eq!(
        serde_json::from_str::<Value>(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap(),
        json!({"rack": "a", "verbose": true})
    );
    assert_eq!(calls[1]["id"], "call_rack_b");
    assert_eq!(
        converted["messages"][2],
        json!({"role": "tool", "tool_call_id": "call_rack_a", "content": "ready"})
    );
    assert_eq!(
        converted["messages"][3],
        json!({"role": "tool", "tool_call_id": "call_rack_b", "content": "needs service"})
    );
    assert_eq!(
        converted["messages"][4],
        json!({"role": "user", "content": "Summarize."})
    );
}

#[test]
fn anthropic_function_definitions_and_choices_preserve_constraints() {
    for (choice, expected, parallel) in [
        (json!({"type": "auto"}), json!("auto"), None),
        (
            json!({"type": "any", "disable_parallel_tool_use": false}),
            json!("required"),
            Some(true),
        ),
        (json!({"type": "none"}), json!("none"), None),
        (
            json!({"type": "tool", "name": "rack_status", "disable_parallel_tool_use": true}),
            json!({"type": "function", "function": {"name": "rack_status"}}),
            Some(false),
        ),
    ] {
        let converted = transforms::messages_to_chat(&json!({
            "model": "rack-model", "max_tokens": 96,
            "messages": [{"role": "user", "content": "Check rack a."}],
            "tools": function_tools(), "tool_choice": choice,
        }))
        .unwrap();
        assert_eq!(
            converted["tools"],
            json!([{
                "type": "function", "function": {
                    "name": "rack_status", "description": "Read a rack's service state.",
                    "parameters": {"type": "object", "properties": {"rack": {"type": "string", "enum": ["a", "b"]}}, "required": ["rack"], "additionalProperties": false}
                }
            }])
        );
        assert_eq!(converted["tool_choice"], expected);
        assert_eq!(
            converted
                .get("parallel_tool_calls")
                .and_then(Value::as_bool),
            parallel
        );
    }
}

#[test]
fn anthropic_tool_responses_keep_multiple_calls_and_optional_text() {
    let mut upstream = tool_response();
    upstream["choices"][0]["message"]["content"] = json!("Checking the racks.");
    upstream["choices"][0]["message"]["tool_calls"].as_array_mut().unwrap().push(json!({
        "id": "call_rack_b", "type": "function",
        "function": {"name": "rack_status", "arguments": "{\"rack\":\"b\",\"codes\":[2,4],\"note\":\"控制\"}"}
    }));
    let transform = ResponseTransform::ChatMessages("rack-public".into());
    let source = usage::response_usage(&transform, &upstream, "/v1/messages");
    let converted = transform.apply(upstream).unwrap();
    assert_eq!(converted["id"], "chatcmpl_rack_contract");
    assert_eq!(converted["stop_reason"], "tool_use");
    assert_eq!(
        converted["content"],
        json!([
            {"type": "text", "text": "Checking the racks."},
            {"type": "tool_use", "id": "call_rack_a", "name": "rack_status", "input": {"rack": "a"}},
            {"type": "tool_use", "id": "call_rack_b", "name": "rack_status", "input": {"rack": "b", "codes": [2,4], "note": "控制"}}
        ])
    );
    assert_eq!(
        (
            source.input,
            source.output,
            source.cache_read,
            source.reasoning
        ),
        (21, 9, 5, 3)
    );
    assert!(source.reported && !source.invalid);
}

#[test]
fn anthropic_tool_requests_reject_unrepresentable_or_malformed_shapes() {
    let tool = json!({"type": "tool_use", "id": "call_rack_a", "name": "rack_status", "input": {"rack": "a"}});
    for (role, content) in [
        (
            "assistant",
            json!([tool, {"type": "text", "text": "after the call"}]),
        ),
        ("assistant", json!([tool, tool])),
        (
            "assistant",
            json!([{"type": "tool_use", "id": "", "name": "rack_status", "input": {}}]),
        ),
        (
            "assistant",
            json!([{"type": "tool_use", "id": "call_rack_a", "name": "", "input": {}}]),
        ),
        (
            "assistant",
            json!([{"type": "tool_use", "id": "call_rack_a", "name": "rack_status", "input": []}]),
        ),
        (
            "assistant",
            json!([{"type": "tool_use", "id": "call_rack_a", "name": "rack_status", "input": {}, "cache_control": {"type": "ephemeral"}}]),
        ),
        ("user", json!([tool])),
        (
            "assistant",
            json!([{"type": "tool_result", "tool_use_id": "call_rack_a", "content": "ready"}]),
        ),
        (
            "user",
            json!([{"type": "tool_result", "tool_use_id": "call_rack_a", "content": "failed", "is_error": true}]),
        ),
        (
            "user",
            json!([{"type": "tool_result", "tool_use_id": "call_rack_a", "is_error": "false"}]),
        ),
        (
            "user",
            json!([{"type": "tool_result", "tool_use_id": "", "content": "ready"}]),
        ),
        (
            "user",
            json!([{"type": "tool_result", "tool_use_id": "call_rack_a", "content": [{"type": "image", "source": {"type": "url", "url": "https://example.invalid/rack"}}]}]),
        ),
        (
            "user",
            json!([{"type": "text", "text": "before the result"}, {"type": "tool_result", "tool_use_id": "call_rack_a", "content": "ready"}]),
        ),
        (
            "assistant",
            json!([{"type": "thinking", "thinking": "private", "signature": "opaque"}, tool]),
        ),
    ] {
        let payload =
            json!({"model": "rack-model", "messages": [{"role": role, "content": content}]});
        assert!(transforms::messages_to_chat(&payload).is_err(), "{payload}");
    }
    for fields in [
        json!({"tools": [{"type": "computer_20250124", "name": "computer", "display_width_px": 640}]}),
        json!({"tools": [{"name": "rack_status", "input_schema": []}]}),
        json!({"tools": [{"name": "rack_status", "description": null, "input_schema": {}}]}),
        json!({"tools": [{"name": "rack_status", "input_schema": {}, "defer_loading": true}]}),
        json!({"tool_choice": {"type": "any"}}),
        json!({"tools": function_tools(), "tool_choice": {"type": "tool", "name": "not_declared"}}),
        json!({"tools": function_tools(), "tool_choice": {"type": "auto", "name": "rack_status"}}),
        json!({"tools": function_tools(), "tool_choice": {"type": "auto", "disable_parallel_tool_use": "true"}}),
        json!({"tools": function_tools(), "tool_choice": {"type": "auto", "other_flag": true}}),
        json!({"tools": function_tools(), "thinking": {"type": "enabled", "budget_tokens": 48}}),
        json!({"tools": function_tools(), "stream": true}),
    ] {
        let mut payload =
            json!({"model": "rack-model", "messages": [{"role": "user", "content": "Check."}]});
        payload
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        assert!(transforms::messages_to_chat(&payload).is_err(), "{payload}");
    }
}

#[test]
fn anthropic_responses_accept_empty_standard_wire_metadata() {
    for text_only in [false, true] {
        let mut baseline = tool_response();
        let message = baseline["choices"][0]["message"].as_object_mut().unwrap();
        message.remove("refusal");
        message.remove("annotations");
        if text_only {
            message.remove("tool_calls");
            message.insert("content".into(), json!("Rack a is ready."));
            baseline["choices"][0]["finish_reason"] = json!("stop");
        }
        for metadata in [
            json!({}),
            json!({"refusal": null}),
            json!({"refusal": ""}),
            json!({"annotations": []}),
            json!({"refusal": null, "annotations": []}),
            json!({"refusal": "", "annotations": []}),
        ] {
            let mut upstream = baseline.clone();
            upstream["choices"][0]["message"]
                .as_object_mut()
                .unwrap()
                .extend(metadata.as_object().unwrap().clone());
            let converted = ResponseTransform::ChatMessages("rack-public".into())
                .apply(upstream)
                .unwrap_or_else(|error| {
                    panic!("empty wire metadata {metadata} was rejected: {error}")
                });
            assert_eq!(
                converted["stop_reason"],
                if text_only { "end_turn" } else { "tool_use" }
            );
            assert_eq!(
                converted["content"],
                if text_only {
                    json!([{"type": "text", "text": "Rack a is ready."}])
                } else {
                    json!([{"type": "tool_use", "id": "call_rack_a", "name": "rack_status", "input": {"rack": "a"}}])
                }
            );
        }
    }
}

#[test]
fn anthropic_responses_reject_meaningful_malformed_and_unknown_wire_metadata() {
    for text_only in [false, true] {
        for (field, value) in [
            ("refusal", json!("metadata-private-sentinel")),
            ("refusal", json!(" ")),
            ("refusal", json!({})),
            ("refusal", json!([])),
            ("refusal", json!(false)),
            ("refusal", json!(0)),
            (
                "annotations",
                json!([{"type": "url_citation", "url_citation": {"url": "https://example.invalid/rack", "title": "metadata-private-sentinel", "start_index": 0, "end_index": 4}}]),
            ),
            ("annotations", json!([null])),
            ("annotations", Value::Null),
            ("annotations", json!({})),
            ("annotations", json!("")),
            ("annotations", json!(false)),
            ("provider_extension", Value::Null),
            ("provider_extension", json!([])),
        ] {
            let mut upstream = tool_response();
            if text_only {
                upstream["choices"][0]["message"]
                    .as_object_mut()
                    .unwrap()
                    .remove("tool_calls");
                upstream["choices"][0]["message"]["content"] = json!("Rack a is ready.");
                upstream["choices"][0]["finish_reason"] = json!("stop");
            }
            upstream["choices"][0]["message"][field] = value;
            let error = ResponseTransform::ChatMessages("rack-public".into())
                .apply(upstream)
                .expect_err("unsupported wire metadata must not be silently discarded");
            assert!(!error.to_string().contains("metadata-private-sentinel"));
        }
    }
}

#[test]
fn anthropic_tool_responses_reject_bad_arguments_without_echoing_them() {
    for arguments in ["{private-argument-sentinel", "[]", "null", "\"scalar\""] {
        let mut upstream = tool_response();
        upstream["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
            json!(arguments);
        let error = ResponseTransform::ChatMessages("rack-public".into())
            .apply(upstream)
            .unwrap_err();
        assert!(!error.to_string().contains(arguments));
    }
}

#[test]
fn anthropic_tool_responses_reject_ambiguous_finish_reasons_and_call_shapes() {
    for reason in [
        json!("stop"),
        json!("content_filter"),
        json!("function_call"),
        json!("provider_unknown"),
        Value::Null,
    ] {
        let mut upstream = tool_response();
        upstream["choices"][0]["finish_reason"] = reason;
        assert!(
            ResponseTransform::ChatMessages("rack-public".into())
                .apply(upstream)
                .is_err()
        );
    }
    for call in [
        json!({"id": "call_rack_a", "type": "custom", "custom": {"name": "rack_status", "input": "arbitrary"}}),
        json!({"id": "", "type": "function", "function": {"name": "rack_status", "arguments": "{}"}}),
        json!({"id": "call_rack_a", "type": "function", "function": {"name": "", "arguments": "{}"}}),
        json!({"id": "call_rack_a", "type": "function", "function": {"name": "rack_status", "arguments": {}, "extra": true}}),
        json!({"id": "call_rack_a", "type": "function", "function": {"name": "rack_status", "arguments": "{}"}, "extra": true}),
    ] {
        let mut upstream = tool_response();
        upstream["choices"][0]["message"]["tool_calls"] = json!([call]);
        assert!(
            ResponseTransform::ChatMessages("rack-public".into())
                .apply(upstream)
                .is_err()
        );
    }
    let mut duplicate = tool_response();
    let call = duplicate["choices"][0]["message"]["tool_calls"][0].clone();
    duplicate["choices"][0]["message"]["tool_calls"]
        .as_array_mut()
        .unwrap()
        .push(call);
    assert!(
        ResponseTransform::ChatMessages("rack-public".into())
            .apply(duplicate)
            .is_err()
    );
    let mut empty = tool_response();
    empty["choices"][0]["message"]["tool_calls"] = json!([]);
    assert!(
        ResponseTransform::ChatMessages("rack-public".into())
            .apply(empty)
            .is_err()
    );
}

#[test]
fn anthropic_text_stop_reasons_are_mapped_or_explicitly_rejected() {
    for (reason, expected) in [("stop", "end_turn"), ("length", "max_tokens")] {
        let converted = ResponseTransform::ChatMessages("rack-public".into()).apply(json!({
            "choices": [{"message": {"role": "assistant", "content": "ready"}, "finish_reason": reason}]
        })).unwrap();
        assert_eq!(
            converted["content"],
            json!([{"type": "text", "text": "ready"}])
        );
        assert_eq!(converted["stop_reason"], expected);
    }
    for reason in [
        json!("content_filter"),
        json!("tool_calls"),
        json!("function_call"),
        json!("provider_unknown"),
        Value::Null,
    ] {
        assert!(ResponseTransform::ChatMessages("rack-public".into()).apply(json!({
            "choices": [{"message": {"role": "assistant", "content": "ready"}, "finish_reason": reason}]
        })).is_err());
    }
}

#[test]
fn anthropic_empty_tool_results_and_tool_only_history_keep_their_meaning() {
    let converted = transforms::messages_to_chat(&json!({"model": "rack-model", "messages": [
        {"role": "assistant", "content": [{"type": "tool_use", "id": "call_rack_a", "name": "rack_status", "input": {}}]},
        {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_rack_a", "is_error": false}]}
    ]})).unwrap();
    assert!(converted["messages"][0]["content"].is_null());
    assert_eq!(
        converted["messages"][0]["tool_calls"][0]["function"]["arguments"],
        "{}"
    );
    assert_eq!(
        converted["messages"][1],
        json!({"role": "tool", "tool_call_id": "call_rack_a", "content": ""})
    );
}

#[tokio::test]
async fn anthropic_and_openai_native_tool_shapes_are_not_converted_or_rejected() {
    for (kind, endpoint, payload, upstream) in [
        (
            "anthropic",
            "/v1/messages",
            json!({
                "model": "rack-model", "max_tokens": 96, "stream": true,
                "messages": [{"role": "user", "content": [{"type": "text", "text": "Check.", "cache_control": {"type": "ephemeral"}}]}],
                "tools": [{"type": "bash_20250124", "name": "bash"}],
                "thinking": {"type": "enabled", "budget_tokens": 48},
                "vendor_extension": {"preserve": true}
            }),
            json!({
                "type": "message", "content": [{"type": "thinking", "thinking": "native", "signature": "opaque"}],
                "usage": {"input_tokens": 21, "output_tokens": 9, "cache_read_input_tokens": 5}
            }),
        ),
        (
            "openai",
            "/v1/chat/completions",
            json!({
                "model": "rack-model", "messages": [{"role": "user", "content": "Check."}],
                "tools": [{"type": "custom", "name": "rack_script", "format": {"type": "text"}}],
                "reasoning_effort": "high", "max_completion_tokens": 96, "vendor_extension": {"preserve": true}
            }),
            json!({
                "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [{"id": "call_native", "type": "custom", "custom": {"name": "rack_script", "input": "native code"}}]}, "finish_reason": "tool_calls"}],
                "usage": {"prompt_tokens": 21, "completion_tokens": 9, "prompt_tokens_details": {"cached_tokens": 5}}
            }),
        ),
    ] {
        let target = RouteTarget {
            public_name: "rack-public".into(),
            upstream_name: "rack-model".into(),
            provider_name: "native-contract".into(),
            provider_kind: kind.into(),
            base_url: "http://127.0.0.1:8080".into(),
            credential_type: "api_key".into(),
            secret_envelope: String::new(),
            proxy_url: None,
            proxy_username: None,
            proxy_secret_envelope: None,
            proxy_reuse_connections: true,
            proxy_preset_id: None,
        };
        let prepared = prepare(
            &target,
            endpoint,
            &payload,
            "mock-native-key",
            HeaderMap::new(),
            &HeaderMap::new(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(prepared.payload, payload, "{kind}");
        assert!(prepared.response.identity(), "{kind}");
        assert_eq!(
            prepared.response.apply(upstream.clone()).unwrap(),
            upstream,
            "{kind}"
        );
    }
}

struct ToolGateway {
    state: AppState,
    token: String,
    server: tokio::task::JoinHandle<()>,
    _directory: tempfile::TempDir,
}

impl Drop for ToolGateway {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl ToolGateway {
    async fn new(mock: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
        let directory = tempfile::tempdir().unwrap();
        let database = db::connect("sqlite::memory:").await.unwrap();
        let secrets = SecretBox::load(directory.path(), None).unwrap();
        let (_, token) = db::create_api_key(
            &database,
            &ApiKeyInput {
                name: "tool-contract".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let provider = db::create_provider(
            &database,
            &ProviderInput {
                name: "tool-contract".into(),
                kind: "openai".into(),
                base_url: format!("http://{address}"),
                api_key: String::new(),
            },
            secrets.encrypt("mock-function-provider-key").unwrap(),
        )
        .await
        .unwrap();
        db::create_model(
            &database,
            &ModelInput {
                provider_id: provider.id,
                public_name: "rack-public".into(),
                upstream_name: "rack-upstream".into(),
                capabilities: Some(vec!["chat".into()]),
                pricing_configured: None,
                input_price_micros: None,
                output_price_micros: None,
                priority: None,
            },
            db::DEFAULT_PROJECT_ID,
        )
        .await
        .unwrap();
        let state = AppState {
            db: database,
            config: Arc::new(Config {
                bind: "127.0.0.1:0".parse().unwrap(),
                data_dir: directory.path().into(),
                database_url: "sqlite::memory:".into(),
                observation_path: directory.path().join("events.duckdb"),
                observation_retention_days: 30,
                public_url: None,
                session_secure: false,
                capture_payloads: false,
                upstream_timeout: Duration::from_secs(3),
                admin_email: None,
                admin_password: None,
                master_key: None,
            }),
            secrets,
            observations: ObservationStore::degraded(
                directory.path().join("events.duckdb"),
                "mock projection unavailable",
            ),
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            oidc_client: reqwest::Client::new(),
            oauth_client: crate::oauth::HttpClient::new(Duration::from_secs(3)).unwrap(),
            budget_locks: Arc::new(Mutex::new(HashMap::new())),
            maintenance: Arc::new(tokio::sync::RwLock::new(())),
            orchestrator: Arc::new(crate::orchestration::Runtime::default()),
        };
        Self {
            state,
            token,
            server,
            _directory: directory,
        }
    }

    async fn request(&self, body: Value) -> (http::StatusCode, Value) {
        let response = crate::api::router(self.state.clone())
            .oneshot(
                Request::post("/v1/messages")
                    .header("authorization", format!("Bearer {}", self.token))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
}

#[tokio::test]
async fn anthropic_function_tools_round_trip_through_gateway_and_keep_source_usage() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = seen.clone();
    let mock = Router::new().route("/v1/chat/completions", post(move |headers: HeaderMap, Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            let mut seen = captured.lock().await;
            seen.push((headers, body));
            Json(if seen.len() == 1 { tool_response() } else { json!({
                "id": "chatcmpl_rack_done",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "Rack a is ready.", "refusal": null, "annotations": []}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 29, "completion_tokens": 6, "total_tokens": 35}
            }) })
        }
    }));
    let gateway = ToolGateway::new(mock).await;
    let mut request = json!({
        "model": "rack-public",
        "max_tokens": 96,
        "messages": [{"role": "user", "content": "Check rack a."}],
        "tools": function_tools(),
        "tool_choice": {"type": "auto", "disable_parallel_tool_use": true}
    });
    let (status, first) = gateway.request(request.clone()).await;
    assert_eq!(status, http::StatusCode::OK, "{first}");
    assert_eq!(first["model"], "rack-public");
    assert_eq!(first["stop_reason"], "tool_use");
    assert_eq!(
        first["content"],
        json!([{"type": "tool_use", "id": "call_rack_a", "name": "rack_status", "input": {"rack": "a"}}])
    );

    let facts = gateway
        .state
        .db
        .query_all(crate::operations::sql(
            "SELECT input_tokens,output_tokens,cache_read_tokens,reasoning_tokens FROM usage_logs",
            vec![],
        ))
        .await
        .unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].try_get::<i64>("", "input_tokens").unwrap(), 21);
    assert_eq!(facts[0].try_get::<i64>("", "output_tokens").unwrap(), 9);
    assert_eq!(facts[0].try_get::<i64>("", "cache_read_tokens").unwrap(), 5);
    assert_eq!(facts[0].try_get::<i64>("", "reasoning_tokens").unwrap(), 3);

    request["messages"].as_array_mut().unwrap().extend([
        json!({"role": "assistant", "content": first["content"]}),
        json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_rack_a", "content": "ready"}]}),
    ]);
    let (status, second) = gateway.request(request).await;
    assert_eq!(status, http::StatusCode::OK, "{second}");
    assert_eq!(second["stop_reason"], "end_turn");
    assert_eq!(
        second["content"],
        json!([{"type": "text", "text": "Rack a is ready."}])
    );

    let seen = seen.lock().await;
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0].0["authorization"],
        "Bearer mock-function-provider-key"
    );
    assert_eq!(seen[0].1["model"], "rack-upstream");
    assert_eq!(
        seen[0].1["tools"][0]["function"]["parameters"],
        function_tools()[0]["input_schema"]
    );
    assert_eq!(seen[0].1["tool_choice"], "auto");
    assert_eq!(seen[0].1["parallel_tool_calls"], false);
    assert!(seen[1].1["messages"][1]["content"].is_null());
    assert_eq!(
        seen[1].1["messages"][1]["tool_calls"][0]["id"],
        "call_rack_a"
    );
    assert_eq!(
        seen[1].1["messages"][2],
        json!({"role": "tool", "tool_call_id": "call_rack_a", "content": "ready"})
    );
}
