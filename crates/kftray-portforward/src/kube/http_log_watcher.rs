use std::collections::HashMap;
use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use anyhow::{
    anyhow,
    Result,
};
use k8s_openapi::chrono;
use tokio::sync::{
    broadcast,
    RwLock,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{
    error,
    info,
};
#[derive(Debug, Clone, PartialEq)]
pub struct HttpLogStateEvent {
    pub config_id: i64,
    pub enabled: bool,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<String>,
}

impl HttpLogStateEvent {
    pub fn new(config_id: i64, enabled: bool) -> Self {
        Self {
            config_id,
            enabled,
            timestamp: chrono::Utc::now(),
            metadata: None,
        }
    }

    pub fn with_metadata(config_id: i64, enabled: bool, metadata: String) -> Self {
        Self {
            config_id,
            enabled,
            timestamp: chrono::Utc::now(),
            metadata: Some(metadata),
        }
    }
}

pub struct HttpLogStateWatcher {
    current_state: Arc<RwLock<HashMap<i64, bool>>>,
    event_sender: broadcast::Sender<HttpLogStateEvent>,
    _processor_task: JoinHandle<()>,
    cancellation_token: CancellationToken,
}

impl Clone for HttpLogStateWatcher {
    fn clone(&self) -> Self {
        Self {
            current_state: self.current_state.clone(),
            event_sender: self.event_sender.clone(),
            _processor_task: tokio::spawn(async {}), // Dummy task for clone
            cancellation_token: self.cancellation_token.clone(),
        }
    }
}

impl Drop for HttpLogStateWatcher {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
        self._processor_task.abort();
    }
}

impl HttpLogStateWatcher {
    pub fn new() -> Self {
        Self::new_with_external_sync(None)
    }

    pub fn new_with_external_sync(_external_state: Option<()>) -> Self {
        info!("Starting HTTP log state watcher with event-driven pattern");

        let current_state = Arc::new(RwLock::new(HashMap::new()));
        let (event_sender, mut event_receiver) = broadcast::channel(256);
        let cancellation_token = CancellationToken::new();

        let current_state_clone = current_state.clone();
        let token_clone = cancellation_token.clone();
        let processor_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token_clone.cancelled() => {
                        info!("HTTP log watcher processor task cancelled");
                        break;
                    }
                    event = event_receiver.recv() => {
                        match event {
                            Ok(event) => {
                                Self::update_current_state(&current_state_clone, &event).await;
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                info!("HTTP log watcher event channel closed");
                                break;
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                error!("HTTP log watcher event channel lagged, continuing");
                                continue;
                            }
                        }
                    }
                }
            }
        });

        Self {
            current_state,
            event_sender,
            _processor_task: processor_task,
            cancellation_token,
        }
    }

    async fn update_current_state(
        current_state: &Arc<RwLock<HashMap<i64, bool>>>, event: &HttpLogStateEvent,
    ) {
        let mut state = current_state.write().await;
        state.insert(event.config_id, event.enabled);
    }

    pub async fn set_http_logs(&self, config_id: i64, enabled: bool) -> Result<()> {
        let event = HttpLogStateEvent::new(config_id, enabled);
        self.event_sender
            .send(event)
            .map_err(|e| anyhow!("Failed to publish HTTP log state change: {}", e))?;
        Ok(())
    }

    pub async fn set_http_logs_with_metadata(
        &self, config_id: i64, enabled: bool, metadata: String,
    ) -> Result<()> {
        let event = HttpLogStateEvent::with_metadata(config_id, enabled, metadata);
        self.event_sender
            .send(event)
            .map_err(|e| anyhow!("Failed to publish HTTP log state change: {}", e))?;
        Ok(())
    }

    pub async fn get_http_logs(&self, config_id: i64) -> bool {
        let state = self.current_state.read().await;
        state.get(&config_id).copied().unwrap_or(false)
    }

    pub async fn wait_for_state_change(
        &self, config_id: i64, expected_state: bool, timeout: Duration,
    ) -> Option<bool> {
        let start = Instant::now();

        while start.elapsed() < timeout {
            if self.get_http_logs(config_id).await == expected_state {
                return Some(expected_state);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        None
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HttpLogStateEvent> {
        self.event_sender.subscribe()
    }

    pub async fn get_all_states(&self) -> HashMap<i64, bool> {
        let state = self.current_state.read().await;
        state.clone()
    }

    pub async fn clear_config(&self, config_id: i64) -> Result<()> {
        self.set_http_logs(config_id, false).await
    }

    pub async fn get_active_configs(&self) -> Vec<i64> {
        let state = self.current_state.read().await;
        state
            .iter()
            .filter_map(|(config_id, enabled)| if *enabled { Some(*config_id) } else { None })
            .collect()
    }

    pub fn create_filtered_subscriber(
        &self, _config_id: i64,
    ) -> broadcast::Receiver<HttpLogStateEvent> {
        self.subscribe()
    }

    pub fn shutdown(&self) {
        self.cancellation_token.cancel();
    }

    pub async fn sync_from_external_state(&self, config_id: i64) -> Result<()> {
        let current_enabled = self.get_http_logs(config_id).await;

        let external_enabled =
            match kftray_commons::utils::http_logs_config::get_http_logs_config(config_id).await {
                Ok(config) => config.enabled,
                Err(e) => {
                    error!("Failed to get HTTP logs for config {}: {:?}", config_id, e);
                    false
                }
            };

        if current_enabled != external_enabled {
            self.set_http_logs(config_id, external_enabled).await?;
        }

        Ok(())
    }
}

impl Default for HttpLogStateWatcher {
    fn default() -> Self {
        Self::new()
    }
}
