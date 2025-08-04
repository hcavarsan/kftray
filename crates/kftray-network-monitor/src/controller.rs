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
    state: Arc<ControllerState>,
}

struct ControllerState {
    task_handle: RwLock<Option<JoinHandle<()>>>,
    is_running: Mutex<bool>,
}

impl NetworkMonitorController {
    pub fn new() -> Self {
        Self {
            state: Arc::new(ControllerState {
                task_handle: RwLock::new(None),
                is_running: Mutex::new(false),
            }),
        }
    }

    pub async fn start(&self) -> Result<(), NetworkMonitorError> {
        let mut running = self.state.is_running.lock().await;
        if *running {
            return Err(NetworkMonitorError::AlreadyRunning);
        }

        info!("Starting network monitor");

        let handle = tokio::spawn(start_network_monitor());

        match self.state.task_handle.try_write() {
            Ok(mut task_handle) => {
                *task_handle = Some(handle);
                *running = true;
                Ok(())
            }
            Err(_) => {
                handle.abort();
                Err(NetworkMonitorError::StartupFailed(
                    "Failed to acquire task handle lock".to_string(),
                ))
            }
        }
    }

    pub async fn stop(&self) -> Result<(), NetworkMonitorError> {
        let mut running = self.state.is_running.lock().await;
        if !*running {
            return Err(NetworkMonitorError::NotRunning);
        }

        info!("Stopping network monitor");

        let handle = {
            let mut task_handle = self.state.task_handle.write().await;
            task_handle.take()
        };

        if let Some(handle) = handle {
            handle.abort();
            if let Err(e) = handle.await {
                if !e.is_cancelled() {
                    *running = true; // Restore state on failure
                    return Err(NetworkMonitorError::ShutdownFailed(e.to_string()));
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
        *self.state.is_running.lock().await
    }
}
