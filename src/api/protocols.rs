//! Public protocol aliases normalize only routing metadata. Native bodies remain
//! intact and every provider attempt belongs to the shared gateway executor.
use super::*;
use crate::providers;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, OriginalUri},
};

mod aisdk;
mod tasks;
mod websocket;

pub(super) struct Input {
    pub payload: Value,
    pub endpoint: &'static str,
    pub wire: Wire,
}

pub(super) enum Wire {
    Json,
    Gemini {
        version: String,
    },
    Multipart(Vec<Part>),
    Task {
        id: String,
        provider: String,
        credential: String,
        upstream_model: String,
        delete: bool,
    },
}

pub(super) struct Part {
    name: String,
    filename: Option<String>,
    content_type: Option<String>,
    data: Bytes,
}

impl Input {
    pub fn json(payload: Value, endpoint: &'static str) -> Self {
        Self {
            payload,
            endpoint,
            wire: Wire::Json,
        }
    }

    pub fn body(&self, payload: &Value) -> Result<(Vec<u8>, String), ApiError> {
        match &self.wire {
            Wire::Json | Wire::Gemini { .. } => Ok((
                serde_json::to_vec(payload).map_err(|e| ApiError::Internal(e.into()))?,
                "application/json".into(),
            )),
            Wire::Task { .. } => Ok((vec![], "application/json".into())),
            Wire::Multipart(parts) => {
                let boundary = format!("pangolin-{}", Uuid::new_v4().simple());
                let mut output = vec![];
                let mut written = std::collections::HashSet::new();
                for part in parts {
                    let data = if part.filename.is_some() {
                        part.data.to_vec()
                    } else {
                        let value = payload.get(&part.name).ok_or_else(|| {
                            providers::invalid("multipart overrides cannot remove fields")
                        })?;
                        value
                            .as_str()
                            .map(str::as_bytes)
                            .map(<[u8]>::to_vec)
                            .unwrap_or_else(|| value.to_string().into_bytes())
                    };
                    written.insert(part.name.as_str());
                    output.extend_from_slice(
                        format!(
                            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"",
                            quoted(&part.name)?
                        )
                        .as_bytes(),
                    );
                    if let Some(filename) = &part.filename {
                        output.extend_from_slice(
                            format!("; filename=\"{}\"", quoted(filename)?).as_bytes(),
                        );
                    }
                    output.extend_from_slice(b"\r\n");
                    if let Some(content_type) = &part.content_type {
                        output.extend_from_slice(
                            format!("Content-Type: {content_type}\r\n").as_bytes(),
                        );
                    }
                    output.extend_from_slice(b"\r\n");
                    output.extend_from_slice(&data);
                    output.extend_from_slice(b"\r\n");
                }
                for (name, value) in payload
                    .as_object()
                    .ok_or_else(|| providers::invalid("invalid multipart metadata"))?
                {
                    if written.contains(name.as_str()) {
                        continue;
                    }
                    let text = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    output.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{text}\r\n",quoted(name)?).as_bytes());
                }
                output.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
                Ok((output, format!("multipart/form-data; boundary={boundary}")))
            }
        }
    }
}

fn quoted(value: &str) -> Result<String, ApiError> {
    if value.contains(['\r', '\n', '\0']) {
        return Err(providers::invalid(
            "invalid multipart field name or filename",
        ));
    }
    Ok(value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/completions", post(json_endpoint))
        .route("/v1/responses/compact", post(json_endpoint))
        .route("/v1/responses", get(websocket::upgrade))
        .route("/v1/models/{*model}", get(model))
        .route("/v1/embeddings", post(json_endpoint))
        .route("/v1/moderations", post(json_endpoint))
        .route("/v1/alpha/search", post(json_endpoint))
        .route("/v1/images/generations", post(json_endpoint))
        .route("/v1/images/edits", post(multipart_endpoint))
        .route("/v1/videos", post(media_endpoint))
        .route(
            "/v1/videos/{id}",
            get(tasks::get_video).delete(tasks::delete_video),
        )
        .route("/v1/audio/speech", post(json_endpoint))
        .route("/v1/audio/transcriptions", post(multipart_endpoint))
        .route("/v1/audio/translations", post(multipart_endpoint))
        .route("/v1/rerank", post(json_endpoint))
        .route("/jina/v1/embeddings", post(json_endpoint))
        .route("/jina/v1/rerank", post(json_endpoint))
        .route("/anthropic/v1/messages", post(json_endpoint))
        .route("/anthropic/v1/models", get(anthropic_models))
        .route("/v1beta/models", get(gemini_models))
        .route("/v1beta/models/{*action}", post(gemini))
        .route("/gemini/{version}/models", get(gemini_models))
        .route("/gemini/{version}/models/{*action}", post(gemini))
        .route("/doubao/v3/contents/generations/tasks", post(json_endpoint))
        .route(
            "/doubao/v3/contents/generations/tasks/{id}",
            get(tasks::get_doubao).delete(tasks::delete_doubao),
        )
        .route("/aisdk/v1/chat/completions", post(aisdk::chat))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
}

fn endpoint(path: &str) -> Result<&'static str, ApiError> {
    let path = path
        .strip_prefix("/jina")
        .or_else(|| path.strip_prefix("/anthropic"))
        .unwrap_or(path);
    providers::ENDPOINTS
        .iter()
        .copied()
        .find(|endpoint| *endpoint == path)
        .ok_or(ApiError::NotFound)
}

