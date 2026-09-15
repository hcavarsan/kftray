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

    // `handle_port_forwarding`'s dispatch loop and `App::drain_forwarding`
    // both reach into process-wide registries owned by `kftray_portforward`
    // and `kftray_commons`, so tests exercising them must not run
    // concurrently with each other or with those crates' own tests that use
    // the same registries. Lock commons before portforward, always in that
    // order, to match how any future combined lock site would have to.
    async fn lock_forwarding_globals() -> (
        tokio::sync::MutexGuard<'static, ()>,
        tokio::sync::MutexGuard<'static, ()>,
    ) {
        let memory = kftray_commons::test_utils::MEMORY_MODE_TEST_MUTEX
            .lock()
            .await;
        let process = kftray_portforward::port_forward::PROCESS_TEST_MUTEX
            .lock()
            .await;
        (memory, process)
    }

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

    async fn spawn_panicking_task(app: &mut App, config_id: i64) {
        let handle = app.forwarding_tasks.spawn(async { panic!("boom") });
        app.task_configs.insert(
            handle.id(),
            crate::tui::input::TaskInfo::new(config_id, false, handle.clone()),
        );

        // Bounded by a timeout rather than a fixed yield budget: the number
        // of yields a scheduled task needs to finish is not a stable
        // quantity a test should assume.
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !handle.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the task must have panicked by now");
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
    fn test_app_new_invariants() {
        let app = App::new(test_logger_state());

        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.active_table, ActiveTable::Stopped);
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);
        assert!(app.stopped_configs.is_empty());
        assert!(app.running_configs.is_empty());
        assert!(app.filtered_stopped_configs.is_empty());
        assert!(app.filtered_running_configs.is_empty());
        assert!(app.selected_rows_stopped.is_empty());
        assert!(app.selected_rows_running.is_empty());
        assert_eq!(app.selected_row_stopped, 0);
        assert_eq!(app.selected_row_running, 0);
        assert_eq!(app.visible_rows, 0);
        assert!(app.error_message.is_none());

        assert_eq!(
            app.forwarding_slots.available_permits(),
            crate::tui::input::FORWARD_DISPATCH_CONCURRENCY
        );
        assert_eq!(
            app.stop_slots.available_permits(),
            crate::tui::input::FORWARD_DISPATCH_CONCURRENCY
        );
        assert!(!app.forwarding_cancel.is_cancelled());
        assert!(app.configs_being_processed.is_empty());
        assert!(app.task_configs.is_empty());
        assert_eq!(app.error_scroll, 0);
    }

    #[tokio::test]
    async fn dispatching_an_already_pending_config_does_not_spawn_a_second_task() {
        let mut app = App::new(test_logger_state());
        let _guard = lock_forwarding_globals().await;
        app.stopped_configs = vec![create_test_config(1)];
        app.selected_row_stopped = 0;
        app.configs_being_processed.insert(
            1,
            std::sync::Arc::new(crate::tui::input::PendingForward::new(1)),
        );

        let tasks_before = app.forwarding_tasks.len();
        let task_configs_before = app.task_configs.len();

        crate::tui::input::handle_port_forwarding(
            &mut app,
            kftray_commons::utils::db_mode::DatabaseMode::Memory,
        )
        .await
        .unwrap();

        assert_eq!(app.forwarding_tasks.len(), tasks_before);
        assert_eq!(app.task_configs.len(), task_configs_before);

        let receiver = app.error_receiver.as_mut().unwrap();
        let reported = receiver
            .try_recv()
            .expect("the busy filter must report the dropped key press");
        assert!(
            reported.contains("still starting"),
            "the message must say the config is still starting: {reported}"
        );
    }

    #[test]
    fn update_configs_aggregates_channel_errors_and_appends_to_an_open_popup() {
        let mut app = App::new(test_logger_state());
        let sender = app.error_sender.clone().unwrap();
        sender.send("first failure".to_string()).unwrap();
        sender.send("second failure".to_string()).unwrap();

        app.update_configs(&[], &[]);

        assert_eq!(
            app.error_message.as_deref(),
            Some("first failure\nsecond failure")
        );
        assert_eq!(app.state, AppState::ShowErrorPopup);

        app.error_scroll = 5;
        sender.send("third failure".to_string()).unwrap();
        app.update_configs(&[], &[]);

        assert_eq!(
            app.error_message.as_deref(),
            Some("first failure\nsecond failure\nthird failure")
        );
        assert_eq!(
            app.error_scroll, 0,
            "a newly appended failure must be visible, not hidden below a stale scroll offset"
        );
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

        let now = std::time::Instant::now();
        let stalled_at = now
            .checked_sub(crate::tui::input::PROCESSING_WATCHDOG + std::time::Duration::from_secs(1))
            .expect("Instant::now() must be far enough past the epoch to subtract the watchdog");
        queued.mark_running_at(stalled_at);
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
        spawn_panicking_task(&mut app, 410_081).await;

        app.update_configs(&[], &[]);
        let reported = app.error_message.clone().unwrap();
        assert!(reported.contains("410081"), "{reported}");
    }

    #[tokio::test]
    async fn a_failure_during_a_modal_is_shown_once_the_modal_closes() {
        let mut app = App::new(test_logger_state());
        spawn_panicking_task(&mut app, 410_082).await;

        // The user is in the middle of confirming a delete when the task dies.
        app.state = AppState::ShowDeleteConfirmation;
        app.update_configs(&[], &[]);
        assert_eq!(
            app.state,
            AppState::ShowDeleteConfirmation,
            "a failure must not tear down the confirmation the user is answering"
        );
        assert!(app.error_message.is_none());

        // The modal closes; the report was held, not dropped.
        app.state = AppState::Normal;
        app.update_configs(&[], &[]);
        assert_eq!(app.state, AppState::ShowErrorPopup);
        let reported = app.error_message.clone().unwrap();
        assert!(reported.contains("410082"), "{reported}");
    }

    #[tokio::test]
    async fn finishing_cancels_queued_forwards_without_waiting_for_a_slot() {
        let mut app = App::new(test_logger_state());
        let _guard = lock_forwarding_globals().await;
        let slots = app.forwarding_slots.available_permits() as u32;
        let _occupied = app
            .forwarding_slots
            .clone()
            .acquire_many_owned(slots)
            .await
            .unwrap();
        app.stopped_configs = vec![create_test_config(1)];
        app.selected_row_stopped = 0;
        crate::tui::input::handle_port_forwarding(
            &mut app,
            kftray_commons::utils::db_mode::DatabaseMode::Memory,
        )
        .await
        .unwrap();

        // Drained without the printing step, so the reports are still here to
        // inspect: an intentional shutdown cancellation must not become one.
        tokio::time::timeout(std::time::Duration::from_secs(1), app.drain_forwarding())
            .await
            .unwrap();
        let reports = app.take_shutdown_reports();
        assert!(reports.is_empty(), "{reports:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_stop_task_that_outlives_the_shutdown_budget_is_detached_not_aborted() {
        let mut app = App::new(test_logger_state());
        let _guard = lock_forwarding_globals().await;
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

        // Models a stop that is still doing real cleanup (releasing a relay
        // or an address claim) when both drain budgets in `drain_forwarding`
        // expire: it must never observe an abort.
        let abort = app.forwarding_tasks.spawn(async move {
            release_rx.await.expect("release must be sent");
            done_tx.send(()).expect("receiver must still be listening");
        });
        app.task_configs
            .insert(abort.id(), crate::tui::input::TaskInfo::new(1, true, abort));

        app.drain_forwarding().await;

        assert_eq!(
            app.forwarding_tasks.len(),
            0,
            "a stop task ignored by both budgets must be removed from the \
             JoinSet so dropping it later cannot abort the stop"
        );
        release_tx
            .send(())
            .expect("a detached stop task must still be alive, not aborted, after the budget");
        tokio::time::timeout(std::time::Duration::from_secs(1), done_rx)
            .await
            .expect("must not time out waiting on a still-running detached task")
            .expect("the detached stop task must run to completion and report back");
    }

    #[test]
    fn an_error_still_on_screen_reaches_the_shutdown_report() {
        let mut app = App::new(test_logger_state());
        // The failure has already moved from the channel into the popup.
        app.error_message = Some("Config 4: the relay never became ready".to_string());
        app.state = AppState::ShowErrorPopup;

        let reports = app.take_shutdown_reports();

        assert_eq!(
            reports,
            vec!["Config 4: the relay never became ready".to_string()]
        );
    }

    #[tokio::test]
    async fn a_saturated_start_batch_does_not_block_stopping() {
        let mut app = App::new(test_logger_state());
        let _guard = lock_forwarding_globals().await;
        let slots = app.forwarding_slots.available_permits() as u32;
        let _occupied = app
            .forwarding_slots
            .clone()
            .acquire_many_owned(slots)
            .await
            .unwrap();

        app.active_table = crate::tui::input::ActiveTable::Running;
        app.running_configs = vec![create_test_config(410_061)];
        app.selected_row_running = 0;
        crate::tui::input::handle_port_forwarding(
            &mut app,
            kftray_commons::utils::db_mode::DatabaseMode::Memory,
        )
        .await
        .unwrap();

        // No cancellation and no `drain_forwarding` here: shutdown draining is
        // a separate concern already covered by
        // `finishing_cancels_queued_forwards_without_waiting_for_a_slot`.
        // Waiting on the pending flag proves the task ran to completion; the
        // error asserted below proves it was `stop_port_forwarding` itself
        // that ran, not merely a task that returned early.
        let pending = app
            .configs_being_processed
            .get(&410_061)
            .expect("the stop must still be tracked as pending until it settles")
            .clone();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while pending.is_active() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the stop must run to completion while every start permit is held");

        let receiver = app.error_receiver.as_mut().unwrap();
        let reported = receiver
            .try_recv()
            .expect("stop_port_forwarding must have actually run and reported its outcome");
        assert!(
            reported.contains("410061"),
            "the report must come from the real backend call for this config: {reported}"
        );

        assert_eq!(
            app.forwarding_slots.available_permits(),
            0,
            "start permits must still be held while the stop completes"
        );
    }
}
