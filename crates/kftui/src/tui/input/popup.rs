use crossterm::event::KeyCode;

use crate::tui::input::{
    App,
    AppState,
};

pub async fn handle_confirmation_popup_input(app: &mut App, key: KeyCode) -> std::io::Result<()> {
    if key == KeyCode::Enter || key == KeyCode::Esc {
        app.state = AppState::Normal;
        app.import_export_message = None;
    }
    Ok(())
}
