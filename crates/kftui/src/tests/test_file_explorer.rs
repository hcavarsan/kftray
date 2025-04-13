use std::path::PathBuf;

use crossterm::event::KeyCode;

use crate::tui::input::{
    handle_export_file_explorer_input,
    handle_export_input_prompt,
    handle_import_file_explorer_input,
    App,
    AppState,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_file_explorer_app() -> App {
        let mut app = App::new();
        app.state = AppState::ImportFileExplorerOpen;
        app.selected_file_path = Some(PathBuf::from("/tmp"));
        app
    }

    #[tokio::test]
    async fn test_handle_import_file_explorer_escape() {
        let mut app = setup_file_explorer_app();

        handle_import_file_explorer_input(&mut app, KeyCode::Esc)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::Normal);
    }

    #[tokio::test]
    async fn test_handle_export_file_explorer_escape() {
        let mut app = setup_file_explorer_app();
        app.state = AppState::ExportFileExplorerOpen;

        handle_export_file_explorer_input(&mut app, KeyCode::Esc)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::Normal);
    }

    #[tokio::test]
    async fn test_handle_export_file_explorer_enter() {
        let mut app = setup_file_explorer_app();
        app.state = AppState::ExportFileExplorerOpen;

        handle_export_file_explorer_input(&mut app, KeyCode::Enter)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::ShowInputPrompt);
    }

    #[tokio::test]
    async fn test_handle_export_input_prompt() {
        let mut app = setup_file_explorer_app();
        app.state = AppState::ShowInputPrompt;
        app.input_buffer = "test.json".to_string();

        handle_export_input_prompt(&mut app, KeyCode::Enter)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::ShowConfirmationPopup);

        let mut app = setup_file_explorer_app();
        app.state = AppState::ShowInputPrompt;

        handle_export_input_prompt(&mut app, KeyCode::Esc)
            .await
            .unwrap();

        assert_eq!(app.state, AppState::Normal);

        let mut app = setup_file_explorer_app();
        app.state = AppState::ShowInputPrompt;
        app.input_buffer = "test".to_string();

        handle_export_input_prompt(&mut app, KeyCode::Char('a'))
            .await
            .unwrap();

        assert_eq!(app.input_buffer, "testa");
        assert_eq!(app.state, AppState::ShowInputPrompt);

        handle_export_input_prompt(&mut app, KeyCode::Backspace)
            .await
            .unwrap();

        assert_eq!(app.input_buffer, "test");
        assert_eq!(app.state, AppState::ShowInputPrompt);
    }
}
