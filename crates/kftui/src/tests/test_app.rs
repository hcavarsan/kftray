use kftray_commons::models::{
    config_model::Config,
    config_state_model::ConfigState,
};

use crate::tests::test_logger_state;
use crate::tui::input::{
    ActiveTable,
    App,
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
            exposure_type: None,
            cert_manager_enabled: None,
            cert_issuer: None,
            cert_issuer_kind: None,
            ingress_class: None,
            ingress_annotations: None,
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
                ..Default::default()
            });
        }

        (configs, config_states)
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

    #[test]
    fn pending_forward_stays_busy_until_completion() {
        let mut app = App::new(test_logger_state());
        let complete = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        app.configs_being_processed.insert(1, complete.clone());
        app.update_configs(&[], &[]);
        assert!(app.configs_being_processed.contains_key(&1));

        complete.store(true, std::sync::atomic::Ordering::Relaxed);
        app.update_configs(&[], &[]);
        assert!(!app.configs_being_processed.contains_key(&1));
    }

    #[tokio::test]
    async fn finishing_cancels_queued_forwards_without_waiting_for_a_slot() {
        let mut app = App::new(test_logger_state());
        let slots = app.forwarding_slots.available_permits() as u32;
        let _occupied = app
            .forwarding_slots
            .clone()
            .acquire_many_owned(slots)
            .await
            .unwrap();
        app.stopped_configs = vec![create_test_config(1)];
        app.table_state_stopped.select(Some(0));
        crate::tui::input::handle_port_forwarding(
            &mut app,
            kftray_commons::utils::db_mode::DatabaseMode::Memory,
        )
        .await
        .unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(1), app.finish_forwarding())
            .await
            .unwrap();
        assert!(app.error_receiver.as_mut().unwrap().try_recv().is_err());
    }
}
