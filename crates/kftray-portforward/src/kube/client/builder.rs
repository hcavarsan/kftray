use std::env;
use std::sync::LazyLock;
use std::time::{
    Duration,
    Instant,
};

use anyhow::Result;
use dashmap::DashMap;
use kube::config::Kubeconfig;
use kube::Client;
use log::{
    debug,
    info,
};

use super::config::{
    create_config_with_context,
    get_kubeconfig_paths_from_option,
    merge_kubeconfigs,
};
use super::connection::create_client_with_config;

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
    cache: DashMap<ClientCacheKey, CachedClient>,
    ttl: Duration,
}

impl ClientCache {
    fn new() -> Self {
        Self {
            cache: DashMap::new(),
            ttl: Duration::from_secs(300),
        }
    }

    fn get(&self, key: &ClientCacheKey) -> Option<Client> {
        if let Some(cached) = self.cache.get(key) {
            if cached.created_at.elapsed() < self.ttl {
                return Some(cached.client.clone());
            } else {
                drop(cached);
                self.cache.remove(key);
            }
        }
        None
    }

    fn insert(&self, key: ClientCacheKey, client: Client) {
        self.cache.insert(
            key,
            CachedClient {
                client,
                created_at: Instant::now(),
            },
        );
    }

    fn cleanup_expired(&self) {
        let before_count = self.cache.len();
        self.cache
            .retain(|_key, cached| cached.created_at.elapsed() < self.ttl);
        let after_count = self.cache.len();
        if before_count > after_count {
            debug!(
                "Cleaned up {} expired client cache entries",
                before_count - after_count
            );
        }
    }
}

static CLIENT_CACHE: LazyLock<ClientCache> = LazyLock::new(ClientCache::new);

pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>, context_name: Option<&str>,
) -> Result<(Option<Client>, Option<Kubeconfig>, Vec<String>)> {
    {
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

        CLIENT_CACHE.cleanup_expired();
        if let Some(cached_client) = CLIENT_CACHE.get(&cache_key) {
            return Ok((Some(cached_client), Some(merged_kubeconfig), all_contexts));
        }

        match create_config_with_context(&merged_kubeconfig, context_name).await {
            Ok(config) => {
                if let Some(client) = create_client_with_config(&config).await {
                    CLIENT_CACHE.insert(cache_key, client.clone());
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
    let _count = CLIENT_CACHE.cache.len();
    CLIENT_CACHE.cache.clear();
}

pub fn get_client_cache_stats() -> (usize, usize) {
    CLIENT_CACHE.cleanup_expired();
    let total = CLIENT_CACHE.cache.len();
    let expired = CLIENT_CACHE
        .cache
        .iter()
        .filter(|entry| entry.value().created_at.elapsed() >= CLIENT_CACHE.ttl)
        .count();
    (total, expired)
}
