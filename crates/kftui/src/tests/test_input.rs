use crossterm::event::KeyCode;
use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::{
    insert_config_with_mode,
    read_configs_with_mode,
};
use kftray_commons::utils::db_mode::DatabaseMode;

use crate::tests::test_logger_state;
use crate::tui::input::{
    ActiveComponent,
    ActiveTable,
    App,
    AppState,
    Consent,
    DeleteButton,
    HttpLogEntry,
    TelemetryButton,
    clear_selection,
    handle_about_input,
    handle_confirmation_popup_input,
    handle_context_selection_input,
    handle_delete_confirmation_input,
    handle_details_input,
    handle_error_popup_input,
    handle_help_input,
    handle_http_logs_config_input,
    handle_http_logs_viewer_input,
    handle_logs_input,
    handle_menu_input,
    handle_normal_input,
    handle_running_table_input,
    handle_search_input,
    handle_stopped_table_input,
    handle_telemetry_consent_input,
    select_first_row,
    show_delete_confirmation,
    toggle_row_selection,
    toggle_select_all,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_configs(num: usize) -> Vec<Config> {
        (0..num)
            .map(|i| Config {
                id: Some(i as i64),
                service: Some(format!("service-{i}")),
                namespace: format!("namespace-{i}"),
                local_port: Some(8080 + i as u16),
                remote_port: Some(80),
                context: Some(format!("context-{i}")),
                workload_type: Some("service".to_string()),
                protocol: "tcp".to_string(),
                remote_address: Some(format!("remote-{i}")),
                local_address: Some("127.0.0.1".to_string()),
                auto_loopback_address: false,
                alias: Some(format!("alias-{i}")),
                domain_enabled: Some(false),
                kubeconfig: None,
                target: Some(format!("target-{i}")),
                http_logs_enabled: Some(false),
                http_logs_max_file_size: Some(10 * 1024 * 1024),
                http_logs_retention_days: Some(7),
                http_logs_auto_cleanup: Some(true),
                exposure_type: None,
                exposure_protocol: None,
                cert_manager_enabled: None,
                cert_issuer: None,
                cert_issuer_kind: None,
                ingress_class: None,
                ingress_annotations: None,
                tags: Default::default(),
            })
            .collect()
    }

    fn setup_app() -> App {
        let mut app = App::new(test_logger_state());
        app.stopped_configs = create_test_configs(3);
        app.running_configs = create_test_configs(2);
        app.table_state_stopped.select(Some(1));
        app.table_state_running.select(Some(0));
        app.selected_row_stopped = 1;
        app.selected_row_running = 0;
        app
    }

    #[test]
    fn test_toggle_row_selection_stopped() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Stopped;

        toggle_row_selection(&mut app);
        assert!(app.selected_rows_stopped.contains(&1));

        toggle_row_selection(&mut app);
        assert!(!app.selected_rows_stopped.contains(&1));
    }

    #[test]
    fn test_toggle_row_selection_running() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Running;

        toggle_row_selection(&mut app);
        assert!(app.selected_rows_running.contains(&0));

        toggle_row_selection(&mut app);
        assert!(!app.selected_rows_running.contains(&0));
    }

    #[test]
    fn test_toggle_select_all_stopped() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Stopped;

        toggle_select_all(&mut app);
        assert_eq!(app.selected_rows_stopped.len(), 3);
        assert!(app.selected_rows_stopped.contains(&0));
        assert!(app.selected_rows_stopped.contains(&1));
        assert!(app.selected_rows_stopped.contains(&2));

        toggle_select_all(&mut app);
        assert_eq!(app.selected_rows_stopped.len(), 0);
    }

    #[test]
    fn test_toggle_select_all_running() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Running;

        toggle_select_all(&mut app);
        assert_eq!(app.selected_rows_running.len(), 2);
        assert!(app.selected_rows_running.contains(&0));
        assert!(app.selected_rows_running.contains(&1));

        toggle_select_all(&mut app);
        assert_eq!(app.selected_rows_running.len(), 0);
    }

    #[test]
    fn test_select_first_row_stopped() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Stopped;
        app.table_state_stopped.select(None);

        select_first_row(&mut app);
        assert_eq!(app.table_state_stopped.selected(), Some(0));
    }

    #[test]
    fn test_select_first_row_running() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Running;
        app.table_state_running.select(None);

        select_first_row(&mut app);
        assert_eq!(app.table_state_running.selected(), Some(0));
    }

    #[test]
    fn test_select_first_row_empty() {
        let mut app = App::new(test_logger_state());
        app.active_table = ActiveTable::Stopped;

        select_first_row(&mut app);
        assert_eq!(app.table_state_stopped.selected(), None);

        app.active_table = ActiveTable::Running;
        select_first_row(&mut app);
        assert_eq!(app.table_state_running.selected(), None);
    }

    #[test]
    fn test_clear_selection_stopped() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Stopped;
        app.selected_rows_running.insert(0);
        app.table_state_running.select(Some(0));

        clear_selection(&mut app);
        assert!(app.selected_rows_running.is_empty());
        assert_eq!(app.table_state_running.selected(), None);
    }

    #[test]
    fn test_clear_selection_running() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Running;
        app.selected_rows_stopped.insert(0);
        app.table_state_stopped.select(Some(0));

        clear_selection(&mut app);
        assert!(app.selected_rows_stopped.is_empty());
        assert_eq!(app.table_state_stopped.selected(), None);
    }

    #[test]
    fn test_scroll_page_up_down() {
        let mut app = setup_app();
        app.active_component = ActiveComponent::StoppedTable;
        app.visible_rows = 2;
        app.stopped_configs = create_test_configs(10);
        app.selected_row_stopped = 5;
        app.table_state_stopped.select(Some(5));

        crate::tui::input::scroll_page_up(&mut app);
        assert_eq!(app.selected_row_stopped, 3);
        assert_eq!(app.table_state_stopped.selected(), Some(3));

        crate::tui::input::scroll_page_down(&mut app);
        assert_eq!(app.selected_row_stopped, 5);
        assert_eq!(app.table_state_stopped.selected(), Some(5));

        app.active_component = ActiveComponent::RunningTable;
        app.running_configs = create_test_configs(10);
        app.selected_row_running = 5;
        app.table_state_running.select(Some(5));

        crate::tui::input::scroll_page_up(&mut app);
        assert_eq!(app.selected_row_running, 3);
        assert_eq!(app.table_state_running.selected(), Some(3));

        crate::tui::input::scroll_page_down(&mut app);
        assert_eq!(app.selected_row_running, 5);
        assert_eq!(app.table_state_running.selected(), Some(5));

        app.active_component = ActiveComponent::Details;
        app.details_scroll_offset = 5;
        app.details_scroll_max_offset = 10;

        crate::tui::input::scroll_page_up(&mut app);
        assert_eq!(app.details_scroll_offset, 3);

        crate::tui::input::scroll_page_down(&mut app);
        assert_eq!(app.details_scroll_offset, 5);
    }

    #[test]
    fn test_app_new() {
        let app = App::new(test_logger_state());
        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);
    }

    #[test]
    fn scrolling_past_the_bottom_does_not_bank_offsets() {
        let mut app = setup_app();
        app.state = AppState::ShowErrorPopup;
        app.error_message = Some("many failures".to_string());
        // What the last render could actually show.
        app.error_scroll_max = 3;

        for _ in 0..10 {
            handle_error_popup_input(&mut app, KeyCode::Down).unwrap();
        }
        assert_eq!(app.error_scroll, 3, "scrolling stops at the last page");

        handle_error_popup_input(&mut app, KeyCode::Up).unwrap();
        assert_eq!(
            app.error_scroll, 2,
            "one key press must reveal the preceding line"
        );
    }

    #[test]
    fn test_handle_error_popup_input() {
        let mut app = setup_app();
        app.state = AppState::ShowErrorPopup;
        app.error_scroll_max = 25;
        app.error_scroll = 12;

        handle_error_popup_input(&mut app, KeyCode::PageDown).unwrap();
        assert_eq!(app.error_scroll, 22, "PageDown advances by 10");

        handle_error_popup_input(&mut app, KeyCode::PageDown).unwrap();
        assert_eq!(
            app.error_scroll, 25,
            "PageDown clamps at the last usable offset"
        );

        handle_error_popup_input(&mut app, KeyCode::PageUp).unwrap();
        assert_eq!(app.error_scroll, 15, "PageUp backs up by 10");

        handle_error_popup_input(&mut app, KeyCode::End).unwrap();
        assert_eq!(app.error_scroll, 25, "End jumps to the last usable offset");

        handle_error_popup_input(&mut app, KeyCode::Home).unwrap();
        assert_eq!(app.error_scroll, 0, "Home jumps back to the top");

        app.error_scroll = 12;
        handle_error_popup_input(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.error_scroll, 0, "Esc must reset the scroll offset");

        app.state = AppState::ShowErrorPopup;
        app.error_scroll = 12;
        handle_error_popup_input(&mut app, KeyCode::Enter).unwrap();
        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.error_scroll, 0, "Enter must reset the scroll offset");
    }

    #[tokio::test]
    async fn test_handle_confirmation_popup_input() {
        let mut app = setup_app();
        app.state = AppState::ShowConfirmationPopup;

        handle_confirmation_popup_input(&mut app, KeyCode::Esc)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowConfirmationPopup;
        handle_confirmation_popup_input(&mut app, KeyCode::Enter)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::Normal);
    }

    #[tokio::test]
    async fn test_handle_help_input() {
        let mut app = setup_app();
        app.state = AppState::ShowHelp;

        handle_help_input(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowHelp;
        handle_help_input(&mut app, KeyCode::Enter).unwrap();
        assert_eq!(app.state, AppState::Normal);
    }

    #[tokio::test]
    async fn test_handle_about_input() {
        let mut app = setup_app();
        app.state = AppState::ShowAbout;

        handle_about_input(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowAbout;
        handle_about_input(&mut app, KeyCode::Enter).unwrap();
        assert_eq!(app.state, AppState::Normal);
    }

    #[tokio::test]
    async fn test_handle_delete_confirmation_input() {
        let mut app = setup_app();
        app.state = AppState::ShowDeleteConfirmation;
        app.selected_delete_button = DeleteButton::Close;

        handle_delete_confirmation_input(&mut app, KeyCode::Left, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.selected_delete_button, DeleteButton::Confirm);

        handle_delete_confirmation_input(&mut app, KeyCode::Right, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.selected_delete_button, DeleteButton::Close);

        handle_delete_confirmation_input(&mut app, KeyCode::Esc, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowDeleteConfirmation;
        app.selected_delete_button = DeleteButton::Close;
        handle_delete_confirmation_input(&mut app, KeyCode::Enter, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::Normal);
    }

    async fn app_with_saved_configs(aliases: &[&str]) -> App {
        let mut app = App::new(test_logger_state());
        for alias in aliases {
            let config = Config {
                alias: Some(alias.to_string()),
                service: Some(alias.to_string()),
                namespace: "default".to_string(),
                protocol: "tcp".to_string(),
                ..Config::default()
            };
            let id = insert_config_with_mode(config.clone(), DatabaseMode::Memory)
                .await
                .unwrap();
            app.stopped_configs.push(Config {
                id: Some(id),
                ..config
            });
        }
        app.active_table = ActiveTable::Stopped;
        app
    }

    async fn confirm_delete(app: &mut App) -> Vec<String> {
        show_delete_confirmation(app);
        assert_eq!(app.state, AppState::ShowDeleteConfirmation);
        app.selected_delete_button = DeleteButton::Confirm;
        handle_delete_confirmation_input(app, KeyCode::Enter, DatabaseMode::Memory)
            .await
            .unwrap();
        while app.forwarding_tasks.join_next().await.is_some() {}
        let mut remaining: Vec<String> = read_configs_with_mode(DatabaseMode::Memory)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|config| config.alias)
            .collect();
        remaining.sort();
        remaining
    }

    #[tokio::test]
    async fn delete_while_searching_removes_the_marked_config() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mut app = app_with_saved_configs(&["alpha", "beta", "gamma"]).await;
        app.search_query = "gamma".to_string();
        app.update_filtered_configs();
        app.table_state_stopped.select(Some(0));
        toggle_row_selection(&mut app);

        let remaining = confirm_delete(&mut app).await;

        assert_eq!(remaining, ["alpha", "beta"]);
    }

    #[tokio::test]
    async fn delete_after_another_config_starts_removes_the_marked_config() {
        use kftray_commons::models::config_state_model::ConfigState;
        let _db = kftray_commons::test_utils::test_db().await;
        let mut app = app_with_saved_configs(&["alpha", "beta", "gamma"]).await;
        let configs = read_configs_with_mode(DatabaseMode::Memory).await.unwrap();
        let states = |running: &str| -> Vec<ConfigState> {
            configs
                .iter()
                .map(|c| ConfigState {
                    config_id: c.id.unwrap(),
                    is_running: c.alias.as_deref() == Some(running),
                    ..Default::default()
                })
                .collect()
        };
        app.update_configs(&configs, &states(""));

        app.table_state_stopped.select(Some(1));
        toggle_row_selection(&mut app);
        app.update_configs(&configs, &states("alpha"));

        let remaining = confirm_delete(&mut app).await;
        assert_eq!(remaining, ["alpha", "gamma"]);
    }

    async fn mark_saved_config(app: &mut App, id: i64) {
        let configs = read_configs_with_mode(DatabaseMode::Memory).await.unwrap();
        let states =
            kftray_commons::utils::config_state::read_config_states_with_mode(DatabaseMode::Memory)
                .await
                .unwrap();
        app.update_configs(&configs, &states);
        app.active_table = ActiveTable::Stopped;
        let row = app
            .stopped_configs
            .iter()
            .position(|config| config.id == Some(id))
            .unwrap();
        app.table_state_stopped.select(Some(row));
        toggle_row_selection(app);
    }

    async fn config_exists(id: i64) -> bool {
        read_configs_with_mode(DatabaseMode::Memory)
            .await
            .unwrap()
            .iter()
            .any(|config| config.id == Some(id))
    }

    #[tokio::test]
    async fn delete_keeps_a_proxy_config_that_still_owes_cluster_cleanup() {
        let _db = kftray_commons::test_utils::test_db().await;
        let config = Config {
            alias: Some("proxy-db".into()),
            service: Some("proxy-db".into()),
            namespace: "default".into(),
            protocol: "tcp".into(),
            context: Some("dev".into()),
            workload_type: Some("proxy".into()),
            remote_address: Some("db.example".into()),
            remote_port: Some(5432),
            local_port: Some(15432),
            ..Config::default()
        };
        let id = insert_config_with_mode(config.clone(), DatabaseMode::Memory)
            .await
            .unwrap();
        let config = Config {
            id: Some(id),
            ..config
        };
        kftray_commons::utils::settings::set_setting_with_mode(
            &format!("uncertain_create:memory:{id}:0000000000000000"),
            &serde_json::json!({"config": config, "destination": null}).to_string(),
            DatabaseMode::Memory,
        )
        .await
        .unwrap();
        let mut app = App::new(test_logger_state());
        mark_saved_config(&mut app, id).await;

        confirm_delete(&mut app).await;

        assert!(config_exists(id).await);
        app.update_configs(&[], &[]);
        assert_eq!(app.state, AppState::ShowErrorPopup);
        let shown = app.error_message.unwrap_or_default();
        assert!(
            shown.contains(&format!(
                "Stop these configurations before deleting them: {id}"
            )),
            "{shown}"
        );
    }

    #[tokio::test]
    async fn delete_waits_for_a_start_in_flight_without_blocking_input() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mut app = app_with_saved_configs(&["alpha"]).await;
        let id = app.stopped_configs[0].id.unwrap();
        mark_saved_config(&mut app, id).await;
        let lock = kftray_portforward::kube::proxy_recovery::acquire_recovery_lock(id).await;
        let start = lock.lock_owned().await;

        show_delete_confirmation(&mut app);
        app.selected_delete_button = DeleteButton::Confirm;
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            handle_delete_confirmation_input(&mut app, KeyCode::Enter, DatabaseMode::Memory),
        )
        .await
        .expect("the delete must not hold up the input loop")
        .unwrap();
        assert_eq!(app.state, AppState::Normal);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(config_exists(id).await);

        drop(start);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while app.forwarding_tasks.join_next().await.is_some() {}
        })
        .await
        .unwrap();
        assert!(!config_exists(id).await);
        app.update_configs(&[], &[]);
        assert_eq!(app.state, AppState::Normal);
    }

    async fn searching_for_gamma() -> (App, Option<i64>) {
        let mut app = app_with_saved_configs(&["alpha", "beta", "gamma"]).await;
        app.search_query = "gamma".to_string();
        app.update_filtered_configs();
        app.table_state_stopped.select(Some(0));
        let highlighted = app.selected_config().and_then(|c| c.id);
        (app, highlighted)
    }

    #[tokio::test]
    async fn http_logs_config_hotkey_while_searching_targets_the_highlighted_config() {
        let _db = kftray_commons::test_utils::test_db().await;
        let (mut app, highlighted) = searching_for_gamma().await;

        handle_stopped_table_input(&mut app, KeyCode::Char('L'), DatabaseMode::Memory)
            .await
            .unwrap();

        assert_eq!(app.http_logs_config_id, highlighted);
    }

    #[tokio::test]
    async fn http_logs_toggle_hotkey_while_searching_targets_the_highlighted_config() {
        let _db = kftray_commons::test_utils::test_db().await;
        let (mut app, highlighted) = searching_for_gamma().await;

        handle_stopped_table_input(&mut app, KeyCode::Char('l'), DatabaseMode::Memory)
            .await
            .unwrap();

        let enabled: Vec<i64> = app
            .http_logs_enabled
            .iter()
            .filter_map(|(id, on)| on.then_some(*id))
            .collect();
        assert_eq!(enabled, vec![highlighted.unwrap()]);
    }

    #[tokio::test]
    async fn http_logs_config_refuses_a_max_file_size_that_import_rejects() {
        use kftray_commons::utils::http_logs_config::get_http_logs_config_with_mode;

        let _db = kftray_commons::test_utils::test_db().await;
        let (mut app, highlighted) = searching_for_gamma().await;
        let id = highlighted.unwrap();
        handle_stopped_table_input(&mut app, KeyCode::Char('L'), DatabaseMode::Memory)
            .await
            .unwrap();
        let before = get_http_logs_config_with_mode(id, DatabaseMode::Memory)
            .await
            .unwrap()
            .max_file_size;

        let mut keys = vec![KeyCode::Down, KeyCode::Enter];
        keys.extend(
            app.http_logs_config_max_file_size_input
                .chars()
                .map(|_| KeyCode::Backspace),
        );
        keys.extend("101".chars().map(KeyCode::Char));
        keys.push(KeyCode::Enter);
        for key in keys {
            handle_http_logs_config_input(&mut app, key, DatabaseMode::Memory)
                .await
                .unwrap();
        }

        let saved = get_http_logs_config_with_mode(id, DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(saved.max_file_size, before);
        assert_eq!(app.state, AppState::ShowErrorPopup);
        assert_eq!(
            app.error_message.as_deref(),
            Some("Max file size must be between 1 and 100 MB")
        );
    }

    #[tokio::test]
    async fn select_all_while_searching_only_deletes_matching_configs() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mut app = app_with_saved_configs(&["db", "api-dev", "api-prod"]).await;
        app.search_query = "api".to_string();
        app.update_filtered_configs();
        toggle_select_all(&mut app);

        let remaining = confirm_delete(&mut app).await;

        assert_eq!(remaining, ["db"]);
    }

    #[test]
    fn select_all_while_searching_toggles_off_after_refresh() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Stopped;
        app.search_query = "alias-2".to_string();
        app.update_filtered_configs();

        toggle_select_all(&mut app);
        assert_eq!(app.selected_rows_stopped, [0].into());

        app.update_filtered_configs();
        toggle_select_all(&mut app);
        assert!(app.selected_rows_stopped.is_empty());
    }

    #[tokio::test]
    async fn reopening_search_drops_rows_marked_in_the_filtered_list() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Stopped;
        app.active_component = ActiveComponent::StoppedTable;
        app.search_query = "alias-2".to_string();
        app.update_filtered_configs();
        app.table_state_stopped.select(Some(0));
        toggle_row_selection(&mut app);

        handle_normal_input(&mut app, KeyCode::Char('/'), DatabaseMode::Memory)
            .await
            .unwrap();
        handle_search_input(&mut app, KeyCode::Enter).unwrap();
        show_delete_confirmation(&mut app);

        assert!(app.selected_rows_stopped.is_empty());
        assert_eq!(app.state, AppState::Normal);
    }

    #[tokio::test]
    async fn test_handle_context_selection_input() {
        let mut app = setup_app();
        app.state = AppState::ShowContextSelection;
        app.contexts = vec!["context1".to_string(), "context2".to_string()];
        app.selected_context_index = 0;
        app.context_list_state.select(Some(0));

        handle_context_selection_input(&mut app, KeyCode::Down, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.selected_context_index, 1);
        assert_eq!(app.context_list_state.selected(), Some(1));

        handle_context_selection_input(&mut app, KeyCode::Up, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.selected_context_index, 0);
        assert_eq!(app.context_list_state.selected(), Some(0));

        handle_context_selection_input(&mut app, KeyCode::Enter, DatabaseMode::File)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_handle_normal_input() {
        let mut app = setup_app();
        app.state = AppState::Normal;

        app.active_component = ActiveComponent::Menu;
        handle_normal_input(&mut app, KeyCode::Tab, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::SearchBar);

        handle_normal_input(&mut app, KeyCode::Tab, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);

        handle_normal_input(&mut app, KeyCode::Tab, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::Details);

        handle_normal_input(&mut app, KeyCode::Tab, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::Menu);
    }

    #[tokio::test]
    async fn test_handle_menu_input() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mut app = setup_app();
        app.state = AppState::Normal;
        app.active_component = ActiveComponent::Menu;
        app.selected_menu_item = 2;

        handle_menu_input(&mut app, KeyCode::Left, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.selected_menu_item, 1);

        handle_menu_input(&mut app, KeyCode::Right, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.selected_menu_item, 2);

        handle_menu_input(&mut app, KeyCode::Down, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::SearchBar);

        app.active_component = ActiveComponent::Menu;
        app.selected_menu_item = 0;
        handle_menu_input(&mut app, KeyCode::Enter, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::ShowHelp);

        app.state = AppState::Normal;
        app.active_component = ActiveComponent::Menu;
        app.selected_menu_item = 4;
        handle_menu_input(&mut app, KeyCode::Enter, DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::ShowSettings);

        app.state = AppState::Normal;
        app.active_component = ActiveComponent::Menu;
        app.selected_menu_item = 5;
        handle_menu_input(&mut app, KeyCode::Enter, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::ShowAbout);
    }

    #[tokio::test]
    async fn settings_stay_closed_when_the_crash_reports_setting_cannot_be_read() {
        let _db = kftray_commons::test_utils::test_db().await;
        let context =
            kftray_commons::utils::db_mode::DatabaseManager::get_context(DatabaseMode::Memory)
                .await
                .unwrap();
        sqlx::query("DROP TABLE settings")
            .execute(&*context.pool)
            .await
            .unwrap();
        let mut app = setup_app();
        app.state = AppState::Normal;
        app.active_component = ActiveComponent::Menu;
        app.selected_menu_item = 4;

        handle_menu_input(&mut app, KeyCode::Enter, DatabaseMode::Memory)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::ShowErrorPopup);
        assert!(
            app.error_message
                .as_deref()
                .is_some_and(|message| message.contains("crash reports setting"))
        );
    }

    #[tokio::test]
    async fn the_consent_popup_stores_the_choice_and_closes() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mut app = setup_app();
        app.state = AppState::ShowTelemetryConsent;

        handle_telemetry_consent_input(&mut app, KeyCode::Right, DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(app.selected_telemetry_button, TelemetryButton::Decline);

        handle_telemetry_consent_input(&mut app, KeyCode::Enter, DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::Normal);
        assert!(!app.settings_telemetry_enabled);
        assert_eq!(
            kftray_commons::utils::settings::get_telemetry_enabled_with_mode(DatabaseMode::Memory)
                .await
                .unwrap(),
            Some(false)
        );
    }

    #[tokio::test]
    async fn the_performance_consent_popup_stores_only_the_performance_setting() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mut app = setup_app();
        app.state = AppState::ShowTelemetryConsent;
        app.active_consent = Consent::Performance;

        handle_telemetry_consent_input(&mut app, KeyCode::Enter, DatabaseMode::Memory)
            .await
            .unwrap();

        assert_eq!(
            (
                kftray_commons::utils::settings::get_performance_enabled_with_mode(
                    DatabaseMode::Memory
                )
                .await
                .unwrap(),
                kftray_commons::utils::settings::get_telemetry_enabled_with_mode(
                    DatabaseMode::Memory
                )
                .await
                .unwrap(),
            ),
            (Some(true), None)
        );
    }

    #[tokio::test]
    async fn answering_a_consent_popup_selects_allow_for_the_next_one() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mut app = setup_app();
        app.state = AppState::ShowTelemetryConsent;
        app.selected_telemetry_button = TelemetryButton::Decline;

        handle_telemetry_consent_input(&mut app, KeyCode::Enter, DatabaseMode::Memory)
            .await
            .unwrap();

        assert_eq!(app.selected_telemetry_button, TelemetryButton::Allow);
    }

    #[tokio::test]
    async fn closing_the_consent_popup_counts_as_no() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mut app = setup_app();
        app.state = AppState::ShowTelemetryConsent;

        handle_telemetry_consent_input(&mut app, KeyCode::Esc, DatabaseMode::Memory)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::Normal);
        assert_eq!(
            kftray_commons::utils::settings::get_telemetry_enabled_with_mode(DatabaseMode::Memory)
                .await
                .unwrap(),
            Some(false)
        );
    }

    #[tokio::test]
    async fn test_handle_stopped_table_input() {
        let mut app = setup_app();
        app.state = AppState::Normal;
        app.active_component = ActiveComponent::StoppedTable;
        app.active_table = ActiveTable::Stopped;
        app.table_state_stopped.select(Some(1));
        app.selected_row_stopped = 1;

        app.selected_rows_stopped.clear();
        handle_stopped_table_input(&mut app, KeyCode::Char('a'), DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.selected_rows_stopped.len(), 3);

        app.selected_rows_stopped.clear();
        handle_stopped_table_input(&mut app, KeyCode::Char(' '), DatabaseMode::File)
            .await
            .unwrap();
        assert!(app.selected_rows_stopped.contains(&1));

        handle_stopped_table_input(&mut app, KeyCode::Right, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::RunningTable);

        app.active_component = ActiveComponent::StoppedTable;
        app.table_state_stopped.select(Some(0));
        handle_stopped_table_input(&mut app, KeyCode::Up, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::SearchBar);

        app.active_component = ActiveComponent::StoppedTable;
        app.selected_rows_stopped.insert(1);
        handle_stopped_table_input(&mut app, KeyCode::Char('d'), DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::ShowDeleteConfirmation);
    }

    #[tokio::test]
    async fn test_handle_running_table_input() {
        let mut app = setup_app();
        app.state = AppState::Normal;
        app.active_component = ActiveComponent::RunningTable;
        app.active_table = ActiveTable::Running;
        app.table_state_running.select(Some(0));
        app.selected_row_running = 0;

        app.selected_rows_running.clear();
        handle_running_table_input(&mut app, KeyCode::Char(' '), DatabaseMode::File)
            .await
            .unwrap();
        assert!(app.selected_rows_running.contains(&0));

        handle_running_table_input(&mut app, KeyCode::Left, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);

        app.active_component = ActiveComponent::RunningTable;
        app.table_state_running.select(Some(0));
        handle_running_table_input(&mut app, KeyCode::Up, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::SearchBar);

        app.active_component = ActiveComponent::RunningTable;
        app.table_state_running.select(Some(1));
        app.selected_row_running = 1;
        handle_running_table_input(&mut app, KeyCode::Down, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::Logs);
    }

    #[tokio::test]
    async fn test_handle_details_input() {
        let mut app = setup_app();
        app.state = AppState::Normal;
        app.active_component = ActiveComponent::Details;
        app.visible_rows = 2;

        handle_details_input(&mut app, KeyCode::Right, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::Logs);

        app.active_component = ActiveComponent::Details;
        handle_details_input(&mut app, KeyCode::Up, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);

        app.active_component = ActiveComponent::Details;
        app.details_scroll_offset = 5;
        app.details_scroll_max_offset = 10;

        handle_details_input(&mut app, KeyCode::PageUp, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.details_scroll_offset, 3);

        handle_details_input(&mut app, KeyCode::PageDown, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(app.details_scroll_offset, 5);
    }

    #[tokio::test]
    async fn test_handle_logs_input() {
        let mut app = setup_app();
        app.state = AppState::Normal;
        app.active_component = ActiveComponent::Logs;

        handle_logs_input(&mut app, KeyCode::Left).await.unwrap();
        assert_eq!(app.active_component, ActiveComponent::Details);

        app.active_component = ActiveComponent::Logs;
        handle_logs_input(&mut app, KeyCode::Up).await.unwrap();
        assert_eq!(app.active_component, ActiveComponent::RunningTable);

        app.active_component = ActiveComponent::Logs;
        handle_logs_input(&mut app, KeyCode::PageUp).await.unwrap();
        handle_logs_input(&mut app, KeyCode::PageDown)
            .await
            .unwrap();
        handle_logs_input(&mut app, KeyCode::Down).await.unwrap();
        handle_logs_input(&mut app, KeyCode::Char('+'))
            .await
            .unwrap();
        handle_logs_input(&mut app, KeyCode::Char('-'))
            .await
            .unwrap();
        handle_logs_input(&mut app, KeyCode::Char(' '))
            .await
            .unwrap();
        handle_logs_input(&mut app, KeyCode::Esc).await.unwrap();
        handle_logs_input(&mut app, KeyCode::Char('h'))
            .await
            .unwrap();
        handle_logs_input(&mut app, KeyCode::Char('f'))
            .await
            .unwrap();
    }

    #[test]
    fn test_show_delete_confirmation() {
        let mut app = setup_app();
        app.active_table = ActiveTable::Stopped;
        app.selected_rows_stopped.insert(1);

        show_delete_confirmation(&mut app);

        assert_eq!(app.state, AppState::ShowDeleteConfirmation);
        assert!(app.delete_confirmation_message.is_some());

        app.state = AppState::Normal;
        app.selected_rows_stopped.clear();

        show_delete_confirmation(&mut app);

        assert_eq!(app.state, AppState::Normal);
    }

    fn app_replaying(local_address: &str, local_port: u16) -> App {
        let mut app = App::new(test_logger_state());
        app.running_configs = vec![Config {
            id: Some(7),
            local_address: Some(local_address.to_string()),
            local_port: Some(local_port),
            ..Config::default()
        }];
        app.http_logs_viewer_config_id = Some(7);
        app.http_logs_detail_mode = true;
        app.http_logs_selected_entry = Some(HttpLogEntry {
            trace_id: "t1".into(),
            request_timestamp: String::new(),
            response_timestamp: None,
            method: "GET".into(),
            path: "/who".into(),
            status_code: None,
            duration_ms: None,
            request_headers: vec![],
            request_body: String::new(),
            response_headers: vec![],
            response_body: String::new(),
        });
        app
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn replay_goes_to_the_address_the_forward_listens_on() {
        use tokio::io::{
            AsyncReadExt,
            AsyncWriteExt,
        };
        use tokio::net::TcpListener;

        async fn answer(listener: TcpListener, body: &'static str) {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        }

        let forward = TcpListener::bind("127.0.0.2:0").await.unwrap();
        let port = forward.local_addr().unwrap().port();
        let other = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        tokio::spawn(answer(forward, "forward"));
        tokio::spawn(answer(other, "other-service"));

        let mut app = app_replaying("127.0.0.2", port);
        handle_http_logs_viewer_input(&mut app, KeyCode::Char('r'))
            .await
            .unwrap();
        finish_replay(&mut app).await;

        let replayed = app.http_logs_selected_entry.unwrap();
        assert_eq!(
            replayed.response_body, "forward",
            "{:?}",
            app.http_logs_replay_result
        );
    }

    async fn finish_replay(app: &mut App) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while app.http_logs_replay.is_some() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                app.poll_http_logs_replay().await;
            }
        })
        .await
        .expect("the replay did not finish in 10 s");
    }

    #[tokio::test]
    async fn replay_to_a_server_that_never_answers_leaves_keys_working() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut held = Vec::new();
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                held.push(socket);
            }
        });

        let mut app = app_replaying("127.0.0.1", port);
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            handle_http_logs_viewer_input(&mut app, KeyCode::Char('r')),
        )
        .await
        .expect("the R key waited for the replay to answer")
        .unwrap();
        let replay = app
            .http_logs_replay
            .as_ref()
            .expect("the replay runs in the background")
            .abort_handle();

        handle_http_logs_viewer_input(&mut app, KeyCode::Esc)
            .await
            .unwrap();

        assert!(!app.http_logs_detail_mode);
        assert!(app.http_logs_replay.is_none());
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !replay.is_finished() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Esc left the replay running");
    }

    async fn read_request(stream: &mut (impl tokio::io::AsyncRead + Unpin)) -> (String, Vec<u8>) {
        use tokio::io::AsyncReadExt;

        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            request.extend_from_slice(&buf[..n]);
            let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") else {
                assert!(n > 0, "the replay closed before its headers ended");
                continue;
            };
            let head = String::from_utf8_lossy(&request[..end]).to_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map_or(0, |value| value.trim().parse::<usize>().unwrap());
            if request.len() >= end + 4 + length || n == 0 {
                return (head, request[end + 4..].to_vec());
            }
        }
    }

    async fn replay_to_one_request(
        entry: HttpLogEntry, respond: fn(&str, &[u8]) -> Vec<u8>,
    ) -> HttpLogEntry {
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (head, body) = read_request(&mut socket).await;
            socket.write_all(&respond(&head, &body)).await.unwrap();
        });

        entry
            .replay(&format!("http://127.0.0.1:{port}"))
            .await
            .unwrap()
    }

    fn logged_request(method: &str, headers: &[&str], body: &str) -> HttpLogEntry {
        HttpLogEntry {
            trace_id: "t1".into(),
            request_timestamp: String::new(),
            response_timestamp: None,
            method: method.into(),
            path: "/api".into(),
            status_code: None,
            duration_ms: None,
            request_headers: headers.iter().map(|h| h.to_string()).collect(),
            request_body: body.into(),
            response_headers: vec![],
            response_body: String::new(),
        }
    }

    #[tokio::test]
    async fn replay_of_a_request_that_accepted_gzip_shows_a_readable_body() {
        use std::io::Write;

        let entry = logged_request("GET", &["Accept-Encoding: gzip, deflate, br"], "");
        let replayed = replay_to_one_request(entry, |head, _| {
            let plain = br#"{"hello":"replay"}"#;
            let (encoding, body) = if head.contains("accept-encoding: gzip") {
                let mut gzip =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                gzip.write_all(plain).unwrap();
                ("Content-Encoding: gzip\r\n", gzip.finish().unwrap())
            } else {
                ("", plain.to_vec())
            };
            let mut reply = format!(
                "HTTP/1.1 200 OK\r\n{encoding}Content-Length: {}\r\n\r\n",
                body.len()
            )
            .into_bytes();
            reply.extend(body);
            reply
        })
        .await;

        assert_eq!(replayed.response_body, r#"{"hello":"replay"}"#);
    }

    #[tokio::test]
    async fn replay_of_a_gzip_request_sends_the_logged_body_as_plain_text() {
        use std::io::Read;

        let entry = logged_request(
            "POST",
            &["Content-Type: application/json", "Content-Encoding: gzip"],
            r#"{"name":"decoded in the log"}"#,
        );
        let replayed = replay_to_one_request(entry, |head, body| {
            let readable = if head.contains("content-encoding: gzip") {
                let mut decoded = String::new();
                flate2::read::GzDecoder::new(body)
                    .read_to_string(&mut decoded)
                    .ok()
                    .map(|_| decoded)
            } else {
                String::from_utf8(body.to_vec()).ok()
            };
            let (status, text) = match readable {
                Some(text) => ("200 OK", text),
                None => ("400 Bad Request", "bad gzip".to_string()),
            };
            format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{text}",
                text.len()
            )
            .into_bytes()
        })
        .await;

        assert_eq!(replayed.status_code.as_deref(), Some("200"));
        assert_eq!(replayed.response_body, r#"{"name":"decoded in the log"}"#);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn replay_of_a_post_on_an_https_forward_sends_the_post_and_its_body() {
        use std::sync::Arc;

        use kftray_commons::test_utils::EnvVarGuard;
        use tokio::io::AsyncWriteExt;

        let dir = tempfile::tempdir().unwrap();
        let _test_mode = EnvVarGuard::set("KFTRAY_TEST_MODE", "1");
        *kftray_portforward::ssl::cert_store::TEST_SSL_VAULT
            .lock()
            .unwrap() = Default::default();
        let cert =
            kftray_portforward::ssl::cert_generator::CertificateGenerator::for_testing(dir.path())
                .generate_for_alias("web")
                .await
                .unwrap();
        let ca = dir.path().join("test-ca.crt");
        let _trusted_ca = EnvVarGuard::set("SSL_CERT_FILE", ca.to_str().unwrap());
        let _no_other_roots = EnvVarGuard::set("SSL_CERT_DIR", dir.path().to_str().unwrap());
        let tls = tokio_rustls::TlsAcceptor::from(Arc::new(
            tokio_rustls::rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(cert.certificate, cert.private_key)
                .unwrap(),
        ));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut first_byte = [0u8; 1];
                socket.peek(&mut first_byte).await.unwrap();
                if first_byte[0] != 0x16 {
                    let (head, _) = read_request(&mut socket).await;
                    let path = head.split_whitespace().nth(1).unwrap().to_string();
                    let redirect = format!(
                        "HTTP/1.1 301 Moved Permanently\r\nLocation: \
                         https://127.0.0.1:{port}{path}\r\nContent-Length: 0\r\n\r\n"
                    );
                    socket.write_all(redirect.as_bytes()).await.unwrap();
                    continue;
                }
                let mut socket = tls.accept(socket).await.unwrap();
                let (head, body) = read_request(&mut socket).await;
                let received = format!(
                    "{} {}",
                    head.lines().next().unwrap(),
                    String::from_utf8_lossy(&body)
                );
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{received}",
                    received.len()
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
                socket.flush().await.unwrap();
            }
        });

        let mut app = app_replaying("127.0.0.1", port);
        app.http_logs_selected_entry = Some(logged_request(
            "POST",
            &["Content-Type: text/plain"],
            "payload=original",
        ));
        handle_http_logs_viewer_input(&mut app, KeyCode::Char('r'))
            .await
            .unwrap();
        finish_replay(&mut app).await;

        let replayed = app.http_logs_selected_entry.unwrap();
        assert_eq!(
            replayed.response_body, "post /api http/1.1 payload=original",
            "{:?}",
            app.http_logs_replay_result
        );
    }
}
