use std::sync::Arc;

use log::info;
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

impl Default for NetworkMonitorController {
    fn default() -> Self {
        Self::new()
    }
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

        let mut task_handle = self.state.task_handle.write().await;
        *task_handle = Some(handle);
        *running = true;
        Ok(())
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
            if (handle.await).is_err() {}
        }
        *running = false;
        Ok(())
    }

    pub async fn restart(&self) -> Result<(), NetworkMonitorError> {
        if self.is_running().await {
            self.stop().await?;
        }
        self.start().await
    }

    pub async fn is_running(&self) -> bool {
        *self.state.is_running.lock().await
    }
}
