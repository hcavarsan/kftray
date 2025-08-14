use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use dashmap::DashMap;
use log::debug;

use crate::kube::models::TargetPod;

#[derive(Clone, Debug)]
pub struct CacheConfig {
    pub cache_ttl: Duration,
    pub validation_interval: Duration,
    pub max_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_ttl: Duration::from_secs(90),
            validation_interval: Duration::from_secs(10),
            max_entries: 1000,
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TargetCacheKey {
    pub selector: String,
    pub namespace: String,
    pub port: String,
}

impl TargetCacheKey {
    pub fn from_target(target: &crate::kube::models::Target) -> Self {
        let selector = match &target.selector {
            crate::kube::models::TargetSelector::ServiceName(name) => format!("service:{name}"),
            crate::kube::models::TargetSelector::PodLabel(label) => format!("label:{label}"),
        };

        let port = match &target.port {
            crate::kube::models::Port::Number(num) => num.to_string(),
            crate::kube::models::Port::Name(name) => name.clone(),
        };

        Self {
            selector,
            namespace: target.namespace.name_any(),
            port,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CachedTarget {
    pub target_pod: TargetPod,
    pub cached_at: Instant,
    pub last_validated_at: Instant,
}

impl CachedTarget {
    pub fn new(target_pod: TargetPod) -> Self {
        let now = Instant::now();
        Self {
            target_pod,
            cached_at: now,
            last_validated_at: now,
        }
    }

    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.cached_at.elapsed() > ttl
    }

    pub fn needs_validation(&self, validation_interval: Duration) -> bool {
        self.last_validated_at.elapsed() > validation_interval
    }

    pub fn mark_validated(&mut self) {
        self.last_validated_at = Instant::now();
    }
}

#[derive(Debug)]
pub struct TargetCache {
    cache: Arc<DashMap<TargetCacheKey, CachedTarget>>,
    config: CacheConfig,
}

impl TargetCache {
    pub fn new(config: CacheConfig) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            config,
        }
    }

    pub fn get(&self, key: &TargetCacheKey) -> Option<TargetPod> {
        if let Some(cached) = self.cache.get(key) {
            if !cached.is_expired(self.config.cache_ttl) {
                debug!("Cache hit for target {key:?}");
                return Some(cached.target_pod.clone());
            } else {
                debug!("Cache entry expired for target {key:?}, performing lazy cleanup");
                drop(cached);
                self.cache.remove(key);
            }
        }
        None
    }

    pub async fn get_with_validation<F, Fut>(
        &self, key: &TargetCacheKey, validator: F,
    ) -> Option<TargetPod>
    where
        F: FnOnce(TargetPod) -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        debug!("Starting cache validation for key: {key:?}");

        let cached_entry = self.cache.get(key);
        if let Some(cached) = cached_entry {
            debug!("Found cached entry for key: {key:?}");

            if cached.is_expired(self.config.cache_ttl) {
                debug!(
                    "Cache entry is expired (older than {} seconds), removing",
                    self.config.cache_ttl.as_secs()
                );
                drop(cached);
                self.cache.remove(key);
                return None;
            }

            let target_pod = cached.target_pod.clone();

            if !cached.needs_validation(self.config.validation_interval) {
                debug!(
                    "Cache entry recently validated, using cached pod: {}",
                    target_pod.pod_name
                );
                return Some(target_pod);
            }

            debug!(
                "Cache entry needs validation (last validated {} seconds ago), validating pod: {}",
                cached.last_validated_at.elapsed().as_secs(),
                target_pod.pod_name
            );

            drop(cached);

            let is_valid = validator(target_pod.clone()).await;
            debug!(
                "Validation result for pod {}: {}",
                target_pod.pod_name, is_valid
            );

            if is_valid {
                if let Some(mut cached) = self.cache.get_mut(key) {
                    if cached.target_pod.pod_name == target_pod.pod_name {
                        cached.mark_validated();
                        debug!(
                            "Updated validation timestamp for pod: {}",
                            target_pod.pod_name
                        );
                    }
                }
                return Some(target_pod);
            } else {
                debug!(
                    "Cached pod {} failed validation, removing from cache to trigger refresh",
                    target_pod.pod_name
                );
                self.cache.remove(key);
                return None;
            }
        }

        debug!("No cached entry found for key: {key:?}");
        None
    }

    pub fn put(&self, key: TargetCacheKey, target_pod: TargetPod) {
        debug!("Caching target pod for {key:?}");

        if self.cache.len() >= self.config.max_entries {
            self.evict_oldest_entries();
        }

        self.cache.insert(key, CachedTarget::new(target_pod));
    }

    pub fn get_timestamp(&self, key: &TargetCacheKey) -> Option<Instant> {
        self.cache.get(key).map(|cached| cached.cached_at)
    }

