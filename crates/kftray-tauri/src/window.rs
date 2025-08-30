use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::WindowPosition;
use kftray_commons::utils::config_dir::get_window_state_path;
use log::{
    info,
    warn,
};
use tauri::{
    Manager,
    WebviewWindow,
    Wry,
};
use tauri_plugin_positioner::{
    Position,
    WindowExt,
};
use tokio::time::sleep;

pub fn save_window_position(window: &WebviewWindow<Wry>) {
    match window.outer_position() {
        Ok(position) => {
            let position_data = WindowPosition {
                x: position.x,
                y: position.y,
            };
            let position_json = serde_json::to_string(&position_data).unwrap();

            match get_window_state_path() {
                Ok(path) => {
                    if let Some(parent_dir) = path.parent() {
                        if let Err(e) = fs::create_dir_all(parent_dir) {
                            info!("Failed to create config directory: {e}");
                            return;
                        }
                    }

                    if fs::write(&path, position_json).is_ok() {
                        info!("Window position saved: {position_data:?}");
                    } else {
                        info!("Failed to save window position.");
                    }
                }
                Err(err) => info!("Failed to get window state path: {err}"),
            }
        }
        _ => {
            info!("Failed to get window position.");
        }
    }
}

pub fn load_window_position() -> Option<WindowPosition> {
    if let Ok(home_path) = get_window_state_path() {
        if home_path.exists() {
            match fs::read_to_string(&home_path) {
                Ok(position_json) => match serde_json::from_str(&position_json) {
                    Ok(position) => {
                        info!("Window position loaded from home directory: {home_path:?}");
                        Some(position)
                    }
                    Err(e) => {
                        handle_corrupted_file(&home_path, e);
                        None
                    }
                },
                Err(e) => {
                    handle_corrupted_file(&home_path, e);
                    None
                }
            }
        } else {
            info!("No window position file found.");
            None
        }
    } else {
        info!("Could not determine window state path.");
        None
    }
}

fn handle_corrupted_file(path: &Path, error: impl std::fmt::Display) {
    warn!("Failed to parse window position JSON: {error}");
    if let Err(delete_err) = fs::remove_file(path) {
        warn!("Failed to delete corrupted window position file: {delete_err}");
    }
}

pub fn toggle_window_visibility(window: &WebviewWindow<Wry>) {
    let app_state = window.state::<AppState>();
    if window.is_visible().unwrap() {
        if !app_state.is_pinned.load(Ordering::SeqCst) {
            window.hide().unwrap();
        }
    } else {
        window.show().unwrap();
        set_default_position(window);
        window.set_focus().unwrap();
    }
}

pub fn set_default_position(window: &WebviewWindow<Wry>) {
    let app_state = window.state::<AppState>();
    app_state.is_plugin_moving.store(true, Ordering::SeqCst);

    if let Some(position) = load_window_position() {
        if is_valid_position(window, position.x, position.y) {
            window
                .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                    position.x, position.y,
                )))
                .unwrap();
        } else {
            reset_to_default_position(window);
        }
    } else {
        reset_to_default_position(window);
    }

    reset_plugin_moving_state_after_delay(&app_state);
}

pub fn is_valid_position(window: &WebviewWindow<Wry>, x: i32, y: i32) -> bool {
    if let Ok(monitors) = window.available_monitors() {
        for monitor in monitors {
            let monitor_position = monitor.position();
            let monitor_size = monitor.size();

            let min_x = monitor_position.x;
            let min_y = monitor_position.y;
            let max_x = min_x + monitor_size.width as i32;
            let max_y = min_y + monitor_size.height as i32;

            if x >= min_x && x <= max_x && y >= min_y && y <= max_y {
                return true;
            }
        }
    }

    false
}

pub fn reset_to_default_position(window: &WebviewWindow<Wry>) {
    let app_state = window.state::<AppState>();
    app_state.is_plugin_moving.store(true, Ordering::SeqCst);

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = window.move_window(Position::Center) {
            warn!("Failed to move window to center: {}", e);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if let Err(e) = window.move_window(Position::TrayCenter) {
            warn!("Failed to move window to tray center: {e}");
        }
    }

    reset_plugin_moving_state_after_delay(&app_state);
}

