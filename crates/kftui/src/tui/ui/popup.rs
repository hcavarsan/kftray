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
#[cfg(debug_assertions)]
use crate::tui::input::UpdateInfo;
use crate::tui::ui::centered_rect;
use crate::tui::ui::{
    BASE,
    BLUE,
    GREEN,
    MAUVE,
    SUBTEXT0,
    SUBTEXT1,
    SURFACE0,
    SURFACE1,
    SURFACE2,
    TEAL,
    TEXT,
    YELLOW,
};
use crate::tui::ui::{
    CRUST,
    LAVENDER,
    MANTLE,
    PINK,
    RED,
};
#[cfg(not(debug_assertions))]
use crate::updater::UpdateInfo;
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
        Line::from(Span::styled(
            "s: Settings (SSL/TLS Config)",
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

pub fn render_about_popup(f: &mut Frame, app: &crate::tui::input::App, area: Rect) {
    let (width_percent, height_percent) = match (area.width, area.height) {
        (w, h) if w < 40 || h < 10 => (90, 70),
        (w, h) if w < 60 || h < 15 => (80, 60),
        (w, h) if w < 80 || h < 20 => (70, 50),
        (w, h) if w < 120 || h < 30 => (60, 45),
        _ => (50, 40),
    };

    let popup_area = centered_rect(width_percent, height_percent, area);
    f.render_widget(Clear, popup_area);

    let main_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            "▋ About",
            Style::default().fg(LAVENDER).bold(),
        ))
        .title_alignment(Alignment::Left)
        .style(Style::default().bg(BASE).fg(TEXT))
        .border_style(Style::default().fg(LAVENDER));

    let inner = main_block.inner(popup_area);
    f.render_widget(main_block, popup_area);

    let is_very_small = popup_area.width < 30 || popup_area.height < 8;
    let is_small = popup_area.width < 50 || popup_area.height < 12;

    if is_very_small {
        let simple_content = vec![
            Line::from(vec![
                Span::styled("kftui ", Style::default().fg(TEAL).bold()),
                Span::styled(built_info::PKG_VERSION, Style::default().fg(TEAL)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Press Esc to close",
                Style::default().fg(SUBTEXT0),
            )),
        ];

        let simple_para = Paragraph::new(Text::from(simple_content))
            .alignment(Alignment::Center)
            .wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(simple_para, inner);
        return;
    }

    let has_update = app.update_info.as_ref().is_some_and(|info| info.has_update);

    let main_constraints = if is_small {
        vec![Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)]
    } else if has_update {
        vec![Constraint::Ratio(1, 4), Constraint::Ratio(3, 4)]
    } else {
        vec![Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)]
    };

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(main_constraints)
        .margin(if is_small { 0 } else { 1 })
        .split(inner);

    if !is_small {
        let header_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(SURFACE1))
            .style(Style::default().bg(MANTLE));
        let header_inner = header_block.inner(main_layout[0]);
        f.render_widget(header_block, main_layout[0]);

        let header_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(header_inner);

        let title_content = vec![Line::from(vec![
            Span::styled("◈ ", Style::default().fg(TEAL)),
            Span::styled("kftui", Style::default().fg(TEAL).bold()),
            Span::raw("  "),
            Span::styled(built_info::PKG_VERSION, Style::default().fg(TEAL)),
        ])];
        let title_para = Paragraph::new(Text::from(title_content)).alignment(Alignment::Center);
        f.render_widget(title_para, header_layout[0]);

        let desc_content = vec![Line::from(vec![Span::styled(
            "Kubernetes Forward TUI",
            Style::default().fg(SUBTEXT1),
        )])];
        let desc_para = Paragraph::new(Text::from(desc_content)).alignment(Alignment::Center);
        f.render_widget(desc_para, header_layout[1]);

        let license_content = vec![Line::from(vec![Span::styled(
            built_info::PKG_LICENSE,
            Style::default().fg(SUBTEXT0),
        )])];
        let license_para = Paragraph::new(Text::from(license_content)).alignment(Alignment::Center);
        f.render_widget(license_para, header_layout[2]);
    } else {
        let header_content = vec![
            Line::from(vec![
                Span::styled("◈ ", Style::default().fg(TEAL)),
                Span::styled("kftui ", Style::default().fg(TEAL).bold()),
                Span::styled(built_info::PKG_VERSION, Style::default().fg(TEAL)),
            ]),
            Line::from(vec![Span::styled(
                "Kubernetes Forward TUI",
                Style::default().fg(SUBTEXT1),
            )]),
            Line::from(vec![Span::styled(
                built_info::PKG_LICENSE,
                Style::default().fg(SUBTEXT0),
            )]),
        ];
        let header_para = Paragraph::new(Text::from(header_content))
            .alignment(Alignment::Center)
            .wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(header_para, main_layout[0]);
    }

    let content_area = main_layout[1];

    if let Some(update_info) = &app.update_info {
        if update_info.has_update {
            let update_area_height = if is_small {
                Constraint::Ratio(2, 3)
            } else {
                Constraint::Ratio(1, 2)
            };

            let update_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([update_area_height, Constraint::Min(0)])
                .split(content_area);

            let update_block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(YELLOW))
                .style(Style::default().bg(MANTLE));
            let update_inner = update_block.inner(update_layout[0]);
            f.render_widget(update_block, update_layout[0]);

            let update_content = if is_small {
                vec![
                    Line::from(vec![
                        Span::styled("Update: ", Style::default().fg(YELLOW)),
                        Span::styled(
                            &update_info.latest_version,
                            Style::default().fg(TEAL).bold(),
                        ),
                    ]),
                    Line::from(Span::styled(
                        "Press Enter to update",
                        Style::default().fg(YELLOW),
                    )),
                ]
            } else {
                vec![
                    Line::from(vec![
                        Span::styled("◆ ", Style::default().fg(YELLOW)),
                        Span::styled("Update Available", Style::default().fg(YELLOW).bold()),
                        Span::raw("  "),
                        Span::styled(
                            &update_info.latest_version,
                            Style::default().fg(TEAL).bold(),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Press ", Style::default().fg(SUBTEXT0)),
                        Span::styled("Enter", Style::default().fg(YELLOW).bold()),
                        Span::styled(" to update", Style::default().fg(SUBTEXT0)),
                    ]),
                ]
            };

            let update_para = Paragraph::new(Text::from(update_content))
                .alignment(Alignment::Center)
                .wrap(ratatui::widgets::Wrap { trim: true });
            f.render_widget(update_para, update_inner);
        } else {
            let status_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Ratio(1, 3), Constraint::Min(0)])
                .split(content_area);

            let content_lines = vec![Line::from(vec![
                Span::styled("● ", Style::default().fg(TEAL)),
                Span::styled("Up to date", Style::default().fg(TEAL).bold()),
            ])];
            let info_para = Paragraph::new(Text::from(content_lines))
                .alignment(Alignment::Center)
                .wrap(ratatui::widgets::Wrap { trim: true });
            f.render_widget(info_para, status_layout[0]);
        }
    }
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
    lines.push(Line::from(vec![
        "  Press <Enter> to close".fg(SUBTEXT0).italic(),
    ]));

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
    let (width_percent, height_percent) = if area.width < 50 {
        (98, 95)
    } else if area.width < 80 {
        (90, 85)
    } else {
        (80, 75)
    };

    let popup_area = centered_rect(width_percent, height_percent, area);

    let popup_block = Block::default()
        .title(" Settings ")
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

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(inner_area);

    let content_area = main_chunks[1];
    let footer_area = main_chunks[2];

    let timeout_enabled =
        !(app.settings_timeout_input == "0" || app.settings_timeout_input.is_empty());

    let settings_data = [
        (
            "Timeout",
            "Enable timeout functionality",
            if timeout_enabled { "ON" } else { "OFF" },
            false,
            0,
        ),
        (
            "Minutes",
            "Set timeout minutes",
            &if timeout_enabled {
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
            !timeout_enabled,
            1,
        ),
        (
            "Network Monitor",
            "Monitor network connectivity",
            if app.settings_network_monitor {
                "ON"
            } else {
                "OFF"
            },
            false,
            2,
        ),
        (
            "SSL/TLS",
            "Enable SSL/TLS for port forwards",
            if app.settings_ssl_enabled {
                "ON"
            } else {
                "OFF"
            },
            false,
            3,
        ),
        (
            "SSL Validity",
            "Certificate validity in days",
            &if app.settings_ssl_enabled {
                format!(
                    "{}d{}",
                    app.settings_ssl_cert_validity_input,
                    if app.settings_editing && app.settings_selected_option == 4 {
                        " [EDIT]"
                    } else {
                        ""
                    }
                )
            } else {
                "OFF".to_string()
            },
            !app.settings_ssl_enabled,
            4,
        ),
    ];

    let is_compact = content_area.width < 60;
    let is_very_compact = content_area.width < 40;

    if is_very_compact {
        render_compact_settings(f, app, &settings_data, content_area);
    } else if is_compact {
        render_table_settings(f, app, &settings_data, content_area, true);
    } else {
        render_table_settings(f, app, &settings_data, content_area, false);
    }

    let instructions = if app.settings_editing {
        "Numbers/Backspace: Edit | Enter: Save | Esc: Cancel"
    } else {
        "↑/↓: Navigate | Enter: Toggle/Edit | Esc: Close"
    };

    let footer_paragraph = Paragraph::new(instructions)
        .style(Style::default().fg(SUBTEXT0))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(SURFACE2)),
        );

    f.render_widget(footer_paragraph, footer_area);
}

