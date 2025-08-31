use crossterm::event::KeyCode;

use crate::tests::test_logger_state;
use crate::tui::input::{
    App,
    AppState,
    handle_about_input,
    handle_error_popup_input,
    handle_help_input,
    show_error_popup,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_error_popup() {
        let mut app = App::new(test_logger_state());
        let error_message = "Test error message";

        show_error_popup(&mut app, error_message.to_string());

        assert_eq!(app.state, AppState::ShowErrorPopup);
        assert_eq!(app.error_message, Some(error_message.to_string()));
    }

    #[test]
    fn test_handle_error_popup_enter() {
        let mut app = App::new(test_logger_state());
        app.state = AppState::ShowErrorPopup;
        app.error_message = Some("Test error".to_string());

        let result = handle_error_popup_input(&mut app, KeyCode::Enter);

        result.unwrap();
        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.error_message, None);
    }

    #[test]
    fn test_handle_error_popup_escape() {
        let mut app = App::new(test_logger_state());
        app.state = AppState::ShowErrorPopup;
        app.error_message = Some("Test error".to_string());

        let result = handle_error_popup_input(&mut app, KeyCode::Esc);

        result.unwrap();
        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.error_message, None);
    }

    #[test]
    fn test_handle_help_input() {
        let mut app = App::new(test_logger_state());
        app.state = AppState::ShowHelp;

        handle_help_input(&mut app, KeyCode::Enter).unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowHelp;
        handle_help_input(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowHelp;
        handle_help_input(&mut app, KeyCode::Char('h')).unwrap();
        assert_eq!(app.state, AppState::Normal);
    }

    #[test]
    fn test_handle_about_input() {
        let mut app = App::new(test_logger_state());
        app.state = AppState::ShowAbout;

        handle_about_input(&mut app, KeyCode::Enter).unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowAbout;
        handle_about_input(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowAbout;
        handle_about_input(&mut app, KeyCode::Char('q')).unwrap();
        assert_eq!(app.state, AppState::Normal);
    }
}
