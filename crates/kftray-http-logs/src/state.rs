use std::collections::HashMap;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::sync::Arc;
use std::time::{
    Duration,
    SystemTime,
};

use anyhow::{
    Context,
    Result,
};
use kftray_commons::models::http_logs_config_model::HttpLogsConfig;
use kftray_commons::utils::http_logs_config::{
    get_http_logs_config,
    update_http_logs_config,
};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{
    debug,
    error,
    info,
    trace,
};

pub const DEFAULT_CLEANUP_INTERVAL_SECS: u64 = 3600;

pub const DEFAULT_CONFIG_RETENTION_SECS: u64 = 24 * 60 * 60;

pub trait LogState: Send + Sync {
    fn is_enabled(&self, config_id: i64) -> Result<bool>;

    fn set_enabled(&self, config_id: i64, enabled: bool) -> Result<()>;
}

#[derive(Debug)]
struct ConfigState {
    enabled: AtomicBool,
    last_updated: SystemTime,
    metadata: Option<String>,
}

impl Clone for ConfigState {
    fn clone(&self) -> Self {
        Self {
            enabled: AtomicBool::new(self.enabled.load(Ordering::SeqCst)),
            last_updated: self.last_updated,
            metadata: self.metadata.clone(),
        }
    }
}

impl ConfigState {
    fn new(enabled: bool, metadata: Option<String>) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            last_updated: SystemTime::now(),
            metadata,
        }
    }

    #[allow(dead_code)]
    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    fn touch(&mut self) {
        self.last_updated = SystemTime::now();
    }

    fn age(&self) -> Result<Duration> {
        SystemTime::now()
            .duration_since(self.last_updated)
            .context("Failed to calculate config age")
    }
}

#[derive(Debug, Clone)]
pub struct LogStateManager {
    state: Arc<Mutex<HashMap<i64, ConfigState>>>,

    #[allow(dead_code)]
    cleanup_task: Arc<Mutex<Option<JoinHandle<()>>>>,

    retention_period: Duration,
}

#[derive(Debug, Clone)]
pub struct LogStateConfig {
    cleanup_interval: Duration,
    retention_period: Duration,
}

impl Default for LogStateConfig {
    fn default() -> Self {
        Self {
            cleanup_interval: Duration::from_secs(DEFAULT_CLEANUP_INTERVAL_SECS),
            retention_period: Duration::from_secs(DEFAULT_CONFIG_RETENTION_SECS),
        }
    }
}

impl LogStateManager {
    pub fn new() -> Self {
        Self::with_config(LogStateConfig::default())
    }

    pub fn with_config(config: LogStateConfig) -> Self {
        let state = Arc::new(Mutex::new(HashMap::new()));
        let state_clone = state.clone();
        let retention_period = config.retention_period;

        let cleanup_task = if tokio::runtime::Handle::try_current().is_ok() {
            let task = tokio::spawn(async move {
                let mut interval = interval(config.cleanup_interval);
                loop {
                    interval.tick().await;
                    trace!("Running scheduled cleanup of HTTP log state");

                    if let Err(e) =
                        Self::cleanup_stale_configs(&state_clone, retention_period).await
                    {
                        error!("Failed to cleanup stale log configs: {:?}", e);
                    }
                }
            });

            Some(task)
        } else {
            None
        };

        Self {
            state,
            cleanup_task: Arc::new(Mutex::new(cleanup_task)),
            retention_period,
        }
    }

    pub async fn set_http_logs(&self, config_id: i64, enable: bool) -> Result<()> {
        debug!("Setting HTTP logs for config {}: {}", config_id, enable);

        let mut http_logs_config = match get_http_logs_config(config_id).await {
            Ok(config) => config,
            Err(_) => HttpLogsConfig::new(config_id),
        };

        http_logs_config.enabled = enable;

        if let Err(e) = update_http_logs_config(&http_logs_config).await {
            error!("Failed to persist HTTP logs config to database: {}", e);
            return Err(anyhow::Error::msg(format!(
                "Failed to persist HTTP logs config: {}",
                e
            )));
        }

        let mut state = self.state.lock().await;
        if let Some(config_state) = state.get_mut(&config_id) {
            config_state.set_enabled(enable);
            config_state.touch();
        } else {
            state.insert(config_id, ConfigState::new(enable, None));
        }

        Ok(())
    }

    pub async fn get_http_logs(&self, config_id: i64) -> Result<bool> {
        let is_enabled = match get_http_logs_config(config_id).await {
            Ok(config) => {
                trace!(
                    "HTTP logs for config {} from database: {}",
                    config_id,
                    config.enabled
                );
                config.enabled
            }
            Err(_) => {
                trace!(
                    "HTTP logs for config {} not found in database, defaulting to false",
                    config_id
                );
                false
            }
        };

        let mut state = self.state.lock().await;
        if let Some(config_state) = state.get_mut(&config_id) {
            config_state.touch();
            config_state.set_enabled(is_enabled);
        } else {
            state.insert(config_id, ConfigState::new(is_enabled, None));
        }

        Ok(is_enabled)
    }

