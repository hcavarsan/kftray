use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use dashmap::DashMap;
use kube::Client;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

use crate::kube::client::create_client_with_specific_context;
use crate::kube::shared_client::ServiceClientKey;
use crate::port_forward::PortForwardProcess;

pub static PORT_FORWARD_REGISTRY: Lazy<PortForwardRegistry> = Lazy::new(PortForwardRegistry::new);

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PortForwardKey {
    pub config_id: i64,
    pub slot: PortForwardSlot,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PortForwardSlot {
    Named(String),
    Expose,
}

impl PortForwardKey {
    pub fn named(config_id: i64, service_name: impl Into<String>) -> Self {
        Self {
            config_id,
            slot: PortForwardSlot::Named(service_name.into()),
        }
    }

    pub fn expose(config_id: i64) -> Self {
        Self {
            config_id,
            slot: PortForwardSlot::Expose,
        }
    }
}

impl std::fmt::Display for PortForwardKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.slot {
            PortForwardSlot::Named(name) => {
                write!(f, "config:{}:service:{}", self.config_id, name)
            }
            PortForwardSlot::Expose => write!(f, "config:{}:expose", self.config_id),
        }
    }
}

pub struct ProcessEntry {
    pub process: PortForwardProcess,
    pub client_key: ServiceClientKey,
    pub inserted_at: Instant,
}

struct CachedClient {
    client: Arc<Client>,
    created_at: Instant,
    ref_count: usize,
}

pub struct PortForwardRegistry {
    processes: DashMap<PortForwardKey, ProcessEntry>,
    clients: DashMap<ServiceClientKey, CachedClient>,
    creation_locks: DashMap<ServiceClientKey, Arc<Mutex<()>>>,
    client_ttl: Duration,
}

impl PortForwardRegistry {
    pub fn new() -> Self {
        Self {
            processes: DashMap::new(),
            clients: DashMap::new(),
            creation_locks: DashMap::new(),
            client_ttl: Duration::from_secs(3600),
        }
    }

