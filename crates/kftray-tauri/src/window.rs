use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{
    Manager,
    Window,
};
use tauri_plugin_positioner::{
    Position,
    WindowExt,
};
use tokio::time::sleep;

use crate::models::window::WindowPosition;
use crate::AppState;

const CONFIG_DIR: &str = ".kftray";
const POSITION_FILE: &str = "window_position.json";

pub fn save_window_position(window: &Window) {
    if let Ok(position) = window.outer_position() {
        let position = WindowPosition {
            x: position.x,
            y: position.y,
        };
        let position_json = serde_json::to_string(&position).unwrap();

        let mut home_path = dirs::home_dir().unwrap();
        home_path.push(CONFIG_DIR);
        fs::create_dir_all(&home_path).unwrap();
        home_path.push(POSITION_FILE);

        fs::write(&home_path, position_json).unwrap();
        println!("Window position saved: {:?}", position);
    } else {
        println!("Failed to get window position.");
    }
}

pub fn load_window_position() -> Option<WindowPosition> {
    let mut home_path = dirs::home_dir().unwrap();
    home_path.push(CONFIG_DIR);
    home_path.push(POSITION_FILE);

    if home_path.exists() {
        match fs::read_to_string(&home_path) {
            Ok(position_json) => match serde_json::from_str(&position_json) {
                Ok(position) => {
                    println!(
                        "Window position loaded from home directory: {:?}",
                        home_path
                    );
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
        println!("No window position file found.");
        None
    }
}

fn handle_corrupted_file(path: &Path, error: impl std::fmt::Display) {
    eprintln!("Failed to parse window position JSON: {}", error);
    if let Err(delete_err) = fs::remove_file(path) {
        eprintln!(
            "Failed to delete corrupted window position file: {}",
            delete_err
        );
    }
}

pub fn toggle_window_visibility(window: &Window) {
    if window.is_visible().unwrap() {
        window.hide().unwrap();
    } else {
        window.show().unwrap();
        set_default_position(window);
        window.set_focus().unwrap();
    }
}

pub fn set_default_position(window: &Window) {
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

pub fn is_valid_position(window: &Window, x: i32, y: i32) -> bool {
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

pub fn reset_to_default_position(window: &Window) {
    let app_state = window.state::<AppState>();
    app_state.is_plugin_moving.store(true, Ordering::SeqCst);

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = window.move_window(Position::Center) {
            eprintln!("Failed to move window to center: {}", e);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if let Err(e) = window.move_window(Position::TrayCenter) {
            eprintln!("Failed to move window to tray center: {}", e);
        }
    }

    reset_plugin_moving_state_after_delay(&app_state);
}

pub fn reset_window_position(window: &Window) {
    let app_state = window.state::<AppState>();
    app_state.is_plugin_moving.store(true, Ordering::SeqCst);

    remove_position_file();
    reset_to_default_position(window);
    reset_plugin_moving_state_after_delay(&app_state);
}

fn remove_position_file() {
    let mut home_path = dirs::home_dir().unwrap();
    home_path.push(CONFIG_DIR);
    home_path.push(POSITION_FILE);

    if home_path.exists() {
        if let Err(e) = fs::remove_file(&home_path) {
            eprintln!("Failed to delete window position file: {}", e);
        } else {
            println!("Window position file deleted successfully.");
        }
    } else {
        println!("No window position file found to delete.");
    }
}

pub fn set_window_position(window: &Window, position: Position) {
    if let Err(e) = window.move_window(position) {
        eprintln!("Failed to move window: {}", e);
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
    window: &Window, _scale_factor: f64, new_inner_size: tauri::PhysicalSize<u32>,
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
