use kftray_commons::models::config_state_model::ConfigState;
use ratatui::{
    layout::{
        Constraint,
        Direction,
        Layout,
        Rect,
    },
    style::{
        Color,
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
use tui_logger::TuiLoggerLevelOutput;
use tui_logger::TuiLoggerWidget;

use crate::tui::input::ActiveTable;
use crate::tui::input::{
    ActiveComponent,
    App,
    AppState,
};
use crate::tui::ui::render_context_selection_popup;
use crate::tui::ui::render_delete_confirmation_popup;
use crate::tui::ui::render_details;
use crate::tui::ui::MAUVE;
use crate::tui::ui::{
    centered_rect,
    draw_configs_tab,
    draw_file_explorer_popup,
    render_about_popup,
    render_background_overlay,
    render_confirmation_popup,
    render_error_popup,
    render_help_popup,
    render_input_prompt,
    render_legend,
    BASE,
    TEXT,
    YELLOW,
};

pub fn draw_ui(f: &mut Frame, app: &mut App, config_states: &[ConfigState]) {
    let size = f.area();

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

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
        .split(chunks[1]);

    draw_configs_tab(
        f,
        app,
        config_states,
        main_chunks[0],
        app.active_component == ActiveComponent::StoppedTable
            || app.active_component == ActiveComponent::RunningTable,
    );

    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(main_chunks[1]);

    let selected_config = match app.active_table {
        ActiveTable::Stopped => app
            .stopped_configs
            .get(app.table_state_stopped.selected().unwrap_or(0))
            .cloned(),
        ActiveTable::Running => app
            .running_configs
            .get(app.table_state_running.selected().unwrap_or(0))
            .cloned(),
    };

    if let Some(config) = selected_config {
        render_details(
            f,
            app,
            &config,
            config_states,
            bottom_chunks[0],
            app.active_component == ActiveComponent::Details,
        );
    } else {
        let empty_block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled("Detail", Style::default().fg(MAUVE)));
        f.render_widget(empty_block, bottom_chunks[0]);
    }

    render_logs(
        f,
        app,
        bottom_chunks[1],
        app.active_component == ActiveComponent::Logs,
    );

    render_legend(f, chunks[2], app.active_component);

    match app.state {
        AppState::ShowHelp => {
            let help_area = centered_rect(50, 50, size);
            render_background_overlay(f, size);
            render_help_popup(f, help_area);
        }
        AppState::ShowAbout => {
            let about_area = centered_rect(30, 60, size);
            render_background_overlay(f, size);
            render_about_popup(f, about_area);
        }
        AppState::ImportFileExplorerOpen => {
            let popup_area = centered_rect(90, 90, size);
            render_background_overlay(f, size);
            draw_file_explorer_popup(f, app, popup_area, true);
        }
        AppState::ExportFileExplorerOpen => {
            let popup_area = centered_rect(80, 60, size);
            render_background_overlay(f, size);
            draw_file_explorer_popup(f, app, popup_area, false);
        }
        AppState::ShowInputPrompt => {
            let input_area = centered_rect(40, 20, size);
            render_background_overlay(f, size);
            render_input_prompt(f, &app.input_buffer, input_area);
        }
        AppState::ShowConfirmationPopup => {
            let confirmation_area = centered_rect(50, 30, size);
            render_background_overlay(f, size);
            render_confirmation_popup(f, &app.import_export_message, confirmation_area);
        }
        AppState::ShowErrorPopup => {
            if let Some(error_message) = &app.error_message {
                let error_area = centered_rect(60, 40, size);
                render_background_overlay(f, size);
                render_error_popup(f, error_message, error_area, 1);
            }
        }
        AppState::ShowDeleteConfirmation => {
            let delete_area = centered_rect(50, 30, size);
            render_background_overlay(f, size);
            render_delete_confirmation_popup(
                f,
                &app.delete_confirmation_message,
                delete_area,
                app.selected_delete_button,
            );
        }
        AppState::ShowContextSelection => {
            let context_selection_area = centered_rect(50, 50, size);
            render_background_overlay(f, size);
            render_context_selection_popup(f, app, context_selection_area);
        }
        _ => {}
    }
}

pub fn log_level_to_color(level: log::Level) -> Style {
    match level {
        log::Level::Error => Style::default().fg(Color::Red),
        log::Level::Warn => Style::default().fg(Color::Yellow),
        log::Level::Info => Style::default().fg(Color::Green),
        log::Level::Debug => Style::default().fg(Color::Cyan),
        log::Level::Trace => Style::default().fg(Color::Blue),
    }
}

pub fn render_logs(f: &mut Frame, app: &mut App, area: Rect, has_focus: bool) {
    let focus_color = if has_focus { YELLOW } else { TEXT };
    let border_modifier = if has_focus {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };

    let logs_widget = TuiLoggerWidget::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Logs", Style::default().fg(MAUVE)))
                .border_style(
                    Style::default()
                        .fg(focus_color)
                        .add_modifier(border_modifier),
                ),
        )
        .style(Style::default().fg(TEXT).bg(BASE))
        .state(&app.logger_state)
        .output_separator('|')
        .output_timestamp(Some(" %H:%M ".to_string()))
        .output_level(Some(TuiLoggerLevelOutput::Long))
        .style(Style::default().fg(Color::White))
        .style_error(log_level_to_color(log::Level::Error))
        .style_warn(log_level_to_color(log::Level::Warn));

    f.render_widget(logs_widget, area);
}
pub fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let menu_titles = ["Help", "Auto Import", "Import", "Export", "About", "Quit"];
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
        Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)
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
