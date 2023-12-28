#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod config;
mod db;

use std::env;
use tauri::{CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu};
use tauri_plugin_positioner::{Position, WindowExt};

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::State;

struct SaveDialogState {
    pub is_open: AtomicBool,
}

impl Default for SaveDialogState {
    fn default() -> Self {
        SaveDialogState {
            is_open: AtomicBool::new(false),
        }
    }
}

#[tauri::command]
fn open_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn close_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(false, Ordering::SeqCst);
}
fn main() {
    env_logger::init();
    let _ = fix_path_env::fix();
    let quit = CustomMenuItem::new("quit".to_string(), "Quit").accelerator("Cmd+Q");
    let system_tray_menu = SystemTrayMenu::new().add_item(quit);
    tauri::Builder::default()
        .manage(SaveDialogState::default())
        .setup(|_app| {
            db::init();
            #[cfg(target_os = "macos")]
            {
                _app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            Ok(())
        })
        .plugin(tauri_plugin_positioner::init())
        .system_tray(SystemTray::new().with_menu(system_tray_menu))
        .on_system_tray_event(|app, event| {
            tauri_plugin_positioner::on_tray_event(app, &event);
            match event {
                SystemTrayEvent::LeftClick {
                    position: _,
                    size: _,
                    ..
                } => {
                    let window = app.get_window("main").unwrap();
                    let _ = window.move_window(Position::TrayCenter);

                    if window.is_visible().unwrap() {
                        window.hide().unwrap();
                    } else {
                        window.show().unwrap();
                        window.set_focus().unwrap();
                    }
                }
                SystemTrayEvent::RightClick {
                    position: _,
                    size: _,
                    ..
                } => {
                    println!("system tray received a right click");
                }
                SystemTrayEvent::DoubleClick {
                    position: _,
                    size: _,
                    ..
                } => {
                    println!("system tray received a double click");
                }
                SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                    "quit" => {
                        std::process::exit(0);
                    }
                    "hide" => {
                        let window = app.get_window("main").unwrap();
                        window.hide().unwrap();
                    }
                    _ => {}
                },
                _ => {}
            }
        })
        .on_window_event(|event| {
            if let tauri::WindowEvent::Focused(is_focused) = event.event() {
                if !is_focused {
                    let app_handle = event.window().app_handle();
                    if let Some(state) = app_handle.try_state::<SaveDialogState>() {
                        if !state.is_open.load(Ordering::SeqCst) {
                            event.window().hide().unwrap();
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            kubeforward::port_forward::start_port_forward,
            kubeforward::port_forward::stop_port_forward,
            kubeforward::port_forward::stop_all_port_forward,
            kubeforward::port_forward::quit_app,
            kubeforward::kubecontext::list_kube_contexts,
            kubeforward::kubecontext::list_namespaces,
            kubeforward::kubecontext::list_services,
            kubeforward::kubecontext::list_service_ports,
			kubeforward::proxy::deploy_and_forward_pod,
            config::get_configs,
            config::insert_config,
            config::delete_config,
            config::get_config,
            config::update_config,
            config::export_configs,
            config::import_configs,
            open_save_dialog,
            close_save_dialog,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