fn render_compact_settings(
    f: &mut Frame, app: &App, settings_data: &[(&str, &str, &str, bool, usize)], area: Rect,
) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let content_area = layout[1];

    let row_height = 3;
    let available_rows = content_area.height as usize / row_height;
    let visible_items = available_rows.min(settings_data.len());

    let selected = app.settings_selected_option;
    let start_idx = if selected < visible_items / 2 {
        0
    } else if selected >= settings_data.len() - visible_items / 2 {
        settings_data.len().saturating_sub(visible_items)
    } else {
        selected.saturating_sub(visible_items / 2)
    };

    let end_idx = (start_idx + visible_items).min(settings_data.len());

    for (i, item_idx) in (start_idx..end_idx).enumerate() {
        let (title, _, value, _is_reserved, option_index) = settings_data[item_idx];
        let is_selected = app.settings_selected_option == option_index;
        let is_editing = app.settings_editing && is_selected;

        let row_y = content_area.y + (i * row_height) as u16;
        let row_area = Rect::new(content_area.x, row_y, content_area.width, row_height as u16);

        if is_selected {
            let bg_block = Block::default().style(Style::default().bg(SURFACE0));
            f.render_widget(bg_block, row_area);
        }

        let item_area = row_area.inner(Margin {
            horizontal: 1,
            vertical: 0,
        });

        let item_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(4),
                Constraint::Min(0),
            ])
            .split(item_area);

        let selection_indicator = if is_selected { "►" } else { " " };
        let sel_para = Paragraph::new(selection_indicator)
            .style(Style::default().fg(YELLOW))
            .alignment(Alignment::Center);
        f.render_widget(sel_para, item_layout[0]);

        let status_indicator = if value == "ON" {
            "[●]"
        } else if value == "OFF" {
            "[○]"
        } else if is_editing {
            "[✎]"
        } else {
            "[≡]"
        };

        let status_style = Style::default().fg(if value == "ON" {
            GREEN
        } else if value == "OFF" {
            RED
        } else if is_editing {
            YELLOW
        } else {
            BLUE
        });

        let status_para = Paragraph::new(status_indicator)
            .style(status_style)
            .alignment(Alignment::Center);
        f.render_widget(status_para, item_layout[1]);

        let title_style = Style::default()
            .fg(if is_selected { YELLOW } else { TEXT })
            .add_modifier(if is_selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });

        let title_para = Paragraph::new(title)
            .style(title_style)
            .alignment(Alignment::Left);
        f.render_widget(title_para, item_layout[2]);

        if i < end_idx - start_idx - 1 {
            let sep_y = row_y + (row_height as u16) - 1;
            let sep_area = Rect::new(
                content_area.x + 6,
                sep_y,
                content_area.width.saturating_sub(12),
                1,
            );

            let separator_text = "· ".repeat((sep_area.width / 2) as usize);
            let sep_para = Paragraph::new(separator_text)
                .style(Style::default().fg(SURFACE1))
                .alignment(Alignment::Center);
            f.render_widget(sep_para, sep_area);
        }
    }
}

