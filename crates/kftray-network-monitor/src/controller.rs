use std::sync::Arc;

use log::{
    info,
    warn,
};
use tokio::sync::{
    Mutex,
    RwLock,
};
use tokio::task::JoinHandle;

use crate::monitor::start_network_monitor;
use crate::types::NetworkMonitorError;

pub struct NetworkMonitorController {
    task_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    is_running: Arc<Mutex<bool>>,
}

impl NetworkMonitorController {
    pub fn new() -> Self {
        Self {
            task_handle: Arc::new(RwLock::new(None)),
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn start(&self) -> Result<(), NetworkMonitorError> {
        let mut running = self.is_running.lock().await;
        if *running {
            return Err(NetworkMonitorError::AlreadyRunning);
        }

        info!("Starting network monitor");

        let handle = tokio::spawn(start_network_monitor());

        {
            let mut task_handle = self.task_handle.write().await;
            *task_handle = Some(handle);
        }

        *running = true;
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), NetworkMonitorError> {
        let mut running = self.is_running.lock().await;
        if !*running {
            return Err(NetworkMonitorError::NotRunning);
        }

        info!("Stopping network monitor");

        {
            let mut task_handle = self.task_handle.write().await;
            if let Some(handle) = task_handle.take() {
                handle.abort();
                if let Err(e) = handle.await {
                    if !e.is_cancelled() {
                        return Err(NetworkMonitorError::ShutdownFailed(e.to_string()));
                    }
                }
            }
        }

        *running = false;
        Ok(())
    }

    pub async fn restart(&self) -> Result<(), NetworkMonitorError> {
        if self.is_running().await {
            if let Err(e) = self.stop().await {
                warn!("Failed to stop network monitor during restart: {e}");
            }
        }
        self.start().await
    }

    pub async fn is_running(&self) -> bool {
        *self.is_running.lock().await
    }
}
