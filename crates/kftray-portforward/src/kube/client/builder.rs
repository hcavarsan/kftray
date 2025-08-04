use std::collections::HashMap;
use std::env;
use std::sync::{
    LazyLock,
    Mutex,
};
use std::time::{
    Duration,
    Instant,
};

use anyhow::Result;
use kube::config::Kubeconfig;
use kube::Client;
use log::{
    debug,
    info,
    warn,
};

use super::config::{
    create_config_with_context,
    get_kubeconfig_paths_from_option,
    merge_kubeconfigs,
};
use super::connection::create_client_with_config;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ClientCacheKey {
    kubeconfig_paths: Vec<String>,
    context_name: String,
}

#[derive(Clone)]
struct CachedClient {
    client: Client,
    created_at: Instant,
}

struct ClientCache {
    cache: HashMap<ClientCacheKey, CachedClient>,
    ttl: Duration,
}

impl ClientCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
            ttl: Duration::from_secs(300), // 5 minutes cache
        }
    }

    fn get(&mut self, key: &ClientCacheKey) -> Option<Client> {
        if let Some(cached) = self.cache.get(key) {
            if cached.created_at.elapsed() < self.ttl {
                debug!("Client cache hit for context: {}", key.context_name);
                return Some(cached.client.clone());
            } else {
                debug!("Client cache expired for context: {}", key.context_name);
                self.cache.remove(key);
            }
        }
        None
    }

    fn insert(&mut self, key: ClientCacheKey, client: Client) {
        debug!("Caching client for context: {}", key.context_name);
        self.cache.insert(
            key,
            CachedClient {
                client,
                created_at: Instant::now(),
            },
        );
    }

    fn cleanup_expired(&mut self) {
        let expired_keys: Vec<_> = self
            .cache
            .iter()
            .filter_map(|(key, cached)| {
                if cached.created_at.elapsed() >= self.ttl {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect();

        for key in expired_keys {
            debug!(
                "Removing expired client cache for context: {}",
                key.context_name
            );
            self.cache.remove(&key);
        }
    }
}

static CLIENT_CACHE: LazyLock<Mutex<ClientCache>> =
    LazyLock::new(|| Mutex::new(ClientCache::new()));

pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>, context_name: Option<&str>,
) -> Result<(Option<Client>, Option<Kubeconfig>, Vec<String>)> {
    {
        let _guard = ENV_LOCK.lock().unwrap();
        env::remove_var("PYTHONHOME");
        env::remove_var("PYTHONPATH");
    }

    let kubeconfig_paths = get_kubeconfig_paths_from_option(kubeconfig)?;
    let (merged_kubeconfig, all_contexts, mut errors) = merge_kubeconfigs(&kubeconfig_paths)?;

    if let Some(context_name) = context_name {
        let cache_key = ClientCacheKey {
            kubeconfig_paths: kubeconfig_paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            context_name: context_name.to_string(),
        };

        if let Ok(mut cache) = CLIENT_CACHE.lock() {
            cache.cleanup_expired();
            if let Some(cached_client) = cache.get(&cache_key) {
                info!("Using cached client for context: {context_name}");
                return Ok((Some(cached_client), Some(merged_kubeconfig), all_contexts));
            }
        }

        match create_config_with_context(&merged_kubeconfig, context_name).await {
            Ok(config) => {
                if let Some(client) = create_client_with_config(&config).await {
                    if let Ok(mut cache) = CLIENT_CACHE.lock() {
                        cache.insert(cache_key, client.clone());
                    }
                    info!("Created and cached new client for context: {context_name}");
                    return Ok((Some(client), Some(merged_kubeconfig), all_contexts));
                } else {
                    errors.push(format!(
                        "Failed to create client for context '{context_name}': All connection strategies failed"
                    ));
                }
            }
            Err(e) => {
                errors.push(format!(
                    "Failed to create configuration for context '{context_name}': {e}. Check if the context exists and is properly configured"
                ));
            }
        }
    } else {
        info!("No specific context provided, returning all available contexts.");
        return Ok((None, None, all_contexts));
    }

    Err(anyhow::anyhow!(
        "Unable to create Kubernetes client. Tried {} kubeconfig path(s). Errors encountered:\n{}",
        kubeconfig_paths.len(),
        errors
            .iter()
            .map(|e| format!("  • {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

pub fn clear_client_cache() {
    if let Ok(mut cache) = CLIENT_CACHE.lock() {
        let count = cache.cache.len();
        cache.cache.clear();
        info!("Cleared {count} cached clients");
    } else {
        warn!("Failed to acquire lock for client cache clearing");
    }
}

pub fn get_client_cache_stats() -> (usize, usize) {
    if let Ok(mut cache) = CLIENT_CACHE.lock() {
        cache.cleanup_expired();
        let total = cache.cache.len();
        let expired = cache
            .cache
            .iter()
            .filter(|(_, cached)| cached.created_at.elapsed() >= cache.ttl)
            .count();
        (total, expired)
    } else {
        (0, 0)
    }
}
