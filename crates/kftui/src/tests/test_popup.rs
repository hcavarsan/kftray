use crossterm::event::KeyCode;

use crate::tui::input::{
    handle_about_input,
    handle_error_popup_input,
    handle_help_input,
    App,
    AppState,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_error_popup_input() {
        let mut app = App::new();
        app.state = AppState::ShowErrorPopup;
        app.error_message = Some("Test error".to_string());

        handle_error_popup_input(&mut app, KeyCode::Enter).unwrap();
        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.error_message, None);

        app.state = AppState::ShowErrorPopup;
        app.error_message = Some("Another test error".to_string());

        handle_error_popup_input(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.state, AppState::Normal);
        assert_eq!(app.error_message, None);
    }

    #[test]
    fn test_handle_help_input() {
        let mut app = App::new();
        app.state = AppState::ShowHelp;

        // Enter should not change state
        handle_help_input(&mut app, KeyCode::Enter).unwrap();
        assert_eq!(app.state, AppState::ShowHelp);

        app.state = AppState::ShowHelp;
        handle_help_input(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowHelp;
        handle_help_input(&mut app, KeyCode::Char('h')).unwrap();
        assert_eq!(app.state, AppState::Normal);
    }

    #[test]
    fn test_handle_about_input() {
        let mut app = App::new();
        app.state = AppState::ShowAbout;

        handle_about_input(&mut app, KeyCode::Enter).unwrap();
        assert_eq!(app.state, AppState::ShowAbout);

        app.state = AppState::ShowAbout;
        handle_about_input(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.state, AppState::Normal);

        app.state = AppState::ShowAbout;
        handle_about_input(&mut app, KeyCode::Char('q')).unwrap();
        assert_eq!(app.state, AppState::Normal);
    }
}
