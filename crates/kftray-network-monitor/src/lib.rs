mod config_manager;
mod controller;
mod health;
mod monitor;
mod network;
mod types;

use std::sync::OnceLock;

pub use controller::NetworkMonitorController;
use kftray_commons::utils::db_mode::DatabaseMode;
pub use types::{
    MonitorConfig,
    NetworkMonitorError,
};

static DEFAULT_CONTROLLER: OnceLock<NetworkMonitorController> = OnceLock::new();

fn get_default_controller() -> &'static NetworkMonitorController {
    DEFAULT_CONTROLLER.get_or_init(NetworkMonitorController::new)
}

pub async fn start() -> Result<(), NetworkMonitorError> {
    start_with_mode(DatabaseMode::File).await
}

pub async fn start_with_mode(mode: DatabaseMode) -> Result<(), NetworkMonitorError> {
    get_default_controller().start(mode).await
}

pub async fn stop() -> Result<(), NetworkMonitorError> {
    get_default_controller().stop().await
}

pub async fn is_running() -> bool {
    get_default_controller().is_running().await
}

pub async fn restart() -> Result<(), NetworkMonitorError> {
    restart_with_mode(DatabaseMode::File).await
}

pub async fn restart_with_mode(mode: DatabaseMode) -> Result<(), NetworkMonitorError> {
    get_default_controller().restart(mode).await
}
