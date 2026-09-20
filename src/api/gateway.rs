use super::*;
use crate::orchestration::{self, AttemptGuard, AttemptOutcome, policy::ErrorMode, stream as sse};
use futures_util::StreamExt;
use std::time::Duration;

pub(super) async fn execute(
    state: AppState,
    headers: HeaderMap,
    body: axum::body::Bytes,
    endpoint: &'static str,
) -> Result<Response, ApiError> {
    let payload = serde_json::from_slice(&body)
        .map_err(|_| ApiError::BadRequest("request body must be valid JSON".into()))?;
    execute_input(
        state,
        headers,
        super::protocols::Input::json(payload, endpoint),
    )
    .await
}

pub(super) async fn execute_input(
    state: AppState,
    headers: HeaderMap,
    input: super::protocols::Input,
) -> Result<Response, ApiError> {
    use super::protocols::Wire;
    let endpoint = input.endpoint;
    let started = Instant::now();
    let started_at = db::now();
    let request_id = context_id(&headers, "x-request-id", "req_");
    let trace_id = context_id(&headers, "x-trace-id", "");
    let initial = gateway_key(&state, &headers).await?;
    let initial_profile = orchestration::load_profile(&state.db, &initial).await?;
    let mut payload = input.payload.clone();
    if !payload.is_object() {
        return Err(ApiError::BadRequest(
            "request body must be a JSON object".into(),
        ));
    }
    if endpoint == "/v1/moderations" {
        if payload.get("model").is_none() || payload["model"] == "" {
            payload["model"] = json!("omni-moderation-latest");
        }
        if payload.get("input").is_none_or(|value| {
            value.is_null()
                || value.as_str().is_some_and(str::is_empty)
                || value.as_array().is_some_and(Vec::is_empty)
        }) {
            return Err(ApiError::BadRequest(
                "moderation input cannot be empty".into(),
            ));
        }
    }
    let requested = payload
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::BadRequest("model is required".into()))?
        .to_owned();
    if payload
        .get("stream")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(ApiError::BadRequest("stream must be a boolean".into()));
    }
    let streaming = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if streaming
        && !matches!(
            crate::providers::capability(endpoint),
            "chat" | "completions" | "responses" | "messages" | "gemini"
        )
    {
        return Err(ApiError::BadRequest(
            "streaming is unsupported for this endpoint".into(),
        ));
    }
    if streaming && (initial.budget_micros.is_some() || initial_profile.budget.is_some()) {
        return Err(ApiError::BadRequest("streaming requires reliable budget settlement and is disabled for budget-limited keys/profiles".into()));
    }
    if crate::providers::is_media(endpoint)
        && (initial.budget_micros.is_some() || initial_profile.budget.is_some())
    {
        return Err(ApiError::BadRequest("media requires unit-based budget settlement and is disabled for budget-limited keys/profiles".into()));
    }
    let _profile_budget = if initial_profile.budget.is_some() {
        Some(
            state
                .lock_budget_key(&format!(
                    "profile:{}",
                    initial_profile.id.as_deref().unwrap_or("")
                ))
                .await,
        )
    } else {
        None
    };
    let _key_budget = if initial.budget_micros.is_some() {
        Some(state.lock_budget_key(&format!("key:{}", initial.id)).await)
    } else {
        None
    };
    let credential = if _key_budget.is_some() || _profile_budget.is_some() {
        gateway_key(&state, &headers).await?
    } else {
        initial
    };
    let profile = orchestration::load_profile(&state.db, &credential).await?;
    if profile.id != initial_profile.id || profile.budget != initial_profile.budget {
        return Err(ApiError::Conflict(
            "API-key profile changed; retry the request".into(),
        ));
    }
    profile.map_model(&requested)?;
    if endpoint.starts_with("/v1/responses") {
        state
            .orchestrator
            .sessions
            .restore(&state.db, &state.secrets, &credential, &mut payload)
            .await?;
    }
    let mut plan = orchestration::prepare(
        &state.db,
        &state.orchestrator,
        &credential,
        profile,
        payload,
        &headers,
        endpoint,
    )
    .await?;
    if let Wire::Task {
        provider,
        credential,
        upstream_model,
        ..
    } = &input.wire
    {
        plan.candidates.retain(|candidate| {
            &candidate.provider_id == provider
                && &candidate.credential_id == credential
                && &candidate.target.upstream_name == upstream_model
        });
    }
    for decision in &plan.decisions {
        tracing::debug!(request_id,stage=decision.stage,candidate=?decision.candidate,reason=decision.reason,"routing decision");
    }
    if plan.candidates.is_empty() {
        return Err(ApiError::BadRequest(
            "no eligible route for requested model and endpoint".into(),
        ));
    }
    let mut reserved = plan.estimated_tokens;
    for candidate in &plan.candidates {
        reserved = reserved.max(plan.attempt_payload(candidate)?.2);
    }
    let mut key_permit = Some(
        state
            .orchestrator
            .admit(
                &format!("key:{}", credential.id),
                &plan.routing.limits,
                reserved,
            )
            .await?,
    );
    let captured = state
        .config
        .capture_payloads
        .then(|| redact_json(plan.payload.clone()).to_string());
    let mut attempts = 0usize;
    let mut contacted = false;
    let mut last_admission = None;
    let mut last_target = None;
    'candidates: for candidate in &plan.candidates {
        for _ in 0..candidate.retry.attempts {
            if attempts >= plan.routing.max_attempts {
                break 'candidates;
            }
            let (payload, mut extra_headers, tokens) = plan.attempt_payload(candidate)?;
            let mut attempt = match AttemptGuard::acquire(
                state.orchestrator.clone(),
                candidate,
                plan.sticky.clone(),
                tokens,
            )
            .await
            {
                Ok(permit) => permit,
                Err(orchestration::Error::Admission(reason)) => {
                    last_admission = Some(reason);
                    continue 'candidates;
                }
                Err(error) => return Err(error.into()),
            };
            attempts += 1;
            if attempts > 1 {
                key_permit
                    .as_ref()
                    .expect("key admission remains owned")
                    .reserve_retry_tokens(tokens)?;
            }
            if attempts > 1 && candidate.retry.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(candidate.retry.delay_ms)).await;
            }
            let target = &candidate.target;
            last_target = Some(target);
            let secret = state
                .secrets
                .decrypt(&target.secret_envelope)
                .map_err(ApiError::Internal)?;
            tracing::debug!(
                request_id,
                attempt = attempts,
                channel = candidate.provider_id,
                credential = candidate.credential_id,
                "upstream attempt"
            );
            if candidate.pass_user_agent
                && let Some(agent) = headers.get(header::USER_AGENT)
            {
                extra_headers.insert(header::USER_AGENT, agent.clone());
            }
            extra_headers.insert("x-request-id", HeaderValue::from_str(&request_id).unwrap());
            extra_headers.insert("x-trace-id", HeaderValue::from_str(&trace_id).unwrap());
            for name in ["x-thread-id", "x-session-id", "idempotency-key"] {
                if let Some(value) = headers.get(name)
                    && value.as_bytes().len() <= 256
                {
                    extra_headers.insert(name, value.clone());
                }
            }
            contacted = true;
            if endpoint == "/v1/chat/completions" && target.provider_kind == "anthropic" {
                let mapped_endpoint = if candidate.endpoint == endpoint {
                    "/v1/messages"
                } else {
                    &candidate.endpoint
                };
                let value = match call_anthropic_chat(
                    &state.client,
                    target,
                    &payload,
                    &secret,
                    &extra_headers,
                    mapped_endpoint,
                )
                .await
                {
                    Ok(value) => value,
                    Err(error) => {
                        if let Some(http) = error.downcast_ref::<AnthropicHttpError>() {
                            let retry = candidate
                                .retry
                                .retry_status(http.status.as_u16(), &http.body);
                            attempt.finish(AttemptOutcome::UpstreamFailure);
                            if retry {
                                continue;
                            }
                            return Ok(error_response(
                                http.status,
                                &candidate.retry,
                                Some(http.body.as_bytes().to_vec()),
                                endpoint,
                            ));
                        }
                        if error.downcast_ref::<reqwest::Error>().is_some() {
                            attempt.finish(AttemptOutcome::UpstreamFailure);
                            if candidate.retry.transport {
                                continue;
                            }
                            return Err(ApiError::Upstream("upstream transport failed".into()));
                        }
                        return Err(ApiError::BadRequest(
                            "unsupported Anthropic request or response shape".into(),
                        ));
                    }
                };
                let bytes = serde_json::to_vec(&value).map_err(|e| ApiError::Internal(e.into()))?;
                if candidate.retry.empty_success && empty_success(Some(&value), &bytes) {
                    attempt.finish(AttemptOutcome::UpstreamFailure);
                    continue;
                }
                let event = build_event(
                    &request_id,
                    &trace_id,
                    started_at,
                    started.elapsed().as_millis() as i64,
                    endpoint,
                    &credential.id,
                    &target.provider_name,
                    &requested,
                    &target.upstream_name,
                    200,
                    None,
                    &payload,
                    Some(&value),
                    captured.clone(),
                    state.config.capture_payloads,
                    target.input_price_micros,
                    target.output_price_micros,
                );
                db::add_api_key_spend(&state.db, &credential.id, event.cost_micros).await?;
                state.observations.record(event);
                attempt.finish(AttemptOutcome::Success);
                if let Some(permit) = &mut key_permit {
                    permit.finish(true);
                }
                return Ok(response(
                    StatusCode::OK,
                    HeaderValue::from_static("application/json"),
                    bytes,
                    &request_id,
                    &trace_id,
                ));
            }
            let mut prepared = crate::providers::prepare_routed(
                target,
                crate::providers::Route {
                    protocol: endpoint,
                    path: &candidate.endpoint,
                    native_version: match &input.wire {
                        Wire::Gemini { version } => Some(version.as_str()),
                        _ => None,
                    },
                },
                &payload,
                &secret,
                extra_headers,
                &headers,
            )
            .await?;
            if streaming && !prepared.response.identity() {
                return Err(ApiError::BadRequest("cross-protocol streaming is unsupported for this provider; use its native endpoint".into()));
            }
            let method = if let Wire::Task { id, delete, .. } = &input.wire {
                prepared.url.push('/');
                prepared.url.push_str(&crate::providers::segment(id));
                crate::providers::request_method(*delete)
            } else {
                http::Method::POST
            };
            let (outbound, content_type) = input.body(&prepared.payload)?;
            prepared.headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&content_type)
                    .map_err(|_| ApiError::BadRequest("invalid content type".into()))?,
            );
            let request = state
                .client
                .request(method, &prepared.url)
                .headers(prepared.headers);
            let upstream = match request.body(outbound).send().await {
                Ok(upstream) => upstream,
                Err(_) => {
                    attempt.finish(AttemptOutcome::UpstreamFailure);
                    if candidate.retry.transport {
                        continue;
                    }
                    return Err(ApiError::Upstream("upstream transport failed".into()));
                }
            };
            let status = upstream.status();
            let content_type = upstream
                .headers()
                .get(header::CONTENT_TYPE)
                .cloned()
                .unwrap_or_else(|| HeaderValue::from_static("application/json"));
            if !status.is_success() {
                let bytes = read_body(upstream).await.unwrap_or_default();
                let retry = candidate
                    .retry
                    .retry_status(status.as_u16(), &String::from_utf8_lossy(&bytes));
                attempt.finish(AttemptOutcome::UpstreamFailure);
                if retry {
                    continue;
                }
                if let Some(permit) = &mut key_permit {
                    permit.finish(false);
                }
                state.observations.record(build_event(
                    &request_id,
                    &trace_id,
                    started_at,
                    started.elapsed().as_millis() as i64,
                    endpoint,
                    &credential.id,
                    &target.provider_name,
                    &requested,
                    &target.upstream_name,
                    status.as_u16() as i32,
                    Some("upstream_http".into()),
                    &payload,
                    None,
                    captured.clone(),
                    false,
                    0,
                    0,
                ));
                let mut response = error_response(status, &candidate.retry, Some(bytes), endpoint);
                response
                    .headers_mut()
                    .insert("x-request-id", HeaderValue::from_str(&request_id).unwrap());
                return Ok(response);
            }
            if streaming {
                if !content_type
                    .to_str()
                    .is_ok_and(|v| v.starts_with("text/event-stream"))
                {
                    attempt.finish(AttemptOutcome::UpstreamFailure);
                    if candidate.retry.transport {
                        continue;
                    }
                    return Err(ApiError::Upstream("upstream did not return SSE".into()));
                }
                let mut events = sse::events(upstream);
                let first =
                    match sse::next(&mut events, candidate.retry.first_event_timeout_ms).await {
                        Ok(Some(event)) if !sse::failed(&event) => event,
                        _ => {
                            attempt.finish(AttemptOutcome::UpstreamFailure);
                            if candidate.retry.transport {
                                continue;
                            }
                            return Err(ApiError::Upstream(
                                "upstream stream failed before first event".into(),
                            ));
                        }
                    };
                let mut guard = StreamEventGuard::new(
                    state.observations.clone(),
                    RequestEvent {
                        request_id: request_id.clone(),
                        trace_id: trace_id.clone(),
                        started_at,
                        finished_at: 0,
                        endpoint: endpoint.into(),
                        api_key_id: Some(credential.id.clone()),
                        provider: Some(target.provider_name.clone()),
                        requested_model: Some(requested.clone()),
                        resolved_model: Some(target.upstream_name.clone()),
                        status_code: status.as_u16() as i32,
                        error_kind: None,
                        latency_ms: 0,
                        ttft_ms: None,
                        input_tokens: 0,
                        output_tokens: 0,
                        cached_tokens: 0,
                        cost_micros: 0,
                        payload_captured: state.config.capture_payloads,
                        request_json: captured.clone(),
                        response_json: None,
                    },
                    started,
                );
                guard.mark_first_byte();
                let mut key = key_permit.take().expect("one downstream response");
                let runtime = state.orchestrator.clone();
                let database = state.db.clone();
                let secrets = state.secrets.clone();
                let session_credential = credential.clone();
                let session_request = plan.session_request.clone();
                let timeout = candidate.retry.event_timeout_ms;
                let error_policy = candidate.retry.clone();
                let mut terminal_state = sse::TerminalState::new(&payload);
                let output = async_stream::stream! {
                    let mut event=first;
                    loop {
                        let terminal=terminal_state.terminal(&event,endpoint);
                        let failed=sse::failed(&event);
                        if let Some(response)=sse::completed_response(&event)
                            && runtime.sessions.persist(&database,&secrets,&session_credential,&session_request,&response).await.is_err() {
                            attempt.finish(AttemptOutcome::LocalFailure);key.finish(false);guard.fail("session_storage_error");
                            yield Err(std::io::Error::other("could not persist response session"));return;
                        }
                        if failed {attempt.finish(AttemptOutcome::UpstreamFailure);key.finish(false);guard.fail("upstream_stream_error");}
                        if terminal&&!failed {attempt.finish(AttemptOutcome::Success);key.finish(true);guard.complete();}
                        if failed {event=sse::error_event(event,&error_policy,endpoint);}
                        if terminal||failed {
                            // Terminal delivery must not hold upstream permits or a
                            // socket open while the client pauses after this event.
                            drop(events);drop(attempt);drop(key);
                            yield Ok::<_,std::io::Error>(sse::encode(&event));return;
                        }
                        // Once this yield is reached this body owns the attempt permanently.
                        // There is deliberately no path back into the candidate/retry loop.
                        yield Ok::<_,std::io::Error>(sse::encode(&event));
                        event=match sse::next(&mut events,timeout).await {
                            Ok(Some(event))=>event,
                            Ok(None)=>{
                                attempt.finish(AttemptOutcome::UpstreamFailure);key.finish(false);guard.fail("incomplete_stream");
                                let error=sse::interrupted_event(&error_policy,endpoint);
                                drop(events);drop(attempt);drop(key);
                                yield Ok::<_,std::io::Error>(sse::encode(&error));return;
                            },
                            Err(_)=>{
                                attempt.finish(AttemptOutcome::UpstreamFailure);key.finish(false);guard.fail("stream_error");
                                let error=sse::interrupted_event(&error_policy,endpoint);
                                drop(events);drop(attempt);drop(key);
                                yield Ok::<_,std::io::Error>(sse::encode(&error));return;
                            }
                        };
                    }
                };
                let mut response = Response::new(Body::from_stream(output));
                *response.status_mut() = status;
                response
                    .headers_mut()
                    .insert(header::CONTENT_TYPE, content_type);
                response
                    .headers_mut()
                    .insert("x-request-id", HeaderValue::from_str(&request_id).unwrap());
                response
                    .headers_mut()
                    .insert("x-trace-id", HeaderValue::from_str(&trace_id).unwrap());
                return Ok(response);
            }
            let mut bytes = match read_body(upstream).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    attempt.finish(AttemptOutcome::UpstreamFailure);
                    if candidate.retry.transport {
                        continue;
                    }
                    return Err(ApiError::Upstream("upstream body failed".into()));
                }
            };
            let mut response_json = serde_json::from_slice::<Value>(&bytes).ok();
            if !prepared.response.identity() {
                let value =
                    prepared
                        .response
                        .apply(response_json.take().ok_or_else(|| {
                            ApiError::Upstream("provider response is not JSON".into())
                        })?)?;
                bytes = serde_json::to_vec(&value).map_err(|e| ApiError::Internal(e.into()))?;
                response_json = Some(value);
            }
            if status != StatusCode::NO_CONTENT
                && candidate.retry.empty_success
                && empty_success(response_json.as_ref(), &bytes)
            {
                attempt.finish(AttemptOutcome::UpstreamFailure);
                continue;
            }
            let event = build_event(
                &request_id,
                &trace_id,
                started_at,
                started.elapsed().as_millis() as i64,
                endpoint,
                &credential.id,
                &target.provider_name,
                &requested,
                &target.upstream_name,
                status.as_u16() as i32,
                None,
                &payload,
                response_json.as_ref(),
                captured.clone(),
                state.config.capture_payloads,
                target.input_price_micros,
                target.output_price_micros,
            );
            db::add_api_key_spend(&state.db, &credential.id, event.cost_micros).await?;
            state.observations.record(event);
            if matches!(
                endpoint,
                "/v1/videos" | "/doubao/v3/contents/generations/tasks"
            ) {
                super::protocols::persist(
                    &state,
                    &credential,
                    &input,
                    candidate,
                    &requested,
                    response_json.as_ref(),
                )
                .await?;
            }
            if endpoint == "/v1/responses"
                && let Some(value) = &response_json
            {
                state
                    .orchestrator
                    .sessions
                    .persist(
                        &state.db,
                        &state.secrets,
                        &credential,
                        &plan.session_request,
                        value,
                    )
                    .await?;
            }
            attempt.finish(AttemptOutcome::Success);
            if let Some(permit) = &mut key_permit {
                permit.finish(true);
            }
            return Ok(response(
                status,
                content_type,
                bytes,
                &request_id,
                &trace_id,
            ));
        }
    }
    if !contacted {
        return Err(ApiError::RateLimited(
            last_admission.unwrap_or("no_available_channel").into(),
        ));
    }
    state.observations.record(RequestEvent {
        request_id,
        trace_id,
        started_at,
        finished_at: db::now(),
        endpoint: endpoint.into(),
        api_key_id: Some(credential.id),
        provider: last_target.map(|t| t.provider_name.clone()),
        requested_model: Some(requested),
        resolved_model: last_target.map(|t| t.upstream_name.clone()),
        status_code: 502,
        error_kind: Some("upstream_unavailable".into()),
        latency_ms: started.elapsed().as_millis() as i64,
        ttft_ms: None,
        input_tokens: 0,
        output_tokens: 0,
        cached_tokens: 0,
        cost_micros: 0,
        payload_captured: state.config.capture_payloads,
        request_json: captured,
        response_json: None,
    });
    Err(ApiError::Upstream(
        "all eligible upstream attempts failed".into(),
    ))
}

