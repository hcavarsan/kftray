use crossterm::event::KeyCode;

use crate::tui::input::App;
use crate::tui::utils::export_configs_to_file;
pub async fn handle_confirmation_popup_input(app: &mut App, key: KeyCode) {
    if key == KeyCode::Enter {
        app.show_confirmation_popup = false;
        app.import_export_message = None;
    }
}

pub async fn handle_input_prompt_input(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            if let Some(selected_file_path) = &app.selected_file_path {
                let export_path: std::path::PathBuf = selected_file_path.join(&app.input_buffer);
                match export_configs_to_file(export_path.to_str().unwrap()).await {
                    Ok(_) => app.import_export_message = Some("Export successful".to_string()),
                    Err(e) => {
                        app.import_export_message = Some(format!("Export failed: {}", e));
                        app.show_error_popup = true;
                    }
                }
            }
            app.show_input_prompt = false;
            app.show_confirmation_popup = true;
        }
        KeyCode::Char(c) => {
            if c == ' ' {
                if let Some(selected_file_path) = &app.selected_file_path {
                    let export_path = selected_file_path.join(&app.input_buffer);
                    match export_configs_to_file(export_path.to_str().unwrap()).await {
                        Ok(_) => app.import_export_message = Some("Export successful".to_string()),
                        Err(e) => {
                            app.import_export_message = Some(format!("Export failed: {}", e));
                            app.show_error_popup = true;
                        }
                    }
                }
                app.show_input_prompt = false;
                app.show_confirmation_popup = true;
            } else {
                app.input_buffer.push(c);
            }
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
        }
        KeyCode::Esc => {
            app.show_input_prompt = false;
        }
        _ => {}
    }
}

pub fn handle_help_input(app: &mut App, key: KeyCode) {
    if key == KeyCode::Esc || key == KeyCode::Char('h') {
        app.show_help = false;
    }
}

pub async fn handle_search_input(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            app.show_search = false;
        }
        KeyCode::Char(c) => {
            app.input_buffer.push(c);
            filter_configs(app);
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
            filter_configs(app);
        }
        KeyCode::Esc => {
            app.show_search = false;
            app.input_buffer.clear();
            filter_configs(app);
        }
        _ => {}
    }
}

fn filter_configs(app: &mut App) {
    let query = app.input_buffer.to_lowercase();

    app.filtered_stopped_configs = app
        .stopped_configs
        .iter()
        .filter(|config| {
            config
                .alias
                .as_ref()
                .map_or_else(|| "".to_string(), |alias| alias.to_lowercase())
                .contains(&query)
        })
        .cloned()
        .collect();

    app.filtered_running_configs = app
        .running_configs
        .iter()
        .filter(|config| {
            config
                .alias
                .as_ref()
                .map_or_else(|| "".to_string(), |alias| alias.to_lowercase())
                .contains(&query)
        })
        .cloned()
        .collect();
}


pub fn handle_error_popup_input(app: &mut App, key: KeyCode) {
    if key == KeyCode::Enter || key == KeyCode::Esc {
        app.error_message = None;
        app.show_error_popup = false;
    }
}
