use std::borrow::Cow;

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{
    Line,
    Span,
    Text,
};
use ratatui::widgets::{
    Block,
    Borders,
    Cell,
    Clear,
    List,
    ListItem,
    Paragraph,
    Row,
    Table,
};

use crate::core::built_info;
use crate::tui::input::App;
use crate::tui::input::DeleteButton;
use crate::tui::ui::centered_rect;
use crate::tui::ui::{
    resize_ascii_art,
    ASCII_LOGO,
};
use crate::tui::ui::{
    BASE,
    BLUE,
    GREEN,
    MAUVE,
    SUBTEXT0,
    SURFACE0,
    SURFACE1,
    SURFACE2,
    TEXT,
    YELLOW,
};
use crate::tui::ui::{
    CRUST,
    LAVENDER,
    MANTLE,
    PINK,
    RED,
    TEAL,
};
fn create_common_popup_style(title: &str, title_color: Color) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            Cow::Borrowed(title),
            Style::default().fg(title_color),
        ))
        .style(Style::default().bg(BASE).fg(MAUVE))
}

pub fn create_bottom_right_shadow_layers(
    area: Rect, shadow_layers: &[(Color, u16)],
) -> Vec<(Rect, Style)> {
    shadow_layers
        .iter()
        .map(|(color, offset)| {
            let shadow_area =
                Rect::new(area.x + *offset, area.y + *offset, area.width, area.height);
            (shadow_area, Style::default().bg(*color))
        })
        .collect()
}

pub fn render_shadow_layers(f: &mut Frame, shadow_layers: Vec<(Rect, Style)>) {
    for (shadow_area, style) in shadow_layers {
        let shadow_block = Block::default().style(style);
        f.render_widget(shadow_block, shadow_area);
    }
}

pub fn render_background_overlay(f: &mut Frame, area: Rect) {
    let overlay = Block::default().style(Style::default().bg(CRUST));
    f.render_widget(overlay, area);
}

fn render_popup(
    f: &mut Frame, area: Rect, title: &str, title_color: Color, content: Text, alignment: Alignment,
) {
    let popup_paragraph = Paragraph::new(content)
        .block(create_common_popup_style(title, title_color))
        .style(Style::default().fg(TEXT).bg(BASE))
        .alignment(alignment);

    f.render_widget(Clear, area);
    f.render_widget(popup_paragraph, area);

    let shadow_layers = [(MANTLE, 1)];

    let popup_shadow_layers = create_bottom_right_shadow_layers(area, &shadow_layers);
    render_shadow_layers(f, popup_shadow_layers);
}

pub fn render_input_prompt(f: &mut Frame, input_buffer: &str, area: Rect) {
    let input_paragraph = Text::raw(input_buffer);
    render_popup(
        f,
        area,
        "Enter file name",
        PINK,
        input_paragraph,
        Alignment::Left,
    );
}

pub fn render_confirmation_popup(f: &mut Frame, message: &Option<String>, area: Rect) {
    let message_text = message.as_deref().unwrap_or("");
    let message_paragraph = Text::raw(message_text);
    render_popup(
        f,
        area,
        "Confirmation",
        MAUVE,
        message_paragraph,
        Alignment::Center,
    );

    let close_button = create_close_button();
    let button_area = Rect::new(
        area.x + (area.width / 2) - 5,
        area.y + area.height - 4,
        10,
        3,
    );
    f.render_widget(close_button, button_area);
}

