use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, thiserror::Error)]
#[error("affinity store unavailable")]
pub(super) struct StoreError;

#[async_trait::async_trait]
pub(super) trait Store: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>, StoreError>;
    async fn set(&self, key: &str, value: &str, ttl: Duration) -> Result<(), StoreError>;
    async fn delete(&self, key: &str) -> Result<(), StoreError>;
}

#[derive(Clone)]
struct Entry {
    value: String,
    expires_at: Duration,
}

pub(super) struct Memory {
    entries: moka::sync::Cache<String, Entry>,
    clock: Arc<dyn Fn() -> Duration + Send + Sync>,
}

impl Memory {
    pub fn new() -> Self {
        Self::with_clock(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
        })
    }

    pub fn with_clock(clock: impl Fn() -> Duration + Send + Sync + 'static) -> Self {
        Self {
            entries: moka::sync::Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(86400))
                .build(),
            clock: Arc::new(clock),
        }
    }

    pub fn expires_at(&self, key: &str) -> Option<Duration> {
        self.entries.get(key).map(|entry| entry.expires_at)
    }
}

#[async_trait::async_trait]
impl Store for Memory {
    async fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        let Some(entry) = self.entries.get(key) else {
            return Ok(None);
        };
        if (self.clock)() >= entry.expires_at {
            return Ok(None);
        }
        Ok(Some(entry.value))
    }

    async fn set(&self, key: &str, value: &str, ttl: Duration) -> Result<(), StoreError> {
        let expires_at = (self.clock)().checked_add(ttl).ok_or(StoreError)?;
        self.entries.insert(
            key.to_owned(),
            Entry {
                value: value.to_owned(),
                expires_at,
            },
        );
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.entries.invalidate(key);
        Ok(())
    }
}

#[cfg(feature = "redis-affinity")]
pub(super) struct Redis(redis::Client);

#[cfg(feature = "redis-affinity")]
impl Redis {
    pub fn new(url: &str) -> Result<Self, StoreError> {
        redis::Client::open(url).map(Self).map_err(|_| StoreError)
    }

    async fn connection(&self) -> Result<redis::aio::MultiplexedConnection, StoreError> {
        tokio::time::timeout(
            Duration::from_secs(2),
            self.0.get_multiplexed_async_connection(),
        )
        .await
        .map_err(|_| StoreError)?
        .map_err(|_| StoreError)
    }
}

#[cfg(feature = "redis-affinity")]
#[async_trait::async_trait]
impl Store for Redis {
    async fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        use redis::AsyncCommands;
        tokio::time::timeout(
            Duration::from_secs(2),
            self.connection()
                .await?
                .get(format!("pangolin:affinity:v1:{key}")),
        )
        .await
        .map_err(|_| StoreError)?
        .map_err(|_| StoreError)
    }

    async fn set(&self, key: &str, value: &str, ttl: Duration) -> Result<(), StoreError> {
        use redis::AsyncCommands;
        tokio::time::timeout(
            Duration::from_secs(2),
            self.connection().await?.set_ex(
                format!("pangolin:affinity:v1:{key}"),
                value,
                ttl.as_secs(),
            ),
        )
        .await
        .map_err(|_| StoreError)?
        .map_err(|_| StoreError)
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        use redis::AsyncCommands;
        let _: usize = tokio::time::timeout(
            Duration::from_secs(2),
            self.connection()
                .await?
                .del(format!("pangolin:affinity:v1:{key}")),
        )
        .await
        .map_err(|_| StoreError)?
        .map_err(|_| StoreError)?;
        Ok(())
    }
}
