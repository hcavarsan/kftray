use kftray_commons::models::config_state_model::ConfigState;
use ratatui::prelude::Alignment;
use ratatui::{
    Frame,
    layout::{
        Constraint,
        Direction,
        Layout,
        Rect,
    },
    style::{
        Modifier,
        Style,
    },
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
};

use crate::tui::input::ActiveComponent;
use crate::tui::input::App;
use crate::tui::ui::draw_configs_table;
use crate::tui::ui::{
    BASE,
    GREEN,
    MAUVE,
    TEXT,
    YELLOW,
};

pub fn render_legend(f: &mut Frame, area: Rect, active_component: ActiveComponent) {
    let common_legend = "ctrlc: quit | h: help | s: settings";

    let menu_legend = "←/→: navigate | enter: open | tab: switch to configs tab";

    let table_legend = "pageup/down: scroll | ↑/↓: navigate | ←/→: switch table | space: select | f: start/stop | d: delete | l: http logs | L: logs config | ctrla: select all | tab: switch to details";

    let details_legend = "pageup/pagedown: scroll | ←/→: switch tabs | tab: switch to menu";

    let logs_legend = "pageup/pagedown: scroll | ←/→: switch focus | c: clear output";

    let search_legend = "↑/↓: navigate | enter/esc: exit search | type: filter configs";

    let legend_message = match active_component {
        ActiveComponent::Menu => format!("{common_legend} | {menu_legend}"),
        ActiveComponent::SearchBar => format!("{common_legend} | {search_legend}"),
        ActiveComponent::StoppedTable | ActiveComponent::RunningTable => {
            format!("{common_legend} | {table_legend}")
        }
        ActiveComponent::Details => format!("{common_legend} | {details_legend}"),
        ActiveComponent::Logs => format!("{common_legend} | {logs_legend}"),
    };

    let available_width = area.width as usize - 2;

    let truncated_legend_message = if legend_message.len() > available_width {
        let end_index = legend_message
            .char_indices()
            .map(|(i, _)| i)
            .nth(available_width.saturating_sub(3))
            .unwrap_or(0);
        format!("{}...", &legend_message[0..end_index])
    } else {
        legend_message
    };

    let styled_legend_message = Span::styled(truncated_legend_message, Style::default().fg(YELLOW));

    let legend_paragraph = Paragraph::new(Text::from(styled_legend_message))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Help", Style::default().fg(MAUVE)))
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .wrap(ratatui::widgets::Wrap { trim: true })
        .alignment(Alignment::Left);

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
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
        .split(area);

    let is_search_focused = app.active_component == ActiveComponent::SearchBar;
    let search_text = if is_search_focused {
        app.search_query.clone()
    } else if app.search_query.is_empty() {
        "Press / to search...".to_string()
    } else {
        format!("/{}", app.search_query)
    };

    let (search_color, search_title) = if is_search_focused {
        (YELLOW, "Search (active)")
    } else if !app.search_query.is_empty() {
        (GREEN, "Search (filtering)")
    } else {
        (TEXT, "Search")
    };

    let search_widget = Paragraph::new(search_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    search_title,
                    Style::default().fg(search_color),
                ))
                .border_style(if is_search_focused {
                    Style::default()
                        .fg(search_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(search_color)
                }),
        )
        .style(Style::default().fg(search_color));

    f.render_widget(search_widget, main_chunks[0]);

    let table_area = main_chunks[1];

    let tables_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(table_area);

    let stopped_configs_to_display = if app.search_query.is_empty() {
        &app.stopped_configs
    } else {
        &app.filtered_stopped_configs
    };

    let running_configs_to_display = if app.search_query.is_empty() {
        &app.running_configs
    } else {
        &app.filtered_running_configs
    };

    let stopped_title = if app.search_query.is_empty() {
        "Stopped Configs".to_string()
    } else {
        format!("Stopped Configs ({})", app.filtered_stopped_configs.len())
    };

    let running_title = if app.search_query.is_empty() {
        "Running Configs".to_string()
    } else {
        format!("Running Configs ({})", app.filtered_running_configs.len())
    };

    draw_configs_table(
        f,
        tables_chunks[0],
        stopped_configs_to_display,
        config_states,
        &mut app.table_state_stopped,
        &stopped_title,
        has_focus && app.active_component == ActiveComponent::StoppedTable,
        &app.selected_rows_stopped,
        &app.configs_being_processed,
        &app.throbber_state,
    );

    draw_configs_table(
        f,
        tables_chunks[1],
        running_configs_to_display,
        config_states,
        &mut app.table_state_running,
        &running_title,
        has_focus && app.active_component == ActiveComponent::RunningTable,
        &app.selected_rows_running,
        &app.configs_being_processed,
        &app.throbber_state,
    );
}
