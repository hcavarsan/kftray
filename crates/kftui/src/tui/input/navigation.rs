use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::insert_config;
use kftray_portforward::client::list_kube_contexts;
use kftray_portforward::core::retrieve_service_configs;

use crate::core::port_forward::{
    start_port_forwarding,
    stop_port_forwarding,
};
use crate::tui::input::ActiveTable;
use crate::tui::input::{
    App,
    AppState,
};

pub async fn handle_port_forward(app: &mut App, config: Config) {
    if app.active_table == ActiveTable::Stopped {
        start_port_forwarding(app, config).await;
    } else {
        stop_port_forwarding(app, config).await;
    }
}

pub async fn handle_auto_add_configs(app: &mut App) {
    let contexts = match list_kube_contexts(None).await {
        Ok(context_infos) => context_infos.into_iter().map(|info| info.name).collect(),
        Err(e) => {
            app.error_message = Some(format!("Failed to list contexts: {}", e));
            app.state = AppState::ShowErrorPopup;
            return;
        }
    };

    app.state = AppState::ShowContextSelection;
    app.contexts = contexts;
    app.selected_context_index = 0;
    app.context_list_state.select(Some(0));
}

pub async fn handle_context_selection(app: &mut App, context: &str) {
    let configs = match retrieve_service_configs(context).await {
        Ok(configs) => configs,
        Err(e) => {
            app.error_message = Some(format!("Failed to retrieve service configs: {}", e));
            app.state = AppState::ShowErrorPopup;
            return;
        }
    };

    for config in configs {
        if let Err(e) = insert_config(config).await {
            app.error_message = Some(format!("Failed to insert config: {}", e));
            app.state = AppState::ShowErrorPopup;
            return;
        }
    }

    app.state = AppState::Normal;
}
