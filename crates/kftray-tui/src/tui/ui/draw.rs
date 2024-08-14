use kftray_commons::models::config_state_model::ConfigState;
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
    },
    widgets::{
        Block,
        BorderType,
        Borders,
        Tabs,
    },
    Frame,
};

use crate::tui::input::{
    ActiveComponent,
    App,
    AppState,
};
use crate::tui::ui::render_delete_confirmation_popup;
use crate::tui::ui::{
    centered_rect,
    draw_configs_tab,
    draw_file_explorer_popup,
    render_background_overlay,
    render_confirmation_popup,
    render_error_popup,
    render_help_popup,
    render_about_popup,
    render_input_prompt,
    render_legend,
    BASE,
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

    match app.state {
        AppState::ShowHelp => {
            let help_area = centered_rect(20, 20, size);
            render_background_overlay(f, size);
            render_help_popup(f, help_area);
        }
        AppState::ShowAbout => {
            let about_area = centered_rect(20, 20, size);
            render_background_overlay(f, size);
            render_about_popup(f, about_area);
        }
        AppState::ImportFileExplorerOpen => {
            let popup_area = centered_rect(40, 80, size);
            render_background_overlay(f, size);
            draw_file_explorer_popup(f, app, popup_area, true);
        }
        AppState::ExportFileExplorerOpen => {
            let popup_area = centered_rect(40, 80, size);
            render_background_overlay(f, size);
            draw_file_explorer_popup(f, app, popup_area, false);
        }
        AppState::ShowInputPrompt => {
            let input_area = centered_rect(20, 20, size);
            render_background_overlay(f, size);
            render_input_prompt(f, &app.input_buffer, input_area);
        }
        AppState::ShowConfirmationPopup => {
            let confirmation_area = centered_rect(30, 20, size);
            render_background_overlay(f, size);
            render_confirmation_popup(f, &app.import_export_message, confirmation_area);
        }
        AppState::ShowErrorPopup => {
            if let Some(error_message) = &app.error_message {
                let error_area = centered_rect(30, 60, size);
                render_background_overlay(f, size);
                render_error_popup(f, error_message, error_area, 1);
            }
        }
        AppState::ShowDeleteConfirmation => {
            let delete_area = centered_rect(30, 20, size);
            render_background_overlay(f, size);
            render_delete_confirmation_popup(
                f,
                &app.delete_confirmation_message,
                delete_area,
                app.selected_delete_button,
            );
        }
        _ => {}
    }
}

pub fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let menu_titles = ["Help", "Import", "Export", "About", "Quit"];
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
                .border_type(BorderType::Rounded)
                .border_style(border_style),
        )
        .style(Style::default().fg(TEXT))
        .highlight_style(Style::default().fg(YELLOW))
        .divider(Span::raw(" | "));

    f.render_widget(menu_titles, area);
}