pub fn render_http_logs_config_popup(f: &mut Frame, app: &App, area: Rect) {
    let (width_percent, height_percent) = if area.width < 60 {
        (95, 70)
    } else if area.width < 80 {
        (85, 60)
    } else if area.width < 120 {
        (65, 50)
    } else {
        (50, 40)
    };

    let popup_area = centered_rect(width_percent, height_percent, area);

    let title = if popup_area.width > 60 {
        format!(
            " HTTP Logs - Config #{} ",
            app.http_logs_config_id.unwrap_or(0)
        )
    } else {
        " HTTP Logs ".to_string()
    };

    let popup_block = Block::default()
        .title(title)
        .title_style(Style::default().fg(YELLOW).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MAUVE))
        .style(Style::default().bg(BASE));

    f.render_widget(Clear, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner_area = popup_area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(2)])
        .split(inner_area);

    let content_area = chunks[0];
    let footer_area = chunks[1];

    let use_vertical_layout = false;

    let grid_areas = if use_vertical_layout {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Length(4),
                Constraint::Length(4),
                Constraint::Length(4),
            ])
            .split(content_area);
        [rows[0], rows[1], rows[2], rows[3]]
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(content_area);

        let top_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[0]);

        let bottom_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]);

        [
            top_columns[0],
            top_columns[1],
            bottom_columns[0],
            bottom_columns[1],
        ]
    };
    let grid_options = [
        (
            0,
            "Enable",
            "",
            (if app.http_logs_config_enabled {
                "ON"
            } else {
                "OFF"
            })
            .to_string(),
        ),
        (
            3,
            "Auto Cleanup",
            "",
            (if app.http_logs_config_auto_cleanup {
                "ON"
            } else {
                "OFF"
            })
            .to_string(),
        ),
        (
            1,
            "Max Size",
            "",
            format!(
                "{}MB{}",
                app.http_logs_config_max_file_size_input,
                if app.http_logs_config_editing && app.http_logs_config_selected_option == 1 {
                    " [EDIT]"
                } else {
                    ""
                }
            ),
        ),
        (
            2,
            "Retention",
            "",
            format!(
                "{}d{}",
                app.http_logs_config_retention_days_input,
                if app.http_logs_config_editing && app.http_logs_config_selected_option == 2 {
                    " [EDIT]"
                } else {
                    ""
                }
            ),
        ),
    ];

    for (grid_index, (option_index, title, _description, value)) in grid_options.iter().enumerate()
    {
        let is_selected = app.http_logs_config_selected_option == *option_index;
        let is_editing = app.http_logs_config_editing && is_selected;

        let border_style = if is_selected {
            Style::default().fg(YELLOW).bold()
        } else {
            Style::default().fg(SURFACE2)
        };

        let bg_style = if is_selected {
            Style::default().bg(SURFACE0)
        } else {
            Style::default().bg(BASE)
        };

        let block = Block::default()
            .title(*title)
            .title_style(
                Style::default()
                    .fg(if is_selected { YELLOW } else { TEXT })
                    .bold(),
            )
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(bg_style);

        let content = vec![
            Line::from(""),
            Line::from(Span::styled(
                value.clone(),
                Style::default()
                    .fg(if is_editing {
                        GREEN
                    } else if is_selected {
                        YELLOW
                    } else {
                        TEXT
                    })
                    .bold()
                    .add_modifier(if is_selected {
                        ratatui::style::Modifier::UNDERLINED
                    } else {
                        ratatui::style::Modifier::empty()
                    }),
            )),
            Line::from(""),
        ];

        let paragraph = Paragraph::new(content)
            .alignment(Alignment::Center)
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: true });

        f.render_widget(paragraph, grid_areas[grid_index]);
    }

    let instructions = if app.http_logs_config_editing {
        "Type numbers | Backspace: delete | Enter: finish editing | Esc: Cancel"
    } else {
        "Arrow keys: Navigate | Enter: Toggle/Edit | Changes save automatically | Esc: Close"
    };

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(SURFACE2));

    let instruction_paragraph = Paragraph::new(instructions)
        .style(Style::default().fg(SUBTEXT0))
        .alignment(Alignment::Center)
        .block(footer_block);

    f.render_widget(instruction_paragraph, footer_area);
}