async fn json_endpoint(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let result = gateway::execute(state, headers, body, endpoint(uri.path())?).await;
    if uri.path() == "/anthropic/v1/messages" {
        protocol_error(result, false)
    } else {
        result
    }
}

async fn media_endpoint(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("multipart/form-data"))
    {
        multipart(state, headers, body, endpoint(uri.path())?).await
    } else {
        gateway::execute(state, headers, body, endpoint(uri.path())?).await
    }
}

async fn multipart_endpoint(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    multipart(state, headers, body, endpoint(uri.path())?).await
}

async fn multipart(
    state: AppState,
    headers: HeaderMap,
    body: Bytes,
    endpoint: &'static str,
) -> Result<Response, ApiError> {
    gateway_key(&state, &headers).await?;
    let boundary = multer::parse_boundary(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
    )
    .map_err(|_| providers::invalid("multipart/form-data boundary required"))?;
    let stream = futures_util::stream::once(async move { Ok::<_, std::io::Error>(body) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut parts = vec![];
    let mut payload = Map::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| providers::invalid("invalid multipart form"))?
    {
        if parts.len() >= 128 {
            return Err(providers::invalid("too many multipart fields"));
        }
        let name = field
            .name()
            .ok_or_else(|| providers::invalid("multipart field name required"))?
            .to_owned();
        let filename = field.file_name().map(str::to_owned);
        let content_type = field.content_type().map(ToString::to_string);
        let data = field
            .bytes()
            .await
            .map_err(|_| providers::invalid("invalid multipart field"))?;
        if filename.is_some() {
            payload.insert(name.clone(), json!({"type":"file","bytes":data.len()}));
        } else {
            let text = std::str::from_utf8(&data)
                .map_err(|_| providers::invalid("multipart text must be UTF-8"))?;
            if payload.contains_key(&name) {
                return Err(providers::invalid(
                    "duplicate multipart text fields are unsupported",
                ));
            }
            let value = if matches!(name.as_str(), "n" | "stream" | "temperature") {
                serde_json::from_str(text)
                    .map_err(|_| providers::invalid("invalid multipart numeric field"))?
            } else {
                json!(text)
            };
            payload.insert(name.clone(), value);
        }
        parts.push(Part {
            name,
            filename,
            content_type,
            data,
        });
    }
    gateway::execute_input(
        state,
        headers,
        Input {
            payload: Value::Object(payload),
            endpoint,
            wire: Wire::Multipart(parts),
        },
    )
    .await
}

fn gemini_headers(mut headers: HeaderMap, uri: &http::Uri) -> Result<HeaderMap, ApiError> {
    if headers.get(header::AUTHORIZATION).is_none() {
        let key = headers
            .get("x-goog-api-key")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .or_else(|| {
                reqwest::Url::parse(&format!("http://localhost{uri}"))
                    .ok()?
                    .query_pairs()
                    .find(|(name, _)| name == "key")
                    .map(|(_, v)| v.into_owned())
            });
        if let Some(key) = key {
            headers.insert(
                header::AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {key}"))
                    .map_err(|_| ApiError::Unauthorized)?,
            );
        }
    }
    Ok(headers)
}

async fn gemini(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let headers = gemini_headers(headers, &uri)?;
    let action = params.get("action").ok_or(ApiError::NotFound)?;
    let (model, operation) = action
        .rsplit_once(':')
        .ok_or_else(|| providers::invalid("Gemini action required"))?;
    let endpoint = match operation {
        "generateContent" => "/v1beta/models:generateContent",
        "streamGenerateContent" => "/v1beta/models:streamGenerateContent",
        _ => return Err(providers::invalid("unsupported Gemini action")),
    };
    if let Some(version) = uri
        .path()
        .strip_prefix("/gemini/")
        .and_then(|v| v.split('/').next())
        && !matches!(version, "v1" | "v1beta" | "v1alpha")
    {
        return Err(providers::invalid("unsupported Gemini API version"));
    }
    let mut payload: Value =
        serde_json::from_slice(&body).map_err(|_| providers::invalid("invalid JSON body"))?;
    if !payload.is_object() {
        return Err(providers::invalid("request must be an object"));
    }
    if ["generation_config", "system_instruction", "tool_config"]
        .iter()
        .any(|name| payload.get(name).is_some())
    {
        return Err(providers::invalid(
            "use Gemini camelCase generationConfig, systemInstruction and toolConfig fields",
        ));
    }
    if payload.get("model").is_some() || payload.get("stream").is_some() {
        return Err(providers::invalid(
            "Gemini model and stream are selected by URL",
        ));
    }
    payload["model"] = json!(model);
    payload["stream"] = json!(operation == "streamGenerateContent");
    let version = params
        .get("version")
        .cloned()
        .unwrap_or_else(|| "v1beta".into());
    protocol_error(
        gateway::execute_input(
            state,
            headers,
            Input {
                payload,
                endpoint,
                wire: Wire::Gemini { version },
            },
        )
        .await,
        true,
    )
}

async fn model(
    State(state): State<AppState>,
    Path(model): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let key = gateway_key(&state, &headers).await?;
    let models = crate::orchestration::visible_models(&state.db, &key, &headers).await?;
    let value = models
        .into_iter()
        .find(|v| v["id"] == model)
        .ok_or(ApiError::NotFound)?;
    Ok(gateway::discovery_response(
        &state,
        &headers,
        &key,
        value,
        "/v1/models/{model}",
    ))
}

async fn anthropic_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let key = gateway_key(&state, &headers).await?;
    let models =
        crate::orchestration::visible_models_for(&state.db, &key, &headers, &["/v1/messages"])
            .await?;
    let data=models.into_iter().map(|v|json!({"id":v["id"],"type":"model","display_name":v["id"],"created_at":time::OffsetDateTime::from_unix_timestamp(v["created"].as_i64().unwrap_or(0)).unwrap_or(time::OffsetDateTime::UNIX_EPOCH).format(&time::format_description::well_known::Rfc3339).unwrap()})).collect::<Vec<_>>();
    Ok(gateway::discovery_response(
        &state,
        &headers,
        &key,
        json!({"first_id":data.first().map(|v|&v["id"]),"last_id":data.last().map(|v|&v["id"]),"data":data,"has_more":false}),
        "/anthropic/v1/models",
    ))
}

