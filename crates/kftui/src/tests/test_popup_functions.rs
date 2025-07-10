use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::tui::ui::popup::{
    create_bottom_right_shadow_layers,
    create_button,
    render_shadow_layers,
};

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::style::Style;
    use ratatui::Terminal;

    use super::*;

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
