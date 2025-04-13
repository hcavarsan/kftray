use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::tui::ui::popup::{
    create_bottom_right_shadow_layers,
    create_button,
    render_shadow_layers,
    wrap_text,
};

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::style::Style;
    use ratatui::Terminal;

    use super::*;

    #[test]
    fn test_wrap_text() {
        let text =
            "This is a test string that should be wrapped to fit within the specified width.";
        let max_width = 20;

        let wrapped = wrap_text(text, max_width);

        assert!(
            !wrapped.lines.is_empty(),
            "Result should contain wrapped lines"
        );

        let mut total_length = 0;
        for line in &wrapped.lines {
            total_length += line.to_string().len();
        }

        assert!(total_length > 0, "Wrapped text should contain characters");

        let original_word_count = text.split_whitespace().count();
        let result_text = wrapped
            .lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<String>>()
            .join(" ");
        let result_word_count = result_text.split_whitespace().count();

        assert_eq!(
            original_word_count, result_word_count,
            "Number of words should be preserved in wrapping"
        );
    }

    #[test]
    fn test_wrap_text_empty() {
        let text = "";
        let max_width = 20;

        let wrapped = wrap_text(text, max_width);

        assert!(
            wrapped.lines.is_empty(),
            "Empty text should produce empty lines"
        );
    }

    #[test]
    fn test_wrap_text_single_word() {
        let text = "Supercalifragilisticexpialidocious";
        let max_width = 10;

        let wrapped = wrap_text(text, max_width);

        assert!(
            !wrapped.lines.is_empty(),
            "Result should contain wrapped lines"
        );

        let result_text = wrapped
            .lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<String>>()
            .join(" ");

        assert!(
            result_text.contains("Supercalifragilisticexpialidocious"),
            "Original word should be preserved"
        );
    }

    #[test]
    fn test_wrap_text_multiline() {
        let text = "This is line one.\nThis is line two.\nThis is line three.";
        let max_width = 30;

        let wrapped = wrap_text(text, max_width);

        assert!(
            wrapped.lines.len() >= 3,
            "Should have at least as many lines as input"
        );
    }

    #[test]
    fn test_create_button() {
        let button_selected = create_button("Test", true);
        let button_not_selected = create_button("Test", false);

        assert!(matches!(
            button_selected,
            ratatui::widgets::Paragraph { .. }
        ));
        assert!(matches!(
            button_not_selected,
            ratatui::widgets::Paragraph { .. }
        ));
    }

    #[test]
    fn test_create_bottom_right_shadow_layers() {
        let area = Rect::new(10, 10, 50, 20);
        let shadow_layers = [(Color::Red, 1), (Color::Blue, 2)];

        let result = create_bottom_right_shadow_layers(area, &shadow_layers);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0.x, 11);
        assert_eq!(result[0].0.y, 11);
        assert_eq!(result[1].0.x, 12);
        assert_eq!(result[1].0.y, 12);
    }

    #[test]
    fn test_render_shadow_layers() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let area = Rect::new(10, 10, 50, 20);
                let shadow_layers = vec![
                    (area, Style::default().bg(Color::Red)),
                    (Rect::new(11, 11, 50, 20), Style::default().bg(Color::Blue)),
                ];

                render_shadow_layers(frame, shadow_layers);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }
}
