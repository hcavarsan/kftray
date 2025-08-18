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
}

impl Clone for HttpLogStateWatcher {
    fn clone(&self) -> Self {
        Self {
            current_state: self.current_state.clone(),
            event_sender: self.event_sender.clone(),
            _processor_task: tokio::spawn(async {}),
        }
    }
}

impl HttpLogStateWatcher {
    pub fn new() -> Self {
        Self::new_with_external_sync(None)
    }

    pub fn new_with_external_sync(
        external_state: Option<Arc<kftray_http_logs::HttpLogState>>,
    ) -> Self {
        info!("Starting HTTP log state watcher with event-driven pattern");

        let current_state = Arc::new(RwLock::new(HashMap::new()));
        let (event_sender, mut event_receiver) = broadcast::channel(256);

        let current_state_clone = current_state.clone();
        let event_sender_clone = event_sender.clone();
        let processor_task = tokio::spawn(async move {
            let mut sync_interval = tokio::time::interval(std::time::Duration::from_millis(500));
            let known_configs: HashMap<i64, bool> = HashMap::new();

            loop {
                tokio::select! {
                    event = event_receiver.recv() => {
                        if let Ok(event) = event {
                            Self::update_current_state(&current_state_clone, &event).await;
                        }
                    }
                    _ = sync_interval.tick() => {
                        if let Some(ref ext_state) = external_state {
                            let current_state_guard = current_state_clone.read().await;
                            let configs_to_check: Vec<i64> = current_state_guard.keys().copied()
                                .chain(known_configs.keys().copied())
                                .collect();
                            drop(current_state_guard);

                            for config_id in configs_to_check {
                                if let Ok(external_enabled) = ext_state.get_http_logs(config_id).await {
                                    let current_enabled = known_configs.get(&config_id).copied().unwrap_or(false);

                                    if current_enabled != external_enabled {


                                        let event = HttpLogStateEvent::new(config_id, external_enabled);
                                        let _ = event_sender_clone.send(event);
                                    }
                                }
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

    pub async fn sync_from_external_state(
        &self, external_state: &kftray_http_logs::HttpLogState, config_id: i64,
    ) -> Result<()> {
        let current_enabled = self.get_http_logs(config_id).await;

        let external_result = external_state.get_http_logs(config_id).await;
        let external_enabled = match external_result {
            Ok(enabled) => enabled,
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
