use kftray_commons::models::{
    config_model::Config,
    config_state_model::ConfigState,
};

use crate::tests::test_logger_state;
use crate::tui::input::{
    ActiveComponent,
    ActiveTable,
    App,
    AppState,
};

#[cfg(test)]
mod tests {

    use super::*;

    fn create_test_config(id: i64) -> Config {
        Config {
            id: Some(id),
            service: Some(format!("service-{id}")),
            namespace: format!("namespace-{id}"),
            local_port: Some(8080 + id as u16),
            remote_port: Some(80),
            context: Some("test-context".to_string()),
            workload_type: Some("deployment".to_string()),
            protocol: "tcp".to_string(),
            remote_address: Some(format!("remote-{id}")),
            local_address: Some("127.0.0.1".to_string()),
            auto_loopback_address: false,
            alias: Some(format!("alias-{id}")),
            domain_enabled: Some(false),
            kubeconfig: None,
            target: Some(format!("target-{id}")),
            http_logs_enabled: Some(false),
            http_logs_max_file_size: Some(10 * 1024 * 1024),
            http_logs_retention_days: Some(7),
            http_logs_auto_cleanup: Some(true),
        }
    }

    fn create_test_configs(
        count: usize, running_indices: &[usize],
    ) -> (Vec<Config>, Vec<ConfigState>) {
        let mut configs = Vec::new();
        let mut config_states = Vec::new();

        for i in 0..count {
            let is_running = running_indices.contains(&i);
            let config = create_test_config(i as i64 + 1);
            configs.push(config);

            config_states.push(ConfigState {
                id: None,
                config_id: i as i64 + 1,
                is_running,
                process_id: if is_running { Some(1234) } else { None },
            });
        }

        (configs, config_states)
    }

    #[test]
    fn test_app_new() {
        let app = App::new(test_logger_state());

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
        let mut app = App::new(test_logger_state());
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
    fn test_update_configs_with_selected_rows() {
        let mut app = App::new(test_logger_state());
        let (configs, config_states) = create_test_configs(3, &[0, 2]);

        app.selected_rows_stopped.insert(0);
        app.selected_rows_stopped.insert(2);
        app.selected_rows_running.insert(1);

        app.update_configs(&configs, &config_states);

        assert!(app.selected_rows_stopped.contains(&0));
        assert!(app.selected_rows_stopped.contains(&2));
        assert!(app.selected_rows_running.contains(&1));
    }

    #[test]
    fn test_scroll_down_stopped_table() {
        let mut app = App::new(test_logger_state());
        app.stopped_configs = vec![
            create_test_config(1),
            create_test_config(2),
            create_test_config(3),
        ];
        app.active_table = ActiveTable::Stopped;
        app.selected_row_stopped = 0;
        app.table_state_stopped.select(Some(0));

        app.scroll_down();
        assert_eq!(app.selected_row_stopped, 1);
        assert_eq!(app.table_state_stopped.selected(), Some(1));

        app.scroll_down();
        assert_eq!(app.selected_row_stopped, 2);
        assert_eq!(app.table_state_stopped.selected(), Some(2));

        app.scroll_down();
        assert_eq!(app.selected_row_stopped, 2);
        assert_eq!(app.table_state_stopped.selected(), Some(2));
    }

    #[test]
    fn test_scroll_up_stopped_table() {
        let mut app = App::new(test_logger_state());
        app.stopped_configs = vec![
            create_test_config(1),
            create_test_config(2),
            create_test_config(3),
        ];
        app.active_table = ActiveTable::Stopped;
        app.selected_row_stopped = 2;
        app.table_state_stopped.select(Some(2));

        app.scroll_up();
        assert_eq!(app.selected_row_stopped, 1);
        assert_eq!(app.table_state_stopped.selected(), Some(1));

        app.scroll_up();
        assert_eq!(app.selected_row_stopped, 0);
        assert_eq!(app.table_state_stopped.selected(), Some(0));

        app.scroll_up();
        assert_eq!(app.selected_row_stopped, 0);
        assert_eq!(app.table_state_stopped.selected(), Some(0));
    }

    #[test]
    fn test_scroll_up_down_running_table() {
        let mut app = App::new(test_logger_state());
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
        let mut app = App::new(test_logger_state());

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
        let mut app = App::new(test_logger_state());

        app.update_visible_rows(30);
        assert_eq!(app.visible_rows, 11);

        app.update_visible_rows(50);
        assert_eq!(app.visible_rows, 31);

        app.update_visible_rows(19);
        assert_eq!(app.visible_rows, 0);
    }
}
