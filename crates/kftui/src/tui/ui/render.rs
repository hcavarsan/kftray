use kftray_commons::models::config_state_model::ConfigState;
use ratatui::{
    layout::{
        Constraint,
        Direction,
        Layout,
        Rect,
    },
    style::Style,
    text::{
        Span,
        Text,
    },
    widgets::{
        Block,
        Borders,
        Clear,
        Paragraph,
    },
    Frame,
};

use crate::tui::input::ActiveComponent;
use crate::tui::input::App;
use crate::tui::ui::draw_configs_table;
use crate::tui::ui::{
    BASE,
    MAUVE,
    TEXT,
    YELLOW,
};

pub fn render_legend(f: &mut Frame, area: Rect, active_component: ActiveComponent) {
    let common_legend = "Ctrl+C: Quit | h: Show Help";

    let menu_legend = "←/→: Navigate Menu | Enter: Select Menu Item | Tab: Switch to Configs Tab";

    let table_legend = "PageUp/PageDown: Scroll | ↑/↓: Navigate | ←/→: Switch Table | Space: Select  | f: Start/Stop Port Forward | d: Delete | Ctrl+A: Select All | Tab: Switch to Details";

    let details_legend = "PageUp/PageDown: Scroll | ←/→: Switch Details/Logs | Tab: Switch to Menu";

    let logs_legend = "PageUp/PageDown: Scroll | ←/→: Switch Focus | c: Clear Output";

    let legend_message = match active_component {
        ActiveComponent::Menu => format!("{} | {}", common_legend, menu_legend),
        ActiveComponent::StoppedTable | ActiveComponent::RunningTable => {
            format!("{} | {}", common_legend, table_legend)
        }
        ActiveComponent::Details => format!("{} | {}", common_legend, details_legend),
        ActiveComponent::Logs => format!("{} | {}", common_legend, logs_legend),
    };

    let styled_legend_message = Span::styled(legend_message, Style::default().fg(YELLOW));

    let legend_paragraph = Paragraph::new(Text::from(styled_legend_message))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Help", Style::default().fg(MAUVE)))
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .style(Style::default().fg(TEXT).bg(BASE));

    f.render_widget(legend_paragraph, area);
}

fn calculate_center_constraints(percent: u16) -> [Constraint; 3] {
    [
        Constraint::Percentage((100 - percent) / 2),
        Constraint::Percentage(percent),
        Constraint::Percentage((100 - percent) / 2),
    ]
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(calculate_center_constraints(percent_y).as_ref())
        .split(r);

    let vertical_center = popup_layout[1];

    let horizontal_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(calculate_center_constraints(percent_x).as_ref())
        .split(vertical_center);

    horizontal_layout[1]
}
pub fn draw_file_explorer_popup(f: &mut Frame, app: &mut App, area: Rect, is_import: bool) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(90), Constraint::Percentage(10)].as_ref())
        .split(area);

    let file_explorer_block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().bg(BASE).fg(TEXT))
        .title(Span::styled("File Explorer", Style::default().fg(MAUVE)));

    f.render_widget(Clear, area);
    f.render_widget(file_explorer_block, area);

    let upper_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[0]);

    let file_explorer_widget = if is_import {
        app.import_file_explorer.widget()
    } else {
        app.export_file_explorer.widget()
    };
    f.render_widget(&file_explorer_widget, upper_chunks[0]);

    f.render_widget(Clear, upper_chunks[1]);

    if let Some(content) = &app.file_content {
        let file_content_widget = Paragraph::new(content.as_str())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled("File Preview", Style::default().fg(MAUVE))),
            )
            .style(Style::default().fg(TEXT).bg(BASE));
        f.render_widget(file_content_widget, upper_chunks[1]);
    } else {
        let empty_widget = Paragraph::new("")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled("File Preview", Style::default().fg(MAUVE))),
            )
            .style(Style::default().fg(TEXT).bg(BASE));
        f.render_widget(empty_widget, upper_chunks[1]);
    }

    let help_text = "↑↓: Navigate  ESC: Close  ENTER/SPACE: Select";
    let help_widget = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Help", Style::default().fg(MAUVE))),
        )
        .style(Style::default().fg(TEXT).bg(BASE));

    f.render_widget(help_widget, chunks[1]);
}

pub fn draw_configs_tab(
    f: &mut Frame, app: &mut App, config_states: &[ConfigState], area: Rect, has_focus: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(100)].as_ref())
        .split(area);

    let tables_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[0]);

    draw_configs_table(
        f,
        tables_chunks[0],
        &app.stopped_configs,
        config_states,
        &mut app.table_state_stopped,
        "Stopped Configs",
        has_focus && app.active_component == ActiveComponent::StoppedTable,
        &app.selected_rows_stopped,
    );

    draw_configs_table(
        f,
        tables_chunks[1],
        &app.running_configs,
        config_states,
        &mut app.table_state_running,
        "Running Configs",
        has_focus && app.active_component == ActiveComponent::RunningTable,
        &app.selected_rows_running,
    );
}
