use kftray_commons::utils::config::upsert_configs_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_portforward::kube::operations::list_kube_contexts;
use kftray_portforward::kube::retrieve_service_configs;

use crate::tui::input::{
    App,
    AppState,
};

pub async fn handle_auto_add_configs(app: &mut App) {
    let contexts = match list_kube_contexts(None).await {
        Ok(context_infos) => context_infos.into_iter().map(|info| info.name).collect(),
        Err(e) => {
            app.error_message = Some(format!("Failed to list Kubernetes contexts: {e}"));
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
    let mut configs = match retrieve_service_configs(context, None).await {
        Ok(configs) => configs,
        Err(e) => {
            app.error_message = Some(format!(
                "Failed to retrieve service configurations from context '{context}': {e}"
            ));
            app.state = AppState::ShowErrorPopup;
            return;
        }
    };

    for config in &mut configs {
        config.domain_enabled = Some(app.auto_import_alias_as_domain);
        config.auto_loopback_address = app.auto_import_auto_loopback;
    }

    if let Err(e) = upsert_configs_with_mode(configs, mode).await {
        app.error_message = Some(format!("Failed to save configuration to database: {e}"));
        app.state = AppState::ShowErrorPopup;
        return;
    }

    app.state = AppState::Normal;
}
