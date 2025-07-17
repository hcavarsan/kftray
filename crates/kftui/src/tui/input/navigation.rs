use kftray_commons::utils::config::insert_config_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_portforward::kube::client::list_kube_contexts;
use kftray_portforward::kube::retrieve_service_configs;

use crate::tui::input::{
    App,
    AppState,
};

pub async fn handle_auto_add_configs(app: &mut App) {
    let contexts = match list_kube_contexts(None).await {
        Ok(context_infos) => context_infos.into_iter().map(|info| info.name).collect(),
        Err(e) => {
            app.error_message = Some(format!("Failed to list contexts: {e}"));
            app.state = AppState::ShowErrorPopup;
            return;
        }
    };

    app.state = AppState::ShowContextSelection;
    app.contexts = contexts;
    app.selected_context_index = 0;
    app.context_list_state.select(Some(0));
}

pub async fn handle_context_selection(app: &mut App, context: &str, mode: DatabaseMode) {
    let configs = match retrieve_service_configs(context, None).await {
        Ok(configs) => configs,
        Err(e) => {
            app.error_message = Some(format!("Failed to retrieve service configs: {e}"));
            app.state = AppState::ShowErrorPopup;
            return;
        }
    };

    for config in configs {
        if let Err(e) = insert_config_with_mode(config, mode).await {
            app.error_message = Some(format!("Failed to insert config: {e}"));
            app.state = AppState::ShowErrorPopup;
            return;
        }
    }

    app.state = AppState::Normal;
}
