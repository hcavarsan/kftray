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
    get_kubeconfig_paths_from_option,
    merge_kubeconfigs,
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
        let key = self.resolve_key(key)?;
        let context_name = key
            .context_name
            .as_deref()
            .expect("resolve_key always returns a key carrying a context");

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
                let client_arc = cached.connection.clone();
                drop(cached);
                drop(guard);
                self.release_creation_lock(&key, &lock);
                return Ok(client_arc);
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

    /// Resolves a key with no explicit context to the kubeconfig's
    /// current-context, so legacy configs and bulk actions that never set an
    /// explicit context keep working through whatever context `kubectl`
    /// would use. The resolved name is folded into the key so the client
    /// cache and creation lock are keyed by the actual context, never by the
    /// absence of one.
    fn resolve_key(&self, key: ServiceClientKey) -> anyhow::Result<ServiceClientKey> {
        if key.context_name.is_some() {
            return Ok(key);
        }

        let paths = get_kubeconfig_paths_from_option(key.kubeconfig_path.clone())?;
        let (kubeconfig, _errors) = merge_kubeconfigs(&paths)?;
        let current_context = kubeconfig.current_context.ok_or_else(|| {
            anyhow::anyhow!(
                "Kubernetes context is required (kubeconfig: {}) and it has no current-context",
                key.kubeconfig_path.as_deref().unwrap_or("default")
            )
        })?;

        Ok(ServiceClientKey {
            context_name: Some(current_context),
            kubeconfig_path: key.kubeconfig_path,
        })
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
    async fn get_connection_errors_when_kubeconfig_has_no_current_context() {
        let manager = SharedClientManager::new();
        let key = ServiceClientKey::new(
            None,
            Some("/nonexistent/kftray-test-kubeconfig-no-current-context".to_string()),
        );

        let err = manager.get_connection(key).await.expect_err(
            "a key without a context and without a resolvable current-context must not resolve",
        );
        assert!(
            err.to_string().contains("current-context"),
            "error should name the missing current-context: {err}"
        );
    }

    #[tokio::test]
    async fn get_connection_resolves_none_context_to_kubeconfig_current_context() {
        use std::io::Write;

        crate::ssl::ensure_crypto_provider_installed();

        let mut kubeconfig_file = tempfile::NamedTempFile::new().expect("create temp kubeconfig");
        write!(
            kubeconfig_file,
            "apiVersion: v1\n\
             kind: Config\n\
             current-context: kftray-test-current-context\n\
             contexts:\n\
             - name: kftray-test-current-context\n  \
               context:\n    \
                 cluster: kftray-test-cluster\n    \
                 user: kftray-test-user\n\
             clusters:\n\
             - name: kftray-test-cluster\n  \
               cluster:\n    \
                 server: https://127.0.0.1:1\n\
             users:\n\
             - name: kftray-test-user\n  \
               user: {{}}\n"
        )
        .expect("write temp kubeconfig");

        let manager = SharedClientManager::new();
        let key = ServiceClientKey::new(
            None,
            Some(kubeconfig_file.path().to_string_lossy().to_string()),
        );

        let err = manager
            .get_connection(key)
            .await
            .expect_err("an unreachable resolved cluster must not produce a client");
        assert!(
            err.to_string().contains("kftray-test-current-context"),
            "resolving a None context must use the kubeconfig's current-context name so the \
             connection attempt (and its error) names it: {err}"
        );
        assert!(
            !err.to_string().contains("Kubernetes context is required"),
            "resolving a None context via the kubeconfig's current-context must not fail with \
             a missing-context error: {err}"
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

    #[tokio::test]
    async fn get_connection_releases_lock_on_post_lock_cache_hit() {
        use http::{
            Request,
            Response,
        };
        use kube::client::Body;
        use tower_test::mock;

        let manager = Arc::new(SharedClientManager::new());
        let key = ServiceClientKey::new(Some("ctx".to_string()), None);

        // Stand in for a concurrent first caller that already holds the
        // creation lock, about to finish populating the cache.
        let lock = manager
            .creation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let guard = lock.clone().lock_owned().await;
        drop(lock);

        let waiter_manager = manager.clone();
        let waiter_key = key.clone();
        let waiter = tokio::spawn(async move { waiter_manager.get_connection(waiter_key).await });

        // Let the waiter's pre-lock cache check (a miss) run and block it
        // on the still-held creation lock, mirroring a real race where a
        // second caller arrives while the first is still creating a client.
        tokio::time::sleep(Duration::from_millis(20)).await;

        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let connection = KubeConnection {
            client: kube::Client::new(mock_service, "default"),
            cluster_url: "https://example.invalid".parse().unwrap(),
        };
        manager
            .clients
            .insert(key.clone(), CachedClient::new(connection));

        // Release the lock: the waiter's post-lock check now sees the
        // cache entry just inserted and must take the cache-hit path.
        drop(guard);

        let result = tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("post-lock cache hit must resolve, not hang")
            .expect("waiter task must not panic");
        assert!(
            result.is_ok(),
            "post-lock cache hit must resolve to the cached connection"
        );

        assert!(
            !manager.creation_locks.contains_key(&key),
            "a post-lock cache hit must release its clone of the creation lock"
        );
    }
}
