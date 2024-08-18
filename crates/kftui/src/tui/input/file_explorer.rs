use std::path::Path;

use crossterm::event::{
    Event,
    KeyCode,
    KeyEvent,
    KeyModifiers,
};
use ratatui_explorer::Input;

use crate::tui::input::App;
use crate::tui::input::AppState;
use crate::utils::config::{
    export_configs_to_file,
    import_configs_from_file,
};
use crate::utils::file::get_file_content;

async fn handle_file_selection(app: &mut App, selected_path: &Path) -> Result<(), std::io::Error> {
    if selected_path.is_file() {
        if selected_path.extension().and_then(|s| s.to_str()) == Some("json") {
            match get_file_content(selected_path) {
                Ok(content) => app.file_content = Some(content),
                Err(e) => handle_file_error(app, e),
            }
        } else {
            app.file_content = None;
        }
    } else {
        app.file_content = None;
    }
    Ok(())
}

fn handle_file_error(app: &mut App, error: std::io::Error) {
    let error_message = format!("Failed to read file content: {}", error);
    app.file_content = None;
    app.import_export_message = Some(error_message.clone());
    app.error_message = Some(error_message);
    app.state = AppState::ShowErrorPopup;
}

async fn handle_import(app: &mut App, selected_path: &Path) -> Result<(), std::io::Error> {
    if selected_path.is_file() {
        match import_configs_from_file(selected_path.to_str().unwrap()).await {
            Ok(_) => show_confirmation_popup(app, "Import successful".to_string()),
            Err(e) => show_error_popup(app, format!("Import failed: {}", e)),
        }
    } else {
        show_error_popup(app, "Selected file is not a JSON file".to_string());
    }
    Ok(())
}

async fn handle_export(app: &mut App, export_path: &Path) -> Result<(), std::io::Error> {
    log::debug!("Starting export of configs to file: {:?}", export_path);

    match export_configs_to_file(export_path.to_str().unwrap()).await {
        Ok(_) => {
            log::debug!("Export successful");

            show_confirmation_popup(app, format!("Export successful: {:?}", export_path));
        }
        Err(e) => {
            log::error!("Export failed: {}", e);
            show_error_popup(app, format!("Export failed: {:?}", e));
        }
    }
    Ok(())
}

fn show_confirmation_popup(app: &mut App, message: String) {
    app.import_export_message = Some(message);
    app.state = AppState::ShowConfirmationPopup;
    log::debug!("State changed to ShowConfirmationPopup");
}

fn show_error_popup(app: &mut App, message: String) {
    app.import_export_message = Some(message.clone());
    app.error_message = Some(message);
    app.state = AppState::ShowErrorPopup;
    log::debug!("State changed to ShowErrorPopup");
}

pub async fn handle_import_file_explorer_input(
    app: &mut App, key: KeyCode,
) -> Result<(), std::io::Error> {
    let key_event = KeyEvent::new(key, KeyModifiers::NONE);

    if key == KeyCode::Enter {
        handle_import_enter_key(app).await?;
        return Ok(());
    }

    app.import_file_explorer
        .handle(Input::from(&Event::Key(key_event)))?;

    match key {
        KeyCode::Esc => close_import_file_explorer(app),
        KeyCode::Backspace => navigate_to_parent_directory(app),
        _ => handle_file_selection_key(app).await?,
    }
    Ok(())
}

async fn handle_import_enter_key(app: &mut App) -> Result<(), std::io::Error> {
    if let Some(selected_path) = app
        .import_file_explorer
        .files()
        .get(app.import_file_explorer.selected_idx())
        .map(|f| f.path().clone())
    {
        if selected_path.is_dir() {
            app.import_file_explorer
                .set_cwd(selected_path.clone())
                .unwrap();
        } else if selected_path.extension().and_then(|s| s.to_str()) == Some("json") {
            handle_import(app, &selected_path).await?;
        } else {
            show_error_popup(app, "Selected file is not a JSON file".to_string());
        }
    }
    Ok(())
}