pub fn render_help_popup(f: &mut Frame, area: Rect) {
    let help_message = vec![
        Line::from(Span::styled("Ctrl+C: Quit", Style::default().fg(YELLOW))),
        Line::from(Span::styled("↑/↓: Navigate", Style::default().fg(YELLOW))),
        Line::from(Span::styled(
            "←/→: Switch Table",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "f: Start/Stop Port Forward",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "Space: Select/Deselect",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "Ctrl+A: Select/Deselect All",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled("h: Show Help", Style::default().fg(YELLOW))),
        Line::from(Span::styled("i: Import", Style::default().fg(YELLOW))),
        Line::from(Span::styled("e: Export", Style::default().fg(YELLOW))),
        Line::from(Span::styled(
            "d: Delete Selected",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "Tab: Switch Focus (Menu/Table)",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "Enter: Select Menu Item",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled("c: Clear Output", Style::default().fg(YELLOW))),
        Line::from(Span::styled(
            "l: Toggle HTTP Logs",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "L: HTTP Logs Config",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "V: View HTTP Logs",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "o: Open HTTP Log File",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "PageUp/PageDown: Scroll Page Up/Down",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled("q: Show About", Style::default().fg(YELLOW))),
    ];

    let help_paragraph = Paragraph::new(Text::from(help_message))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Help", Style::default().fg(MAUVE)))
                .style(Style::default().bg(BASE).fg(MAUVE)),
        )
        .alignment(Alignment::Left)
        .wrap(ratatui::widgets::Wrap { trim: true });

    f.render_widget(Clear, area);
    f.render_widget(help_paragraph, area);
}

pub fn render_about_popup(f: &mut Frame, area: Rect) {
    let resized_logo = resize_ascii_art(ASCII_LOGO, 1.0);

    let about_message = vec![
        Line::from(Span::styled(
            format!("App Version: {}", built_info::PKG_VERSION),
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            format!("Author: {}", built_info::PKG_AUTHORS),
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            format!("License: {}", built_info::PKG_LICENSE),
            Style::default().fg(YELLOW),
        )),
    ];

    let mut combined_message = Vec::new();
    for line in resized_logo {
        combined_message.push(Line::from(Span::styled(line, Style::default().fg(TEAL))));
    }
    combined_message.push(Line::from(""));
    combined_message.extend(about_message);

    let about_paragraph = Paragraph::new(Text::from(combined_message))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("About", Style::default().fg(MAUVE)))
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup_area = centered_rect(80, 80, area);
    f.render_widget(Clear, popup_area);
    f.render_widget(about_paragraph, popup_area);
}

pub fn render_error_popup(f: &mut Frame, error_message: &str, area: Rect, top_padding: usize) {
    let (width_percent, height_percent) = if area.width < 80 {
        (95, 80)
    } else if area.width < 120 {
        (90, 70)
    } else {
        (80, 60)
    };

    let popup_area = centered_rect(width_percent, height_percent, area);
    let content_width = popup_area.width.saturating_sub(4) as usize;

    let mut lines = Vec::new();

    for _ in 0..top_padding {
        lines.push("".into());
    }

    lines.push("".into());

    let parts: Vec<&str> = error_message.split(": ").collect();

    if parts.len() > 1 {
        for (i, part) in parts.iter().enumerate() {
            if part.starts_with("Failed to") {
                continue;
            }

            if i == 0 {
                let wrapped_lines = wrap_text_simple(part, content_width.saturating_sub(4));
                for line in wrapped_lines {
                    lines.push(Line::from(vec![format!("  {line}").red().bold()]));
                }
            } else {
                let wrapped_lines = wrap_text_simple(part, content_width.saturating_sub(6));
                for line in wrapped_lines {
                    lines.push(Line::from(vec![format!("    {line}").fg(TEXT)]));
                }
            }
        }
    } else {
        let wrapped_lines = wrap_text_simple(error_message, content_width.saturating_sub(4));
        for line in wrapped_lines {
            lines.push(Line::from(vec![format!("  {line}").fg(TEXT)]));
        }
    }

    lines.push("".into());
    lines.push(Line::from(vec!["  Press <Enter> to close"
        .fg(SUBTEXT0)
        .italic()]));

    let formatted_text = Text::from(lines).centered();
    render_popup(
        f,
        popup_area,
        "Error",
        RED,
        formatted_text,
        Alignment::Center,
    );
}

fn wrap_text_simple(text: &str, max_width: usize) -> Vec<String> {
    if max_width < 10 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.len() + word.len() + 1 > max_width && !current_line.is_empty() {
            lines.push(current_line);
            current_line = String::new();
        }
        if !current_line.is_empty() {
            current_line.push(' ');
        }
        current_line.push_str(word);
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(text.to_string());
    }

    lines
}

