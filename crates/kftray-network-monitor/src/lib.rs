mod config_manager;
mod controller;
mod health;
mod monitor;
mod network;
mod types;

use controller::NetworkMonitorController;
pub use types::{
    MonitorConfig,
    NetworkMonitorError,
};

static CONTROLLER: tokio::sync::OnceCell<NetworkMonitorController> =
    tokio::sync::OnceCell::const_new();

async fn get_controller() -> &'static NetworkMonitorController {
    CONTROLLER
        .get_or_init(|| async { NetworkMonitorController::new() })
        .await
}

pub async fn start() -> Result<(), NetworkMonitorError> {
    get_controller().await.start().await
}

pub async fn stop() -> Result<(), NetworkMonitorError> {
    get_controller().await.stop().await
}

pub async fn is_running() -> bool {
    get_controller().await.is_running().await
}

pub async fn restart() -> Result<(), NetworkMonitorError> {
    get_controller().await.restart().await
}
