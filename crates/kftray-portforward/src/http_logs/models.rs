use std::collections::HashMap;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::sync::Arc;

use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct HttpLogState {
    pub enable_http_logs: Arc<Mutex<HashMap<i64, AtomicBool>>>,
}

impl HttpLogState {
    pub fn new() -> Self {
        HttpLogState {
            enable_http_logs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn set_http_logs(&self, config_id: i64, enable: bool) {
        let mut logs = self.enable_http_logs.lock().await;
        logs.entry(config_id)
            .or_insert_with(|| AtomicBool::new(enable))
            .store(enable, Ordering::SeqCst);
    }

    pub async fn get_http_logs(&self, config_id: i64) -> bool {
        let logs = self.enable_http_logs.lock().await;
        if let Some(state) = logs.get(&config_id) {
            state.load(Ordering::SeqCst)
        } else {
            false
        }
    }
}

impl Default for HttpLogState {
    fn default() -> Self {
        Self::new()
    }
}