fn create_close_button() -> Paragraph<'static> {
    Paragraph::new(Span::styled("<Close>", Style::default().fg(LAVENDER)))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .alignment(Alignment::Center)
}

pub fn render_delete_confirmation_popup(
    f: &mut Frame, message: &Option<String>, area: Rect, selected_button: DeleteButton,
) {
    let message_text = message.as_deref().unwrap_or("");
    let message_paragraph = Text::raw(message_text);
    render_popup(
        f,
        area,
        "Delete Confirmation",
        RED,
        message_paragraph,
        Alignment::Center,
    );

    let confirm_button = create_button("<Confirm>", selected_button == DeleteButton::Confirm);
    let close_button = create_button("<Close>", selected_button == DeleteButton::Close);

    let confirm_button_area = Rect::new(
        area.x + (area.width / 2) - 15,
        area.y + area.height - 4,
        10,
        3,
    );
    let close_button_area = Rect::new(
        area.x + (area.width / 2) + 5,
        area.y + area.height - 4,
        10,
        3,
    );

    f.render_widget(confirm_button, confirm_button_area);
    f.render_widget(close_button, close_button_area);
}

pub fn create_button(label: &str, is_selected: bool) -> Paragraph<'_> {
    let style = if is_selected {
        Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT)
    };

    Paragraph::new(Span::styled(label, style))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .alignment(Alignment::Center)
}

pub fn render_context_selection_popup(f: &mut Frame, app: &mut App, area: Rect) {
    let contexts: Vec<ListItem> = app
        .contexts
        .iter()
        .map(|context| ListItem::new(context.clone()))
        .collect();

    let context_list = List::new(contexts)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Select Context", Style::default().fg(MAUVE)))
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .highlight_style(Style::default().fg(YELLOW).add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");

    f.render_widget(Clear, area);
    f.render_stateful_widget(context_list, area, &mut app.context_list_state);

    let explanation_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("- Select a context to automatically import services with the annotation "),
            Span::styled("kftray.app/enabled: true", Style::default().fg(YELLOW)),
            Span::raw("."),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("- If a service has "),
            Span::styled(
                "kftray.app/configs: \"test-9999-http\"",
                Style::default().fg(YELLOW),
            ),
            Span::raw(
                ", it will use 'test' as alias, '9999' as local port, and 'http' as target port.",
            ),
        ]),
    ];

    let explanation_paragraph = Paragraph::new(Text::from(explanation_text))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .alignment(Alignment::Left)
        .wrap(ratatui::widgets::Wrap { trim: true });

    let explanation_area = Rect::new(area.x, area.y + area.height - 9, area.width, 9);

    f.render_widget(explanation_paragraph, explanation_area);
}

