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

use tokio::sync::Mutex;
use tokio::time::interval;

#[derive(Clone, Debug)]
pub struct HttpLogState {
    pub enable_http_logs: Arc<Mutex<HashMap<i64, (AtomicBool, SystemTime)>>>,
}

impl HttpLogState {
    pub fn new() -> Self {
        let state = HttpLogState {
            enable_http_logs: Arc::new(Mutex::new(HashMap::new())),
        };

        // Only spawn cleanup task if we're in a Tokio runtime
        if tokio::runtime::Handle::try_current().is_ok() {
            let state_clone = state.clone();
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_secs(3600));
                loop {
                    interval.tick().await;
                    if let Err(e) = state_clone.cleanup_stale_configs().await {
                        tracing::error!("Failed to cleanup stale configs: {:?}", e);
                    }
                }
            });
        }

        state
    }

    pub async fn set_http_logs(&self, config_id: i64, enable: bool) -> anyhow::Result<()> {
        let mut logs = self.enable_http_logs.lock().await;
        logs.insert(config_id, (AtomicBool::new(enable), SystemTime::now()));
        Ok(())
    }

    pub async fn get_http_logs(&self, config_id: i64) -> anyhow::Result<bool> {
        let logs = self.enable_http_logs.lock().await;
        Ok(logs
            .get(&config_id)
            .map(|(state, _)| state.load(Ordering::SeqCst))
            .unwrap_or(false))
    }

    async fn cleanup_stale_configs(&self) -> anyhow::Result<()> {
        let mut logs = self.enable_http_logs.lock().await;
        let now = SystemTime::now();
        logs.retain(|_, (_, last_updated)| {
            now.duration_since(*last_updated)
                .map(|duration| duration < Duration::from_secs(24 * 60 * 60))
                .unwrap_or(true)
        });
        Ok(())
    }
}

impl Default for HttpLogState {
    fn default() -> Self {
        Self::new()
    }
}
