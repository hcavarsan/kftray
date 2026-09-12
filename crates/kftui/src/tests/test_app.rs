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
        let pending = std::sync::Arc::new(crate::tui::input::PendingForward::new(1));
        app.configs_being_processed.insert(1, pending.clone());
        app.update_configs(&[], &[]);
        assert!(app.configs_being_processed.contains_key(&1));

        pending.finish();
        app.update_configs(&[], &[]);
        assert!(!app.configs_being_processed.contains_key(&1));
    }

    #[test]
    fn a_stalled_pending_forward_is_reported_but_never_dropped() {
        let mut app = App::new(test_logger_state());
        let queued = std::sync::Arc::new(crate::tui::input::PendingForward::new(1));
        app.configs_being_processed.insert(1, queued.clone());

        app.update_configs(&[], &[]);
        assert!(
            app.configs_being_processed.contains_key(&1),
            "a config still waiting for a slot must keep its busy indicator"
        );
        assert!(app.error_message.is_none());

        queued.mark_running_at(std::time::Instant::now() - std::time::Duration::from_secs(31));
        app.update_configs(&[], &[]);
        assert!(
            app.configs_being_processed.contains_key(&1),
            "a stalled operation must keep running so its own rollback can finish"
        );
        let reported = app.error_message.clone().unwrap();
        assert!(reported.contains("still working"), "{reported}");

        app.error_message = None;
        app.update_configs(&[], &[]);
        assert!(
            app.error_message.is_none(),
            "the stall must be reported once, not on every redraw"
        );

        queued.finish();
        app.update_configs(&[], &[]);
        assert!(!app.configs_being_processed.contains_key(&1));
    }

    #[tokio::test]
    async fn a_panicking_forward_task_reaches_the_error_popup() {
        let mut app = App::new(test_logger_state());
        let handle = app.forwarding_tasks.spawn(async { panic!("boom") });
        app.task_configs.insert(handle.id(), 410_081);

        // Yield until the task has actually finished: a fixed sleep would make
        // this pass or fail on scheduling rather than on the behaviour.
        for _ in 0..200 {
            if handle.is_finished() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(handle.is_finished(), "the task must have panicked by now");

        app.update_configs(&[], &[]);
        let reported = app.error_message.clone().unwrap();
        assert!(reported.contains("410081"), "{reported}");
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

    #[tokio::test]
    async fn a_saturated_start_batch_does_not_block_stopping() {
        let mut app = App::new(test_logger_state());
        let slots = app.forwarding_slots.available_permits() as u32;
        let _occupied = app
            .forwarding_slots
            .clone()
            .acquire_many_owned(slots)
            .await
            .unwrap();

        app.active_table = crate::tui::input::ActiveTable::Running;
        app.running_configs = vec![create_test_config(410_061)];
        app.table_state_running.select(Some(0));
        crate::tui::input::handle_port_forwarding(
            &mut app,
            kftray_commons::utils::db_mode::DatabaseMode::Memory,
        )
        .await
        .unwrap();

        let receiver = app.error_receiver.as_mut().unwrap();
        let reported = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
            .await
            .expect("the stop must run while every start permit is held");
        assert!(reported.is_some());
    }
}
