use axum::body::Bytes;
use futures_util::{Stream, StreamExt};
use litellm_framing::Framer;

pub struct Frame {
    pub event: Option<String>,
    pub data: Option<String>,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

pub fn frames<S, E>(input: S) -> impl Stream<Item = Result<Frame, std::io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send,
    E: std::error::Error + Send + Sync + 'static,
{
    litellm_framing::sse::SseFramer.frame(input).map(|result| {
        result
            .map(|frame| Frame {
                event: frame.event,
                data: frame.data,
                id: frame.id,
                retry: frame.retry,
            })
            .map_err(|_| std::io::Error::other("invalid SSE frame"))
    })
}
