use crossterm::event::{
    Event,
    KeyCode,
    KeyEvent,
    KeyModifiers,
};
use ratatui_explorer::Input;

use crate::tui::input::App;
use crate::tui::utils::{
    get_file_content,
    import_configs_from_file,
};

pub async fn handle_file_explorer_input(app: &mut App, key: KeyCode) -> Result<(), std::io::Error> {
    let key_event = KeyEvent::new(key, KeyModifiers::NONE);
    app.file_explorer
        .handle(Input::from(&Event::Key(key_event)))?;
    if key == KeyCode::Esc {
        app.file_explorer_open = false;
    } else if key == KeyCode::Enter || key == KeyCode::Char(' ') {
        let selected_idx = app.file_explorer.selected_idx();
        if let Some(selected_file) = app.file_explorer.files().get(selected_idx) {
            let selected_path = selected_file.path();
            if selected_path.is_dir() {
                app.selected_file_path = Some(selected_path.to_path_buf());
                app.file_explorer_open = false;
                app.show_input_prompt = true;
            } else if selected_path.is_file() {
                match import_configs_from_file(selected_path.to_str().unwrap()).await {
                    Ok(_) => app.import_export_message = Some("Import successful".to_string()),
                    Err(e) => {
                        app.import_export_message = Some(format!("Import failed: {}", e));
                        app.error_message = Some(format!("Import failed: {}", e));
                        app.show_error_popup = true;
                    }
                }
                app.file_explorer_open = false;
                app.show_confirmation_popup = true;

                match get_file_content(selected_path) {
                    Ok(content) => {
                        println!("File content read successfully");
                        app.file_content = Some(content);
                    }
                    Err(e) => {
                        println!("Failed to read file content: {}", e);
                        app.file_content = None;
                        app.import_export_message = Some(format!("Failed to read file content: {}", e));
                        app.error_message = Some(format!("Failed to read file content: {}", e));
                        app.show_error_popup = true;
                    }
                }
            }
        }
    } else {
        let selected_idx = app.file_explorer.selected_idx();
        if let Some(selected_file) = app.file_explorer.files().get(selected_idx) {
            let selected_path = selected_file.path();
            if selected_path.is_file() {
                match get_file_content(selected_path) {
                    Ok(content) => {
                        app.file_content = Some(content);
                    }
                    Err(e) => {
                        app.file_content = None;
                        app.import_export_message = Some(format!("Failed to read file content: {}", e));
                        app.error_message = Some(format!("Failed to read file content: {}", e));
                        app.show_error_popup = true;
                    }
                }
            } else {
                app.file_content = None;
            }
        }
    }
    Ok(())
}

