#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::{
    Arc,
    Mutex,
};

mod commands;
mod config;
mod db;
mod keychain;
mod kubeforward;
mod logging;
mod models;
mod remote_config;
mod tray;
mod window;

use tokio::runtime::Runtime;
use tauri::{
    GlobalShortcutManager,
    Manager,
};
use tauri_plugin_positioner::{
    Position,
    WindowExt,
};

use crate::models::window::SaveDialogState;
use crate::tray::{
    create_tray_menu,
    handle_run_event,
    handle_system_tray_event,
    handle_window_event,
};
use crate::window::{
    load_window_position,
    toggle_window_visibility,
};

struct AppState {
    is_moving: Arc<Mutex<bool>>,
	runtime: Arc<Runtime>,
}

fn main() {

    logging::setup_logging();

    let _ = fix_path_env::fix();

    // configure tray menu
    let system_tray = create_tray_menu();
    let is_moving = Arc::new(Mutex::new(false));
	let runtime = Arc::new(Runtime::new().expect("Failed to create a Tokio runtime"));
    let app = tauri::Builder::default()
        .manage(SaveDialogState::default())
        .manage(AppState {
            is_moving: is_moving.clone(),
			runtime: runtime.clone(),
        })
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

            #[cfg(debug_assertions)]
            window.open_devtools();

            // Load window position
            match load_window_position() {
                Some(position) => {
                    println!(
                        "Setting window position to: x: {}, y: {}",
                        position.x, position.y
                    );
                    window
                        .set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                            position.x, position.y,
                        )))
                        .unwrap_or_else(|e| eprintln!("Failed to set window position: {}", e));
                }
                None => {
                    if let Err(e) = window.move_window(Position::Center) {
                        eprintln!("Failed to move window to center: {}", e);
                    }
                }
            }
            // register global shortcut to open the app
            let mut shortcut = app.global_shortcut_manager();

            shortcut
                .register("CmdOrCtrl+Shift+F1", move || {
                    toggle_window_visibility(&window);
                })
                .unwrap_or_else(|err| println!("{:?}", err));

            Ok(())
        })
        .plugin(tauri_plugin_positioner::init())
        .system_tray(system_tray)
        .on_system_tray_event(handle_system_tray_event)
        .on_window_event(handle_window_event)
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
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(handle_run_event);
}