pub fn reset_window_position(window: &WebviewWindow<Wry>) {
    let app_state = window.state::<AppState>();
    app_state.is_plugin_moving.store(true, Ordering::SeqCst);

    remove_position_file();
    reset_to_default_position(window);
    reset_plugin_moving_state_after_delay(&app_state);
}

fn remove_position_file() {
    if let Ok(path) = get_window_state_path() {
        if path.exists() {
            if let Err(e) = fs::remove_file(&path) {
                warn!("Failed to delete window position file: {e}");
            } else {
                info!("Window position file deleted successfully.");
            }
        } else {
            info!("No window position file found to delete.");
        }
    } else {
        info!("Could not determine window state path.");
    }
}

pub fn set_window_position(window: &WebviewWindow<Wry>, position: Position) {
    if let Err(e) = window.move_window(position) {
        warn!("Failed to move window: {e}");
    }
}

fn reset_plugin_moving_state_after_delay(app_state: &AppState) {
    let is_plugin_moving = app_state.is_plugin_moving.clone();
    app_state.runtime.spawn(async move {
        sleep(Duration::from_millis(150)).await;
        is_plugin_moving.store(false, Ordering::SeqCst);
    });
}

pub fn adjust_window_size_and_position(
    window: &WebviewWindow<Wry>, _scale_factor: f64, new_inner_size: tauri::PhysicalSize<u32>,
) {
    let app_state = window.state::<AppState>();
    app_state.is_plugin_moving.store(true, Ordering::SeqCst);

    let new_width = (new_inner_size.width as f64) as u32;
    let new_height = (new_inner_size.height as f64) as u32;

    window
        .set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
            new_width, new_height,
        )))
        .unwrap();

    reset_plugin_moving_state_after_delay(&app_state);
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_handle_corrupted_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("corrupted.json");

        fs::write(&test_file, "corrupted data").unwrap();
        assert!(test_file.exists());

        handle_corrupted_file(&test_file, "Test error");

        assert!(!test_file.exists());
    }

    #[test]
    fn test_window_position_serialization() {
        let position = WindowPosition { x: 100, y: 200 };
        let json = serde_json::to_string(&position).unwrap();

        assert!(json.contains("100"), "JSON should contain x value");
        assert!(json.contains("200"), "JSON should contain y value");

        let deserialized: WindowPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.x, 100);
        assert_eq!(deserialized.y, 200);
    }

    #[test]
    fn test_save_load_window_position() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("window_state.json");

        let position = WindowPosition { x: 100, y: 200 };
        let json = serde_json::to_string(&position).unwrap();

        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        fs::write(&file_path, json).unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        let loaded_position: WindowPosition = serde_json::from_str(&content).unwrap();

        assert_eq!(loaded_position.x, 100);
        assert_eq!(loaded_position.y, 200);
    }

    #[test]
    fn test_create_directory_for_window_position() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("config/window_state.json");

        if file_path.parent().unwrap().exists() {
            fs::remove_dir_all(file_path.parent().unwrap()).unwrap();
        }

        if let Some(parent_dir) = file_path.parent() {
            fs::create_dir_all(parent_dir).unwrap();
            assert!(parent_dir.exists(), "Directory should be created");
        }
    }

    #[test]
    fn test_is_valid_position() {
        let screen_width = 1920;
        let screen_height = 1080;

        let valid_positions = [(100, 100), (0, 0), (screen_width - 1, screen_height - 1)];

        for (x, y) in valid_positions {
            assert!(x >= 0 && x < screen_width, "X position should be valid");
            assert!(y >= 0 && y < screen_height, "Y position should be valid");
        }

        let invalid_positions = [
            (-100, 100),
            (100, -100),
            (screen_width + 100, screen_height + 100),
        ];

        for (x, y) in invalid_positions {
            assert!(
                !(x >= 0 && x < screen_width && y >= 0 && y < screen_height),
                "Position should be invalid"
            );
        }
    }
}
