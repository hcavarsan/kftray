use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};
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

/// A creation lock shared by every caller racing to build a client for one
/// context. The count tracks callers currently interested (registered but
/// not yet released), so the lock entry can be reclaimed exactly when the
/// last interested caller releases it, instead of inferring "unshared" from
/// `Arc::strong_count`, which a stray clone elsewhere could perturb.
type CreationLock = Arc<(Mutex<()>, AtomicUsize)>;

/// RAII guard for creation-lock interest. Incrementing the counter and
/// registering its release used to be two separate steps performed at every
/// exit point of `get_connection`; a task cancelled between the increment
/// and one of those manual release calls (e.g. dropped while awaiting the
/// mutex or the client-creation future) left the counter incremented
/// forever, pinning the entry so `cleanup_expired` could never reclaim it.
/// Tying the release to `Drop` instead makes it run exactly once no matter
/// how the caller's future exits, cancellation included.
struct CreationLockInterest<'a> {
    manager: &'a SharedClientManager,
    key: ServiceClientKey,
    lock: CreationLock,
}

impl<'a> CreationLockInterest<'a> {
    fn new(manager: &'a SharedClientManager, key: ServiceClientKey, lock: CreationLock) -> Self {
        lock.1.fetch_add(1, Ordering::SeqCst);
        Self { manager, key, lock }
    }
}

impl Drop for CreationLockInterest<'_> {
    fn drop(&mut self) {
        self.manager.release_creation_lock(&self.key, &self.lock);
    }
}

/// A kubeconfig resolution's file state at the time it was resolved: each
/// path considered paired with its last-modified time (`None` if the path
/// did not exist). Compared against a fresh stat on a cache hit so a
/// current-context resolution is not replayed after the file it came from
/// changed underneath it, even while still within the TTL.
type KubeconfigSignature = Vec<(PathBuf, Option<std::time::SystemTime>)>;

fn kubeconfig_signature(paths: &[PathBuf]) -> KubeconfigSignature {
    paths
        .iter()
        .map(|path| {
            let mtime = std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok();
            (path.clone(), mtime)
        })
        .collect()
}

pub struct SharedClientManager {
    clients: DashMap<ServiceClientKey, CachedClient>,
    client_ttl: Duration,
    creation_locks: DashMap<ServiceClientKey, CreationLock>,
    /// Resolved current-context per kubeconfig path, so a `None`-context
    /// `get_connection`/`resolve_key` call only reads and merges the
    /// kubeconfig on the first miss; later calls reuse the recorded
    /// context instead of touching disk again.
    resolved_contexts: DashMap<Option<String>, (String, Instant, KubeconfigSignature)>,
}

impl SharedClientManager {
    pub fn new() -> Self {
        Self::with_ttl(Duration::from_secs(3600))
    }

    fn with_ttl(client_ttl: Duration) -> Self {
        Self {
            clients: DashMap::new(),
            client_ttl,
            creation_locks: DashMap::new(),
            resolved_contexts: DashMap::new(),
        }
    }

