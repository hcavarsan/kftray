#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::{
    Arc,
    Mutex,
};

use log::error;

use crate::utils::validate_configs::alert_multiple_configs;
mod commands;
mod config;
mod config_state;
mod db;
mod keychain;
mod kubeforward;
mod logging;
mod migration;
mod models;
mod remote_config;
mod tray;
mod utils;
mod window;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use tauri::{
    GlobalShortcutManager,
    Manager,
};
use tokio::runtime::Runtime;

use crate::models::kube::HttpLogState;
use crate::models::window::AppState;
use crate::models::window::SaveDialogState;
use crate::tray::{
    create_tray_menu,
    handle_run_event,
    handle_system_tray_event,
    handle_window_event,
};
use crate::window::toggle_window_visibility;

fn main() {
    let _ = logging::setup_logging();

    let _ = fix_path_env::fix();

    // configure tray menu
    let system_tray = create_tray_menu();
    let is_moving = Arc::new(Mutex::new(false));
    let is_plugin_moving = Arc::new(AtomicBool::new(false));
    let is_pinned = Arc::new(AtomicBool::new(false));
    let runtime = Arc::new(Runtime::new().expect("Failed to create a Tokio runtime"));
    let http_log_state = HttpLogState::new();

    let app = tauri::Builder::default()
        .manage(SaveDialogState::default())
        .manage(AppState {
            is_moving: is_moving.clone(),
            is_plugin_moving: is_plugin_moving.clone(),
            is_pinned: is_pinned.clone(),
            runtime: runtime.clone(),
        })
        .manage(http_log_state.clone())
        .setup(move |app| {
            let app_handle = app.app_handle();

            tauri::async_runtime::spawn(async move {
                alert_multiple_configs(app_handle).await;
            });

            tauri::async_runtime::spawn(async move {
                if let Err(e) = config::clean_all_custom_hosts_entries().await {
                    error!("Failed to clean custom hosts entries: {}", e);
                }

                if let Err(e) = db::init().await {
                    error!("Failed to initialize database: {}", e);
                }

                if let Err(e) = migration::migrate_configs().await {
                    error!("Failed to migrate configs: {}", e);
                }
            });

            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            let window = app.get_window("main").unwrap();

            #[cfg(debug_assertions)]
            window.open_devtools();

            if is_pinned.load(Ordering::SeqCst) {
                window.set_always_on_top(true).unwrap();
            }

            // register global shortcut to open the app
            let mut shortcut = app.global_shortcut_manager();

            shortcut
                .register("CmdOrCtrl+Shift+F1", move || {
                    toggle_window_visibility(&window);
                })
                .unwrap_or_else(|err| error!("{:?}", err));

            Ok(())
        })
        .plugin(tauri_plugin_positioner::init())
        .system_tray(system_tray)
        .on_system_tray_event(handle_system_tray_event)
        .on_window_event(handle_window_event)
        .invoke_handler(tauri::generate_handler![
            kubeforward::commands::start_port_forward_tcp_cmd,
            kubeforward::commands::start_port_forward_udp_cmd,
            kubeforward::commands::stop_port_forward_cmd,
            kubeforward::commands::stop_all_port_forward_cmd,
            kubeforward::kubecontext::list_kube_contexts,
            kubeforward::kubecontext::list_namespaces,
            kubeforward::kubecontext::list_services,
            kubeforward::kubecontext::list_pods,
            kubeforward::kubecontext::list_ports,
            kubeforward::commands::deploy_and_forward_pod_cmd,
            kubeforward::commands::stop_proxy_forward_cmd,
            kubeforward::commands::set_http_logs_cmd,
            kubeforward::commands::get_http_logs_cmd,
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
            commands::open_log_file,
            commands::clear_http_logs,
            commands::get_http_log_size,
            keychain::store_key,
            keychain::get_key,
            keychain::delete_key,
            window::toggle_pin_state,
            config_state::get_config_states,
            config_state::get_config_state_by_config_id
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(handle_run_event);
}
