use std::collections::HashSet;

use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{
    Constraint,
    Rect,
};
use ratatui::style::{
    Color,
    Style,
};
use ratatui::widgets::{
    Block,
    Borders,
    Cell,
    Row,
    Table,
    TableState,
};

use crate::tests::test_logger_state;
use crate::tui::input::{
    ActiveComponent,
    App,
};
use crate::tui::ui::render::{
    centered_rect,
    draw_configs_tab,
    draw_file_explorer_popup,
    render_legend,
};
use crate::tui::ui::table::{
    draw_configs_table,
    render_details,
    style_bold,
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
        app
    }

    #[test]
    fn test_table_creation() {
        let sample_config = create_test_config();
        assert_eq!(sample_config.id, Some(1));

        let widths = [
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(15),
            Constraint::Length(15),
            Constraint::Length(15),
            Constraint::Length(15),
            Constraint::Length(15),
            Constraint::Length(8),
        ];

        let header_cells = [
            "Selected",
            "ID",
            "Service",
            "Namespace",
            "Local Port",
            "Remote Port",
            "Context",
            "Protocol",
        ]
        .iter()
        .map(|h| Cell::from(*h));

        let header = Row::new(header_cells).style(Style::default().fg(Color::White));

        let table = Table::new(Vec::<Row>::new(), widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL))
            .row_highlight_style(Style::default());

        assert!(matches!(table, Table { .. }));
    }

    #[test]
    fn test_draw_configs_table() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 50);
                let config = create_test_config();
                let config_state = create_test_config_state();
                let mut table_state = TableState::default();
                let selected_rows = HashSet::new();

                draw_configs_table(
                    frame,
                    area,
                    &[config],
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

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn test_render_details() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 50);
                let config = create_test_config();
                let config_state = create_test_config_state();
                let mut app = create_test_app();

                render_details(frame, &mut app, &config, &[config_state], area, true);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn test_style_bold() {
        let style = style_bold();
        assert_ne!(style, Style::default());
    }

    #[test]
    fn test_render_legend() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();

        let components = [
            ActiveComponent::Menu,
            ActiveComponent::SearchBar,
            ActiveComponent::StoppedTable,
            ActiveComponent::RunningTable,
            ActiveComponent::Details,
            ActiveComponent::Logs,
        ];

        for width in 5..60 {
            for component in components {
                let area = Rect::new(0, 0, width, 3);
                terminal
                    .draw(|frame| {
                        render_legend(frame, area, component);
                    })
                    .unwrap();
            }
        }
    }

    #[test]
    fn test_centered_rect() {
        let area = Rect::new(0, 0, 100, 50);
        let centered = centered_rect(80, 80, area);

        assert!(centered.width < area.width);
        assert!(centered.height < area.height);
        assert!(centered.x > area.x);
        assert!(centered.y > area.y);
    }

    #[test]
    fn test_draw_file_explorer_popup() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 50);
                draw_file_explorer_popup(frame, &mut app, area, true);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 50);
                draw_file_explorer_popup(frame, &mut app, area, false);
            })
            .unwrap();
    }

    #[test]
    fn test_draw_configs_tab() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        let config = create_test_config();
        let config_state = create_test_config_state();

        app.stopped_configs.push(config.clone());
        app.running_configs.push(config);

        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 50);
                draw_configs_tab(frame, &mut app, &[config_state], area, true);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }
}
