use crate::{
    api::{ApiError, AppState},
    crypto::SecretBox,
};
use object_store::{
    ObjectStore, ObjectStoreExt, PutMode, PutOptions, aws::AmazonS3Builder, local::LocalFileSystem,
    path::Path,
};
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
    }
    Ok(())
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
pub async fn open(
    state: &AppState,
    project: &str,
    storage: &str,
    config: &Config,
    secret: Option<&str>,
) -> Result<Arc<dyn ObjectStore>, ApiError> {
    validate(config)?;
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
    }
}
pub fn owned_prefix(config: &Config, project: &str, storage: &str) -> Result<String, ApiError> {
    if ![project, storage]
        .iter()
        .all(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
    {
        return Err(invalid());
    }
    let prefix = match config {
        Config::S3 { prefix, .. } if !prefix.is_empty() => format!("{prefix}/"),
        _ => String::new(),
    };
    Ok(format!("{prefix}pangolin/{project}/{storage}/"))
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
        Err(e) => Err(ApiError::Internal(e.into())),
    }
}
