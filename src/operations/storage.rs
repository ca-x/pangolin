use crate::{
    api::{ApiError, AppState},
    crypto::SecretBox,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use object_store::{
    ClientOptions, ObjectStore, ObjectStoreExt, PutMode, PutOptions, aws::AmazonS3Builder,
    gcp::GoogleCloudStorageBuilder, http::HttpBuilder, local::LocalFileSystem, path::Path,
};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Config {
    Local {
        directory: String,
    },
    S3 {
        endpoint: String,
        bucket: String,
        region: String,
        #[serde(default)]
        prefix: String,
    },
    Gcs {
        bucket: String,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        endpoint: Option<String>,
    },
    Webdav {
        endpoint: String,
        #[serde(default)]
        prefix: String,
    },
}
pub fn invalid() -> ApiError {
    ApiError::BadRequest("invalid storage configuration or owned object key".into())
}
pub fn validate(config: &Config) -> Result<(), ApiError> {
    match config {
        Config::Local { directory } => {
            if directory.is_empty()
                || directory.len() > 128
                || !directory
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            {
                return Err(invalid());
            }
        }
        Config::S3 {
            endpoint,
            bucket,
            region,
            prefix,
        } => {
            let url = reqwest::Url::parse(endpoint).map_err(|_| invalid())?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || bucket.is_empty()
                || region.is_empty()
                || !safe_prefix(prefix)
            {
                return Err(invalid());
            }
        }
        Config::Gcs {
            bucket,
            prefix,
            endpoint,
        } => {
            if bucket.is_empty()
                || bucket.len() > 222
                || !bucket
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                || !safe_prefix(prefix)
                || endpoint
                    .as_deref()
                    .is_some_and(|endpoint| remote_endpoint(endpoint).is_err())
            {
                return Err(invalid());
            }
        }
        Config::Webdav { endpoint, prefix } => {
            remote_endpoint(endpoint)?;
            if !safe_prefix(prefix) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
fn remote_endpoint(value: &str) -> Result<reqwest::Url, ApiError> {
    let url = reqwest::Url::parse(value).map_err(|_| invalid())?;
    let host = url.host_str().ok_or_else(invalid)?;
    let test_loopback = cfg!(test)
        && matches!(url.scheme(), "http" | "https")
        && host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if (!test_loopback && url.scheme() != "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || (!test_loopback
            && (host.eq_ignore_ascii_case("localhost")
                || host.ends_with(".localhost")
                || host.ends_with(".local")
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| !crate::catalog::refresh::public_address(ip))))
    {
        return Err(invalid());
    }
    Ok(url)
}
fn safe_prefix(value: &str) -> bool {
    value.len() <= 256
        && value.split('/').all(|s| {
            !matches!(s, "." | "..")
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        })
}
pub fn envelope(
    secrets: &SecretBox,
    project: &str,
    storage: &str,
    secret: &Value,
) -> Result<String, ApiError> {
    Ok(secrets.encrypt(
        &serde_json::json!({"version":1,"project_id":project,"storage_id":storage,"secret":secret})
            .to_string(),
    )?)
}
pub fn validate_secret(config: &Config, secret: &Value) -> Result<(), ApiError> {
    if !secret.is_object()
        || serde_json::to_vec(secret).map_or(true, |encoded| encoded.len() > 64 * 1024)
    {
        return Err(invalid());
    }
    let bounded =
        |value: Option<&str>| value.is_some_and(|value| !value.is_empty() && value.len() <= 4096);
    match config {
        Config::Local { .. } => Ok(()),
        Config::S3 { .. }
            if bounded(secret["access_key_id"].as_str())
                && bounded(secret["secret_access_key"].as_str()) =>
        {
            Ok(())
        }
        Config::Gcs { .. }
            if secret.get("service_account").is_some_and(|value| {
                value.is_object() || value.as_str().is_some_and(|text| !text.is_empty())
            }) =>
        {
            Ok(())
        }
        Config::Webdav { .. }
            if bounded(secret["username"].as_str()) && bounded(secret["password"].as_str()) =>
        {
            Ok(())
        }
        _ => Err(invalid()),
    }
}
pub async fn open(
    state: &AppState,
    project: &str,
    storage: &str,
    config: &Config,
    secret: Option<&str>,
) -> Result<Arc<dyn ObjectStore>, ApiError> {
    validate(config)?;
    match config {
        Config::S3 { endpoint, .. } | Config::Webdav { endpoint, .. } => {
            ensure_public_resolution(endpoint).await?
        }
        Config::Gcs {
            endpoint: Some(endpoint),
            ..
        } => ensure_public_resolution(endpoint).await?,
        _ => {}
    }
    match config {
        Config::Local { directory } => {
            let root = state.config.data_dir.join("storage");
            std::fs::create_dir_all(&root).map_err(|e| ApiError::Internal(e.into()))?;
            let path = root.join(directory);
            std::fs::create_dir_all(&path).map_err(|e| ApiError::Internal(e.into()))?;
            let canonical =
                std::fs::canonicalize(&path).map_err(|e| ApiError::Internal(e.into()))?;
            if !canonical
                .starts_with(std::fs::canonicalize(root).map_err(|e| ApiError::Internal(e.into()))?)
            {
                return Err(invalid());
            }
            Ok(Arc::new(
                LocalFileSystem::new_with_prefix(canonical)
                    .map_err(|e| ApiError::Internal(e.into()))?,
            ))
        }
        Config::S3 {
            endpoint,
            bucket,
            region,
            ..
        } => {
            let secret: Value =
                serde_json::from_str(&state.secrets.decrypt(secret.ok_or_else(invalid)?)?)
                    .map_err(|_| invalid())?;
            if secret["version"] != 1
                || secret["project_id"] != project
                || secret["storage_id"] != storage
            {
                return Err(invalid());
            }
            let credentials = &secret["secret"];
            let mut builder = AmazonS3Builder::new()
                .with_endpoint(endpoint)
                .with_bucket_name(bucket)
                .with_region(region)
                .with_access_key_id(credentials["access_key_id"].as_str().ok_or_else(invalid)?)
                .with_secret_access_key(
                    credentials["secret_access_key"]
                        .as_str()
                        .ok_or_else(invalid)?,
                );
            if let Some(token) = credentials["session_token"].as_str() {
                builder = builder.with_token(token)
            }
            Ok(Arc::new(
                builder.build().map_err(|e| ApiError::Internal(e.into()))?,
            ))
        }
        Config::Gcs {
            bucket, endpoint, ..
        } => {
            let credentials = decrypted_secret(state, project, storage, secret)?;
            let service_account = credentials
                .get("service_account")
                .cloned()
                .ok_or_else(invalid)?;
            let service_account = match service_account {
                Value::String(value) => value,
                value if value.is_object() => value.to_string(),
                _ => return Err(invalid()),
            };
            let mut builder = GoogleCloudStorageBuilder::new()
                .with_bucket_name(bucket)
                .with_service_account_key(service_account);
            if let Some(endpoint) = endpoint {
                builder = builder.with_base_url(endpoint);
            }
            Ok(Arc::new(
                builder.build().map_err(|e| ApiError::Internal(e.into()))?,
            ))
        }
        Config::Webdav { endpoint, .. } => {
            let credentials = decrypted_secret(state, project, storage, secret)?;
            let username = credentials["username"].as_str().ok_or_else(invalid)?;
            let password = credentials["password"].as_str().ok_or_else(invalid)?;
            let mut headers = HeaderMap::new();
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!(
                    "Basic {}",
                    STANDARD.encode(format!("{username}:{password}"))
                ))
                .map_err(|_| invalid())?,
            );
            let options = ClientOptions::new()
                .with_allow_http(cfg!(test))
                .with_default_headers(headers);
            Ok(Arc::new(
                HttpBuilder::new()
                    .with_url(endpoint)
                    .with_client_options(options)
                    .build()
                    .map_err(|e| ApiError::Internal(e.into()))?,
            ))
        }
    }
}
async fn ensure_public_resolution(endpoint: &str) -> Result<(), ApiError> {
    let url = remote_endpoint(endpoint)?;
    let host = url.host_str().ok_or_else(invalid)?;
    let port = url.port_or_known_default().ok_or_else(invalid)?;
    let addresses = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| ApiError::Upstream("storage DNS lookup timed out".into()))?
    .map_err(|_| ApiError::Upstream("storage DNS lookup failed".into()))?
    .collect::<Vec<_>>();
    if addresses.is_empty()
        || (!cfg!(test)
            && addresses
                .iter()
                .any(|address| !crate::catalog::refresh::public_address(address.ip())))
    {
        return Err(invalid());
    }
    Ok(())
}
fn decrypted_secret(
    state: &AppState,
    project: &str,
    storage: &str,
    secret: Option<&str>,
) -> Result<Value, ApiError> {
    let document: Value =
        serde_json::from_str(&state.secrets.decrypt(secret.ok_or_else(invalid)?)?)
            .map_err(|_| invalid())?;
    if document["version"] != 1
        || document["project_id"] != project
        || document["storage_id"] != storage
        || !document["secret"].is_object()
    {
        return Err(invalid());
    }
    Ok(document["secret"].clone())
}
pub fn owned_prefix(config: &Config, project: &str, storage: &str) -> Result<String, ApiError> {
    if ![project, storage]
        .iter()
        .all(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
    {
        return Err(invalid());
    }
    let prefix = match config {
        Config::S3 { prefix, .. } | Config::Gcs { prefix, .. } | Config::Webdav { prefix, .. }
            if !prefix.is_empty() =>
        {
            format!("{prefix}/")
        }
        _ => String::new(),
    };
    Ok(format!("{prefix}pangolin/{project}/{storage}/"))
}
pub async fn test_connection(
    state: &AppState,
    project: &str,
    storage: &str,
    config: &Config,
    secret: Option<&str>,
) -> Result<(), ApiError> {
    if let Config::Webdav { endpoint, .. } = config {
        ensure_public_resolution(endpoint).await?;
        let credentials = decrypted_secret(state, project, storage, secret)?;
        let username = credentials["username"].as_str().ok_or_else(invalid)?;
        let password = credentials["password"].as_str().ok_or_else(invalid)?;
        let authorization = HeaderValue::from_str(&format!(
            "Basic {}",
            STANDARD.encode(format!("{username}:{password}"))
        ))
        .map_err(|_| invalid())?;
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            state
                .oidc_client
                .request(
                    reqwest::Method::from_bytes(b"PROPFIND").expect("static HTTP method"),
                    endpoint,
                )
                .header(AUTHORIZATION, authorization)
                .header("depth", "0")
                .send(),
        )
        .await
        .map_err(|_| ApiError::Upstream("storage connection timed out".into()))?
        .map_err(|_| ApiError::Upstream("storage connection failed".into()))?;
        return if response.status().is_success() || response.status().as_u16() == 207 {
            Ok(())
        } else {
            Err(ApiError::Upstream("storage connection failed".into()))
        };
    }
    let store = open(state, project, storage, config, secret).await?;
    let prefix = Path::parse(owned_prefix(config, project, storage)?).map_err(|_| invalid())?;
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        store.list_with_delimiter(Some(&prefix)),
    )
    .await
    .map_err(|_| ApiError::Upstream("storage connection timed out".into()))?
    .map(|_| ())
    .map_err(|_| ApiError::Upstream("storage connection failed".into()))
}
pub async fn put(store: &dyn ObjectStore, key: &str, bytes: Vec<u8>) -> Result<(), ApiError> {
    let path = Path::parse(key).map_err(|_| invalid())?;
    match store
        .put_opts(
            &path,
            bytes.clone().into(),
            PutOptions {
                mode: PutMode::Create,
                ..Default::default()
            },
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(object_store::Error::AlreadyExists { .. }) => {
            let existing = store
                .get(&path)
                .await
                .map_err(|e| ApiError::Internal(e.into()))?
                .bytes()
                .await
                .map_err(|e| ApiError::Internal(e.into()))?;
            if existing.as_ref() != bytes {
                return Err(ApiError::Conflict(
                    "immutable object content differs".into(),
                ));
            }
            Ok(())
        }
        Err(object_store::Error::NotImplemented { .. }) => {
            // WebDAV has no portable create-if-absent primitive. Refuse to
            // replace an existing different object, then use its overwrite-only
            // PUT for a key that was absent at the preceding read.
            match store.get(&path).await {
                Ok(existing) => {
                    let existing = existing
                        .bytes()
                        .await
                        .map_err(|error| ApiError::Internal(error.into()))?;
                    if existing.as_ref() == bytes {
                        Ok(())
                    } else {
                        Err(ApiError::Conflict(
                            "immutable object content differs".into(),
                        ))
                    }
                }
                Err(object_store::Error::NotFound { .. }) => store
                    .put(&path, bytes.into())
                    .await
                    .map(|_| ())
                    .map_err(|error| ApiError::Internal(error.into())),
                Err(error) => Err(ApiError::Internal(error.into())),
            }
        }
        Err(e) => Err(ApiError::Internal(e.into())),
    }
}