    pub async fn set_config_metadata(&self, config_id: i64, metadata: String) -> Result<()> {
        let mut state = self.state.lock().await;

        if let Some(config_state) = state.get_mut(&config_id) {
            config_state.metadata = Some(metadata);
            config_state.touch();
        } else {
            state.insert(config_id, ConfigState::new(false, Some(metadata)));
        }

        Ok(())
    }

    pub async fn config_count(&self) -> usize {
        let state = self.state.lock().await;
        state.len()
    }

    pub async fn run_cleanup(&self) -> Result<usize> {
        Self::cleanup_stale_configs(&self.state, self.retention_period).await
    }

    pub async fn load_from_database(&self) -> Result<()> {
        use kftray_commons::utils::http_logs_config::read_all_http_logs_configs;

        debug!("Loading HTTP logs configurations from database");

        match read_all_http_logs_configs().await {
            Ok(configs) => {
                let mut state = self.state.lock().await;
                for config in configs {
                    if let Some(existing_state) = state.get_mut(&config.config_id) {
                        existing_state.set_enabled(config.enabled);
                        existing_state.touch();
                    } else {
                        state.insert(config.config_id, ConfigState::new(config.enabled, None));
                    }
                }
                info!(
                    "Loaded {} HTTP logs configurations from database",
                    state.len()
                );
            }
            Err(e) => {
                error!(
                    "Failed to load HTTP logs configurations from database: {}",
                    e
                );
                return Err(anyhow::Error::msg(format!(
                    "Failed to load HTTP logs configs: {}",
                    e
                )));
            }
        }

        Ok(())
    }

    async fn cleanup_stale_configs(
        state: &Arc<Mutex<HashMap<i64, ConfigState>>>, retention_period: Duration,
    ) -> Result<usize> {
        let mut state_guard = state.lock().await;
        debug!("Cleaning up stale HTTP log configurations");

        let before_count = state_guard.len();

        state_guard.retain(|config_id, config_state| match config_state.age() {
            Ok(age) if age > retention_period => {
                trace!(
                    "Removing stale config {}: last updated {:?} ago",
                    config_id,
                    age
                );
                false
            }
            Ok(_) => true,
            Err(e) => {
                error!("Error checking config {} age: {:?}", config_id, e);
                true
            }
        });

        let removed = before_count - state_guard.len();
        if removed > 0 {
            info!("Removed {} stale HTTP log configurations", removed);
        }

        Ok(removed)
    }

    pub async fn shutdown(&self) -> Result<()> {
        debug!("Shutting down LogStateManager");

        let mut task_guard = self.cleanup_task.lock().await;
        if let Some(task) = task_guard.take() {
            task.abort();
            debug!("Aborted stale config cleanup task");
        }

        Ok(())
    }
}

impl Default for LogStateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LogStateManager {
    fn drop(&mut self) {
        if let Ok(mut task_guard) = self.cleanup_task.try_lock() {
            if let Some(task) = task_guard.take() {
                task.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_set_and_get_http_logs() {
        let manager = LogStateManager::new();

        assert!(!manager.get_http_logs(1).await.unwrap());

        manager.set_http_logs(1, true).await.unwrap();
        assert!(manager.get_http_logs(1).await.unwrap());

        manager.set_http_logs(1, false).await.unwrap();
        assert!(!manager.get_http_logs(1).await.unwrap());
    }

    #[tokio::test]
    async fn test_cleanup_stale_configs() {
        let config = LogStateConfig {
            cleanup_interval: Duration::from_millis(1000),
            retention_period: Duration::from_millis(100),
        };

        let manager = LogStateManager::with_config(config);

        manager.set_http_logs(1, true).await.unwrap();
        manager.set_http_logs(2, false).await.unwrap();
        assert_eq!(manager.config_count().await, 2);

        tokio::time::sleep(Duration::from_millis(150)).await;

        let removed = manager.run_cleanup().await.unwrap();
        assert_eq!(removed, 2, "Expected both configs to be removed as stale");
        assert_eq!(
            manager.config_count().await,
            0,
            "Expected no configs to remain"
        );
    }

    #[tokio::test]
    async fn test_cleanup_basic() {
        let config = LogStateConfig {
            cleanup_interval: Duration::from_millis(1000),
            retention_period: Duration::from_millis(500),
        };

        let manager = LogStateManager::with_config(config);

        manager.set_http_logs(1, true).await.unwrap();
        manager.set_http_logs(2, false).await.unwrap();

        assert_eq!(manager.config_count().await, 2);

        assert!(manager.get_http_logs(1).await.unwrap());
        assert!(!manager.get_http_logs(2).await.unwrap());

        let removed = manager.run_cleanup().await.unwrap();
        assert_eq!(removed, 0, "Expected no configs to be removed yet");
        assert_eq!(
            manager.config_count().await,
            2,
            "Expected both configs to remain"
        );
    }

    #[tokio::test]
    async fn test_metadata() {
        let manager = LogStateManager::new();

        manager
            .set_config_metadata(1, "Test Config".to_string())
            .await
            .unwrap();

        assert!(!manager.get_http_logs(1).await.unwrap());

        manager.set_http_logs(1, true).await.unwrap();

        assert!(manager.get_http_logs(1).await.unwrap());
    }
}
