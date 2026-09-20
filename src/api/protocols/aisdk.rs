use super::*;
use futures_util::StreamExt;

pub(super) async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let ui_stream = headers
        .get("x-vercel-ai-ui-message-stream")
        .is_some_and(|value| value == "v1");
    let mut payload: Value =
        serde_json::from_slice(&body).map_err(|_| providers::invalid("invalid JSON request"))?;
    providers::ensure_fields(
        &payload,
        &[
            "model",
            "messages",
            "temperature",
            "top_p",
            "max_tokens",
            "stream",
            "stop",
            "system",
        ],
    )?;
    let mut messages = vec![];
    if let Some(system) = payload.as_object_mut().unwrap().remove("system") {
        if !system.is_string() {
            return Err(providers::invalid("AI SDK system must be text"));
        }
        messages.push(json!({"role":"system","content":system}));
    }
    for message in payload["messages"]
        .as_array()
        .ok_or_else(|| providers::invalid("AI SDK messages required"))?
    {
        providers::ensure_fields(message, &["id", "role", "content", "parts"])?;
        let mut text = String::new();
        if let Some(parts) = message.get("parts").and_then(Value::as_array) {
            if message
                .get("content")
                .is_some_and(|value| !value.is_null() && value != "")
            {
                return Err(providers::invalid(
                    "AI SDK messages must use parts or content, not both",
                ));
            }
            for part in parts {
                providers::ensure_fields(part, &["type", "text", "state"])?;
                if part["type"] != "text" {
                    return Err(providers::invalid("unsupported AI SDK message part"));
                }
                text.push_str(
                    part["text"]
                        .as_str()
                        .ok_or_else(|| providers::invalid("AI SDK text part requires text"))?,
                );
            }
        } else {
            text = message["content"]
                .as_str()
                .ok_or_else(|| providers::invalid("AI SDK content must be text"))?
                .to_owned();
        }
        messages.push(json!({"role":message["role"],"content":text}));
    }
    payload["messages"] = json!(messages);
    payload["stream"] = json!(true);
    let response =
        gateway::execute_input(state, headers, Input::json(payload, "/v1/chat/completions"))
            .await?;
    if !response.status().is_success() {
        return Ok(response);
    }
    let (mut parts, body) = response.into_parts();
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(if ui_stream {
            "text/event-stream"
        } else {
            "text/plain; charset=utf-8"
        }),
    );
    parts.headers.insert(
        if ui_stream {
            "x-vercel-ai-ui-message-stream"
        } else {
            "x-vercel-ai-data-stream"
        },
        HeaderValue::from_static("v1"),
    );
    parts
        .headers
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    let stream = async_stream::stream! {
        let events=providers::framing::frames(body.into_data_stream());futures_util::pin_mut!(events);
        let mut reason="stop".to_owned();let mut usage=json!({"promptTokens":0,"completionTokens":0});
        let text_id=format!("text_{}",Uuid::new_v4().simple());
        if ui_stream {
            yield Ok::<_,std::io::Error>(frame(json!({"type":"start","messageId":format!("msg_{}",Uuid::new_v4().simple())})));
            yield Ok(frame(json!({"type":"start-step"})));
            yield Ok(frame(json!({"type":"text-start","id":text_id})));
        }
        while let Some(event)=events.next().await{
            let event=match event{Ok(value)=>value,Err(_)=>{yield Ok::<_,std::io::Error>(error(ui_stream,"upstream stream failed"));return;}};
            let Some(data)=event.data else{continue;};
            if data=="[DONE]"{
                if ui_stream {yield Ok(frame(json!({"type":"text-end","id":text_id})));yield Ok(frame(json!({"type":"finish-step"})));yield Ok(frame(json!({"type":"finish","finishReason":reason})));yield Ok(Bytes::from_static(b"data: [DONE]\n\n"));}
                else {yield Ok(Bytes::from(format!("d:{}\n",json!({"finishReason":reason,"usage":usage}))));}return;
            }
            let value:Value=match serde_json::from_str(&data){Ok(value)=>value,Err(_)=>{yield Ok(error(ui_stream,"invalid upstream event"));return;}};
            if value.get("error").is_some(){yield Ok(error(ui_stream,"upstream request failed"));return;}
            if let Some(counts)=value.get("usage"){usage=json!({"promptTokens":counts["prompt_tokens"],"completionTokens":counts["completion_tokens"]});}
            if let Some(choices)=value["choices"].as_array(){for choice in choices{
                if let Some(text)=choice["delta"]["content"].as_str(){yield Ok(if ui_stream {frame(json!({"type":"text-delta","id":text_id,"delta":text}))}else{Bytes::from(format!("0:{}\n",json!(text)))});}
                if choice["delta"].as_object().is_some_and(|delta|delta.keys().any(|key|!matches!(key.as_str(),"role"|"content"))){yield Ok(error(ui_stream,"unsupported AI SDK response part"));return;}
                if let Some(finish)=choice["finish_reason"].as_str(){reason=finish.to_owned();}
            }}
        }
    };
    Ok(Response::from_parts(parts, Body::from_stream(stream)))
}

fn frame(value: Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}
fn error(ui_stream: bool, message: &str) -> Bytes {
    if ui_stream {
        frame(json!({"type":"error","errorText":message}))
    } else {
        Bytes::from(format!("3:{}\n", json!(message)))
    }
}
