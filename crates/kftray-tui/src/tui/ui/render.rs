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
        Line,
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

use crate::tui::input::{
    ActiveTable,
    App,
};
use crate::tui::ui::{
    draw_main_tab,
    render_details,
};
use crate::tui::ui::{
    BASE,
    TEXT,
    YELLOW,
};

pub fn render_legend(f: &mut Frame, area: Rect) {
    let legend_message = vec![Line::from(vec![
        Span::styled("CtrlC: Quit", Style::default().fg(YELLOW)),
        Span::raw(" | "),
        Span::styled("↑/↓: Navigate", Style::default().fg(YELLOW)),
        Span::raw(" | "),
        Span::styled("←/→: Switch Table", Style::default().fg(YELLOW)),
        Span::raw(" | "),
        Span::styled("f: Start/Stop Port Forward", Style::default().fg(YELLOW)),
        Span::raw(" | "),
        Span::styled("Space: Select/Deselect", Style::default().fg(YELLOW)),
        Span::raw(" | "),
        Span::styled("h: Toggle Help", Style::default().fg(YELLOW)),
        Span::raw(" | "),
        Span::styled("i: Import", Style::default().fg(YELLOW)),
        Span::raw(" | "),
        Span::styled("e: Export", Style::default().fg(YELLOW)),
    ])];

    let legend_paragraph = Paragraph::new(Text::from(legend_message))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .style(Style::default().fg(TEXT).bg(BASE));

    f.render_widget(legend_paragraph, area);
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints(
            [
                ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
                ratatui::layout::Constraint::Percentage(percent_y),
                ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    let vertical_center = popup_layout[1];

    let horizontal_layout = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints(
            [
                ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
                ratatui::layout::Constraint::Percentage(percent_x),
                ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(vertical_center);

    horizontal_layout[1]
}

pub fn draw_file_explorer_popup(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(area);

    let file_explorer_widget = app.file_explorer.widget();
    let file_explorer_block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().bg(BASE).fg(TEXT));
    f.render_widget(Clear, area);
    f.render_widget(file_explorer_block, area);
    f.render_widget(&file_explorer_widget, chunks[0]);

    if let Some(content) = &app.file_content {
        let file_content_widget = Paragraph::new(content.as_str())
            .block(Block::default().borders(Borders::ALL).title("File Preview"))
            .style(Style::default().fg(TEXT).bg(BASE));
        f.render_widget(file_content_widget, chunks[1]);
    }
}

pub fn draw_configs_tab(f: &mut Frame, app: &mut App, config_states: &[ConfigState], area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Min(0),
                Constraint::Length(10),
            ]
            .as_ref(),
        )
        .split(area);

    draw_main_tab(f, app, config_states, chunks[0]);

    if !app.filtered_stopped_configs.is_empty() || !app.filtered_running_configs.is_empty() {
        let selected_row = match app.active_table {
            ActiveTable::Stopped => app.selected_row_stopped,
            ActiveTable::Running => app.selected_row_running,
        };
        let configs = match app.active_table {
            ActiveTable::Stopped => &app.filtered_stopped_configs,
            ActiveTable::Running => &app.filtered_running_configs,
        };

        if !configs.is_empty() && selected_row < configs.len() {
            render_details(f, &configs[selected_row], config_states, chunks[1]);
        }
    }
}
