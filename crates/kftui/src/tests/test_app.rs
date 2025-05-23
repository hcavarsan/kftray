use kftray_commons::models::{
    config_model::Config,
    config_state_model::ConfigState,
};

use crate::tui::input::{
    ActiveComponent,
    ActiveTable,
    App,
    AppState,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config(id: i64, is_running: bool) -> (Config, ConfigState) {
        let config = Config {
            id: Some(id),
            service: Some(format!("service-{id}")),
            namespace: "default".to_string(),
            local_port: Some(8080 + (id as u16)),
            remote_port: Some(80),
            context: format!("test-context-{id}"),
            workload_type: Some("service".to_string()),
            protocol: "tcp".to_string(),
            remote_address: Some(format!("remote-{id}")),
            local_address: Some("127.0.0.1".to_string()),
            auto_loopback_address: false,
            alias: Some(format!("alias-{id}")),
            domain_enabled: Some(false),
            kubeconfig: None,
            target: Some(format!("target-{id}")),
        };

        let config_state = ConfigState {
            id: Some(id * 10),
            config_id: id,
            is_running,
        };

        (config, config_state)
    }

    fn create_test_configs(
        count: usize, running_indices: &[usize],
    ) -> (Vec<Config>, Vec<ConfigState>) {
        let mut configs = Vec::new();
        let mut config_states = Vec::new();

        for i in 0..count {
            let is_running = running_indices.contains(&i);
            let (config, state) = create_test_config(i as i64 + 1, is_running);
            configs.push(config);
            config_states.push(state);
        }

        (configs, config_states)
    }

    #[test]
    fn test_app_new() {
        let app = App::new();

        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);
        assert_eq!(app.active_table, ActiveTable::Stopped);
        assert!(app.stopped_configs.is_empty());
        assert!(app.running_configs.is_empty());
        assert!(app.selected_rows_stopped.is_empty());
        assert!(app.selected_rows_running.is_empty());
        assert_eq!(app.selected_row_stopped, 0);
        assert_eq!(app.selected_row_running, 0);
        assert_eq!(app.error_message, None);
    }

    #[test]
    fn test_update_configs() {
        let mut app = App::new();
        let (configs, config_states) = create_test_configs(5, &[1, 3]);

        app.update_configs(&configs, &config_states);

        assert_eq!(app.stopped_configs.len(), 3);
        assert_eq!(app.running_configs.len(), 2);

        assert_eq!(app.stopped_configs[0].id, Some(1));
        assert_eq!(app.stopped_configs[1].id, Some(3));
        assert_eq!(app.stopped_configs[2].id, Some(5));

        assert_eq!(app.running_configs[0].id, Some(2));
        assert_eq!(app.running_configs[1].id, Some(4));
    }

    #[test]
    fn test_scroll_up_down_stopped_table() {
        let mut app = App::new();
        let (configs, config_states) = create_test_configs(3, &[]);

        app.update_configs(&configs, &config_states);
        app.table_state_stopped.select(Some(1));
        app.selected_row_stopped = 1;

        app.scroll_up();
        assert_eq!(app.table_state_stopped.selected(), Some(0));
        assert_eq!(app.selected_row_stopped, 0);

        app.scroll_up();
        assert_eq!(app.table_state_stopped.selected(), Some(0));
        assert_eq!(app.selected_row_stopped, 0);

        app.scroll_down();
        assert_eq!(app.table_state_stopped.selected(), Some(1));
        assert_eq!(app.selected_row_stopped, 1);

        app.scroll_down();
        assert_eq!(app.table_state_stopped.selected(), Some(2));
        assert_eq!(app.selected_row_stopped, 2);

        app.scroll_down();
        assert_eq!(app.table_state_stopped.selected(), Some(2));
        assert_eq!(app.selected_row_stopped, 2);
    }

    #[test]
    fn test_scroll_up_down_running_table() {
        let mut app = App::new();
        let (configs, config_states) = create_test_configs(3, &[0, 1, 2]);

        app.update_configs(&configs, &config_states);
        app.active_table = ActiveTable::Running;
        app.table_state_running.select(Some(1));
        app.selected_row_running = 1;

        app.scroll_up();
        assert_eq!(app.table_state_running.selected(), Some(0));
        assert_eq!(app.selected_row_running, 0);

        app.scroll_up();
        assert_eq!(app.table_state_running.selected(), Some(0));
        assert_eq!(app.selected_row_running, 0);

        app.scroll_down();
        assert_eq!(app.table_state_running.selected(), Some(1));
        assert_eq!(app.selected_row_running, 1);

        app.scroll_down();
        assert_eq!(app.table_state_running.selected(), Some(2));
        assert_eq!(app.selected_row_running, 2);

        app.scroll_down();
        assert_eq!(app.table_state_running.selected(), Some(2));
        assert_eq!(app.selected_row_running, 2);
    }

    #[test]
    fn test_scroll_with_empty_tables() {
        let mut app = App::new();

        app.scroll_down();
        assert_eq!(app.table_state_stopped.selected(), None);

        app.scroll_up();
        assert_eq!(app.table_state_stopped.selected(), None);

        app.active_table = ActiveTable::Running;

        app.scroll_down();
        assert_eq!(app.table_state_running.selected(), None);

        app.scroll_up();
        assert_eq!(app.table_state_running.selected(), None);
    }

    #[test]
    fn test_update_visible_rows() {
        let mut app = App::new();

        app.update_visible_rows(30);
        assert_eq!(app.visible_rows, 11);

        app.update_visible_rows(50);
        assert_eq!(app.visible_rows, 31);

        app.update_visible_rows(19);
        assert_eq!(app.visible_rows, 0);
    }
}
