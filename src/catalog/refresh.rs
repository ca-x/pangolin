use super::{
    repository::{self, Source},
    types::{MAX_BYTES, invalid},
};
use crate::api::ApiError;
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use futures_util::StreamExt;
use sea_orm::DatabaseConnection;
use serde::Serialize;
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    time::Duration,
};

pub struct Download {
    pub status: u16,
    pub body: Vec<u8>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub signature: Option<String>,
}
pub trait Transport: Send + Sync {
    fn fetch<'a>(
        &'a self,
        source: &'a Source,
    ) -> Pin<Box<dyn Future<Output = Result<Download, ApiError>> + Send + 'a>>;
}
pub struct HttpsTransport;

fn pinned_client(host: &str, addresses: &[SocketAddr]) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, addresses)
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(5))
}

pub fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let n = u32::from(ip);
            let excluded = [
                (0x00000000, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 4),
                (0xf0000000, 4),
            ];
            !excluded
                .iter()
                .any(|(network, bits)| n & (!0u32 << (32 - bits)) == *network)
        }
        IpAddr::V6(ip) => {
            if let Some(ip) = ip.to_ipv4_mapped() {
                return public_address(IpAddr::V4(ip));
            }
            let segments = ip.segments();
            (segments[0] & 0xe000) == 0x2000
                && segments[0] != 0x2002
                && segments[0] != 0x3fff
                && !(segments[0] == 0x2001 && (segments[1] == 0x0db8 || segments[1] < 0x200))
        }
    }
}