fn navigate_to_parent_directory(app: &mut App) {
    if let Some(parent_path) = app.import_file_explorer.cwd().parent() {
        app.import_file_explorer
            .set_cwd(parent_path.to_path_buf())
            .unwrap();
    }
}

async fn handle_file_selection_key(app: &mut App) -> Result<(), std::io::Error> {
    if let Some(selected_path) = app
        .import_file_explorer
        .files()
        .get(app.import_file_explorer.selected_idx())
        .map(|f| f.path().clone())
    {
        handle_file_selection(app, &selected_path).await?;
    }
    Ok(())
}

pub async fn handle_export_file_explorer_input(
    app: &mut App, key: KeyCode,
) -> Result<(), std::io::Error> {
    let key_event = KeyEvent::new(key, KeyModifiers::NONE);

    if key == KeyCode::Enter || key == KeyCode::Char(' ') {
        handle_export_enter_key(app).await?;
        return Ok(());
    }

    app.export_file_explorer
        .handle(Input::from(&Event::Key(key_event)))?;

    match app.state {
        AppState::ShowInputPrompt => handle_export_input_prompt(app, key).await?,
        _ => match key {
            KeyCode::Esc => close_export_file_explorer(app),
            KeyCode::Backspace => navigate_to_parent_directory(app),
            _ => log::debug!("Unhandled key: {:?}", key),
        },
    }
    Ok(())
}

async fn handle_export_enter_key(app: &mut App) -> Result<(), std::io::Error> {
    if let Some(selected_file) = app
        .export_file_explorer
        .files()
        .get(app.export_file_explorer.selected_idx())
    {
        let selected_path = selected_file.path().clone();
        log::debug!("Selected path: {:?}", selected_path);

        if selected_file.is_dir() {
            log::debug!("Changed working directory to: {:?}", selected_path);
            app.selected_file_path = Some(selected_path.clone());
            app.state = AppState::ShowInputPrompt;
        }
    } else {
        log::warn!("No file selected");
    }
    Ok(())
}

pub async fn handle_export_input_prompt(app: &mut App, key: KeyCode) -> Result<(), std::io::Error> {
    log::debug!("Handling input prompt key: {:?}", key);

    match key {
        KeyCode::Enter => handle_export_enter_key_press(app).await?,
        KeyCode::Char(c) => update_input_buffer(app, c),
        KeyCode::Backspace => remove_last_char_from_input_buffer(app),
        KeyCode::Esc => cancel_input_prompt(app),
        _ => log::debug!("Unhandled key in input prompt: {:?}", key),
    }
    Ok(())
}

async fn handle_export_enter_key_press(app: &mut App) -> Result<(), std::io::Error> {
    if let Some(selected_file_path) = &app.selected_file_path {
        let export_path = selected_file_path.join(&app.input_buffer);
        log::debug!("Export path: {:?}", export_path);
        handle_export(app, &export_path).await?;
        app.input_buffer.clear();
    } else {
        log::error!("No selected file path for export");
    }
    app.input_buffer.clear();
    Ok(())
}

fn update_input_buffer(app: &mut App, c: char) {
    app.input_buffer.push(c);
    log::debug!("Input buffer updated: {}", app.input_buffer);
}

fn remove_last_char_from_input_buffer(app: &mut App) {
    app.input_buffer.pop();
    log::debug!("Input buffer updated: {}", app.input_buffer);
}

fn cancel_input_prompt(app: &mut App) {
    app.state = AppState::Normal;
    app.input_buffer.clear();
    log::debug!("Input prompt canceled");
}

fn close_import_file_explorer(app: &mut App) {
    log::debug!("File explorer closed");
    app.state = AppState::Normal;
    app.file_content = None;
    app.selected_file_path = std::env::current_dir().ok();
}

fn close_export_file_explorer(app: &mut App) {
    log::debug!("File explorer closed");
    app.state = AppState::Normal;
    app.file_content = None;
    app.selected_file_path = std::env::current_dir().ok();
}
