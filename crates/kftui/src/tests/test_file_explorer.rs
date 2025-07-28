use std::path::PathBuf;

use crossterm::event::KeyCode;
use kftray_commons::utils::db_mode::DatabaseMode;

use crate::tests::test_logger_state;
use crate::tui::input::handle_import_file_explorer_input;
use crate::tui::input::{
    show_confirmation_popup,
    show_error_popup,
    App,
    AppState,
};
#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;

    use super::*;
    use crate::tui::input::file_explorer::handle_export_enter_key;
    use crate::tui::input::file_explorer::handle_file_error;
    use crate::tui::input::file_explorer::handle_file_selection;
    use crate::tui::input::file_explorer::handle_file_selection_key;
    use crate::tui::input::file_explorer::handle_import_enter_key;
    use crate::tui::input::file_explorer::navigate_to_parent_directory;
    use crate::tui::input::handle_export_file_explorer_input;
    use crate::tui::input::handle_export_input_prompt;

    fn setup_file_explorer_app() -> App {
        let mut app = App::new(test_logger_state());
        app.state = AppState::ImportFileExplorerOpen;
        app.selected_file_path = Some(PathBuf::from("/tmp"));
        app
    }

    #[tokio::test]
    async fn test_handle_import_file_explorer_escape() {
        let mut app = setup_file_explorer_app();

        handle_import_file_explorer_input(&mut app, KeyCode::Esc, DatabaseMode::File)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::Normal);
    }

    #[tokio::test]
    async fn test_handle_export_file_explorer_escape() {
        let mut app = setup_file_explorer_app();
        app.state = AppState::ExportFileExplorerOpen;

        handle_export_file_explorer_input(&mut app, KeyCode::Esc, DatabaseMode::File)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::Normal);
    }

    #[tokio::test]
    async fn test_handle_export_file_explorer_enter() {
        let mut app = setup_file_explorer_app();
        app.state = AppState::ExportFileExplorerOpen;

        handle_export_file_explorer_input(&mut app, KeyCode::Enter, DatabaseMode::File)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::ShowInputPrompt);
    }

    #[tokio::test]
    async fn test_handle_export_input_prompt() {
        let mut app = setup_file_explorer_app();
        app.state = AppState::ShowInputPrompt;
        app.input_buffer = "test.json".to_string();

        handle_export_input_prompt(&mut app, KeyCode::Enter, DatabaseMode::File)
            .await
            .unwrap();

        assert!(
            app.state == AppState::ShowConfirmationPopup || app.state == AppState::ShowErrorPopup,
            "App state should be either ShowConfirmationPopup or ShowErrorPopup after export"
        );

        let mut app = setup_file_explorer_app();
        app.state = AppState::ShowInputPrompt;

        handle_export_input_prompt(&mut app, KeyCode::Esc, DatabaseMode::File)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::Normal);

        let mut app = setup_file_explorer_app();
        app.state = AppState::ShowInputPrompt;
        app.input_buffer = "test".to_string();

        handle_export_input_prompt(&mut app, KeyCode::Char('a'), DatabaseMode::File)
            .await
            .unwrap();

        assert_eq!(app.input_buffer, "testa");
        assert_eq!(app.state, AppState::ShowInputPrompt);

        handle_export_input_prompt(&mut app, KeyCode::Backspace, DatabaseMode::File)
            .await
            .unwrap();

        assert_eq!(app.input_buffer, "test");
        assert_eq!(app.state, AppState::ShowInputPrompt);
    }

    #[test]
    fn test_show_confirmation_popup() {
        let mut app = setup_file_explorer_app();
        let message = "Test confirmation".to_string();

        show_confirmation_popup(&mut app, message.clone());

        assert_eq!(app.state, AppState::ShowConfirmationPopup);
        assert_eq!(app.import_export_message, Some(message));
    }

    #[test]
    fn test_show_error_popup() {
        let mut app = setup_file_explorer_app();
        let message = "Test error".to_string();

        show_error_popup(&mut app, message.clone());

        assert_eq!(app.state, AppState::ShowErrorPopup);
        assert_eq!(app.error_message, Some(message.clone()));
        assert_eq!(app.import_export_message, Some(message));
    }

    #[test]
    fn test_handle_file_error() {
        let mut app = setup_file_explorer_app();
        let error = io::Error::new(io::ErrorKind::NotFound, "File not found");

        handle_file_error(&mut app, error);

        assert_eq!(app.state, AppState::ShowErrorPopup);
        assert!(app.error_message.is_some());
        assert!(app.import_export_message.is_some());
        assert!(app.file_content.is_none());
    }

    #[tokio::test]
    async fn test_handle_file_selection() {
        let mut app = setup_file_explorer_app();

        let path = Path::new("/tmp/nonexistent_file.json");
        handle_file_selection(&mut app, path).await.unwrap();
        assert!(app.file_content.is_none());

        let path = Path::new("/tmp");
        handle_file_selection(&mut app, path).await.unwrap();
        assert!(app.file_content.is_none());
    }

    #[tokio::test]
    async fn test_handle_import_enter_key() {
        let mut app = setup_file_explorer_app();

        let result = handle_import_enter_key(&mut app, DatabaseMode::File).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_export_enter_key() {
        let mut app = setup_file_explorer_app();

        let result = handle_export_enter_key(&mut app).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_navigate_to_parent_directory() {
        let mut app = setup_file_explorer_app();

        navigate_to_parent_directory(&mut app);
    }

    #[tokio::test]
    async fn test_handle_file_selection_key() {
        let mut app = setup_file_explorer_app();

        let result = handle_file_selection_key(&mut app).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_export_file_explorer_backspace() {
        let mut app = setup_file_explorer_app();
        app.state = AppState::ExportFileExplorerOpen;

        handle_export_file_explorer_input(&mut app, KeyCode::Backspace, DatabaseMode::File)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_handle_export_file_explorer_space() {
        let mut app = setup_file_explorer_app();
        app.state = AppState::ExportFileExplorerOpen;

        handle_export_file_explorer_input(&mut app, KeyCode::Char(' '), DatabaseMode::File)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_handle_import_file_explorer_backspace() {
        let mut app = setup_file_explorer_app();

        handle_import_file_explorer_input(&mut app, KeyCode::Backspace, DatabaseMode::File)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_handle_import_file_explorer_other_key() {
        let mut app = setup_file_explorer_app();

        handle_import_file_explorer_input(&mut app, KeyCode::Down, DatabaseMode::File)
            .await
            .unwrap();
    }
}
