use kftray_commons::models::{
    config_model::Config,
    config_state_model::ConfigState,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use crate::tests::test_logger_state;
use crate::tui::input::{
    ActiveComponent,
    ActiveTable,
    App,
    AppState,
};
use crate::tui::ui::draw::{
    draw_header,
    draw_ui,
    render_logs,
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
            context: Some("test-context".to_string()),
            workload_type: Some("service".to_string()),
            protocol: "tcp".to_string(),
            remote_address: Some("remote-address".to_string()),
            local_address: Some("127.0.0.1".to_string()),
            auto_loopback_address: false,
            alias: Some("test-alias".to_string()),
            domain_enabled: Some(false),
            kubeconfig: None,
            target: Some("test-target".to_string()),
            http_logs_enabled: Some(false),
            http_logs_max_file_size: Some(10 * 1024 * 1024),
            http_logs_retention_days: Some(7),
            http_logs_auto_cleanup: Some(true),
        }
    }

    fn create_test_config_state() -> ConfigState {
        ConfigState {
            id: Some(1),
            config_id: 1,
            is_running: true,
            process_id: Some(1234),
        }
    }

    fn create_test_app() -> App {
        let mut app = App::new(test_logger_state());
        app.file_content = Some("test content".to_string());
        app.stopped_configs = vec![create_test_config()];
        app
    }

    #[test]
    fn test_draw_ui() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        let config_state = create_test_config_state();

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[config_state]);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn test_draw_ui_with_different_states() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        let config_state = create_test_config_state();

        app.state = AppState::ShowHelp;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());

        app.state = AppState::ShowAbout;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.state = AppState::ImportFileExplorerOpen;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.state = AppState::ExportFileExplorerOpen;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.state = AppState::ShowInputPrompt;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.state = AppState::ShowConfirmationPopup;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.state = AppState::ShowErrorPopup;
        app.error_message = Some("Test error message".to_string());
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.state = AppState::ShowDeleteConfirmation;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.state = AppState::ShowContextSelection;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[config_state]);
            })
            .unwrap();
    }

    #[test]
    fn test_draw_ui_with_different_active_components() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        let config_state = create_test_config_state();

        app.active_component = ActiveComponent::Menu;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.active_component = ActiveComponent::StoppedTable;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.active_component = ActiveComponent::RunningTable;
        app.active_table = ActiveTable::Running;
        app.running_configs.push(create_test_config());
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.active_component = ActiveComponent::Details;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, std::slice::from_ref(&config_state));
            })
            .unwrap();

        app.active_component = ActiveComponent::Logs;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[config_state]);
            })
            .unwrap();
    }

    #[test]
    fn test_render_logs() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 30);
                render_logs(frame, &mut app, area, true);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 30);
                render_logs(frame, &mut app, area, false);
            })
            .unwrap();
    }

    #[test]
    fn test_draw_header() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = create_test_app();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 3);
                draw_header(frame, &app, area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());

        let mut menu_app = create_test_app();
        menu_app.active_component = ActiveComponent::Menu;

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 3);
                draw_header(frame, &menu_app, area);
            })
            .unwrap();
    }
}
