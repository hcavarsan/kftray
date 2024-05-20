use std::fs;

#[cfg(target_os = "linux")]
use enigo::{
    Enigo,
    Mouse,
    Settings,
};
#[cfg(not(target_os = "linux"))]
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

        // Ensure the file is empty before writing
        fs::write(&home_path, "").unwrap();
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
        let position_json = fs::read_to_string(&home_path).unwrap();
        let position: WindowPosition = serde_json::from_str(&position_json).unwrap();
        println!(
            "Window position loaded from home directory: {:?}",
            home_path
        );
        Some(position)
    } else {
        println!("No window position file found.");
        None
    }
}

#[cfg(target_os = "linux")]
pub fn move_window_to_mouse_position(window: &tauri::Window) {
    if let Ok(window_size) = window.inner_size() {
        let settings = Settings::default();
        let enigo = Enigo::new(&settings).unwrap();
        let mouse_position = enigo.location().unwrap();

        println!("Mouse Position: {:#?}", mouse_position);
        println!("Window Size: {:#?}", window_size);

        let window_width = window_size.width as f64;

        let offset_x = 50.0;

        let new_x = mouse_position.0 as f64 - window_width + offset_x;
        let new_y = mouse_position.1 as f64;

        println!("New Window Position: x: {}, y: {}", new_x, new_y);

        std::thread::sleep(Duration::from_millis(200));

        if let Err(e) = window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
            new_x, new_y,
        ))) {
            eprintln!("Failed to set window position: {}", e);
        }
    }
}

pub fn toggle_window_visibility(window: &tauri::Window) {
    if window.is_visible().unwrap() {
        save_window_position(window);
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
    if window.is_visible().unwrap() {
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
    } else {
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
}