fn render_table_settings(
    f: &mut Frame, app: &App, settings_data: &[(&str, &str, &str, bool, usize)], area: Rect,
    compact: bool,
) {
    let content_area = area;

    let row_height = 3;
    let total_rows = settings_data.len();
    let available_height = content_area.height as usize;
    let max_visible_rows = available_height / row_height;

    let start_row = if total_rows <= max_visible_rows {
        0
    } else {
        let selected = app.settings_selected_option;
        if selected < max_visible_rows / 2 {
            0
        } else if selected >= total_rows - max_visible_rows / 2 {
            total_rows.saturating_sub(max_visible_rows)
        } else {
            selected.saturating_sub(max_visible_rows / 2)
        }
    };

    let end_row = (start_row + max_visible_rows).min(total_rows);

    for (i, row_index) in (start_row..end_row).enumerate() {
        if row_index >= settings_data.len() {
            break;
        }

        let (title, description, value, is_reserved, option_index) = settings_data[row_index];
        let is_selected = app.settings_selected_option == option_index;
        let is_editing = app.settings_editing && is_selected;
        let is_disabled = (app.settings_timeout_input.is_empty()
            || app.settings_timeout_input == "0")
            && option_index == 1
            || (option_index == 4 && !app.settings_ssl_enabled);

        let row_y = content_area.y + (i * row_height) as u16;
        let row_area = Rect::new(content_area.x, row_y, content_area.width, row_height as u16);

        if is_selected {
            let bg_block = Block::default().style(Style::default().bg(SURFACE0));
            f.render_widget(bg_block, row_area);
        }

        let row_content = row_area.inner(Margin {
            horizontal: 2,
            vertical: 0,
        });

        if compact {
            render_compact_row(
                f,
                title,
                value,
                RowState {
                    is_selected,
                    is_editing,
                    is_disabled,
                    is_reserved,
                },
                row_content,
            );
        } else {
            render_full_row(
                f,
                title,
                description,
                value,
                RowState {
                    is_selected,
                    is_editing,
                    is_disabled,
                    is_reserved,
                },
                row_content,
            );
        }

        if i < end_row - start_row - 1 {
            let sep_y = row_y + (row_height as u16) - 1;
            if sep_y < content_area.y + content_area.height {
                let sep_area = Rect::new(
                    content_area.x + 4,
                    sep_y,
                    content_area.width.saturating_sub(8),
                    1,
                );

                let separator_text = "· ".repeat((sep_area.width / 2) as usize);
                let sep_para = Paragraph::new(separator_text)
                    .style(Style::default().fg(SURFACE1))
                    .alignment(Alignment::Center);
                f.render_widget(sep_para, sep_area);
            }
        }
    }
}

