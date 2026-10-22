use super::*;

#[test]
fn conflicting_output_aliases_fail_closed() {
    for payload in [
        json!({"max_completion_tokens":32,"max_tokens":128}),
        json!({"max_tokens":32,"max_output_tokens":128}),
        json!({"max_completion_tokens":32,"max_output_tokens":128}),
    ] {
        assert!(output_limit(&payload).is_err(), "{payload}");
    }
}

#[test]
fn matching_output_aliases_share_one_explicit_budget() {
    assert_eq!(
        output_limit(&json!({
            "max_tokens":73,
            "max_completion_tokens":73,
            "max_output_tokens":73,
            "reasoning":{"effort":"high"}
        }))
        .unwrap(),
        73
    );
}

#[tokio::test]
async fn native_token_controls_preserve_extensions_reasoning_and_absent_caps() {
    let fixture = database_fixture().await;
    let (provider, _) = add_model(&fixture, "native", "public", "actual").await;
    for (kind, endpoint, body) in [
        (
            "openai",
            "/v1/chat/completions",
            json!({"model":"public","messages":[{"role":"user","content":"hello"}],"reasoning_effort":"high","custom_vendor":{"enabled":true}}),
        ),
        (
            "openai",
            "/v1/chat/completions",
            json!({"model":"public","messages":[],"max_completion_tokens":73,"reasoning_effort":"medium","custom_vendor":{"enabled":true}}),
        ),
        (
            "openai",
            "/v1/responses",
            json!({"model":"public","input":"hello","reasoning":{"effort":"high"},"custom_vendor":{"enabled":true}}),
        ),
        (
            "openai",
            "/v1/responses",
            json!({"model":"public","input":"hello","max_output_tokens":73,"reasoning":{"effort":"medium"},"custom_vendor":{"enabled":true}}),
        ),
        (
            "anthropic",
            "/v1/messages",
            json!({"model":"public","messages":[],"max_tokens":2048,"thinking":{"type":"enabled","budget_tokens":1024},"custom_vendor":{"enabled":true}}),
        ),
        (
            "gemini",
            "/v1beta/models:generateContent",
            json!({"model":"public","contents":[],"custom_vendor":{"enabled":true}}),
        ),
        (
            "gemini",
            "/v1beta/models:generateContent",
            json!({"model":"public","contents":[],"generationConfig":{"maxOutputTokens":73,"thinkingConfig":{"thinkingBudget":32}},"custom_vendor":{"enabled":true}}),
        ),
    ] {
        sql(
            &fixture,
            "UPDATE providers SET kind=? WHERE id=?",
            vec![kind.into(), provider.clone().into()],
        )
        .await;
        let mut expected = body.clone();
        expected["model"] = json!("actual");
        let result = prepare(
            &fixture.db,
            &Runtime::default(),
            &fixture.key,
            load_profile(&fixture.db, &fixture.key).await.unwrap(),
            body,
            &HeaderMap::new(),
            endpoint,
        )
        .await
        .unwrap();
        assert_eq!(result.candidates.len(), 1, "{kind}: {endpoint}");
        let (payload, _, _) = result.attempt_payload(&result.candidates[0]).unwrap();
        assert_eq!(payload, expected, "{kind}: {endpoint}");
        let prepared = crate::providers::prepare(
            &result.candidates[0].target,
            endpoint,
            &payload,
            "contract-key",
            HeaderMap::new(),
            &HeaderMap::new(),
            None,
        )
        .await
        .unwrap();
        if kind == "gemini" {
            expected.as_object_mut().unwrap().remove("model");
        }
        assert_eq!(prepared.payload, expected, "{kind}: {endpoint}");
    }
}

#[tokio::test]
async fn tpm_reservation_honors_explicit_total_output_without_double_counting_reasoning() {
    let fixture = database_fixture().await;
    let (provider, _) = add_model(&fixture, "reserved", "public", "actual").await;
    sql(
        &fixture,
        "UPDATE projects SET settings_json=? WHERE id=?",
        vec![
            json!({"version":1,"routing":{"version":1,"limits":{"tpm":100000}}})
                .to_string()
                .into(),
            fixture.key.project_id.clone().into(),
        ],
    )
    .await;
    for (kind, endpoint, body, reserved_output) in [
        (
            "openai",
            "/v1/chat/completions",
            json!({"model":"public","messages":[],"max_completion_tokens":73,"n":3,"reasoning_effort":"high","custom_vendor":{"enabled":true}}),
            219,
        ),
        (
            "openai",
            "/v1/responses",
            json!({"model":"public","input":"hello","max_output_tokens":73,"reasoning":{"effort":"high"},"custom_vendor":{"enabled":true}}),
            73,
        ),
        (
            "anthropic",
            "/v1/messages",
            json!({"model":"public","messages":[],"max_tokens":2048,"thinking":{"type":"enabled","budget_tokens":1024},"custom_vendor":{"enabled":true}}),
            2048,
        ),
        (
            "gemini",
            "/v1beta/models:generateContent",
            json!({"model":"public","contents":[],"generationConfig":{"maxOutputTokens":73,"candidateCount":3,"thinkingConfig":{"thinkingBudget":32}},"custom_vendor":{"enabled":true}}),
            219,
        ),
    ] {
        sql(
            &fixture,
            "UPDATE providers SET kind=? WHERE id=?",
            vec![kind.into(), provider.clone().into()],
        )
        .await;
        let mut expected = body.clone();
        expected["model"] = json!("actual");
        let result = prepare(
            &fixture.db,
            &Runtime::default(),
            &fixture.key,
            load_profile(&fixture.db, &fixture.key).await.unwrap(),
            body,
            &HeaderMap::new(),
            endpoint,
        )
        .await
        .unwrap();
        let (payload, _, tokens) = result.attempt_payload(&result.candidates[0]).unwrap();
        assert_eq!(payload, expected, "{kind}: {endpoint}");
        assert_eq!(
            tokens,
            u32::try_from(serde_json::to_vec(&payload).unwrap().len()).unwrap() + reserved_output,
            "{kind}: {endpoint}"
        );
        let runtime = Runtime::default();
        assert!(matches!(
            runtime
                .admit(
                    "insufficient",
                    &Limits {
                        tpm: Some(tokens - 1),
                        ..Default::default()
                    },
                    tokens,
                )
                .await,
            Err(Error::Admission("tpm"))
        ));
        assert!(
            runtime
                .admit(
                    "exact",
                    &Limits {
                        tpm: Some(tokens),
                        ..Default::default()
                    },
                    tokens,
                )
                .await
                .is_ok()
        );
    }
}

#[tokio::test]
async fn byte_reservations_and_default_output_are_not_context_measurements() {
    let fixture = database_fixture().await;
    let (_, model) = add_model(&fixture, "unmatched", "public", "unmatched-tokenizer").await;
    sql(
        &fixture,
        "UPDATE models SET catalog_metadata_json=? WHERE id=?",
        vec![
            json!({"card":{"limits":{"context":1,"output":80}}})
                .to_string()
                .into(),
            model.into(),
        ],
    )
    .await;
    for mut body in [
        json!({"model":"public","messages":[{"role":"user","content":"longer than the context card"}]}),
        json!({"model":"public","messages":[{"role":"user","content":"longer than the context card"}],"max_completion_tokens":73}),
    ] {
        let result = plan(&fixture, body.clone()).await.unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert!(result.estimated_tokens > 80);
        body["model"] = json!("unmatched-tokenizer");
        assert_eq!(
            result.attempt_payload(&result.candidates[0]).unwrap().0,
            body
        );
    }
}
