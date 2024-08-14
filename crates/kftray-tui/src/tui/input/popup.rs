use crossterm::event::KeyCode;

use crate::tui::input::{
    App,
    AppState,
};

pub async fn handle_confirmation_popup_input(app: &mut App, key: KeyCode) -> std::io::Result<()> {
    if key == KeyCode::Enter {
        app.state = AppState::Normal;
        app.import_export_message = None;
    }
    Ok(())
}

pub fn handle_help_input(app: &mut App, key: KeyCode) -> std::io::Result<()> {
    if key == KeyCode::Esc || key == KeyCode::Char('h') {
        app.state = AppState::Normal;
    }
    Ok(())
}

pub fn handle_about_input(app: &mut App, key: KeyCode) -> std::io::Result<()> {
    if key == KeyCode::Esc || key == KeyCode::Char('i') {
        app.state = AppState::Normal;
    }
    Ok(())
}

pub fn handle_error_popup_input(app: &mut App, key: KeyCode) -> std::io::Result<()> {
    if key == KeyCode::Enter || key == KeyCode::Esc {
        app.error_message = None;
        app.state = AppState::Normal;
    }
    Ok(())
}
