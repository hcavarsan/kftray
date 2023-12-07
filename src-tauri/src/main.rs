#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod db;
mod config;

use tauri::{CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu};
use tauri_plugin_positioner::{Position, WindowExt};
use std::env;
use kubeforward;


fn main() {
	env_logger::init();
    let _ = fix_path_env::fix();
    let quit = CustomMenuItem::new("quit".to_string(), "Quit").accelerator("Cmd+Q");
    let system_tray_menu = SystemTrayMenu::new().add_item(quit);
    tauri::Builder::default()
        .setup(|app| {
            // Initialize the database.
            db::init();
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            Ok(())
        })
        .plugin(tauri_plugin_positioner::init())
        .system_tray(SystemTray::new().with_menu(system_tray_menu))
        .on_system_tray_event(|app, event| {
            tauri_plugin_positioner::on_tray_event(app, &event);
            match event {
                SystemTrayEvent::LeftClick { .. } => {
                    if let Some(window) = app.get_window("main") {
                        let _ = window.move_window(Position::TrayCenter);
                        match window.is_visible() {
                            Ok(true) => {
                                if let Err(e) = window.hide() {
                                    println!("Failed to hide window: {}", e);
                                }
                            }
                            Ok(false) => {
                                if let Err(e) = window.show() {
                                    println!("Failed to show window: {}", e);
                                }
                                if let Err(e) = window.set_focus() {
                                    println!("Failed to set focus: {}", e);
                                }
                            }
                            Err(e) => {
                                println!("Failed to check window visibility: {}", e);
                            }
                        }
                    }
                }
                SystemTrayEvent::RightClick { .. } => {
                    println!("system tray received a right click");
                }
                SystemTrayEvent::DoubleClick { .. } => {
                    println!("system tray received a double click");
                }
                SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                    "quit" => {
                        std::process::exit(0);
                    }
                    "hide" => {
                        if let Some(window) = app.get_window("main") {
                            let _ = window.hide();
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        })
        .on_window_event(|event| {
            if let tauri::WindowEvent::Focused(is_focused) = event.event() {
                // detect click outside of the focused window and hide the app
                if !is_focused {
                    let _ = event.window().hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
			kubeforward::port_forward::start_port_forward,
            kubeforward::port_forward::stop_port_forward,
            kubeforward::port_forward::quit_app,
            config::get_configs,
            config::insert_config,
            config::delete_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
