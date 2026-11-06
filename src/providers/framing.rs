use aimux_stream::sse::SseStream;
use axum::body::Bytes;
use futures_util::{Stream, StreamExt};

pub struct Frame {
    pub event: Option<String>,
    pub data: Option<String>,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

pub fn frames<S, E>(input: S) -> impl Stream<Item = Result<Frame, std::io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    frames_with_limit(input, 1024 * 1024)
}

pub fn internal_frames<S, E>(input: S) -> impl Stream<Item = Result<Frame, std::io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    frames_with_limit(input, 2 * 1024 * 1024)
}

fn frames_with_limit<S, E>(
    input: S,
    limit: usize,
) -> impl Stream<Item = Result<Frame, std::io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    let bounded: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + '_>> =
        Box::pin(async_stream::try_stream! {
            futures_util::pin_mut!(input);
            let mut size = 0_usize;
            let mut line_has_bytes = false;
            let mut previous_cr = false;
            while let Some(chunk) = input.next().await {
                let chunk = chunk.map_err(|_| std::io::Error::other("upstream SSE transport error"))?;
                for byte in &chunk {
                    size = size.saturating_add(1);
                    if *byte == b'\n' && previous_cr {
                        previous_cr = false;
                    } else if matches!(*byte, b'\r' | b'\n') {
                        if !line_has_bytes { size = 0; }
                        line_has_bytes = false;
                        previous_cr = *byte == b'\r';
                    } else {
                        line_has_bytes = true;
                        previous_cr = false;
                    }
                    if size > limit {
                        Err(std::io::Error::other("upstream SSE event exceeds limit"))?;
                    }
                }
                yield chunk;
            }
        });
    SseStream::new(bounded).map(|result: Result<_, aimux_stream::sse::SseError>| {
        result
            .map(|frame| Frame {
                event: frame.event,
                data: Some(frame.data),
                id: frame.id,
                retry: frame.retry,
            })
            .map_err(|_| std::io::Error::other("invalid SSE frame"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn aimux_frames_fragmented_utf8_crlf_comments_and_multiline_data() {
        let chunks = ": keepalive\r\nevent: usage\r\nid: 12\r\nretry: 20\r\ndata: 鲮鲤\r\ndata: tail\r\n\r\n"
            .as_bytes().iter().map(|byte| Ok::<_, std::io::Error>(Bytes::from(vec![*byte]))).collect::<Vec<_>>();
        let events = frames(futures_util::stream::iter(chunks));
        futures_util::pin_mut!(events);
        let frame = events.next().await.unwrap().unwrap();
        assert_eq!(frame.event.as_deref(), Some("usage"));
        assert_eq!(frame.id.as_deref(), Some("12"));
        assert_eq!(frame.retry, Some(20));
        assert_eq!(frame.data.as_deref(), Some("鲮鲤\ntail"));
        assert!(events.next().await.is_none());
    }

    #[tokio::test]
    async fn aimux_frame_limit_applies_across_chunks_and_to_comment_only_streams() {
        let chunks = vec![
            Ok::<_, std::io::Error>(Bytes::from_static(b": ")),
            Ok(Bytes::from(vec![b'x'; 1024 * 1024])),
        ];
        let events = frames(futures_util::stream::iter(chunks));
        futures_util::pin_mut!(events);
        assert!(events.next().await.unwrap().is_err());
    }

    #[tokio::test]
    async fn aimux_does_not_turn_truncated_or_invalid_utf8_into_success() {
        for bytes in [b"data: truncated".as_slice(), b"data: \xff\n\n".as_slice()] {
            let events = frames(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
                Bytes::copy_from_slice(bytes),
            )]));
            futures_util::pin_mut!(events);
            assert!(events.next().await.is_none_or(|event| event.is_err()));
        }
    }

    #[tokio::test]
    async fn accepted_multiline_events_survive_bounded_internal_reencoding() {
        let source = format!(
            "data:{{\n{}data:\"done\":true}}\n\n",
            "data:\t\t\t\t\n".repeat(100_000)
        );
        assert!(source.len() < 1024 * 1024);
        let events = frames(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
            Bytes::from(source),
        )]));
        futures_util::pin_mut!(events);
        let frame = events.next().await.unwrap().unwrap();
        let data = frame.data.unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&data).is_ok());
        let encoded = crate::orchestration::stream::encode(&eventsource_stream::Event {
            event: "response.completed".into(),
            data: data.clone(),
            id: String::new(),
            retry: None,
        });
        assert!(encoded.len() > 1024 * 1024);
        let reparsed = internal_frames(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
            encoded,
        )]));
        futures_util::pin_mut!(reparsed);
        assert_eq!(
            reparsed.next().await.unwrap().unwrap().data.as_deref(),
            Some(data.as_str())
        );
        let oversized = internal_frames(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
            Bytes::from(vec![b'x'; 2 * 1024 * 1024 + 1]),
        )]));
        futures_util::pin_mut!(oversized);
        assert!(oversized.next().await.unwrap().is_err());
    }
}
