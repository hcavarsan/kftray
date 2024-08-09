use kftray_commons::models::config_state_model::ConfigState;
use ratatui::prelude::Alignment;
use ratatui::widgets::Clear;
use ratatui::{
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
        Line,
        Span,
        Text,
    },
    widgets::{
        Block,
        Borders,
        Paragraph,
        Tabs,
    },
    Frame,
};

use crate::tui::input::ActiveComponent;
use crate::tui::input::App;
use crate::tui::ui::{
    centered_rect,
    draw_configs_tab,
    draw_file_explorer_popup,
    render_confirmation_popup,
    render_help_popup,
    render_input_prompt,
    render_legend,
    BASE,
    RED,
    TEXT,
    YELLOW,
};

pub fn draw_ui(f: &mut Frame, app: &mut App, config_states: &[ConfigState]) {
    let size = f.size();

    let background = Block::default().style(Style::default().bg(BASE));
    f.render_widget(background, size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(size);

    if chunks.len() != 3 {
        return;
    }

    draw_header(f, app, chunks[0]);
    draw_configs_tab(f, app, config_states, chunks[1]);
    render_legend(f, chunks[2]);

    if app.show_help {
        let help_area = centered_rect(60, 40, size);
        render_help_popup(f, help_area);
    }

    if app.file_explorer_open {
        let popup_area = centered_rect(60, 40, size);
        draw_file_explorer_popup(f, app, popup_area);
    }

    if app.show_input_prompt {
        let input_area = centered_rect(60, 10, size);
        render_input_prompt(f, &app.input_buffer, input_area);
    }

    if app.show_confirmation_popup {
        let confirmation_area = centered_rect(60, 10, size);
        render_confirmation_popup(f, &app.import_export_message, confirmation_area);
    }

    if let Some(error_message) = &app.error_message {
        let error_area = centered_rect(60, 10, size);
        render_error_popup(f, error_message, error_area);
    }
}

pub fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let menu_titles = ["Import", "Export", "Help", "Quit"];
    let menu: Vec<Line> = menu_titles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style =
                if app.active_component == ActiveComponent::Menu && app.selected_menu_item == i {
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT)
                };
            Line::from(Span::styled(*t, style))
        })
        .collect();

    let border_style = if app.active_component == ActiveComponent::Menu {
        Style::default().fg(YELLOW)
    } else {
        Style::default().fg(TEXT)
    };

    let menu_titles = Tabs::new(menu)
        .block(
            Block::default()
                .title("Menu")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .style(Style::default().fg(TEXT))
        .highlight_style(Style::default().fg(YELLOW))
        .divider(Span::raw(" | "));

    f.render_widget(menu_titles, area);
}

pub fn render_error_popup(f: &mut Frame, error_message: &str, area: Rect) {
    let error_paragraph = Paragraph::new(Text::raw(error_message))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Error")
                .style(Style::default().bg(BASE).fg(RED)),
        )
        .style(Style::default().fg(TEXT));

    let close_button = Paragraph::new("<Close>")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .alignment(Alignment::Center);
    let button_area = Rect::new(
        area.x + (area.width / 2) - 5,
        area.y + area.height - 3,
        10,
        3,
    );

    f.render_widget(Clear, area);
    f.render_widget(error_paragraph, area);
    f.render_widget(close_button, button_area);
}