pub fn source_url(value: &str) -> Result<reqwest::Url, ApiError> {
    let url = reqwest::Url::parse(value).map_err(|_| invalid("invalid catalog source URL"))?;
    if value.len() > 2048
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return Err(invalid(
            "catalog sources require credential-free HTTPS on port 443",
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| invalid("catalog source hostname required"))?;
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err(invalid("catalog source must be publicly routable"));
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>()
        && !public_address(ip)
    {
        return Err(invalid("catalog source must be publicly routable"));
    }
    Ok(url)
}

impl Transport for HttpsTransport {
    fn fetch<'a>(
        &'a self,
        source: &'a Source,
    ) -> Pin<Box<dyn Future<Output = Result<Download, ApiError>> + Send + 'a>> {
        Box::pin(async move {
            let url = source_url(&source.url)?;
            let host = url.host_str().unwrap().trim_matches(['[', ']']);
            let addresses =
                tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((host, 443)))
                    .await
                    .map_err(|_| invalid("catalog DNS lookup timed out"))?
                    .map_err(|_| invalid("catalog DNS lookup failed"))?
                    .collect::<Vec<SocketAddr>>();
            if addresses.is_empty()
                || addresses
                    .iter()
                    .any(|address| !public_address(address.ip()))
            {
                return Err(invalid("catalog source resolved to a non-public address"));
            }
            // Pin this resolution for the entire request. Redirects and proxy
            // environment variables cannot bypass the address checks.
            let client = pinned_client(host, &addresses)
                .build()
                .map_err(|e| ApiError::Internal(e.into()))?;
            let mut request = client.get(url).header("accept", "application/json");
            if let Some(etag) = &source.etag {
                request = request.header("if-none-match", etag);
            }
            if let Some(modified) = &source.last_modified {
                request = request.header("if-modified-since", modified);
            }
            let response = request
                .send()
                .await
                .map_err(|_| ApiError::Upstream("catalog download failed".into()))?;
            let metadata = |name: &str| {
                response
                    .headers()
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .filter(|v| v.len() <= 2048)
                    .map(str::to_owned)
            };
            let status = response.status().as_u16();
            let etag = metadata("etag");
            let last_modified = metadata("last-modified");
            let signature = metadata("x-pangolin-catalog-signature");
            if status == 304 {
                return Ok(Download {
                    status,
                    body: vec![],
                    etag,
                    last_modified,
                    signature,
                });
            }
            if status != 200 {
                return Err(ApiError::Upstream(format!(
                    "catalog source returned HTTP {status}"
                )));
            }
            if response
                .content_length()
                .is_some_and(|n| n > MAX_BYTES as u64)
            {
                return Err(invalid("catalog download exceeds 4 MiB"));
            }
            let mut body = vec![];
            let mut chunks = response.bytes_stream();
            while let Some(chunk) = chunks.next().await {
                let chunk =
                    chunk.map_err(|_| ApiError::Upstream("catalog download body failed".into()))?;
                if body.len().saturating_add(chunk.len()) > MAX_BYTES {
                    return Err(invalid("catalog download exceeds 4 MiB"));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Download {
                status,
                body,
                etag,
                last_modified,
                signature,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        http::{HeaderMap, StatusCode},
        routing::get,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[tokio::test]
    async fn task4_real_client_pins_dns_and_refuses_redirects() {
        let reached = Arc::new(AtomicUsize::new(0));
        let seen = reached.clone();
        let redirected = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let redirect_address = redirected.local_addr().unwrap();
        let second = tokio::spawn(async move {
            axum::serve(
                redirected,
                Router::new().route(
                    "/leak",
                    get(move || {
                        let seen = seen.clone();
                        async move {
                            seen.fetch_add(1, Ordering::SeqCst);
                            "unreachable"
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let first = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/catalog",
                    get(move |headers: HeaderMap| async move {
                        assert_eq!(
                            headers["host"],
                            format!("catalog-pin.invalid:{}", address.port())
                        );
                        (
                            StatusCode::FOUND,
                            [("location", format!("http://{redirect_address}/leak"))],
                        )
                    }),
                ),
            )
            .await
            .unwrap();
        });
        // Public-address/HTTPS policy is tested independently. This local HTTP
        // fixture exercises the exact production reqwest builder over real TCP.
        let client = pinned_client("catalog-pin.invalid", &[address])
            .build()
            .unwrap();
        let response = client
            .get(format!(
                "http://catalog-pin.invalid:{}/catalog",
                address.port()
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(reached.load(Ordering::SeqCst), 0);
        first.abort();
        second.abort();
    }
}

pub fn public_key(value: &str) -> Result<VerifyingKey, ApiError> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| invalid("catalog public key must be base64 Ed25519"))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| invalid("catalog public key must contain 32 bytes"))?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| invalid("invalid Ed25519 public key"))
}

fn verify(source: &Source, download: &Download) -> Result<bool, ApiError> {
    if source.signature_policy == "none" {
        return Ok(false);
    }
    let Some(signature) = &download.signature else {
        if source.signature_policy == "required" {
            return Err(invalid("catalog signature is required"));
        }
        return Ok(false);
    };
    let key = public_key(
        source
            .public_key
            .as_deref()
            .ok_or_else(|| invalid("signed source needs a pinned public key"))?,
    )?;
    let signature = STANDARD
        .decode(signature)
        .map_err(|_| invalid("invalid catalog signature encoding"))?;
    let signature =
        Signature::from_slice(&signature).map_err(|_| invalid("invalid catalog signature size"))?;
    key.verify_strict(&download.body, &signature)
        .map_err(|_| invalid("catalog signature verification failed"))?;
    Ok(true)
}

#[derive(Serialize)]
pub struct Outcome {
    pub source_id: String,
    pub status: &'static str,
    pub snapshot_id: Option<String>,
}

pub async fn refresh(
    db: &DatabaseConnection,
    id: &str,
    audit: Option<repository::CatalogAudit<'_>>,
) -> Result<Outcome, ApiError> {
    refresh_with(db, id, &HttpsTransport, audit).await
}

/// The audit row of a refresh must state which outcome it recorded; otherwise a failed
/// attempt and an activation are indistinguishable in the trail.
fn outcome_audit<'a>(
    audit: Option<&'a repository::CatalogAudit<'a>>,
    status: &str,
) -> Option<repository::CatalogAudit<'a>> {
    audit.map(|audit| repository::CatalogAudit {
        actor_user_id: audit.actor_user_id,
        action: audit.action,
        resource_id: audit.resource_id,
        details: serde_json::json!({"status": status}),
    })
}

pub async fn refresh_with<T: Transport>(
    db: &DatabaseConnection,
    id: &str,
    transport: &T,
    audit: Option<repository::CatalogAudit<'_>>,
) -> Result<Outcome, ApiError> {
    let source = repository::source(db, id).await?;
    if !source.enabled {
        return Err(invalid("catalog source is disabled"));
    }
    let result = async {
        let download = tokio::time::timeout(Duration::from_secs(20), transport.fetch(&source))
            .await
            .map_err(|_| ApiError::Upstream("catalog refresh timed out".into()))??;
        if download.status == 304 {
            repository::not_modified(db, &source, outcome_audit(audit.as_ref(), "not_modified"))
                .await?;
            return Ok(Outcome {
                source_id: source.id.clone(),
                status: "not_modified",
                snapshot_id: source.active_snapshot_id.clone(),
            });
        }
        if download.status != 200 {
            return Err(ApiError::Upstream(format!(
                "catalog source returned HTTP {}",
                download.status
            )));
        }
        if download.body.len() > MAX_BYTES {
            return Err(invalid("catalog download exceeds 4 MiB"));
        }
        let verified = verify(&source, &download)?;
        let snapshot = repository::activate(
            db,
            &source,
            &download.body,
            download.etag,
            download.last_modified,
            verified,
            outcome_audit(audit.as_ref(), "activated"),
        )
        .await?;
        Ok(Outcome {
            source_id: source.id.clone(),
            status: "activated",
            snapshot_id: Some(snapshot),
        })
    }
    .await;
    if let Err(error) = &result {
        // The failure bookkeeping is a mutation too, so it carries its own audit row. If
        // that row cannot be written the attempt is rolled back and its error wins: the
        // caller must not believe the source recorded a refresh it never recorded.
        repository::failed(
            db,
            &source,
            &error.public_message(),
            outcome_audit(audit.as_ref(), "failed"),
        )
        .await?;
    }
    result
}
