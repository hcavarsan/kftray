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
        let context_name = key.context_name.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "Kubernetes context is required (kubeconfig: {})",
                key.kubeconfig_path.as_deref().unwrap_or("default")
            )
        })?;
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

        let guard = lock.lock().await;

        if let Some(cached) = self.clients.get(&key) {
            if !cached.is_expired(self.client_ttl) {
                return Ok(cached.connection.clone());
            }
            drop(cached);
            self.clients.remove(&key);
        }

        match create_client_with_specific_context(key.kubeconfig_path.clone(), context_name).await {
            Ok(connection) => {
                let cached_client = CachedClient::new(connection);
                let client_arc = cached_client.connection.clone();
                self.clients.insert(key.clone(), cached_client);
                drop(guard);
                self.release_creation_lock(&key, &lock);
                Ok(client_arc)
            }
            Err(error) => {
                drop(guard);
                self.release_creation_lock(&key, &lock);
                Err(error)
            }
        }
    }

    /// Drops a creation lock nobody else is using. Every creation attempt —
    /// success or failure — calls this so the lock entry does not outlive the
    /// callers racing to build a client for one context; a lock another
    /// waiter still holds a clone of is left in place.
    fn release_creation_lock(&self, key: &ServiceClientKey, lock: &Arc<Mutex<()>>) {
        self.creation_locks.remove_if(key, |_, entry| {
            Arc::ptr_eq(entry, lock) && Arc::strong_count(entry) == 2
        });
    }

    pub fn invalidate_client(&self, key: &ServiceClientKey) {
        self.clients.remove(key);
    }

    pub fn cleanup_expired(&self) {
        self.clients
            .retain(|_, cached| !cached.is_expired(self.client_ttl));
        self.creation_locks
            .retain(|key, lock| self.clients.contains_key(key) || Arc::strong_count(lock) > 1);
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

    #[tokio::test]
    async fn get_connection_errors_when_context_is_missing() {
        let manager = SharedClientManager::new();
        let key = ServiceClientKey::new(None, None);

        let err = manager
            .get_connection(key)
            .await
            .expect_err("a key without a context must not resolve to a connection");
        assert!(
            err.to_string().contains("context"),
            "error should name the missing context: {err}"
        );
    }

    #[tokio::test]
    async fn get_connection_releases_lock_after_failed_creation() {
        let manager = SharedClientManager::new();
        let key = ServiceClientKey::new(
            Some("kftray-test-nonexistent-context".to_string()),
            Some("/nonexistent/kftray-test-kubeconfig".to_string()),
        );

        manager.get_connection(key.clone()).await.expect_err(
            "a context absent from the (empty) kubeconfig must fail to create a client",
        );

        assert!(
            !manager.creation_locks.contains_key(&key),
            "a failed creation with no other waiter must not leak its lock entry"
        );
    }

    #[test]
    fn release_creation_lock_keeps_entry_when_another_waiter_holds_it() {
        let manager = SharedClientManager::new();
        let key = ServiceClientKey::new(Some("ctx".to_string()), None);
        let lock = manager
            .creation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _second_waiter = lock.clone();

        manager.release_creation_lock(&key, &lock);

        assert!(
            manager.creation_locks.contains_key(&key),
            "a lock another waiter still references must not be dropped"
        );
    }

    #[test]
    fn cleanup_expired_retains_lock_entries_still_in_use() {
        let manager = SharedClientManager::new();
        let key = ServiceClientKey::new(Some("ctx".to_string()), None);
        let lock = manager
            .creation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();

        manager.cleanup_expired();
        assert!(
            manager.creation_locks.contains_key(&key),
            "cleanup_expired must not drop a lock entry an in-flight creation still holds"
        );

        drop(lock);
        manager.cleanup_expired();
        assert!(
            !manager.creation_locks.contains_key(&key),
            "cleanup_expired should drop an unheld lock entry once no client is cached for it"
        );
    }
}