struct RowState {
    is_selected: bool,
    is_editing: bool,
    is_disabled: bool,
    is_reserved: bool,
}

fn render_compact_row(f: &mut Frame, title: &str, value: &str, state: RowState, area: Rect) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(20),
            Constraint::Min(0),
        ])
        .split(area);

    let indicator = if state.is_selected { "►" } else { " " };
    let indicator_para = Paragraph::new(indicator).style(Style::default().fg(YELLOW));
    f.render_widget(indicator_para, layout[0]);

    let title_style = Style::default()
        .fg(if state.is_reserved || state.is_disabled {
            SURFACE2
        } else if state.is_selected {
            YELLOW
        } else {
            TEXT
        })
        .add_modifier(if state.is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });

    let title_para = Paragraph::new(title)
        .style(title_style)
        .alignment(Alignment::Left);
    f.render_widget(title_para, layout[1]);

    let value_style = Style::default()
        .fg(if state.is_reserved || state.is_disabled {
            SURFACE2
        } else if state.is_editing || value == "ON" {
            GREEN
        } else if value == "OFF" {
            RED
        } else if state.is_selected {
            YELLOW
        } else {
            TEXT
        })
        .add_modifier(if state.is_editing {
            Modifier::UNDERLINED | Modifier::BOLD
        } else if state.is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });

    let value_para = Paragraph::new(value)
        .style(value_style)
        .alignment(Alignment::Left);
    f.render_widget(value_para, layout[2]);
}

