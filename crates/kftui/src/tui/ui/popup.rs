use std::borrow::Cow;

use ratatui::prelude::*;
use ratatui::text::{
    Line,
    Span,
    Text,
};
use ratatui::widgets::{
    Block,
    Borders,
    Clear,
    List,
    ListItem,
    Paragraph,
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
    CRUST,
    LAVENDER,
    MANTLE,
    MAUVE,
    PINK,
    RED,
    SUBTEXT0,
    TEAL,
    TEXT,
    YELLOW,
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
    let content_width = popup_area.width.saturating_sub(4) as usize; // Account for borders

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
    // Use responsive sizing like error popup
    let (width_percent, height_percent) = if area.width < 80 {
        (95, 85)
    } else if area.width < 120 {
        (85, 75)
    } else {
        (70, 65)
    };

    let popup_area = centered_rect(width_percent, height_percent, area);
    let content_width = popup_area.width.saturating_sub(4) as usize; // Account for borders

    let mut lines = Vec::new();

    // Add some top padding
    lines.push("".into());

    // Timeout Setting Section
    let timeout_indicator = if app.settings_selected_option == 0 {
        "▶ "
    } else {
        "  "
    };
    let timeout_style = if app.settings_selected_option == 0 {
        Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(LAVENDER)
    };

    lines.push(Line::from(vec![
        Span::raw(timeout_indicator),
        Span::styled("Global Port Forward Timeout", timeout_style),
    ]));

    // Wrap timeout description text
    let timeout_desc =
        "Automatically disconnect port forwards after the specified time. Set to 0 to disable.";
    let timeout_wrapped = wrap_text_simple(timeout_desc, content_width.saturating_sub(6));
    for line in timeout_wrapped {
        lines.push(Line::from(vec![Span::raw(format!("    {line}")).fg(TEXT)]));
    }

    // Timeout value display
    let timeout_display =
        if app.settings_timeout_input == "0" || app.settings_timeout_input.is_empty() {
            "disabled"
        } else {
            "minutes"
        };

    let timeout_value_line = if app.settings_editing && app.settings_selected_option == 0 {
        Line::from(vec![
            Span::raw("    Value: "),
            Span::styled(
                format!("{}_", app.settings_timeout_input),
                Style::default().fg(TEAL).add_modifier(Modifier::UNDERLINED),
            ),
            Span::styled(format!(" {timeout_display}"), Style::default().fg(TEXT)),
        ])
    } else {
        Line::from(vec![
            Span::raw("    Value: "),
            Span::styled(
                &app.settings_timeout_input,
                Style::default().fg(TEAL).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {timeout_display}"), Style::default().fg(TEXT)),
        ])
    };
    lines.push(timeout_value_line);
    lines.push("".into());

    // Network Monitor Section
    let monitor_indicator = if app.settings_selected_option == 1 {
        "▶ "
    } else {
        "  "
    };
    let monitor_style = if app.settings_selected_option == 1 {
        Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(LAVENDER)
    };

    lines.push(Line::from(vec![
        Span::raw(monitor_indicator),
        Span::styled("Network Monitor", monitor_style),
    ]));

    // Wrap network monitor description text
    let monitor_desc = "Monitor network connectivity and automatically reconnect port forwards when network is restored.";
    let monitor_wrapped = wrap_text_simple(monitor_desc, content_width.saturating_sub(6));
    for line in monitor_wrapped {
        lines.push(Line::from(vec![Span::raw(format!("    {line}")).fg(TEXT)]));
    }

    // Network monitor status
    lines.push(Line::from(vec![
        Span::raw("    Status: "),
        Span::styled(
            if app.settings_network_monitor {
                "Enabled"
            } else {
                "Disabled"
            },
            Style::default()
                .fg(if app.settings_network_monitor {
                    TEAL
                } else {
                    RED
                })
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push("".into());

    // Navigation instructions - wrap if needed
    let nav_text = if app.settings_editing && app.settings_selected_option == 0 {
        "↑/↓: Navigate | Enter: Save | Backspace: Delete | Esc: Close"
    } else if app.settings_selected_option == 0 {
        "↑/↓: Navigate | Enter: Edit | Esc: Close"
    } else {
        "↑/↓: Navigate | Enter: Toggle | Esc: Close"
    };

    let nav_wrapped = wrap_text_simple(nav_text, content_width.saturating_sub(4));
    for line in nav_wrapped {
        lines.push(Line::from(vec![Span::raw(format!("  {line}"))
            .fg(PINK)
            .italic()]));
    }

    let formatted_text = Text::from(lines);
    render_popup(
        f,
        popup_area,
        "Settings",
        MAUVE,
        formatted_text,
        Alignment::Left,
    );
}
