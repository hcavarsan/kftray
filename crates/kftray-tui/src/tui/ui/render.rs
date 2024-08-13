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
use crate::tui::ui::render_details;
use crate::tui::ui::{
    draw_main_tab,
    BASE,
    MAUVE,
    TEXT,
    YELLOW,
};

pub fn render_legend(f: &mut Frame, area: Rect) {
    let legend_message = vec![Line::from(vec![
        Span::styled("CtrlC: Quit", Style::default().fg(YELLOW)),
        Span::raw(" | "),
        Span::styled("Tab: Toggle Menu", Style::default().fg(YELLOW)),
        Span::raw(" | "),
        Span::styled("h: Toggle Help", Style::default().fg(YELLOW)),
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
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    let vertical_center = popup_layout[1];

    let horizontal_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
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

pub fn draw_configs_tab(f: &mut Frame, app: &mut App, config_states: &[ConfigState], area: Rect) {
    let table_height = app.visible_rows as u16;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(table_height), Constraint::Min(table_height)].as_ref())
        .split(area);

    draw_main_tab(f, app, config_states, chunks[0]);

    if !app.stopped_configs.is_empty() || !app.running_configs.is_empty() {
        let selected_row = match app.active_table {
            ActiveTable::Stopped => app.selected_row_stopped,
            ActiveTable::Running => app.selected_row_running,
        };
        let configs = match app.active_table {
            ActiveTable::Stopped => &app.stopped_configs,
            ActiveTable::Running => &app.running_configs,
        };

        if !configs.is_empty() && selected_row < configs.len() {
            render_details(f, &configs[selected_row], config_states, chunks[1]);
        }
    }
}
