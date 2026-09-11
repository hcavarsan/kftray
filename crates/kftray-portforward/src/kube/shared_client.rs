use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

use crate::kube::client::{
    KubeConnection,
    create_client_with_specific_context,
};

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
    connection: Arc<KubeConnection>,
    created_at: Instant,
}

impl CachedClient {
    fn new(connection: KubeConnection) -> Self {
        Self {
            connection: Arc::new(connection),
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
    creation_locks: DashMap<ServiceClientKey, Arc<Mutex<()>>>,
}

impl SharedClientManager {
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
            client_ttl: Duration::from_secs(3600),
            creation_locks: DashMap::new(),
        }
    }

    pub async fn get_connection(
        &self, key: ServiceClientKey,
    ) -> anyhow::Result<Arc<KubeConnection>> {
        let context_name = key
            .context_name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Kubernetes context is required"))?;
        if let Some(cached) = self.clients.get(&key) {
            if !cached.is_expired(self.client_ttl) {
                return Ok(cached.connection.clone());
            }
            drop(cached);
            self.clients.remove(&key);
        }

        let lock = self
            .creation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();

        let _guard = lock.lock().await;

        if let Some(cached) = self.clients.get(&key) {
            if !cached.is_expired(self.client_ttl) {
                return Ok(cached.connection.clone());
            }
            drop(cached);
            self.clients.remove(&key);
        }

        let result =
            create_client_with_specific_context(key.kubeconfig_path.clone(), context_name).await;

        match result {
            Ok(client) => {
                let cached_client = CachedClient::new(client);
                let client_arc = cached_client.connection.clone();
                self.clients.insert(key, cached_client);
                Ok(client_arc)
            }
            Err(e) => {
                self.creation_locks.remove(&key);
                Err(e)
            }
        }
    }

    pub fn invalidate_client(&self, key: &ServiceClientKey) {
        self.clients.remove(key);
    }

    pub fn cleanup_expired(&self) {
        self.clients
            .retain(|_, cached| !cached.is_expired(self.client_ttl));
        self.creation_locks
            .retain(|key, _| self.clients.contains_key(key));
    }
}

impl Default for SharedClientManager {
    fn default() -> Self {
        Self::new()
    }
}

pub static SHARED_CLIENT_MANAGER: Lazy<SharedClientManager> = Lazy::new(SharedClientManager::new);