fn render_full_row(
    f: &mut Frame, title: &str, description: &str, value: &str, state: RowState, area: Rect,
) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(15),
        ])
        .split(area);

    let indicator = if state.is_selected { "►" } else { " " };
    let indicator_para = Paragraph::new(indicator).style(Style::default().fg(YELLOW));
    f.render_widget(indicator_para, layout[0]);

    let title_style = Style::default()
        .fg(if state.is_reserved || state.is_disabled {
            SURFACE2
        } else if state.is_selected {
            YELLOW
        } else {
            TEXT
        })
        .add_modifier(if state.is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });

    let title_para = Paragraph::new(title)
        .style(title_style)
        .alignment(Alignment::Left);
    f.render_widget(title_para, layout[1]);

    let desc_para = Paragraph::new(description)
        .style(Style::default().fg(SUBTEXT0).italic())
        .alignment(Alignment::Left)
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(desc_para, layout[2]);

    let value_style = Style::default()
        .fg(if state.is_reserved || state.is_disabled {
            SURFACE2
        } else if state.is_editing || value == "ON" {
            GREEN
        } else if value == "OFF" {
            RED
        } else if state.is_selected {
            YELLOW
        } else {
            TEXT
        })
        .add_modifier(if state.is_editing {
            Modifier::UNDERLINED | Modifier::BOLD
        } else if state.is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });

    let value_para = Paragraph::new(value)
        .style(value_style)
        .alignment(Alignment::Left);
    f.render_widget(value_para, layout[3]);
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
                {
                    let preview: String = entry.trace_id.chars().take(12).collect();
                    Cell::from(Span::styled(preview, Style::default().fg(MAUVE)))
                },
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
                Cell::from(
                    entry
                        .request_timestamp
                        .get(11..19)
                        .unwrap_or(entry.request_timestamp.as_str()),
                ),
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
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let request_area = chunks[0];
        let response_area = chunks[1];

        render_request_side(f, app, entry, request_area);

        render_response_side(f, app, entry, response_area);
    } else {
        let error_message = Paragraph::new("No request selected")
            .style(Style::default().fg(RED).italic())
            .alignment(Alignment::Center);
        f.render_widget(error_message, area);
    }
}

fn render_request_side(
    f: &mut Frame, app: &App, entry: &crate::tui::input::HttpLogEntry, area: Rect,
) {
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

    if entry.request_body.is_empty() || entry.request_body.trim() == "<empty body>" {
        lines.push(Line::from(Span::styled(
            "  (empty)",
            Style::default().fg(SUBTEXT0).italic(),
        )));
    } else {
        let formatted_body = format_body_content(&entry.request_body, &entry.request_headers);
        for line in formatted_body.lines() {
            lines.push(Line::from(format!("  {}", line)));
        }
    }

    let visible_height = area.height as usize;
    let max_scroll = lines.len().saturating_sub(visible_height);
    let start_index = app.http_logs_viewer_scroll.min(max_scroll);
    let end_index = (start_index + visible_height).min(lines.len());

    let visible_lines = lines[start_index..end_index].to_vec();

    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(Style::default().fg(TEXT))
        .wrap(ratatui::widgets::Wrap { trim: false })
        .block(Block::default().borders(Borders::RIGHT).title("Request"));

    f.render_widget(paragraph, area);
}

