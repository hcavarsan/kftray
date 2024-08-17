use kftray_commons::models::config_model::Config;

use crate::core::port_forward::{
    start_port_forwarding,
    stop_port_forwarding,
};
use crate::tui::input::ActiveTable;
use crate::tui::input::App;

pub async fn handle_port_forward(app: &mut App, config: Config) {
    if app.active_table == ActiveTable::Stopped {
        start_port_forwarding(app, config).await;
    } else {
        stop_port_forwarding(app, config).await;
    }
}
