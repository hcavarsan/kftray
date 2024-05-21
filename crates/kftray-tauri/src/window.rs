use std::fs;

use tauri_plugin_positioner::{
    Position,
    WindowExt,
};

use crate::models::window::WindowPosition;

pub fn save_window_position(window: &tauri::Window) {
    if let Ok(position) = window.outer_position() {
        let position = WindowPosition {
            x: position.x as f64,
            y: position.y as f64,
        };
        let position_json = serde_json::to_string(&position).unwrap();

        let mut home_path = dirs::home_dir().unwrap();
        home_path.push(".kftray");
        fs::create_dir_all(&home_path).unwrap();
        home_path.push("window_position.json");

        fs::write(&home_path, position_json).unwrap();

        println!("Window position saved to home directory: {:?}", home_path);
    } else {
        println!("Failed to get window position.");
    }
}

pub fn load_window_position() -> Option<WindowPosition> {
    let mut home_path = dirs::home_dir().unwrap();
    home_path.push(".kftray");
    home_path.push("window_position.json");

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
                    eprintln!("Failed to parse window position JSON: {}", e);
                    if let Err(delete_err) = fs::remove_file(&home_path) {
                        eprintln!(
                            "Failed to delete corrupted window position file: {}",
                            delete_err
                        );
                    }
                    None
                }
            },
            Err(e) => {
                eprintln!("Failed to read window position file: {}", e);
                if let Err(delete_err) = fs::remove_file(&home_path) {
                    eprintln!(
                        "Failed to delete corrupted window position file: {}",
                        delete_err
                    );
                }
                None
            }
        }
    } else {
        println!("No window position file found.");
        None
    }
}

pub fn toggle_window_visibility(window: &tauri::Window) {
    if window.is_visible().unwrap() && window.is_focused().unwrap() {
        save_window_position(window);
        println!("Hiding window after toggle");
        window.hide().unwrap();
    } else {
        if let Some(position) = load_window_position() {
            println!(
                "Setting window position to: x: {}, y: {}",
                position.x, position.y
            );
            window
                .set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                    position.x, position.y,
                )))
                .unwrap();
        } else {
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
        }
        window.show().unwrap();
        window.set_focus().unwrap();
    }
}

pub fn reset_window_position(window: &tauri::Window) {
    save_window_position(window);
    println!("Hiding window before resetting position.");
    window.hide().unwrap();
    println!("Window hidden successfully.");
    let mut home_path = dirs::home_dir().unwrap();
    home_path.push(".kftray");
    home_path.push("window_position.json");

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