pub fn render_settings_popup(f: &mut Frame, app: &App, area: Rect) {
    let (width_percent, height_percent) = if area.width < 60 {
        (95, 75)
    } else if area.width < 80 {
        (85, 65)
    } else if area.width < 120 {
        (65, 55)
    } else {
        (50, 45)
    };

    let popup_area = centered_rect(width_percent, height_percent, area);

    let title = if popup_area.width > 60 {
        " Settings "
    } else {
        " Config "
    };

    let popup_block = Block::default()
        .title(title)
        .title_style(Style::default().fg(YELLOW).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MAUVE))
        .style(Style::default().bg(BASE));

    f.render_widget(Clear, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner_area = popup_area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });

    let use_vertical_layout = false;

    let chunks = if use_vertical_layout {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(2)])
            .split(inner_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(0),
                Constraint::Length(2),
            ])
            .split(inner_area)
    };

    let (description_area, content_area, footer_area) = if use_vertical_layout {
        (None, chunks[0], chunks[1])
    } else {
        (Some(chunks[0]), chunks[1], chunks[2])
    };

    if let Some(desc_area) = description_area {
        let description_text = match app.settings_selected_option {
            0 => "Enable timeout functionality",
            1 => "Set timeout minutes",
            2 => "Monitor network connectivity",
            3 => "Reserved for future use",
            _ => "Navigate with arrow keys",
        };

        let description_paragraph = Paragraph::new(description_text)
            .style(Style::default().fg(SUBTEXT0).italic())
            .alignment(Alignment::Center)
            .wrap(ratatui::widgets::Wrap { trim: true });

        f.render_widget(description_paragraph, desc_area);
    }

    let grid_areas = if use_vertical_layout {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Length(4),
                Constraint::Length(4),
                Constraint::Length(4),
            ])
            .split(content_area);
        [rows[0], rows[1], rows[2], rows[3]]
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(content_area);

        let top_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[0]);

        let bottom_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]);

        [
            top_columns[0],
            top_columns[1],
            bottom_columns[0],
            bottom_columns[1],
        ]
    };

    let timeout_enabled =
        !(app.settings_timeout_input == "0" || app.settings_timeout_input.is_empty());
    let grid_options = [
        (
            0,
            "Timeout",
            "",
            (if timeout_enabled { "ON" } else { "OFF" }).to_string(),
        ),
        (
            1,
            "Minutes",
            "",
            if timeout_enabled {
                format!(
                    "{}m{}",
                    app.settings_timeout_input,
                    if app.settings_editing && app.settings_selected_option == 1 {
                        " [EDIT]"
                    } else {
                        ""
                    }
                )
            } else {
                "OFF".to_string()
            },
        ),
        (
            2,
            "Monitor",
            "",
            (if app.settings_network_monitor {
                "ON"
            } else {
                "OFF"
            })
            .to_string(),
        ),
        (3, "Reserved", "", "N/A".to_string()),
    ];

    for (grid_index, (option_index, title, _description, value)) in grid_options.iter().enumerate()
    {
        let is_selected = app.settings_selected_option == *option_index;
        let is_editing = app.settings_editing && is_selected;
        let is_disabled = *option_index == 1 && !timeout_enabled;
        let is_reserved = *option_index == 3;

        let border_style = if is_reserved {
            Style::default().fg(SURFACE2).dim()
        } else if is_disabled {
            Style::default().fg(SURFACE2)
        } else if is_selected {
            Style::default().fg(YELLOW).bold()
        } else {
            Style::default().fg(SURFACE2)
        };

        let bg_style = if is_reserved {
            Style::default().bg(BASE).dim()
        } else if is_disabled {
            Style::default().bg(BASE)
        } else if is_selected {
            Style::default().bg(SURFACE0)
        } else {
            Style::default().bg(BASE)
        };

        let block = Block::default()
            .title(*title)
            .title_style(
                Style::default()
                    .fg(if is_reserved || is_disabled {
                        SURFACE2
                    } else if is_selected {
                        YELLOW
                    } else {
                        TEXT
                    })
                    .bold(),
            )
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(bg_style);

        let content = vec![
            Line::from(""),
            Line::from(Span::styled(
                value.clone(),
                Style::default()
                    .fg(if is_reserved || is_disabled {
                        SURFACE2
                    } else if is_editing {
                        GREEN
                    } else if is_selected {
                        YELLOW
                    } else {
                        TEXT
                    })
                    .bold()
                    .add_modifier(if is_selected && !is_disabled && !is_reserved {
                        Modifier::UNDERLINED
                    } else {
                        Modifier::empty()
                    }),
            )),
            Line::from(""),
        ];

        let paragraph = Paragraph::new(content)
            .alignment(Alignment::Center)
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: true });

        f.render_widget(paragraph, grid_areas[grid_index]);
    }

    let instructions = if app.settings_editing {
        "Type numbers | Backspace: delete | Enter: save | Esc: Cancel"
    } else {
        "Arrow keys: Navigate | Enter: Toggle/Edit | Changes save automatically | Esc: Close"
    };

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(SURFACE2));

    let instruction_paragraph = Paragraph::new(instructions)
        .style(Style::default().fg(SUBTEXT0))
        .alignment(Alignment::Center)
        .block(footer_block);

    f.render_widget(instruction_paragraph, footer_area);
}

