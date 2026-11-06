use aimux_provider_utils::{Fetch, FetchError, FetchRequest, FetchResponse};
use futures_util::StreamExt;

pub use aimux_provider_utils::FetchResponse as Response;

#[derive(Clone)]
pub struct Transport {
    client: reqwest::Client,
}

impl Transport {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    pub async fn send(
        &self,
        method: http::Method,
        url: &str,
        headers: http::HeaderMap,
        body: Vec<u8>,
    ) -> Result<Response, FetchError> {
        let mut request = FetchRequest::new(
            method,
            reqwest::Url::parse(url)
                .map_err(|_| FetchError::Other("invalid upstream URL".to_owned().into()))?,
        );
        request.headers = headers;
        request.body = body.into();
        self.fetch(request).await
    }
}

#[async_trait::async_trait]
impl Fetch for Transport {
    async fn fetch(&self, request: FetchRequest) -> Result<FetchResponse, FetchError> {
        let signal = request.signal;
        let mut builder = self
            .client
            .request(request.method, request.url)
            .headers(request.headers)
            .body(request.body);
        if let Some(timeout) = request.timeout {
            builder = builder.timeout(timeout);
        }
        let response = if let Some(signal) = &signal {
            tokio::select! {
                biased;
                _ = signal.cancelled() => return Err(FetchError::Aborted),
                response = builder.send() => response.map_err(transport_error)?,
            }
        } else {
            builder.send().await.map_err(transport_error)?
        };
        let status = response.status();
        let headers = response.headers().clone();
        let url = response.url().clone();
        let chunks = response.bytes_stream();
        let body = async_stream::stream! {
            futures_util::pin_mut!(chunks);
            loop {
                let next = if let Some(signal) = &signal {
                    tokio::select! {
                        biased;
                        _ = signal.cancelled() => { yield Err(FetchError::Aborted); break; },
                        next = chunks.next() => next,
                    }
                } else {
                    chunks.next().await
                };
                match next {
                    Some(chunk) => yield chunk.map_err(transport_error),
                    None => break,
                }
            }
        };
        Ok(FetchResponse {
            status,
            headers,
            url,
            body: body.boxed(),
        })
    }
}

fn transport_error(error: reqwest::Error) -> FetchError {
    if error.is_timeout() {
        FetchError::Timeout
    } else {
        FetchError::Io("upstream transport failed".to_owned().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Bytes, response::IntoResponse, routing::post};
    use http::{HeaderMap, HeaderValue, StatusCode};

    #[tokio::test]
    async fn aimux_transport_preserves_native_bytes_status_headers_and_refuses_redirects() {
        let app = Router::new()
            .route(
                "/native",
                post(|body: Bytes| async move {
                    (
                        StatusCode::CREATED,
                        [("x-provider-field", "preserved")],
                        body,
                    )
                        .into_response()
                }),
            )
            .route(
                "/redirect",
                post(|| async { (StatusCode::TEMPORARY_REDIRECT, [("location", "/native")]) }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let transport =
            Transport::new(super::super::http_client(std::time::Duration::from_secs(2)).unwrap());
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            HeaderValue::from_static("application/octet-stream"),
        );
        let bytes = vec![0, 255, 1, 0, 10];
        let response = transport
            .send(
                http::Method::POST,
                &format!("{base}/native"),
                headers,
                bytes.clone(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["x-provider-field"], "preserved");
        let chunks = response.bytes_stream().collect::<Vec<_>>().await;
        assert_eq!(
            chunks
                .into_iter()
                .flat_map(|chunk| chunk.unwrap())
                .collect::<Vec<_>>(),
            bytes
        );
        let response = transport
            .send(
                http::Method::POST,
                &format!("{base}/redirect"),
                HeaderMap::new(),
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        let mut request = FetchRequest::new(
            http::Method::POST,
            reqwest::Url::parse(&format!("{base}/native")).unwrap(),
        );
        request.signal = Some(Default::default());
        let signal = request.signal.clone().unwrap();
        let response = transport.fetch(request).await.unwrap();
        signal.abort();
        assert!(matches!(
            response.bytes_stream().next().await,
            Some(Err(FetchError::Aborted))
        ));
        server.abort();
    }
}
