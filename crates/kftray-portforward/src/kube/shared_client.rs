use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use dashmap::DashMap;
use kube::Client;
use once_cell::sync::Lazy;

use crate::kube::client::create_client_with_specific_context;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ServiceClientKey {
    pub context_name: Option<String>,
    pub kubeconfig_path: Option<String>,
}

impl ServiceClientKey {
    pub fn new(context_name: Option<String>, kubeconfig_path: Option<String>) -> Self {
        Self {
            context_name,
            kubeconfig_path,
        }
    }
}

struct CachedClient {
    client: Arc<Client>,
    created_at: Instant,
}

impl CachedClient {
    fn new(client: Client) -> Self {
        Self {
            client: Arc::new(client),
            created_at: Instant::now(),
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }
}

pub struct SharedClientManager {
    clients: DashMap<ServiceClientKey, CachedClient>,
    client_ttl: Duration,
}

impl SharedClientManager {
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
            client_ttl: Duration::from_secs(3600),
        }
    }

    pub async fn get_client(&self, key: ServiceClientKey) -> anyhow::Result<Arc<Client>> {
        if let Some(cached) = self.clients.get(&key) {
            if !cached.is_expired(self.client_ttl) {
                return Ok(cached.client.clone());
            }
            self.clients.remove(&key);
        }

        let (client_opt, _, _) = create_client_with_specific_context(
            key.kubeconfig_path.clone(),
            key.context_name.as_deref(),
        )
        .await?;

        let client = client_opt.ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to create client for context: {:?}",
                key.context_name
            )
        })?;

        let cached_client = CachedClient::new(client);
        let client_arc = cached_client.client.clone();
        self.clients.insert(key, cached_client);
        Ok(client_arc)
    }

    pub fn invalidate_client(&self, key: &ServiceClientKey) {
        self.clients.remove(key);
    }

    pub fn cleanup_expired(&self) {
        self.clients
            .retain(|_, cached| !cached.is_expired(self.client_ttl));
    }
}

impl Default for SharedClientManager {
    fn default() -> Self {
        Self::new()
    }
}

pub static SHARED_CLIENT_MANAGER: Lazy<SharedClientManager> = Lazy::new(SharedClientManager::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_client_key() {
        let key1 = ServiceClientKey::new(
            Some("context1".to_string()),
            Some("/path/to/config".to_string()),
        );

        let key2 = ServiceClientKey::new(
            Some("context1".to_string()),
            Some("/path/to/config".to_string()),
        );

        assert_eq!(key1, key2);
    }
}
