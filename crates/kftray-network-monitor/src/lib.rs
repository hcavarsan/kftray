mod config_manager;
mod controller;
mod health;
mod monitor;
mod network;
mod types;

use std::sync::OnceLock;

pub use controller::NetworkMonitorController;
pub use types::{
    MonitorConfig,
    NetworkMonitorError,
};

static DEFAULT_CONTROLLER: OnceLock<NetworkMonitorController> = OnceLock::new();

fn get_default_controller() -> &'static NetworkMonitorController {
    DEFAULT_CONTROLLER.get_or_init(NetworkMonitorController::new)
}

pub async fn start() -> Result<(), NetworkMonitorError> {
    get_default_controller().start().await
}

pub async fn stop() -> Result<(), NetworkMonitorError> {
    get_default_controller().stop().await
}

pub async fn is_running() -> bool {
    get_default_controller().is_running().await
}

pub async fn restart() -> Result<(), NetworkMonitorError> {
    get_default_controller().restart().await
}
