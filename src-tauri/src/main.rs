#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod config;
mod db;
mod keychain;
mod logging;
mod remote_config;
mod tray;

use std::env;
use tauri::{GlobalShortcutManager, Manager, SystemTrayEvent};
use tauri_plugin_positioner::{Position, WindowExt};
use tokio::runtime::Runtime;

use std::sync::atomic::Ordering;

use commands::SaveDialogState;

fn main() {
    logging::setup_logging();
    let _ = fix_path_env::fix();
    // configure tray menu
    let system_tray = tray::create_tray_menu();
    tauri::Builder::default()
        .manage(SaveDialogState::default())
        .setup(move |app| {
            let _ = config::clean_all_custom_hosts_entries();
            let _ = db::init();
            if let Err(e) = config::migrate_configs() {
                eprintln!("Failed to migrate configs: {}", e);
            }

            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            let window = app.get_window("main").unwrap();
            // register global shortcut to open the app
            let mut shortcut = app.global_shortcut_manager();
            shortcut
                .register("CmdOrCtrl+Shift+F1", move || {
                    if window.is_visible().unwrap() {
                        window.hide().unwrap();
                    } else {
                        #[cfg(target_os = "linux")]
                        let _ = window.move_window(Position::TopRight);
                        #[cfg(target_os = "windows")]
                        let _ = window.move_window(Position::BottomRight);
                        #[cfg(target_os = "macos")]
                        let _ = window.move_window(Position::TrayCenter);

                        window.show().unwrap();
                        window.set_focus().unwrap();
                    }
                })
                .unwrap_or_else(|err| println!("{:?}", err));

            Ok(())
        })
        .plugin(tauri_plugin_positioner::init())
        .system_tray(system_tray)
        .on_system_tray_event(|app, event| {
            tauri_plugin_positioner::on_tray_event(app, &event);
            match event {
                SystemTrayEvent::LeftClick {
                    position: _,
                    size: _,
                    ..
                } => {
                    // temp solution due to a limitation in libappindicator and tray events in linux
                    let window = app.get_window("main").unwrap();
                    #[cfg(target_os = "linux")]
                    let _ = window.move_window(Position::TopRight);
                    #[cfg(target_os = "windows")]
                    let _ = window.move_window(Position::TrayCenter);
                    #[cfg(target_os = "macos")]
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
                        let runtime = Runtime::new().expect("Failed to create a Tokio runtime");

                        runtime.block_on(async {
                            match kubeforward::port_forward::stop_all_port_forward().await {
                                Ok(_) => {
                                    println!("Successfully stopped all port forwards.");
                                }
                                Err(err) => {
                                    eprintln!("Failed to stop port forwards: {}", err);
                                }
                            }
                        });

                        std::process::exit(0);
                    }
                    "open" => {
                        let window = app.get_window("main").unwrap();
                        #[cfg(target_os = "linux")]
                        let _ = window.move_window(Position::TopRight);
                        window.show().unwrap();
                        window.set_focus().unwrap();
                        #[cfg(target_os = "windows")]
                        let _ = window.move_window(Position::TrayCenter);
                        window.show().unwrap();
                        window.set_focus().unwrap();
                        #[cfg(target_os = "macos")]
                        let _ = window.move_window(Position::TrayCenter);
                        window.show().unwrap();
                        window.set_focus().unwrap();
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
            kubeforward::kubecontext::list_kube_contexts,
            kubeforward::kubecontext::list_namespaces,
            kubeforward::kubecontext::list_services,
            kubeforward::kubecontext::list_service_ports,
            kubeforward::proxy::deploy_and_forward_pod,
            kubeforward::proxy::stop_proxy_forward,
            config::get_configs,
            config::insert_config,
            config::delete_config,
            config::get_config,
            config::update_config,
            config::export_configs,
            config::import_configs,
            config::delete_configs,
            config::delete_all_configs,
            commands::open_save_dialog,
            commands::close_save_dialog,
            commands::import_configs_from_github,
            keychain::store_key,
            keychain::get_key,
            keychain::delete_key
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