fn context_id(headers: &HeaderMap, name: &str, prefix: &str) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 128
                && v.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
        })
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{prefix}{}", Uuid::new_v4().simple()))
}

pub(super) fn discovery_response(
    state: &AppState,
    headers: &HeaderMap,
    key: &crate::models::ApiKeyCredential,
    value: Value,
    endpoint: &str,
) -> Response {
    let request_id = context_id(headers, "x-request-id", "req_");
    let trace_id = context_id(headers, "x-trace-id", "");
    state.observations.record(RequestEvent {
        request_id: request_id.clone(),
        trace_id: trace_id.clone(),
        started_at: db::now(),
        finished_at: db::now(),
        endpoint: endpoint.into(),
        api_key_id: Some(key.id.clone()),
        provider: None,
        requested_model: None,
        resolved_model: None,
        status_code: 200,
        error_kind: None,
        latency_ms: 0,
        ttft_ms: None,
        input_tokens: 0,
        output_tokens: 0,
        cached_tokens: 0,
        cost_micros: 0,
        payload_captured: false,
        request_json: None,
        response_json: None,
    });
    response(
        StatusCode::OK,
        HeaderValue::from_static("application/json"),
        value.to_string(),
        &request_id,
        &trace_id,
    )
}

fn response(
    status: StatusCode,
    content_type: HeaderValue,
    bytes: impl Into<Body>,
    request: &str,
    trace: &str,
) -> Response {
    let mut response = Response::new(bytes.into());
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    response
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_str(request).unwrap());
    response
        .headers_mut()
        .insert("x-trace-id", HeaderValue::from_str(trace).unwrap());
    response
}

