use crate::tui::input::{
    clear_selection,
    select_first_row,
    toggle_row_selection,
    toggle_select_all,
    ActiveTable,
    App,
};

#[cfg(test)]
mod tests {
    use kftray_commons::models::config_model::Config;

    use super::*;
    use crate::tui::input::ActiveComponent;

    fn create_test_configs(count: usize) -> Vec<Config> {
        (1..=count)
            .map(|i| Config {
                id: Some(i as i64),
                service: Some(format!("service-{}", i)),
                namespace: "default".to_string(),
                local_port: Some(8080 + (i as u16)),
                remote_port: Some(80),
                context: format!("test-context-{}", i),
                workload_type: Some("service".to_string()),
                protocol: "tcp".to_string(),
                remote_address: Some(format!("remote-{}", i)),
                local_address: Some("127.0.0.1".to_string()),
                alias: Some(format!("alias-{}", i)),
                domain_enabled: Some(false),
                kubeconfig: None,
                target: Some(format!("target-{}", i)),
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
}
