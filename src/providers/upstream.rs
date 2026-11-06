//! Bounded HTTP clients shared by aimux transport and provider operations.
use std::{collections::HashMap, sync::Mutex};

const MAX_UPSTREAM_CLIENTS: usize = 1024;

pub struct ProxySettings<'a> {
    pub url: &'a str,
    pub username: Option<&'a str>,
    pub password: Option<&'a str>,
    pub reuse_connections: bool,
}

pub struct ClientPool {
    clients: Mutex<HashMap<String, reqwest::Client>>,
    max_capacity: usize,
}

impl Default for ClientPool {
    fn default() -> Self {
        Self {
            clients: Mutex::new(HashMap::new()),
            max_capacity: MAX_UPSTREAM_CLIENTS,
        }
    }
}

impl ClientPool {
    pub fn for_proxy(
        &self,
        default: &reqwest::Client,
        timeout: std::time::Duration,
        settings: Option<ProxySettings<'_>>,
    ) -> Result<reqwest::Client, reqwest::Error> {
        let Some(settings) = settings else {
            return Ok(default.clone());
        };
        if !settings.reuse_connections {
            return http_client_with_proxy(timeout, Some(settings));
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(settings.url.as_bytes());
        hasher.update(&[0]);
        hasher.update(settings.username.unwrap_or_default().as_bytes());
        hasher.update(&[0]);
        hasher.update(settings.password.unwrap_or_default().as_bytes());
        let key = hasher.finalize().to_hex().to_string();
        if let Some(client) = self
            .clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
            .cloned()
        {
            return Ok(client);
        }
        let client = http_client_with_proxy(timeout, Some(settings))?;
        let mut clients = self
            .clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = clients.get(&key) {
            return Ok(existing.clone());
        }
        if clients.len() >= self.max_capacity {
            clients.clear();
        }
        clients.insert(key, client.clone());
        Ok(client)
    }

    #[cfg(test)]
    pub(crate) fn with_capacity(max_capacity: usize) -> Self {
        assert!(max_capacity > 0);
        Self {
            clients: Mutex::new(HashMap::new()),
            max_capacity,
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

pub fn http_client(timeout: std::time::Duration) -> Result<reqwest::Client, reqwest::Error> {
    http_client_with_proxy(timeout, None)
}

pub fn http_client_with_proxy(
    timeout: std::time::Duration,
    proxy: Option<ProxySettings<'_>>,
) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("pangolin/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .timeout(timeout);
    if let Some(proxy) = proxy {
        let mut configured = reqwest::Proxy::all(proxy.url)?;
        if let Some(username) = proxy.username {
            configured = configured.basic_auth(username, proxy.password.unwrap_or_default());
        }
        builder = builder.proxy(configured);
        if !proxy.reuse_connections {
            builder = builder.pool_max_idle_per_host(0);
        }
    }
    builder.build()
}