fn error_response(
    status: StatusCode,
    policy: &orchestration::policy::Retry,
    bytes: Option<Vec<u8>>,
    endpoint: &str,
) -> Response {
    if matches!(policy.error_mode, ErrorMode::PassThrough)
        && let Some(bytes) = bytes
    {
        let mut response =
            (status, [(header::CONTENT_TYPE, "application/json")], bytes).into_response();
        response.extensions_mut().insert(super::errors::PassThrough);
        return response;
    }
    let message = if matches!(policy.error_mode, ErrorMode::Custom) {
        policy
            .error_message
            .as_deref()
            .unwrap_or("upstream request failed")
    } else {
        "upstream request failed"
    };
    let value = super::errors::document(
        super::errors::protocol(endpoint),
        status,
        super::errors::openai_type(status),
        message,
    );
    (status, Json(value)).into_response()
}

async fn read_body(response: reqwest::Response) -> std::result::Result<Vec<u8>, std::io::Error> {
    let mut stream = response.bytes_stream();
    let mut output = vec![];
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| std::io::Error::other("upstream body error"))?;
        if output.len().saturating_add(chunk.len()) > 16 * 1024 * 1024 {
            return Err(std::io::Error::other("upstream body exceeds limit"));
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

fn empty_success(value: Option<&Value>, bytes: &[u8]) -> bool {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return true;
    }
    value.is_some_and(|v| {
        v.is_null()
            || v.as_object().is_some_and(|v| v.is_empty())
            || v.get("choices")
                .and_then(Value::as_array)
                .is_some_and(|choices| {
                    choices.iter().all(|choice| {
                        let message = &choice["message"];
                        let text = message.get("content").or_else(|| choice.get("text"));
                        let empty_text = text.is_none_or(|text| {
                            text.is_null()
                                || text.as_str().is_some_and(|text| text.trim().is_empty())
                                || text.as_array().is_some_and(Vec::is_empty)
                        });
                        empty_text
                            && message
                                .get("tool_calls")
                                .and_then(Value::as_array)
                                .is_none_or(Vec::is_empty)
                            && message.get("function_call").is_none()
                            && message
                                .get("refusal")
                                .and_then(Value::as_str)
                                .is_none_or(str::is_empty)
                    })
                })
            || v.get("output")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
    })
}

#[cfg(test)]
mod tests;