pub fn render_http_logs_viewer_popup(f: &mut Frame, app: &mut App, area: Rect) {
    app.update_http_logs_viewer();

    let popup_area = centered_rect(90, 80, area);

    let title = if app.http_logs_detail_mode {
        format!(
            " HTTP Request Details - Config #{} ",
            app.http_logs_viewer_config_id.unwrap_or(0)
        )
    } else {
        let auto_scroll_indicator = if app.http_logs_viewer_auto_scroll {
            " [LIVE]"
        } else {
            " [PAUSED]"
        };
        format!(
            " HTTP Requests - Config #{}{} ",
            app.http_logs_viewer_config_id.unwrap_or(0),
            auto_scroll_indicator
        )
    };

    let popup_block = Block::default()
        .title(title)
        .title_style(Style::default().fg(YELLOW).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MAUVE))
        .style(Style::default().bg(BASE));

    f.render_widget(Clear, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner_area = popup_area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(2)])
        .split(inner_area);

    let content_area = chunks[0];
    let footer_area = chunks[1];

    if app.http_logs_detail_mode {
        render_http_request_detail(f, app, content_area);
    } else {
        render_http_requests_list(f, app, content_area);
    }

    render_http_logs_footer(f, app, footer_area);
}

fn render_http_requests_list(f: &mut Frame, app: &mut App, area: Rect) {
    if app.http_logs_requests.is_empty() {
        let empty_message = Paragraph::new("No HTTP requests found in logs")
            .style(Style::default().fg(SUBTEXT0).italic())
            .alignment(Alignment::Center);
        f.render_widget(empty_message, area);
        return;
    }

    let rows: Vec<Row> = app
        .http_logs_requests
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let status_color = match entry.status_code.as_deref() {
                Some(code) if code.starts_with('2') => GREEN,
                Some(code) if code.starts_with('4') => YELLOW,
                Some(code) if code.starts_with('5') => RED,
                _ => TEXT,
            };

            let method_color = match entry.method.as_str() {
                "GET" => GREEN,
                "POST" => BLUE,
                "PUT" => YELLOW,
                "DELETE" => RED,
                _ => TEXT,
            };

            let row_style = if i == app.http_logs_list_selected {
                Style::default().bg(SURFACE1)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(Span::styled(
                    &entry.trace_id[..entry.trace_id.len().min(12)],
                    Style::default().fg(MAUVE),
                )),
                Cell::from(Span::styled(
                    &entry.method,
                    Style::default().fg(method_color),
                )),
                Cell::from(entry.path.as_str()),
                Cell::from(Span::styled(
                    entry.status_code.as_deref().unwrap_or("-"),
                    Style::default().fg(status_color),
                )),
                Cell::from(entry.duration_ms.as_deref().unwrap_or("-")),
                Cell::from(if entry.request_timestamp.len() >= 19 {
                    &entry.request_timestamp[11..19]
                } else {
                    &entry.request_timestamp
                }), // Show only time part
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(15),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("Trace ID"),
            Cell::from("Method"),
            Cell::from("Path"),
            Cell::from("Status"),
            Cell::from("Duration"),
            Cell::from("Time"),
        ])
        .style(Style::default().fg(MAUVE).bold()),
    )
    .row_highlight_style(Style::default().bg(SURFACE1).fg(TEXT));

    let mut table_state = ratatui::widgets::TableState::default();
    table_state.select(Some(app.http_logs_list_selected));

    f.render_stateful_widget(table, area, &mut table_state);
}