fn render_response_side(
    f: &mut Frame, app: &App, entry: &crate::tui::input::HttpLogEntry, area: Rect,
) {
    let mut lines = vec![];

    if entry.status_code.is_some() {
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

        if entry.response_body.is_empty() || entry.response_body.trim() == "<empty body>" {
            lines.push(Line::from(Span::styled(
                "  (empty)",
                Style::default().fg(SUBTEXT0).italic(),
            )));
        } else {
            let formatted_body = format_body_content(&entry.response_body, &entry.response_headers);
            for line in formatted_body.lines() {
                lines.push(Line::from(format!("  {}", line)));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            "No response available",
            Style::default().fg(SUBTEXT0).italic(),
        )));
    }

    if let Some(replay_error) = &app.http_logs_replay_result
        && (replay_error.starts_with("Request failed") || replay_error.starts_with("Failed to"))
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "━".repeat(30),
            Style::default().fg(SURFACE2),
        )));
        lines.push(Line::from(Span::styled(
            "Replay Error:",
            Style::default().bold().fg(RED),
        )));
        lines.push(Line::from(""));
        for error_line in replay_error.lines() {
            lines.push(Line::from(Span::styled(
                format!("  {}", error_line),
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
        .wrap(ratatui::widgets::Wrap { trim: false })
        .block(Block::default().title("Response"));

    f.render_widget(paragraph, area);
}

fn format_body_content(body: &str, headers: &[String]) -> String {
    if body.is_empty() || body.trim() == "<empty body>" {
        return body.to_string();
    }

    let is_json = headers.iter().any(|h| {
        h.to_lowercase().contains("content-type") && h.to_lowercase().contains("application/json")
    }) || (body.trim_start().starts_with('{') && body.trim_end().ends_with('}'))
        || (body.trim_start().starts_with('[') && body.trim_end().ends_with(']'));

    if is_json
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body.trim())
        && let Ok(pretty) = serde_json::to_string_pretty(&parsed)
    {
        return pretty;
    }

    body.to_string()
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

pub fn render_update_confirmation_popup(
    f: &mut Frame, update_info: &UpdateInfo, area: Rect,
    selected_button: crate::tui::input::UpdateButton,
) {
    let message_text = format!(
        "A new version is available!\n\nCurrent version: {}\nLatest version: {}\n\nWould you like to update now?",
        update_info.current_version, update_info.latest_version
    );
    let message_paragraph = Text::raw(message_text);
    render_popup(
        f,
        area,
        "Update Available",
        GREEN,
        message_paragraph,
        Alignment::Center,
    );

    let update_button = create_button(
        "<Update>",
        selected_button == crate::tui::input::UpdateButton::Update,
    );
    let cancel_button = create_button(
        "<Cancel>",
        selected_button == crate::tui::input::UpdateButton::Cancel,
    );

    let update_button_area = Rect::new(
        area.x + (area.width / 2) - 15,
        area.y + area.height - 4,
        10,
        3,
    );
    let cancel_button_area = Rect::new(
        area.x + (area.width / 2) + 5,
        area.y + area.height - 4,
        10,
        3,
    );

    f.render_widget(update_button, update_button_area);
    f.render_widget(cancel_button, cancel_button_area);
}

pub fn render_update_progress_popup(f: &mut Frame, message: &Option<String>, area: Rect) {
    let message_text = message.as_deref().unwrap_or("Downloading update...");
    let message_paragraph = Text::raw(message_text);
    render_popup(
        f,
        area,
        "Updating",
        BLUE,
        message_paragraph,
        Alignment::Center,
    );
}

pub fn render_restart_notification_popup(f: &mut Frame, area: Rect) {
    let message_text = "Update completed successfully!\n\nPlease restart the application to apply the new version.";
    let message_paragraph = Text::raw(message_text);
    render_popup(
        f,
        area,
        "Update Complete",
        GREEN,
        message_paragraph,
        Alignment::Center,
    );

    let ok_button = create_button("<OK>", true);
    let button_area = Rect::new(
        area.x + (area.width / 2) - 5,
        area.y + area.height - 4,
        10,
        3,
    );
    f.render_widget(ok_button, button_area);
}
