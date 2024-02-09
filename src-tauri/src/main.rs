#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod config;
mod db;

use log::LevelFilter;
use std::env;
use std::fs::OpenOptions;
use std::path::PathBuf;
use tauri::{
    CustomMenuItem, GlobalShortcutManager, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu,
};
use tauri_plugin_positioner::{Position, WindowExt};
use tokio::runtime::Runtime;

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

fn get_log_path() -> PathBuf {
    let home_dir = dirs::home_dir().expect("Could not find the home directory");
    home_dir.join(".kftray").join("app.log")
}

fn setup_logging() {
    let log_filter = match env::var("RUST_LOG") {
        Ok(filter) => filter.parse().unwrap_or(LevelFilter::Info),
        Err(_) => LevelFilter::Off,
    };

    if env::var("KFTRAY_DEBUG").is_ok() {
        let log_path = get_log_path();
        let log_dir = log_path.parent().expect("Could not find the log directory");
        std::fs::create_dir_all(log_dir).expect("Could not create log directory");

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .expect("Could not open log file");

        env_logger::Builder::from_default_env()
            .filter_level(log_filter)
            .format_timestamp_secs()
            .target(env_logger::Target::Pipe(Box::new(log_file)))
            .init();
    } else {
        env_logger::Builder::new()
            .filter_level(log_filter)
            .format_timestamp_secs()
            .init();
    }
}

fn main() {
    setup_logging();
    let _ = fix_path_env::fix();
    let quit = CustomMenuItem::new("quit".to_string(), "Quit").accelerator("CmdOrCtrl+Shift+Q");
    let open = CustomMenuItem::new("open".to_string(), "Open App");
    let system_tray_menu = SystemTrayMenu::new().add_item(open).add_item(quit);

    tauri::Builder::default()
        .manage(SaveDialogState::default())
        .setup(move |app| {
            db::init();
            if let Err(e) = config::migrate_configs() {
                eprintln!("Failed to migrate configs: {}", e);
            }

            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            let window = app.get_window("main").unwrap();

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
                    #[cfg(target_os = "linux")]
                    let _ = window.move_window(Position::TopRight);
                    #[cfg(target_os = "windows")]
                    let _ = window.move_window(Position::BottomRight);
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
                        #[cfg(target_os = "windows")]
                        let _ = window.move_window(Position::BottomRight);
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
            open_save_dialog,
            close_save_dialog,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