fn render_http_request_detail(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(entry) = &app.http_logs_selected_entry {
        let mut lines = vec![];

        if entry.trace_id.starts_with("replay-") {
            lines.push(Line::from(Span::styled(
                "[ REPLAYED REQUEST ]",
                Style::default().bold().fg(YELLOW),
            )));
            lines.push(Line::from(""));
        }

        lines.extend(vec![
            Line::from(vec![
                Span::styled("Trace ID: ", Style::default().bold()),
                Span::styled(&entry.trace_id, Style::default().fg(MAUVE)),
            ]),
            Line::from(vec![
                Span::styled("Method: ", Style::default().bold()),
                Span::styled(&entry.method, Style::default().fg(GREEN)),
            ]),
            Line::from(vec![
                Span::styled("Path: ", Style::default().bold()),
                Span::raw(&entry.path),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().bold()),
                Span::styled(
                    entry.status_code.as_deref().unwrap_or("Pending"),
                    Style::default().fg(GREEN),
                ),
            ]),
            Line::from(vec![
                Span::styled("Duration: ", Style::default().bold()),
                Span::raw(entry.duration_ms.as_deref().unwrap_or("N/A")),
                Span::raw("ms"),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Request Headers:",
                Style::default().bold().fg(BLUE),
            )),
        ]);

        for header in &entry.request_headers {
            lines.push(Line::from(Span::styled(
                format!("  {}", header),
                Style::default().fg(SUBTEXT0),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Request Body:",
            Style::default().bold().fg(BLUE),
        )));

        if entry.request_body.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (empty)",
                Style::default().fg(SUBTEXT0).italic(),
            )));
        } else {
            for line in entry.request_body.lines() {
                lines.push(Line::from(format!("  {}", line)));
            }
        }

        if entry.status_code.is_some() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Response Headers:",
                Style::default().bold().fg(GREEN),
            )));

            for header in &entry.response_headers {
                lines.push(Line::from(Span::styled(
                    format!("  {}", header),
                    Style::default().fg(SUBTEXT0),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Response Body:",
                Style::default().bold().fg(GREEN),
            )));

            if entry.response_body.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (empty)",
                    Style::default().fg(SUBTEXT0).italic(),
                )));
            } else {
                for line in entry.response_body.lines() {
                    lines.push(Line::from(format!("  {}", line)));
                }
            }
        }

        if let Some(replay_error) = &app.http_logs_replay_result {
            if replay_error.starts_with("Request failed") || replay_error.starts_with("Failed to") {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "━".repeat(50),
                    Style::default().fg(SURFACE2),
                )));
                lines.push(Line::from(Span::styled(
                    "Replay Error:",
                    Style::default().bold().fg(RED),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    replay_error,
                    Style::default().fg(RED),
                )));
            }
        }

        let visible_height = area.height as usize;
        let max_scroll = lines.len().saturating_sub(visible_height);
        let start_index = app.http_logs_viewer_scroll.min(max_scroll);
        let end_index = (start_index + visible_height).min(lines.len());

        let visible_lines = lines[start_index..end_index].to_vec();

        let paragraph = Paragraph::new(Text::from(visible_lines))
            .style(Style::default().fg(TEXT))
            .wrap(ratatui::widgets::Wrap { trim: false });

        f.render_widget(paragraph, area);
    } else {
        let error_message = Paragraph::new("No request selected")
            .style(Style::default().fg(RED).italic())
            .alignment(Alignment::Center);
        f.render_widget(error_message, area);
    }
}

fn render_http_logs_footer(f: &mut Frame, app: &mut App, area: Rect) {
    let instructions = if app.http_logs_detail_mode {
        "↑/↓: Scroll | PgUp/PgDn: Page | R: Replay Request | Esc: Back to List".to_string()
    } else {
        let auto_scroll_status = if app.http_logs_viewer_auto_scroll {
            "ON"
        } else {
            "OFF"
        };
        let list_info = format!(
            "{}/{}",
            app.http_logs_list_selected + 1,
            app.http_logs_requests.len().max(1)
        );
        format!(
            "↑/↓: Select | Enter: View Details | A: Auto-scroll {} | Esc: Close | {}",
            auto_scroll_status, list_info
        )
    };

    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(SURFACE2));

    let instruction_paragraph = Paragraph::new(instructions)
        .style(Style::default().fg(SUBTEXT0))
        .alignment(Alignment::Center)
        .block(footer_block);

    f.render_widget(instruction_paragraph, area);
}
