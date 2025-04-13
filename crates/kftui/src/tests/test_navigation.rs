use kftray_commons::models::config_model::Config;

use crate::tui::input::navigation::{
    handle_auto_add_configs,
    handle_port_forward,
};
use crate::tui::input::{
    ActiveTable,
    App,
    AppState,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> Config {
        Config {
            id: Some(1),
            service: Some("test-service".to_string()),
            namespace: "default".to_string(),
            local_port: Some(8080),
            remote_port: Some(80),
            context: "test-context".to_string(),
            workload_type: Some("service".to_string()),
            protocol: "tcp".to_string(),
            remote_address: Some("remote-address".to_string()),
            local_address: Some("127.0.0.1".to_string()),
            alias: Some("test-alias".to_string()),
            domain_enabled: Some(false),
            kubeconfig: None,
            target: Some("test-target".to_string()),
        }
    }

    #[tokio::test]
    async fn test_handle_port_forward_stopped() {
        let mut app = App::new();
        app.active_table = ActiveTable::Stopped;
        let config = create_test_config();

        handle_port_forward(&mut app, config).await;

        assert_eq!(
            app.active_table,
            ActiveTable::Stopped,
            "Active table should remain Stopped"
        );
    }

    #[tokio::test]
    async fn test_handle_port_forward_running() {
        let mut app = App::new();
        app.active_table = ActiveTable::Running;
        let config = create_test_config();

        handle_port_forward(&mut app, config).await;

        assert_eq!(
            app.active_table,
            ActiveTable::Running,
            "Active table should remain Running"
        );
    }

    #[tokio::test]
    async fn test_handle_auto_add_configs_error() {
        let mut app = App::new();

        handle_auto_add_configs(&mut app).await;

        if app.state == AppState::ShowErrorPopup {
            assert!(
                app.error_message.is_some(),
                "Error message should be present in error state"
            );
            if let Some(error_msg) = &app.error_message {
                println!("Error message: {}", error_msg);
            }
        } else {
            assert_eq!(
                app.state,
                AppState::ShowContextSelection,
                "App should be in context selection state"
            );
            assert_eq!(
                app.selected_context_index, 0,
                "First context should be selected"
            );
            assert!(
                app.context_list_state.selected().is_some(),
                "Context list should have a selection"
            );
        }
    }
}
