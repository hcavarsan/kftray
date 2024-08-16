use std::borrow::Cow;

use ratatui::prelude::Modifier;
use ratatui::prelude::{
    Alignment,
    Color,
    Line,
};
use ratatui::text::{
    Span,
    Text,
};
use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{
        Block,
        Borders,
        Clear,
        Paragraph,
    },
    Frame,
};

use crate::core::built_info;
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
        .style(Style::default().bg(BASE).fg(TEXT))
}

fn create_bottom_right_shadow_layers(
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

fn render_shadow_layers(f: &mut Frame, shadow_layers: Vec<(Rect, Style)>) {
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
    ];

    let help_paragraph = Paragraph::new(Text::from(help_message))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Help", Style::default().fg(TEAL)))
                .style(Style::default().bg(BASE).fg(TEXT)),
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
                .title(Span::styled("About", Style::default().fg(TEAL)))
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });

    let popup_area = centered_rect(80, 80, area);
    f.render_widget(Clear, popup_area);
    f.render_widget(about_paragraph, popup_area);
}

pub fn render_error_popup(f: &mut Frame, error_message: &str, area: Rect, top_padding: usize) {
    let max_text_width = area.width.saturating_sub(4) as usize;
    let wrapped_text = wrap_text(error_message, max_text_width);

    let mut padded_text = Text::default();
    for _ in 0..top_padding {
        padded_text.lines.push(Line::from(""));
    }
    padded_text.lines.extend(wrapped_text.lines.clone());

    let text_height = (wrapped_text.lines.len() + top_padding) as u16 + 2;

    let popup_area = Rect::new(
        area.x + (area.width / 4),
        area.y + (area.height / 4),
        area.width / 2,
        text_height + 4,
    );

    render_popup(f, popup_area, "Error", RED, padded_text, Alignment::Center);

    let button_area = Rect::new(
        popup_area.x + (popup_area.width / 2) - 5,
        popup_area.y + text_height,
        10,
        3,
    );

    let close_button = create_close_button();

    let shadow_layers = [(MANTLE, 1)];

    let button_shadow_layers = create_bottom_right_shadow_layers(button_area, &shadow_layers);
    render_shadow_layers(f, button_shadow_layers);

    f.render_widget(close_button, button_area);
}

fn wrap_text(text: &str, max_width: usize) -> Text {
    let mut wrapped_lines = Vec::new();
    for line in text.lines() {
        let mut current_line = String::new();
        for word in line.split_whitespace() {
            if current_line.len() + word.len() + 1 > max_width {
                wrapped_lines.push(Line::from(current_line));
                current_line = String::new();
            }
            if !current_line.is_empty() {
                current_line.push(' ');
            }
            current_line.push_str(word);
        }
        if !current_line.is_empty() {
            wrapped_lines.push(Line::from(current_line));
        }
    }
    Text::from(wrapped_lines)
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

fn create_button(label: &str, is_selected: bool) -> Paragraph<'_> {
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