    /// Get or create a client for the given key, incrementing ref_count.
    pub async fn acquire_client(&self, key: ServiceClientKey) -> anyhow::Result<Arc<Client>> {
        // Fast path: cached and not expired
        if let Some(mut cached) = self.clients.get_mut(&key) {
            if cached.created_at.elapsed() <= self.client_ttl {
                cached.ref_count += 1;
                return Ok(cached.client.clone());
            }
            drop(cached);
            self.clients.remove(&key);
        }

        // Serialize creation per key
        let lock = self
            .creation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();

        let _guard = lock.lock().await;

        // Double-check after acquiring lock
        if let Some(mut cached) = self.clients.get_mut(&key) {
            if cached.created_at.elapsed() <= self.client_ttl {
                cached.ref_count += 1;
                return Ok(cached.client.clone());
            }
            drop(cached);
            self.clients.remove(&key);
        }

        let result = async {
            let (client_opt, _, _) = create_client_with_specific_context(
                key.kubeconfig_path.clone(),
                key.context_name.as_deref(),
            )
            .await?;

            client_opt.ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to create client for context: {:?}",
                    key.context_name
                )
            })
        }
        .await;

        match result {
            Ok(client) => {
                let client_arc = Arc::new(client);
                self.clients.insert(
                    key,
                    CachedClient {
                        client: client_arc.clone(),
                        created_at: Instant::now(),
                        ref_count: 1,
                    },
                );
                Ok(client_arc)
            }
            Err(e) => {
                self.creation_locks.remove(&key);
                Err(e)
            }
        }
    }

    /// Insert a new process entry.
    pub fn insert_process(
        &self, key: PortForwardKey, process: PortForwardProcess, client_key: ServiceClientKey,
    ) {
        self.processes.insert(
            key,
            ProcessEntry {
                process,
                client_key,
                inserted_at: Instant::now(),
            },
        );
    }

    /// Mutate a process entry in-place.
    pub fn with_process_mut<F, R>(&self, key: &PortForwardKey, f: F) -> Option<R>
    where
        F: FnOnce(&mut ProcessEntry) -> R,
    {
        self.processes.get_mut(key).map(|mut entry| f(&mut entry))
    }

    /// Remove a single process, decrement client ref_count.
    pub fn remove_process(&self, key: &PortForwardKey) -> Option<ProcessEntry> {
        let entry = self.processes.remove(key).map(|(_, v)| v);
        if let Some(ref entry) = entry {
            self.decrement_client_ref(&entry.client_key);
        }
        entry
    }

    /// Remove all processes for a given config_id.
    pub fn remove_processes_for_config(&self, config_id: i64) -> Vec<ProcessEntry> {
        let keys: Vec<PortForwardKey> = self
            .processes
            .iter()
            .filter(|entry| entry.key().config_id == config_id)
            .map(|entry| entry.key().clone())
            .collect();

        let mut entries = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some((_, entry)) = self.processes.remove(&key) {
                self.decrement_client_ref(&entry.client_key);
                entries.push(entry);
            }
        }
        entries
    }

    /// Read-only iteration for a given config_id.
    pub fn processes_for_config(&self, config_id: i64) -> Vec<(PortForwardKey, ServiceClientKey)> {
        self.processes
            .iter()
            .filter(|entry| entry.key().config_id == config_id)
            .map(|entry| (entry.key().clone(), entry.value().client_key.clone()))
            .collect()
    }

    /// Query helper: get the active pod name for a config.
    pub async fn get_active_pod(&self, config_id: i64) -> Option<String> {
        // Collect forwarders first to avoid holding DashMap refs across await
        let forwarders: Vec<_> = self
            .processes
            .iter()
            .filter(|entry| entry.key().config_id == config_id)
            .filter_map(|entry| entry.value().process.direct_forwarder.clone())
            .collect();

        for forwarder in forwarders {
            if let Some(pod_name) = forwarder.get_current_active_pod().await {
                return Some(pod_name);
            }
        }
        None
    }

    /// Drain all processes.
    pub fn remove_all(&self) -> Vec<(PortForwardKey, ProcessEntry)> {
        let keys: Vec<PortForwardKey> = self
            .processes
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        let mut entries = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some((k, entry)) = self.processes.remove(&key) {
                self.decrement_client_ref(&entry.client_key);
                entries.push((k, entry));
            }
        }
        entries
    }

    /// Remove expired clients (those with ref_count == 0 past TTL).
    pub fn cleanup_expired_clients(&self) {
        self.clients.retain(|_, cached| {
            // Keep if still referenced or not yet expired
            cached.ref_count > 0 || cached.created_at.elapsed() <= self.client_ttl
        });
        self.creation_locks
            .retain(|key, _| self.clients.contains_key(key));
    }

    /// Force-remove a client regardless of ref_count.
    pub fn invalidate_client(&self, key: &ServiceClientKey) {
        self.clients.remove(key);
    }

    /// Check if any process exists for a config_id.
    pub fn has_process_for_config(&self, config_id: i64) -> bool {
        self.processes
            .iter()
            .any(|entry| entry.key().config_id == config_id)
    }

    /// Get all process keys (for iteration during stop_all).
    pub fn all_keys(&self) -> Vec<PortForwardKey> {
        self.processes
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get a process key matching a config_id (first match).
    pub fn find_key_for_config(&self, config_id: i64) -> Option<PortForwardKey> {
        self.processes
            .iter()
            .find(|entry| entry.key().config_id == config_id)
            .map(|entry| entry.key().clone())
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.processes.is_empty()
    }

    /// Number of tracked processes.
    pub fn len(&self) -> usize {
        self.processes.len()
    }

    /// Clear all processes without decrementing refs (for tests).
    #[cfg(test)]
    pub fn clear(&self) {
        self.processes.clear();
        self.clients.clear();
        self.creation_locks.clear();
    }

    fn decrement_client_ref(&self, client_key: &ServiceClientKey) {
        if let Some(mut cached) = self.clients.get_mut(client_key) {
            cached.ref_count = cached.ref_count.saturating_sub(1);
            if cached.ref_count == 0 {
                let elapsed = cached.created_at.elapsed();
                drop(cached);
                // Remove immediately if also expired
                if elapsed > self.client_ttl {
                    self.clients.remove(client_key);
                    self.creation_locks.remove(client_key);
                }
            }
        }
    }
}

impl Default for PortForwardRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_forward_key_named() {
        let key = PortForwardKey::named(1, "my-service");
        assert_eq!(key.config_id, 1);
        assert_eq!(key.slot, PortForwardSlot::Named("my-service".to_string()));
        assert_eq!(key.to_string(), "config:1:service:my-service");
    }

    #[test]
    fn test_port_forward_key_expose() {
        let key = PortForwardKey::expose(42);
        assert_eq!(key.config_id, 42);
        assert_eq!(key.slot, PortForwardSlot::Expose);
        assert_eq!(key.to_string(), "config:42:expose");
    }

    #[test]
    fn test_port_forward_key_equality() {
        let k1 = PortForwardKey::named(1, "svc");
        let k2 = PortForwardKey::named(1, "svc");
        let k3 = PortForwardKey::named(1, "other");
        let k4 = PortForwardKey::expose(1);

        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
        assert_ne!(k1, k4);
    }

    #[test]
    fn test_registry_new() {
        let registry = PortForwardRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }
}
