use anyhow::Result;
use chrono::{
    DateTime,
    Utc,
};

use crate::state::LogStateManager;

#[derive(Debug, Clone)]
pub struct TraceInfo {
    pub trace_id: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct HttpLogState {
    state_manager: LogStateManager,
}

impl HttpLogState {
    pub fn new() -> Self {
        Self {
            state_manager: LogStateManager::new(),
        }
    }

    pub async fn new_with_database_load() -> Result<Self> {
        let state = Self::new();
        state.state_manager.load_from_database().await?;
        Ok(state)
    }

    pub async fn set_http_logs(&self, config_id: i64, enable: bool) -> Result<()> {
        self.state_manager.set_http_logs(config_id, enable).await
    }

    pub async fn get_http_logs(&self, config_id: i64) -> Result<bool> {
        self.state_manager.get_http_logs(config_id).await
    }

    pub async fn load_from_database(&self) -> Result<()> {
        self.state_manager.load_from_database().await
    }
}

impl Default for HttpLogState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn calculate_time_diff(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    (end - start).num_milliseconds()
}
