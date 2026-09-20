use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures_util::{SinkExt, StreamExt};
use tokio::{sync::mpsc, task::JoinSet};

const MAX_STREAMS: usize = 16;
const QUEUE_SIZE: usize = 8;

pub(super) async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    gateway_key(&state, &headers).await?;
    Ok(ws
        .max_message_size(1024 * 1024)
        .max_frame_size(1024 * 1024)
        .on_upgrade(move |socket| run(socket, state, headers)))
}

fn error(lane: Option<&Value>, code: &str, message: &str) -> Value {
    let mut value = json!({"type":"error","error":{"type":"invalid_request_error","code":code,"message":message}});
    if let Some(lane) = lane {
        value["stream_id"] = lane.clone();
    }
    value
}

async fn run(socket: WebSocket, state: AppState, mut headers: HeaderMap) {
    let session = format!("ws_{}", Uuid::new_v4().simple());
    headers.insert(
        "x-pangolin-websocket-generation",
        HeaderValue::from_str(
            &state
                .orchestrator
                .generation
                .load(std::sync::atomic::Ordering::Relaxed)
                .to_string(),
        )
        .unwrap(),
    );
    headers.insert("x-session-id", HeaderValue::from_str(&session).unwrap());
    headers.insert(
        "x-pangolin-websocket-session",
        HeaderValue::from_str(&session).unwrap(),
    );
    let (mut sink, mut source) = socket.split();
    let (output, mut events) = mpsc::channel::<Value>(32);
    let (control, mut controls) = mpsc::channel::<Message>(8);
    let write_timeout = state
        .config
        .upstream_timeout
        .min(std::time::Duration::from_secs(30));
    let mut writer = tokio::spawn(async move {
        loop {
            let message = tokio::select! {
                value=events.recv()=>match value{Some(value)=>Message::Text(value.to_string().into()),None=>break},
                control=controls.recv()=>match control{Some(message)=>message,None=>break},
            };
            if !matches!(
                tokio::time::timeout(write_timeout, sink.send(message)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    });
    let mut lanes: HashMap<String, mpsc::Sender<Value>> = HashMap::new();
    let mut workers = JoinSet::new();
    loop {
        tokio::select! {
            _=&mut writer=>break,
            message=source.next()=>{
                let Some(Ok(message))=message else{break;};
                let text=match message{
                    Message::Text(text)=>text,
                    Message::Close(_)=>break,
                    Message::Ping(data)=>{if control.try_send(Message::Pong(data)).is_err(){break;}continue;},
                    Message::Pong(_)=>continue,
                    Message::Binary(_)=>{if output.try_send(error(None,"invalid_event","response.create requires a text frame")).is_err(){break;}continue;}
                };
                let result=enqueue(&state,&headers,&session,&text,&output,&mut lanes,&mut workers);
                if let Err(value)=result && output.try_send(value).is_err(){break;}
            }
        }
    }
    writer.abort();
    workers.abort_all();
    while workers.join_next().await.is_some() {}
}

fn enqueue(
    state: &AppState,
    headers: &HeaderMap,
    session: &str,
    text: &str,
    output: &mpsc::Sender<Value>,
    lanes: &mut HashMap<String, mpsc::Sender<Value>>,
    workers: &mut JoinSet<()>,
) -> Result<(), Value> {
    let mut payload: Value = serde_json::from_str(text)
        .map_err(|_| error(None, "invalid_event", "invalid response.create JSON"))?;
    if !payload.is_object() || payload["type"] != "response.create" {
        return Err(error(
            None,
            "invalid_event",
            "only response.create is supported",
        ));
    }
    let lane = payload.get("stream_id").cloned();
    if lane.as_ref().is_some_and(|v| {
        v.as_str().is_none_or(|v| {
            v.is_empty()
                || v.len() > 64
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    }) {
        return Err(error(
            lane.as_ref(),
            "invalid_stream_id",
            "invalid stream_id",
        ));
    }
    if payload.get("generate").is_some() {
        return Err(error(
            lane.as_ref(),
            "unsupported_event",
            "generate is unsupported",
        ));
    }
    let lane_key = lane
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    for field in ["type", "stream_id", "background"] {
        payload.as_object_mut().unwrap().remove(field);
    }
    payload["stream"] = json!(true);
    if !lanes.contains_key(&lane_key) {
        if lanes.len() >= MAX_STREAMS {
            return Err(error(
                lane.as_ref(),
                "websocket_stream_limit_reached",
                "WebSocket stream limit reached",
            ));
        }
        let (sender, mut receiver) = mpsc::channel::<Value>(QUEUE_SIZE);
        let output = output.clone();
        let state = state.clone();
        let lane = lane.clone();
        let mut headers = headers.clone();
        headers.insert(
            "x-session-id",
            HeaderValue::from_str(&format!("{session}:{lane_key}")).unwrap(),
        );
        workers.spawn(async move {
            while let Some(payload) = receiver.recv().await {
                let result = tokio::time::timeout(
                    state.config.upstream_timeout,
                    process(&output, &state, headers.clone(), payload, lane.clone()),
                )
                .await;
                let failure = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(cause)) => Some(request_error(lane.as_ref(), cause)),
                    Err(_) => Some(error(
                        lane.as_ref(),
                        "timeout_error",
                        "response.create timed out",
                    )),
                };
                if let Some(value) = failure
                    && output.send(value).await.is_err()
                {
                    break;
                }
            }
        });
        lanes.insert(lane_key.clone(), sender);
    }
    lanes[&lane_key].try_send(payload).map_err(|_| {
        error(
            lane.as_ref(),
            "websocket_queue_limit_reached",
            "WebSocket request queue is full",
        )
    })
}

pub(super) fn request_error(lane: Option<&Value>, cause: ApiError) -> Value {
    let (status, kind, message) = cause.public_parts();
    let mut value = json!({"type":"error","status":status.as_u16(),"error":{"type":kind,"code":"request_failed","message":message}});
    if let Some(lane) = lane {
        value["stream_id"] = lane.clone();
    }
    value
}

async fn process(
    output: &mpsc::Sender<Value>,
    state: &AppState,
    headers: HeaderMap,
    payload: Value,
    lane: Option<Value>,
) -> Result<(), ApiError> {
    let response = gateway::execute_input(
        state.clone(),
        headers,
        Input::json(payload, "/v1/responses"),
    )
    .await?;
    if !response.status().is_success() {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .map_err(|_| ApiError::Upstream("invalid error response".into()))?;
        let body: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"error":{"message":"upstream request failed"}}));
        let mut event = json!({"type":"error","status":status.as_u16(),"error":body["error"]});
        if let Some(lane) = lane {
            event["stream_id"] = lane;
        }
        output
            .send(event)
            .await
            .map_err(|_| ApiError::Upstream("WebSocket disconnected".into()))?;
        return Ok(());
    }
    let events = providers::framing::frames(response.into_body().into_data_stream());
    futures_util::pin_mut!(events);
    while let Some(event) = events.next().await {
        let event = event.map_err(|_| ApiError::Upstream("invalid Responses stream".into()))?;
        let Some(data) = event.data else {
            continue;
        };
        if data == "[DONE]" {
            continue;
        }
        let mut value: Value = serde_json::from_str(&data)
            .map_err(|_| ApiError::Upstream("Responses stream event must be JSON".into()))?;
        if !value.is_object() {
            return Err(ApiError::Upstream(
                "Responses stream event must be an object".into(),
            ));
        }
        if let Some(lane) = &lane {
            value["stream_id"] = lane.clone();
        }
        output
            .send(value)
            .await
            .map_err(|_| ApiError::Upstream("WebSocket disconnected".into()))?;
    }
    Ok(())
}
