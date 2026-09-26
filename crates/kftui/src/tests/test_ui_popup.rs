use ratatui::layout::Rect;

use crate::tests::test_logger_state;
use crate::tui::input::{
    App,
    DeleteButton,
};
use crate::tui::ui::popup::{
    render_about_popup,
    render_background_overlay,
    render_confirmation_popup,
    render_context_selection_popup,
    render_delete_confirmation_popup,
    render_error_popup,
    render_help_popup,
    render_input_prompt,
};

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn create_test_app() -> App {
        let mut app = App::new(test_logger_state());
        app.contexts = vec!["context1".to_string(), "context2".to_string()];
        app
    }

    fn rendered_screen(width: u16, height: u16, draw: impl FnOnce(&mut ratatui::Frame)) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(draw).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn test_render_background_overlay() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 50);
                render_background_overlay(frame, area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn test_render_input_prompt() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(10, 10, 50, 20);
                let input_buffer = "test input";
                render_input_prompt(frame, "Enter file name", input_buffer, area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn test_render_confirmation_popup() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(10, 10, 50, 20);
                let message = Some("Confirmation test".to_string());
                render_confirmation_popup(frame, &message, area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn test_render_help_popup() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(10, 10, 80, 40);
                render_help_popup(frame, area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn test_render_about_popup() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = create_test_app();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 50);
                render_about_popup(frame, &app, area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn test_render_error_popup() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 50);
                let error_message = "This is an error test message";
                let _ = render_error_popup(frame, error_message, area, 2, 0);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn a_long_error_stays_reachable_by_scrolling() {
        // One line per failed configuration: a batch can report more than the
        // popup shows, and dismissing it must not be the only way out.
        let error_message: String = (0..60)
            .map(|index| format!("config {index} failed to start: connection refused"))
            .collect::<Vec<_>>()
            .join("\n");
        let rendered = |scroll: usize| {
            rendered_screen(100, 24, |frame| {
                let area = Rect::new(0, 0, 100, 24);
                let _ = render_error_popup(frame, &error_message, area, 1, scroll);
            })
        };

        let top = rendered(0);
        let bottom = rendered(usize::MAX);
        assert!(
            top.contains("more line(s)"),
            "a clipped error must say so: {top}"
        );
        assert_ne!(
            top, bottom,
            "scrolling to the end must reveal failures the first page clipped"
        );
        assert!(
            bottom.contains("config 59"),
            "the last failure must be reachable"
        );
    }

    #[test]
    fn each_newline_separated_failure_starts_its_own_row() {
        let width = 60u16;
        let screen = rendered_screen(width, 20, |frame| {
            let area = Rect::new(0, 0, width, 20);
            let error_message = "config 1 failed: reason one\nconfig 2 failed: reason two";
            let _ = render_error_popup(frame, error_message, area, 0, 0);
        });

        let chars: Vec<char> = screen.chars().collect();
        let rows: Vec<String> = chars
            .chunks(width as usize)
            .map(|row| row.iter().collect())
            .collect();

        assert!(
            rows.iter().any(|row| row.contains("reason one")),
            "the first failure must be on screen: {screen}"
        );
        assert!(
            rows.iter().any(|row| row.contains("reason two")),
            "the second failure must be on screen: {screen}"
        );
        assert!(
            !rows
                .iter()
                .any(|row| row.contains("reason one") && row.contains("config 2")),
            "failures joined by \\n must not be merged onto the same rendered row: {screen}"
        );
    }

    #[test]
    fn a_narrow_popup_keeps_the_dismissal_key_visible() {
        // Sixteen columns leave an interior the wrapper gives up on.
        let screen = rendered_screen(16, 8, |frame| {
            let area = Rect::new(0, 0, 16, 8);
            let _ = render_error_popup(frame, "boom", area, 0, 0);
        });
        assert!(screen.contains("<Enter>"), "{screen}");
    }

    #[test]
    fn a_popup_with_one_interior_row_shows_only_the_way_out() {
        // A three-row area leaves one row inside the borders.
        let screen = rendered_screen(60, 3, |frame| {
            let area = Rect::new(0, 0, 60, 3);
            let _ = render_error_popup(frame, "first line\nsecond line", area, 0, 0);
        });
        assert!(screen.contains("<Enter>"), "{screen}");
    }

    #[test]
    fn a_short_terminal_still_shows_how_to_close_a_long_error() {
        use crate::tui::input::AppState;
        use crate::tui::ui::draw::draw_ui;

        // Through `draw_ui`, which sizes the popup area itself: rendering the
        // popup directly would skip the reduction that makes small terminals
        // tight in the first place.
        let mut app = create_test_app();
        app.state = AppState::ShowErrorPopup;
        app.error_message = Some(
            (0..40)
                .map(|index| format!("config {index} failed to start: connection refused"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        app.error_scroll = usize::MAX;

        let screen = rendered_screen(80, 15, |frame| draw_ui(frame, &mut app, &[]));

        assert!(
            screen.contains("<Enter>"),
            "the way to dismiss the popup must be on screen even at the last scroll offset: \
             {screen}"
        );
        assert!(
            screen.contains("config 39"),
            "the last failure must be reachable on a short terminal: {screen}"
        );
        assert!(
            app.error_scroll_max > 0,
            "a batch this long must have offsets to scroll through"
        );
        assert_eq!(
            app.error_scroll, app.error_scroll_max,
            "scrolling past the end clamps to the last offset"
        );
    }

    #[test]
    fn test_render_delete_confirmation_popup() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(10, 10, 50, 20);
                let message = Some("Delete confirmation test".to_string());
                render_delete_confirmation_popup(frame, &message, area, DeleteButton::Confirm);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }

    #[test]
    fn test_render_context_selection_popup() {
        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();

        terminal
            .draw(|frame| {
                let area = Rect::new(10, 10, 80, 40);
                render_context_selection_popup(frame, &mut app, area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }
}