    pub async fn get_connection(
        &self, key: ServiceClientKey,
    ) -> anyhow::Result<Arc<KubeConnection>> {
        let key = self.resolve_key(key).await?;
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
            .or_insert_with(|| Arc::new((Mutex::new(()), AtomicUsize::new(0))))
            .clone();
        let _interest = CreationLockInterest::new(self, key.clone(), lock.clone());

        let guard = lock.0.lock().await;

        if let Some(cached) = self.clients.get(&key) {
            if !cached.is_expired(self.client_ttl) {
                let client_arc = cached.connection.clone();
                drop(cached);
                drop(guard);
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
                Ok(client_arc)
            }
            Err(error) => {
                drop(guard);
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
    ///
    /// The kubeconfig read + merge only happens on a cache miss: the
    /// resolved context is cached per kubeconfig path in
    /// `resolved_contexts`, so a repeated cache-hit call never touches disk,
    /// and the read itself runs on a blocking thread since it is synchronous
    /// file IO. Resolving the paths and stating them (env var read plus one
    /// stat per file) is cheap enough to redo on every call: it is what
    /// lets a still-fresh TTL entry be treated as stale once the kubeconfig
    /// it came from changed on disk, instead of only when the TTL expires.
    async fn resolve_key(&self, key: ServiceClientKey) -> anyhow::Result<ServiceClientKey> {
        if key.context_name.is_some() {
            return Ok(key);
        }

        let current_paths = get_kubeconfig_paths_from_option(key.kubeconfig_path.clone())?;
        let current_signature = kubeconfig_signature(&current_paths);

        if let Some(resolved) = self.resolved_contexts.get(&key.kubeconfig_path) {
            let (context_name, resolved_at, signature) = resolved.value();
            if resolved_at.elapsed() <= self.client_ttl && *signature == current_signature {
                return Ok(ServiceClientKey {
                    context_name: Some(context_name.clone()),
                    kubeconfig_path: key.kubeconfig_path,
                });
            }
        }

        let kubeconfig_path = key.kubeconfig_path.clone();
        let current_context = tokio::task::spawn_blocking(move || {
            let (kubeconfig, errors) = merge_kubeconfigs(&current_paths)?;
            kubeconfig.current_context.ok_or_else(|| {
                if errors.is_empty() {
                    anyhow::anyhow!(
                        "Kubernetes context is required (kubeconfig: {}) and it has no \
                         current-context",
                        kubeconfig_path.as_deref().unwrap_or("default")
                    )
                } else {
                    anyhow::anyhow!(
                        "Kubernetes context is required (kubeconfig: {}) and it has no \
                         current-context; it also failed to read: {}",
                        kubeconfig_path.as_deref().unwrap_or("default"),
                        errors.join("; ")
                    )
                }
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("kubeconfig resolution task panicked: {e}"))??;

        self.resolved_contexts.insert(
            key.kubeconfig_path.clone(),
            (current_context.clone(), Instant::now(), current_signature),
        );

        Ok(ServiceClientKey {
            context_name: Some(current_context),
            kubeconfig_path: key.kubeconfig_path,
        })
    }

    /// Drops a creation lock nobody else is using. Every creation attempt —
    /// success or failure — calls this so the lock entry does not outlive the
    /// callers racing to build a client for one context; a lock another
    /// waiter still holds a clone of is left in place.
    fn release_creation_lock(&self, key: &ServiceClientKey, lock: &CreationLock) {
        if lock.1.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.creation_locks.remove_if(key, |_, entry| {
                Arc::ptr_eq(entry, lock) && entry.1.load(Ordering::SeqCst) == 0
            });
        }
    }

    /// Invalidates the cached client(s) for `key`. A `None` context means
    /// the caller does not know (or no longer trusts) which context is
    /// current for this kubeconfig. If a context was already resolved for
    /// this kubeconfig path (see `resolved_contexts`), only that exact
    /// entry is evicted and the recorded resolution is dropped too, so a
    /// later `resolve_key` call re-reads the current context from disk
    /// instead of replaying the stale cached one; otherwise fall back to
    /// dropping every entry for the kubeconfig path, since there is no
    /// recorded context to target. An explicit context is checked against
    /// the recorded resolution for its kubeconfig path too and evicts it
    /// when it matches, so a later `None`-context call does not replay a
    /// resolution an exact-context eviction just invalidated.
    pub fn invalidate_client(&self, key: &ServiceClientKey) {
        if key.context_name.is_none() {
            if let Some((_, (resolved, _, _))) = self.resolved_contexts.remove(&key.kubeconfig_path)
            {
                let resolved_key = ServiceClientKey {
                    context_name: Some(resolved),
                    kubeconfig_path: key.kubeconfig_path.clone(),
                };
                self.clients.remove(&resolved_key);
                return;
            }
            self.clients
                .retain(|cached_key, _| cached_key.kubeconfig_path != key.kubeconfig_path);
            return;
        }

        if let Some(ctx) = key.context_name.as_deref() {
            self.resolved_contexts
                .remove_if(&key.kubeconfig_path, |_, (resolved, _, _)| resolved == ctx);
        }
        self.clients.remove(key);
    }

    pub fn cleanup_expired(&self) {
        self.clients
            .retain(|_, cached| !cached.is_expired(self.client_ttl));
        self.creation_locks.retain(|key, lock| {
            self.clients.contains_key(key) || lock.1.load(Ordering::SeqCst) > 0
        });
        self.resolved_contexts
            .retain(|kubeconfig_path, (context_name, resolved_at, _)| {
                if resolved_at.elapsed() > self.client_ttl {
                    return false;
                }
                let resolved_key = ServiceClientKey {
                    context_name: Some(context_name.clone()),
                    kubeconfig_path: kubeconfig_path.clone(),
                };
                self.clients.contains_key(&resolved_key)
            });
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

    /// Polls until the creation lock's interest counter reaches `expected`,
    /// instead of sleeping a fixed duration and hoping the other task got
    /// scheduled in time.
    async fn wait_for_interest_count(lock: &CreationLock, expected: usize) {
        for _ in 0..500 {
            if lock.1.load(Ordering::SeqCst) == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!(
            "creation lock interest count did not reach {expected} in time (last seen {})",
            lock.1.load(Ordering::SeqCst)
        );
    }

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
        assert!(
            err.to_string().contains("failed to read"),
            "error should surface the underlying kubeconfig read failure instead of \
             discarding it: {err}"
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

    fn kubeconfig_yaml(context_name: &str) -> String {
        format!(
            "apiVersion: v1\n\
             kind: Config\n\
             current-context: {context_name}\n\
             contexts:\n\
             - name: {context_name}\n  \
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
    }

    /// Rewrites `path` and restores its mtime to `mtime`, so the write is
    /// invisible to a signature comparison keyed on the file's timestamp.
    fn rewrite_kubeconfig_preserving_mtime(
        path: &std::path::Path, contents: &str, mtime: std::time::SystemTime,
    ) {
        std::fs::write(path, contents).expect("rewrite temp kubeconfig");
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .and_then(|file| file.set_times(std::fs::FileTimes::new().set_modified(mtime)))
            .expect("restore the kubeconfig's mtime");
    }

    #[tokio::test]
    async fn resolve_key_with_none_context_is_cached_on_a_hit_within_ttl() {
        use std::io::Write;

        let mut kubeconfig_file = tempfile::NamedTempFile::new().expect("create temp kubeconfig");
        write!(
            kubeconfig_file,
            "{}",
            kubeconfig_yaml("kftray-test-cache-hit-context")
        )
        .expect("write temp kubeconfig");

        let manager = SharedClientManager::new();
        let key = ServiceClientKey::new(
            None,
            Some(kubeconfig_file.path().to_string_lossy().to_string()),
        );

        let first_resolved = manager
            .resolve_key(key.clone())
            .await
            .expect("a kubeconfig with a current-context must resolve");
        assert_eq!(
            first_resolved.context_name.as_deref(),
            Some("kftray-test-cache-hit-context")
        );

        let original_mtime = std::fs::metadata(kubeconfig_file.path())
            .and_then(|meta| meta.modified())
            .expect("read the kubeconfig's mtime");
        // Rewritten with a different current-context but restored to its
        // original mtime: a cache hit must reuse the recorded resolution
        // since nothing it checks (TTL, mtime) says the file changed.
        rewrite_kubeconfig_preserving_mtime(
            kubeconfig_file.path(),
            &kubeconfig_yaml("kftray-test-changed-context"),
            original_mtime,
        );

        let second_resolved = manager
            .resolve_key(key)
            .await
            .expect("a cache hit must resolve without needing the rewritten kubeconfig to parse");
        assert_eq!(
            second_resolved.context_name.as_deref(),
            Some("kftray-test-cache-hit-context"),
            "a cache hit with an unchanged mtime must reuse the recorded resolved context"
        );
    }

    #[tokio::test]
    async fn resolve_key_re_resolves_when_the_kubeconfig_mtime_changes() {
        use std::io::Write;

        let mut kubeconfig_file = tempfile::NamedTempFile::new().expect("create temp kubeconfig");
        write!(
            kubeconfig_file,
            "{}",
            kubeconfig_yaml("kftray-test-original-context")
        )
        .expect("write temp kubeconfig");

        let manager = SharedClientManager::new();
        let key = ServiceClientKey::new(
            None,
            Some(kubeconfig_file.path().to_string_lossy().to_string()),
        );

        let first_resolved = manager
            .resolve_key(key.clone())
            .await
            .expect("a kubeconfig with a current-context must resolve");
        assert_eq!(
            first_resolved.context_name.as_deref(),
            Some("kftray-test-original-context")
        );

        // A different current-context, with the mtime moved well past
        // filesystem timestamp resolution, simulates the user switching
        // contexts (e.g. `kubectl config use-context`) while the TTL is
        // still fresh.
        let advanced_mtime = std::time::SystemTime::now() + Duration::from_secs(60);
        rewrite_kubeconfig_preserving_mtime(
            kubeconfig_file.path(),
            &kubeconfig_yaml("kftray-test-switched-context"),
            advanced_mtime,
        );

        let second_resolved = manager
            .resolve_key(key)
            .await
            .expect("the rewritten kubeconfig must still resolve");
        assert_eq!(
            second_resolved.context_name.as_deref(),
            Some("kftray-test-switched-context"),
            "a changed mtime must force a re-resolution even though the TTL has not expired"
        );
    }

    #[tokio::test]
    async fn invalidate_client_with_none_context_uses_recorded_resolved_context_only() {
        use http::{
            Request,
            Response,
        };
        use kube::client::Body;
        use tower_test::mock;

        let manager = SharedClientManager::new();
        let kubeconfig_path = Some("kftray-test-recorded-context-kubeconfig".to_string());

        let make_cached_client = || {
            let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
            CachedClient::new(KubeConnection {
                client: kube::Client::new(mock_service, "default"),
                cluster_url: "https://example.invalid".parse().unwrap(),
            })
        };

        let recorded_key =
            ServiceClientKey::new(Some("recorded-ctx".to_string()), kubeconfig_path.clone());
        let other_key =
            ServiceClientKey::new(Some("other-ctx".to_string()), kubeconfig_path.clone());

        manager
            .clients
            .insert(recorded_key.clone(), make_cached_client());
        manager
            .clients
            .insert(other_key.clone(), make_cached_client());
        manager.resolved_contexts.insert(
            kubeconfig_path.clone(),
            ("recorded-ctx".to_string(), Instant::now(), Vec::new()),
        );

        manager.invalidate_client(&ServiceClientKey::new(None, kubeconfig_path.clone()));

        assert!(
            !manager.clients.contains_key(&recorded_key),
            "the recorded resolved-context entry must be evicted"
        );
        assert!(
            manager.clients.contains_key(&other_key),
            "invalidation with a recorded resolved context must hit only that exact key, not \
             sweep every entry for the kubeconfig path"
        );
        assert!(
            !manager.resolved_contexts.contains_key(&kubeconfig_path),
            "invalidation must drop the recorded resolved context so a later resolve re-reads \
             the current context from disk"
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

    #[tokio::test]
    async fn invalidate_client_with_none_context_removes_resolved_entry() {
        use std::io::Write;

        use http::{
            Request,
            Response,
        };
        use kube::client::Body;
        use tower_test::mock;

        let mut kubeconfig_file = tempfile::NamedTempFile::new().expect("create temp kubeconfig");
        write!(
            kubeconfig_file,
            "apiVersion: v1\n\
             kind: Config\n\
             current-context: kftray-test-invalidate-context\n\
             contexts:\n\
             - name: kftray-test-invalidate-context\n  \
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
        let kubeconfig_path = kubeconfig_file.path().to_string_lossy().to_string();
        let raw_key = ServiceClientKey::new(None, Some(kubeconfig_path.clone()));
        let resolved_key = ServiceClientKey::new(
            Some("kftray-test-invalidate-context".to_string()),
            Some(kubeconfig_path.clone()),
        );

        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let connection = KubeConnection {
            client: kube::Client::new(mock_service, "default"),
            cluster_url: "https://example.invalid".parse().unwrap(),
        };
        manager
            .clients
            .insert(resolved_key.clone(), CachedClient::new(connection));
        manager.resolved_contexts.insert(
            Some(kubeconfig_path.clone()),
            (
                "kftray-test-invalidate-context".to_string(),
                Instant::now(),
                Vec::new(),
            ),
        );

        manager.invalidate_client(&raw_key);

        assert!(
            !manager.clients.contains_key(&resolved_key),
            "invalidate_client with a None-context key must remove the entry cached under \
             the resolved current-context key, not just the never-cached raw key"
        );
        assert!(
            !manager
                .resolved_contexts
                .contains_key(&Some(kubeconfig_path)),
            "invalidation must drop the recorded resolved context so a later resolve re-reads \
             the current context from disk"
        );
    }

    #[tokio::test]
    async fn invalidate_client_with_none_context_clears_every_entry_for_kubeconfig_path() {
        use http::{
            Request,
            Response,
        };
        use kube::client::Body;
        use tower_test::mock;

        let manager = SharedClientManager::new();
        let kubeconfig_path = Some("kftray-test-shared-kubeconfig".to_string());

        let make_cached_client = || {
            let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
            CachedClient::new(KubeConnection {
                client: kube::Client::new(mock_service, "default"),
                cluster_url: "https://example.invalid".parse().unwrap(),
            })
        };

        let key_a = ServiceClientKey::new(Some("context-a".to_string()), kubeconfig_path.clone());
        let key_b = ServiceClientKey::new(Some("context-b".to_string()), kubeconfig_path.clone());
        let other_kubeconfig_key = ServiceClientKey::new(
            Some("context-a".to_string()),
            Some("kftray-test-other-kubeconfig".to_string()),
        );

        manager.clients.insert(key_a.clone(), make_cached_client());
        manager.clients.insert(key_b.clone(), make_cached_client());
        manager
            .clients
            .insert(other_kubeconfig_key.clone(), make_cached_client());

        manager.invalidate_client(&ServiceClientKey::new(None, kubeconfig_path));

        assert!(
            !manager.clients.contains_key(&key_a),
            "every cached context for the kubeconfig path must be dropped"
        );
        assert!(
            !manager.clients.contains_key(&key_b),
            "every cached context for the kubeconfig path must be dropped, not just one \
             re-resolved current-context entry"
        );
        assert!(
            manager.clients.contains_key(&other_kubeconfig_key),
            "entries for a different kubeconfig path must be left untouched"
        );
    }

    #[tokio::test]
    async fn cancelling_get_connection_releases_creation_lock_interest() {
        let manager = Arc::new(SharedClientManager::new());
        let key = ServiceClientKey::new(Some("ctx".to_string()), None);

        // Hold the creation lock so the spawned get_connection call
        // registers interest and then blocks awaiting the mutex, mirroring
        // a real second caller arriving while a first creation is in
        // flight.
        let lock = manager
            .creation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new((Mutex::new(()), AtomicUsize::new(0))))
            .clone();
        let guard = lock.0.lock().await;

        let waiter_manager = manager.clone();
        let waiter_key = key.clone();
        let handle = tokio::spawn(async move { waiter_manager.get_connection(waiter_key).await });

        // Let the waiter register interest and block on the held mutex.
        wait_for_interest_count(&lock, 1).await;
        assert_eq!(
            lock.1.load(Ordering::SeqCst),
            1,
            "waiter must have registered interest before being cancelled"
        );

        // Cancel the waiter mid-await by dropping its future, the way
        // task cancellation does.
        handle.abort();
        let _ = handle.await;

        assert_eq!(
            lock.1.load(Ordering::SeqCst),
            0,
            "a cancelled get_connection must release its creation-lock interest"
        );

        drop(guard);
    }

    #[test]
    fn release_creation_lock_keeps_entry_when_another_waiter_holds_it() {
        let manager = SharedClientManager::new();
        let key = ServiceClientKey::new(Some("ctx".to_string()), None);
        let lock = manager
            .creation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new((Mutex::new(()), AtomicUsize::new(0))))
            .clone();
        // Two callers registered interest; only one releases.
        lock.1.fetch_add(2, Ordering::SeqCst);

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
            .or_insert_with(|| Arc::new((Mutex::new(()), AtomicUsize::new(0))))
            .clone();
        lock.1.fetch_add(1, Ordering::SeqCst);

        manager.cleanup_expired();
        assert!(
            manager.creation_locks.contains_key(&key),
            "cleanup_expired must not drop a lock entry an in-flight creation still holds"
        );

        lock.1.fetch_sub(1, Ordering::SeqCst);
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
            .or_insert_with(|| Arc::new((Mutex::new(()), AtomicUsize::new(0))))
            .clone();
        lock.1.fetch_add(1, Ordering::SeqCst);
        let guard = lock.0.lock().await;

        let waiter_manager = manager.clone();
        let waiter_key = key.clone();
        let waiter = tokio::spawn(async move { waiter_manager.get_connection(waiter_key).await });

        // Let the waiter's pre-lock cache check (a miss) run and block it
        // on the still-held creation lock, mirroring a real race where a
        // second caller arrives while the first is still creating a client.
        wait_for_interest_count(&lock, 2).await;

        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let connection = KubeConnection {
            client: kube::Client::new(mock_service, "default"),
            cluster_url: "https://example.invalid".parse().unwrap(),
        };
        manager
            .clients
            .insert(key.clone(), CachedClient::new(connection));

        // Release the lock the way a real creator finishing its own attempt
        // would: drop the guard, then release this caller's interest. The
        // waiter's post-lock check now sees the cache entry just inserted
        // and must take the cache-hit path.
        drop(guard);
        manager.release_creation_lock(&key, &lock);

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

    #[tokio::test]
    async fn invalidate_client_with_explicit_context_drops_matching_resolved_context() {
        use http::{
            Request,
            Response,
        };
        use kube::client::Body;
        use tower_test::mock;

        let manager = SharedClientManager::new();
        let kubeconfig_path = Some("kftray-test-explicit-invalidate-kubeconfig".to_string());

        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let connection = KubeConnection {
            client: kube::Client::new(mock_service, "default"),
            cluster_url: "https://example.invalid".parse().unwrap(),
        };

        let key = ServiceClientKey::new(Some("explicit-ctx".to_string()), kubeconfig_path.clone());
        manager
            .clients
            .insert(key.clone(), CachedClient::new(connection));
        manager.resolved_contexts.insert(
            kubeconfig_path.clone(),
            ("explicit-ctx".to_string(), Instant::now(), Vec::new()),
        );

        manager.invalidate_client(&key);

        assert!(
            !manager.clients.contains_key(&key),
            "the invalidated client entry must be removed"
        );
        assert!(
            !manager.resolved_contexts.contains_key(&kubeconfig_path),
            "invalidating the context currently recorded as resolved must drop the recorded \
             resolution too, so a later None-context call re-reads the kubeconfig instead of \
             replaying the now-invalid context"
        );
    }

    #[tokio::test]
    async fn invalidate_client_with_explicit_context_leaves_unrelated_resolved_context() {
        let manager = SharedClientManager::new();
        let kubeconfig_path = Some("kftray-test-unrelated-kubeconfig".to_string());

        manager.resolved_contexts.insert(
            kubeconfig_path.clone(),
            ("current-ctx".to_string(), Instant::now(), Vec::new()),
        );

        manager.invalidate_client(&ServiceClientKey::new(
            Some("some-other-ctx".to_string()),
            kubeconfig_path.clone(),
        ));

        assert!(
            manager.resolved_contexts.contains_key(&kubeconfig_path),
            "invalidating a context that is not the currently recorded resolution must not \
             touch the recorded resolution"
        );
    }

    #[tokio::test]
    async fn resolve_key_re_resolves_after_ttl_expires() {
        use std::io::Write;

        let mut kubeconfig_file = tempfile::NamedTempFile::new().expect("create temp kubeconfig");
        write!(
            kubeconfig_file,
            "apiVersion: v1\n\
             kind: Config\n\
             current-context: kftray-test-ttl-context\n\
             contexts:\n\
             - name: kftray-test-ttl-context\n  \
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

        let manager = SharedClientManager::with_ttl(Duration::from_millis(20));
        let key = ServiceClientKey::new(
            None,
            Some(kubeconfig_file.path().to_string_lossy().to_string()),
        );

        let first = manager
            .resolve_key(key.clone())
            .await
            .expect("initial resolution must succeed");
        assert_eq!(
            first.context_name.as_deref(),
            Some("kftray-test-ttl-context")
        );

        // Stamp the recorded resolution as older than the ttl and corrupt its
        // value; a fresh disk read is the only way the next resolve_key call
        // can recover the real context name.
        manager.resolved_contexts.insert(
            key.kubeconfig_path.clone(),
            (
                "stale-context".to_string(),
                Instant::now() - Duration::from_millis(21),
                Vec::new(),
            ),
        );

        let second = manager
            .resolve_key(key)
            .await
            .expect("expired cache entry must fall back to a fresh kubeconfig read");
        assert_eq!(
            second.context_name.as_deref(),
            Some("kftray-test-ttl-context"),
            "an expired resolved-context cache entry must not be reused; resolve_key must \
             re-read the kubeconfig instead of returning the stale recorded context"
        );
    }
}
