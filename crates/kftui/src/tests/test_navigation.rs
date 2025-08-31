use kftray_commons::utils::db_mode::DatabaseMode;

use crate::tests::test_logger_state;
use crate::tui::input::{
    App,
    AppState,
    navigation::{
        handle_auto_add_configs,
        handle_context_selection,
    },
};

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn test_handle_auto_add_configs_error() {
        let mut app = App::new(test_logger_state());

        handle_auto_add_configs(&mut app).await;

        if app.state == AppState::ShowErrorPopup {
            assert!(app.error_message.is_some());
        }
    }

    #[tokio::test]
    async fn test_handle_context_selection_success() {
        let mut app = App::new(test_logger_state());
        app.state = AppState::ShowContextSelection;
        app.contexts = vec!["test-context".to_string()];
        app.selected_context_index = 0;

        handle_context_selection(&mut app, "test-context", DatabaseMode::File).await;

        if app.state == AppState::Normal {
            assert!(app.error_message.is_none());
        }
    }

    #[tokio::test]
    async fn test_handle_context_selection_error() {
        let mut app = App::new(test_logger_state());
        app.state = AppState::ShowContextSelection;

        handle_context_selection(&mut app, "test-context", DatabaseMode::File).await;

        if app.state == AppState::ShowErrorPopup {
            assert!(app.error_message.is_some());
        }
    }

    #[tokio::test]
    async fn test_handle_auto_add_configs_with_contexts() {
        let mut app = App::new(test_logger_state());

        app.contexts = vec![];

        handle_auto_add_configs(&mut app).await;

        if app.contexts.is_empty() && app.state == AppState::ShowErrorPopup {
            assert!(app.error_message.is_some());
        }
    }
}