    fn evict_oldest_entries(&self) {
        let entries_to_remove = (self.cache.len() / 4).max(1);

        let mut oldest_entries: Vec<_> = self
            .cache
            .iter()
            .map(|entry| (entry.key().clone(), entry.cached_at))
            .collect();

        oldest_entries.sort_by_key(|(_, cached_at)| *cached_at);

        for (key, _) in oldest_entries.into_iter().take(entries_to_remove) {
            self.cache.remove(&key);
            debug!("Evicted old cache entry for {key:?}");
        }
    }

    pub fn invalidate(&self, key: &TargetCacheKey) {
        if self.cache.remove(key).is_some() {
            debug!("Invalidated cache entry for {key:?}");
        }
    }

    pub fn force_refresh(&self, key: &TargetCacheKey) {
        if self.cache.remove(key).is_some() {
            debug!("Forced refresh for cache entry {key:?}");
        }
    }

    pub fn clear(&self) {
        let count = self.cache.len();
        self.cache.clear();
        debug!("Cleared all {count} cache entries");
    }

    pub fn get_stats(&self) -> CacheStats {
        let total_entries = self.cache.len();
        let mut expired_entries = 0;
        let mut needs_validation = 0;

        for entry in self.cache.iter() {
            if entry.is_expired(self.config.cache_ttl) {
                expired_entries += 1;
            } else if entry.needs_validation(self.config.validation_interval) {
                needs_validation += 1;
            }
        }

        CacheStats {
            total_entries,
            expired_entries,
            valid_entries: total_entries - expired_entries,
            needs_validation,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub expired_entries: usize,
    pub valid_entries: usize,
    pub needs_validation: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kube::models::{
        NameSpace,
        Port,
        Target,
        TargetPod,
        TargetSelector,
    };

    #[test]
    fn test_cache_config_default() {
        let config = CacheConfig::default();
        assert_eq!(config.cache_ttl, Duration::from_secs(90));
        assert_eq!(config.validation_interval, Duration::from_secs(10));
        assert_eq!(config.max_entries, 1000);
    }

    #[test]
    fn test_target_cache_key_from_target() {
        let target = Target {
            selector: TargetSelector::ServiceName("test-service".to_string()),
            port: Port::Number(8080),
            namespace: NameSpace(Some("default".to_string())),
        };

        let key = TargetCacheKey::from_target(&target);
        assert_eq!(key.selector, "service:test-service");
        assert_eq!(key.namespace, "default");
        assert_eq!(key.port, "8080");

        let target2 = Target {
            selector: TargetSelector::PodLabel("app=web".to_string()),
            port: Port::Name("http".to_string()),
            namespace: NameSpace(None),
        };

        let key2 = TargetCacheKey::from_target(&target2);
        assert_eq!(key2.selector, "label:app=web");
        assert_eq!(key2.namespace, "default");
        assert_eq!(key2.port, "http");
    }

    #[tokio::test]
    async fn test_cached_target() {
        let target_pod = TargetPod {
            pod_name: "test-pod".to_string(),
            port_number: 8080,
        };

        let cached = CachedTarget::new(target_pod.clone());
        assert_eq!(cached.target_pod.pod_name, "test-pod");
        assert_eq!(cached.target_pod.port_number, 8080);
        assert!(!cached.is_expired(Duration::from_millis(100)));

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(cached.is_expired(Duration::from_millis(100)));
    }

    #[test]
    fn test_cache_invalidation() {
        let cache = TargetCache::new(CacheConfig::default());

        let key = TargetCacheKey {
            selector: "service:test".to_string(),
            namespace: "default".to_string(),
            port: "8080".to_string(),
        };

        let target_pod = TargetPod {
            pod_name: "test-pod".to_string(),
            port_number: 8080,
        };

        cache.put(key.clone(), target_pod);
        assert!(cache.get(&key).is_some());

        cache.invalidate(&key);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn test_cache_stats() {
        let cache = TargetCache::new(CacheConfig::default());
        let stats = cache.get_stats();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.valid_entries, 0);
        assert_eq!(stats.needs_validation, 0);
    }

    #[test]
    fn test_force_refresh() {
        let cache = TargetCache::new(CacheConfig::default());

        let key = TargetCacheKey {
            selector: "service:test".to_string(),
            namespace: "default".to_string(),
            port: "8080".to_string(),
        };

        let target_pod = TargetPod {
            pod_name: "test-pod".to_string(),
            port_number: 8080,
        };

        cache.put(key.clone(), target_pod);
        assert!(cache.get(&key).is_some());

        cache.force_refresh(&key);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn test_clear_cache() {
        let cache = TargetCache::new(CacheConfig::default());

        for i in 0..5 {
            let key = TargetCacheKey {
                selector: format!("service:test-{i}"),
                namespace: "default".to_string(),
                port: "8080".to_string(),
            };

            let target_pod = TargetPod {
                pod_name: format!("test-pod-{i}"),
                port_number: 8080,
            };

            cache.put(key, target_pod);
        }

        assert_eq!(cache.get_stats().total_entries, 5);

        cache.clear();
        assert_eq!(cache.get_stats().total_entries, 0);
    }
}
