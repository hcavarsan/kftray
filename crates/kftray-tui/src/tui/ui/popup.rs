use ratatui::prelude::Alignment;
use ratatui::prelude::Line;
use ratatui::text::Span;
use ratatui::{
    layout::Rect,
    style::Style,
    text::Text,
    widgets::{
        Block,
        Borders,
        Clear,
        Paragraph,
    },
    Frame,
};

use crate::tui::ui::{
    BASE,
    TEXT,
    YELLOW,
};

pub fn render_input_prompt(f: &mut Frame, input_buffer: &str, area: Rect) {
    let input_paragraph = Paragraph::new(Text::raw(input_buffer))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Enter file name")
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .style(Style::default().fg(TEXT));

    f.render_widget(input_paragraph, area);
}

pub fn render_confirmation_popup(f: &mut Frame, message: &Option<String>, area: Rect) {
    let message_text = message.as_deref().unwrap_or("");
    let message_paragraph = Paragraph::new(Text::raw(message_text))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Confirmation")
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .style(Style::default().fg(TEXT));

    f.render_widget(Clear, area);
    f.render_widget(message_paragraph, area);

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
    f.render_widget(close_button, button_area);
}

pub fn render_help_popup(f: &mut Frame, area: Rect) {
    let help_message = vec![
        Line::from(Span::styled("CtrlC: Quit", Style::default().fg(YELLOW))),
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
        Line::from(Span::styled("h: Show Help", Style::default().fg(YELLOW))),
        Line::from(Span::styled("i: Import", Style::default().fg(YELLOW))),
        Line::from(Span::styled("e: Export", Style::default().fg(YELLOW))),
    ];

    let help_paragraph = Paragraph::new(Text::from(help_message))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .style(Style::default().bg(BASE).fg(TEXT)),
        )
        .style(Style::default().fg(TEXT));

    f.render_widget(Clear, area);
    f.render_widget(help_paragraph, area);
}
