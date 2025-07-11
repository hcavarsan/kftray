use crate::tui::input::{
    clear_selection,
    handle_about_input,
    handle_confirmation_popup_input,
    handle_context_selection_input,
    handle_delete_confirmation_input,
    handle_details_input,
    handle_error_popup_input,
    handle_help_input,
    handle_logs_input,
    handle_menu_input,
    handle_normal_input,
    handle_running_table_input,
    handle_stopped_table_input,
    select_first_row,
    show_delete_confirmation,
    toggle_row_selection,
    toggle_select_all,
    ActiveComponent,
    ActiveTable,
    App,
    AppState,
    DeleteButton,
};

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;
    use kftray_commons::models::config_model::Config;

    use super::*;

    fn create_test_configs(count: usize) -> Vec<Config> {
        (1..=count)
            .map(|i| Config {
                id: Some(i as i64),
                service: Some(format!("service-{i}")),
                namespace: "default".to_string(),
                local_port: Some(8080 + (i as u16)),
                remote_port: Some(80),
                context: Some(format!("test-context-{i}")),
                workload_type: Some("service".to_string()),
                protocol: "tcp".to_string(),
                remote_address: Some(format!("remote-{i}")),
                local_address: Some("127.0.0.1".to_string()),
                auto_loopback_address: false,
                alias: Some(format!("alias-{i}")),
                domain_enabled: Some(false),
                kubeconfig: None,
                target: Some(format!("target-{i}")),
            })
            .collect()
    }

    fn setup_app() -> App {
        let mut app = App::new();
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
        let mut app = App::new();
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
    fn test_app_default() {
        let app = App::default();
        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);
    }

    #[tokio::test]
    async fn test_handle_error_popup_input() {
        let mut app = setup_app();
        app.state = AppState::ShowErrorPopup;

        handle_error_popup_input(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowErrorPopup;
        handle_error_popup_input(&mut app, KeyCode::Enter).unwrap();
        assert_eq!(app.state, AppState::Normal);
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

        handle_delete_confirmation_input(&mut app, KeyCode::Left)
            .await
            .unwrap();
        assert_eq!(app.selected_delete_button, DeleteButton::Confirm);

        handle_delete_confirmation_input(&mut app, KeyCode::Right)
            .await
            .unwrap();
        assert_eq!(app.selected_delete_button, DeleteButton::Close);

        handle_delete_confirmation_input(&mut app, KeyCode::Esc)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowDeleteConfirmation;
        app.selected_delete_button = DeleteButton::Close;
        handle_delete_confirmation_input(&mut app, KeyCode::Enter)
            .await
            .unwrap();
        assert_eq!(app.state, AppState::Normal);
    }

    #[tokio::test]
    async fn test_handle_context_selection_input() {
        let mut app = setup_app();
        app.state = AppState::ShowContextSelection;
        app.contexts = vec!["context1".to_string(), "context2".to_string()];
        app.selected_context_index = 0;
        app.context_list_state.select(Some(0));

        handle_context_selection_input(&mut app, KeyCode::Down)
            .await
            .unwrap();
        assert_eq!(app.selected_context_index, 1);
        assert_eq!(app.context_list_state.selected(), Some(1));

        handle_context_selection_input(&mut app, KeyCode::Up)
            .await
            .unwrap();
        assert_eq!(app.selected_context_index, 0);
        assert_eq!(app.context_list_state.selected(), Some(0));

        handle_context_selection_input(&mut app, KeyCode::Enter)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_handle_normal_input() {
        let mut app = setup_app();
        app.state = AppState::Normal;

        app.active_component = ActiveComponent::Menu;
        handle_normal_input(&mut app, KeyCode::Tab).await.unwrap();
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);

        handle_normal_input(&mut app, KeyCode::Tab).await.unwrap();
        assert_eq!(app.active_component, ActiveComponent::Details);

        handle_normal_input(&mut app, KeyCode::Tab).await.unwrap();
        assert_eq!(app.active_component, ActiveComponent::Menu);
    }

    #[tokio::test]
    async fn test_handle_menu_input() {
        let mut app = setup_app();
        app.state = AppState::Normal;
        app.active_component = ActiveComponent::Menu;
        app.selected_menu_item = 2;

        handle_menu_input(&mut app, KeyCode::Left).await.unwrap();
        assert_eq!(app.selected_menu_item, 1);

        handle_menu_input(&mut app, KeyCode::Right).await.unwrap();
        assert_eq!(app.selected_menu_item, 2);

        handle_menu_input(&mut app, KeyCode::Down).await.unwrap();
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);

        app.active_component = ActiveComponent::Menu;
        app.selected_menu_item = 0;
        handle_menu_input(&mut app, KeyCode::Enter).await.unwrap();
        assert_eq!(app.state, AppState::ShowHelp);

        app.state = AppState::Normal;
        app.active_component = ActiveComponent::Menu;
        app.selected_menu_item = 4;
        handle_menu_input(&mut app, KeyCode::Enter).await.unwrap();
        assert_eq!(app.state, AppState::ShowSettings);

        app.state = AppState::Normal;
        app.active_component = ActiveComponent::Menu;
        app.selected_menu_item = 5;
        handle_menu_input(&mut app, KeyCode::Enter).await.unwrap();
        assert_eq!(app.state, AppState::ShowAbout);
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
        handle_stopped_table_input(&mut app, KeyCode::Char('a'))
            .await
            .unwrap();
        assert_eq!(app.selected_rows_stopped.len(), 3);

        app.selected_rows_stopped.clear();
        handle_stopped_table_input(&mut app, KeyCode::Char(' '))
            .await
            .unwrap();
        assert!(app.selected_rows_stopped.contains(&1));

        handle_stopped_table_input(&mut app, KeyCode::Right)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::RunningTable);

        app.active_component = ActiveComponent::StoppedTable;
        app.table_state_stopped.select(Some(0));
        handle_stopped_table_input(&mut app, KeyCode::Up)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::Menu);

        app.active_component = ActiveComponent::StoppedTable;
        app.selected_rows_stopped.insert(1);
        handle_stopped_table_input(&mut app, KeyCode::Char('d'))
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
        handle_running_table_input(&mut app, KeyCode::Char(' '))
            .await
            .unwrap();
        assert!(app.selected_rows_running.contains(&0));

        handle_running_table_input(&mut app, KeyCode::Left)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);

        app.active_component = ActiveComponent::RunningTable;
        app.table_state_running.select(Some(0));
        handle_running_table_input(&mut app, KeyCode::Up)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::Menu);

        app.active_component = ActiveComponent::RunningTable;
        app.table_state_running.select(Some(1));
        app.selected_row_running = 1;
        handle_running_table_input(&mut app, KeyCode::Down)
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

        handle_details_input(&mut app, KeyCode::Right)
            .await
            .unwrap();
        assert_eq!(app.active_component, ActiveComponent::Logs);

        app.active_component = ActiveComponent::Details;
        handle_details_input(&mut app, KeyCode::Up).await.unwrap();
        assert_eq!(app.active_component, ActiveComponent::StoppedTable);

        app.active_component = ActiveComponent::Details;
        app.details_scroll_offset = 5;
        app.details_scroll_max_offset = 10;

        handle_details_input(&mut app, KeyCode::PageUp)
            .await
            .unwrap();
        assert_eq!(app.details_scroll_offset, 3);

        handle_details_input(&mut app, KeyCode::PageDown)
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
}
