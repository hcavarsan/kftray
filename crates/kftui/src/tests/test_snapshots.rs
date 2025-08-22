use std::collections::HashSet;

use insta::{
    assert_snapshot,
    Settings,
};
use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
use ratatui::backend::TestBackend;
use ratatui::prelude::Widget;
use ratatui::Terminal;

use crate::tests::test_logger_state;
use crate::tui::input::{
    ActiveComponent,
    ActiveTable,
    App,
    AppState,
    DeleteButton,
};
use crate::tui::ui::draw::{
    draw_header,
    draw_ui,
    render_logs,
};
use crate::tui::ui::popup::{
    render_about_popup,
    render_background_overlay,
    render_confirmation_popup,
    render_delete_confirmation_popup,
    render_error_popup,
    render_help_popup,
    render_input_prompt,
};
use crate::tui::ui::render::{
    centered_rect,
    render_legend,
};
use crate::tui::ui::table::{
    draw_configs_table,
    render_details,
};

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

    // Add some test data
    app.file_content = Some("test file content".to_string());
    app.stopped_configs.push(create_test_config());
    app.running_configs.push(create_test_config());
    app.contexts = vec!["context1".to_string(), "context2".to_string()];
    app.error_message = None;
    app.import_export_message = Some("Test confirmation message".to_string());
    app.delete_confirmation_message = Some("Delete confirmation test".to_string());
    app.input_buffer = "test-input".to_string();

    app
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a standard terminal for testing
    fn setup_terminal() -> Terminal<TestBackend> {
        // Create a standard terminal size that's large enough to render all UI elements
        let backend = TestBackend::new(100, 30);
        Terminal::new(backend).unwrap()
    }

    // Use insta settings to ensure deterministic snapshots
    fn setup_snapshot() -> Settings {
        let mut settings = Settings::clone_current();
        settings.set_snapshot_path("src/tests/snapshots");
        settings
    }

    #[test]
    fn test_draw_ui_normal() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::Normal;

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_draw_ui_help_popup() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::ShowHelp;

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_draw_ui_about_popup() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::ShowAbout;

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_draw_ui_import_file_explorer() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::ImportFileExplorerOpen;

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_draw_ui_export_file_explorer() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::ExportFileExplorerOpen;

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_draw_ui_input_prompt() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::ShowInputPrompt;

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_draw_ui_confirmation_popup() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::ShowConfirmationPopup;

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_draw_ui_error_popup() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::ShowErrorPopup;
        app.error_message = Some("Test error message".to_string());

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_draw_ui_delete_confirmation() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::ShowDeleteConfirmation;

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_draw_ui_context_selection() {
        let _settings = setup_snapshot();
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.state = AppState::ShowContextSelection;

        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn test_different_active_components() {
        let _settings = setup_snapshot();

        // Test with Menu active
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.active_component = ActiveComponent::Menu;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();
        assert_snapshot!("draw_ui_menu_active", terminal.backend());

        // Test with StoppedTable active
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.active_component = ActiveComponent::StoppedTable;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();
        assert_snapshot!("draw_ui_stopped_table_active", terminal.backend());

        // Test with RunningTable active
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.active_component = ActiveComponent::RunningTable;
        app.active_table = ActiveTable::Running;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();
        assert_snapshot!("draw_ui_running_table_active", terminal.backend());

        // Test with Details active
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.active_component = ActiveComponent::Details;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();
        assert_snapshot!("draw_ui_details_active", terminal.backend());

        // Test with Logs active
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        app.active_component = ActiveComponent::Logs;
        terminal
            .draw(|frame| {
                draw_ui(frame, &mut app, &[create_test_config_state()]);
            })
            .unwrap();
        assert_snapshot!("draw_ui_logs_active", terminal.backend());
    }

    #[test]
    fn test_individual_components() {
        let _settings = setup_snapshot();

        // Test render_logs
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_logs(frame, &mut app, area, true);
            })
            .unwrap();
        assert_snapshot!("render_logs_focused", terminal.backend());

        // Test render_logs unfocused
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_logs(frame, &mut app, area, false);
            })
            .unwrap();
        assert_snapshot!("render_logs_unfocused", terminal.backend());

        // Test draw_header
        let mut terminal = setup_terminal();
        let app = create_test_app();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_header(frame, &app, area);
            })
            .unwrap();
        assert_snapshot!("draw_header", terminal.backend());

        // Test centered_rect
        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let centered = centered_rect(80, 80, area);
                // Draw a block in the centered rect to make it visible
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title("Centered Rect")
                    .render(centered, frame.buffer_mut());
            })
            .unwrap();
        assert_snapshot!("centered_rect", terminal.backend());

        // Test render_legend
        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_legend(frame, area, ActiveComponent::Menu);
            })
            .unwrap();
        assert_snapshot!("render_legend", terminal.backend());
    }

    #[test]
    fn test_popups() {
        let _settings = setup_snapshot();

        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_background_overlay(frame, area);
            })
            .unwrap();
        assert_snapshot!("render_background_overlay", terminal.backend());

        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let input_buffer = "test input";
                render_input_prompt(frame, input_buffer, area);
            })
            .unwrap();
        assert_snapshot!("render_input_prompt", terminal.backend());

        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let message = Some("Confirmation test".to_string());
                render_confirmation_popup(frame, &message, area);
            })
            .unwrap();
        assert_snapshot!("render_confirmation_popup", terminal.backend());

        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_help_popup(frame, area);
            })
            .unwrap();
        assert_snapshot!("render_help_popup", terminal.backend());

        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_about_popup(frame, area);
            })
            .unwrap();
        assert_snapshot!("render_about_popup", terminal.backend());

        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let error_message = "This is an error test message";
                render_error_popup(frame, error_message, area, 2);
            })
            .unwrap();
        assert_snapshot!("render_error_popup", terminal.backend());

        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let message = Some("Delete confirmation test".to_string());
                render_delete_confirmation_popup(frame, &message, area, DeleteButton::Confirm);
            })
            .unwrap();
        assert_snapshot!(
            "render_delete_confirmation_popup_confirm",
            terminal.backend()
        );

        let mut terminal = setup_terminal();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let message = Some("Delete confirmation test".to_string());
                render_delete_confirmation_popup(frame, &message, area, DeleteButton::Close);
            })
            .unwrap();
        assert_snapshot!("render_delete_confirmation_popup_close", terminal.backend());
    }

    #[test]
    fn test_tables() {
        let _settings = setup_snapshot();

        let mut terminal = setup_terminal();
        let config = create_test_config();
        let config_state = create_test_config_state();
        let mut table_state = ratatui::widgets::TableState::default();
        let selected_rows = HashSet::new();

        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_configs_table(
                    frame,
                    area,
                    &vec![config],
                    &[config_state],
                    &mut table_state,
                    "Test Table",
                    true,
                    &selected_rows,
                    &std::collections::HashMap::new(),
                    &throbber_widgets_tui::ThrobberState::default(),
                );
            })
            .unwrap();
        assert_snapshot!("draw_configs_table", terminal.backend());

        // Test render_details
        let mut terminal = setup_terminal();
        let mut app = create_test_app();
        let config = create_test_config();
        let config_state = create_test_config_state();

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_details(frame, &mut app, &config, &[config_state], area, true);
            })
            .unwrap();
        assert_snapshot!("render_details", terminal.backend());
    }
}