async fn gemini_models(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let headers = gemini_headers(headers, &uri)?;
    let key = gateway_key(&state, &headers).await?;
    let models = crate::orchestration::visible_models_for(
        &state.db,
        &key,
        &headers,
        &["/v1beta/models:generateContent"],
    )
    .await?;
    let streaming = crate::orchestration::visible_models_for(
        &state.db,
        &key,
        &headers,
        &["/v1beta/models:streamGenerateContent"],
    )
    .await?;
    Ok(gateway::discovery_response(
        &state,
        &headers,
        &key,
        json!({"models":models.into_iter().map(|v|{let mut methods=vec!["generateContent"];if streaming.iter().any(|m|m["id"]==v["id"]){methods.push("streamGenerateContent");}json!({"name":format!("models/{}",v["id"].as_str().unwrap_or("")),"displayName":v["id"],"supportedGenerationMethods":methods})}).collect::<Vec<_>>()}),
        "/v1beta/models",
    ))
}

pub(super) fn protocol_error(
    result: Result<Response, ApiError>,
    gemini: bool,
) -> Result<Response, ApiError> {
    match result {
        Ok(response) => Ok(response),
        Err(error) => {
            let message = error.to_string();
            let status = error.into_response().status();
            if gemini {
                Ok((status,Json(json!({"error":{"code":status.as_u16(),"status":status.canonical_reason().unwrap_or("ERROR").to_ascii_uppercase().replace(' ',"_"),"message":message}}))).into_response())
            } else {
                Ok((status,Json(json!({"type":"error","error":{"type":"invalid_request_error","message":message}}))).into_response())
            }
        }
    }
}

pub(super) use tasks::persist;
