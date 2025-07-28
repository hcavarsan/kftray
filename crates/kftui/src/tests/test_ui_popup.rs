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
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    fn create_test_app() -> App {
        let mut app = App::new(test_logger_state());
        app.contexts = vec!["context1".to_string(), "context2".to_string()];
        app
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
                render_input_prompt(frame, input_buffer, area);
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
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 100, 50);
                render_about_popup(frame, area);
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
                render_error_popup(frame, error_message, area, 2);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
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
