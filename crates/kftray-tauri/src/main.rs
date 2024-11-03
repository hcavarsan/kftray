#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::{
    Arc,
    Mutex,
};
use std::error::Error;
use kftray_commons::utils::path_validation::alert_multiple_configs;
use log::error;

mod commands;
mod logging;
mod tray;
mod window;

use std::sync::atomic::{
    AtomicBool,
    Ordering,
};

use kftray_commons::models::window::{
    AppState,
    SaveDialogState,
};
use kftray_portforward::models::kube::HttpLogState;
use tauri::{
    GlobalShortcutManager,
    Manager,
};
use tokio::runtime::Runtime;

use crate::commands::portforward::check_and_emit_changes;
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

    let system_tray = create_tray_menu();
    let is_moving = Arc::new(Mutex::new(false));
    let is_plugin_moving = Arc::new(AtomicBool::new(false));
    let is_pinned = Arc::new(AtomicBool::new(false));
    let runtime = Arc::new(Runtime::new().expect("Failed to create a Tokio runtime"));
    let http_log_state = HttpLogState::new();

    // Initialize database and state manager synchronously before app setup
    let init_result = runtime.block_on(async {
        // First ensure the config directory exists
        let config_dir = kftray_commons::utils::paths::ensure_config_dir().await?;

        // Also ensure the parent directory of the database file exists
        let db_path = kftray_commons::utils::paths::get_db_path().await?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        match kftray_commons::init().await {
            Ok((database, state_manager)) => {
                // Verify database connection
                if !database.is_connected().await.map_err(|e| kftray_commons::error::Error::db_connection(e.to_string()))? {
                    error!("Database connection test failed");
                    return Err(kftray_commons::error::Error::db_connection(
                        "Database connection test failed".to_string()
                    ));
                }

                // Clean up and initialize states
                if let Err(e) = kftray_commons::utils::hosts::clean_all_custom_hosts_entries(&database).await {
                    error!("Failed to clean custom hosts entries: {}", e);
                }

                if let Err(e) = kftray_commons::check_and_manage_ports(&database, &state_manager).await {
                    error!("Error in port management: {}", e);
                }

                Ok((database, state_manager))
            }
            Err(e) => {
                error!("Failed to initialize database and state manager: {}", e);
                Err(e)
            }
        }
    });

    // Ensure initialization succeeded before continuing
    let (database, state_manager) = init_result.expect("Failed to initialize application state");

    let app = tauri::Builder::default()
        .manage(SaveDialogState::default())
        .manage(AppState {
            is_moving: is_moving.clone(),
            is_plugin_moving: is_plugin_moving.clone(),
            is_pinned: is_pinned.clone(),
            runtime: runtime.clone(),
        })
        .manage(http_log_state.clone())
        .manage(database)
        .manage(state_manager)
        .setup(move |app| {
            let app_handle = app.app_handle();
            let app_handle_clone = app_handle.clone();

            tauri::async_runtime::spawn(async move {
                alert_multiple_configs(app_handle_clone).await;
            });

            let app_handle_clone = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                check_and_emit_changes(app_handle_clone).await;
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
            commands::portforward::start_port_forward_tcp_cmd,
            commands::portforward::start_port_forward_udp_cmd,
            commands::portforward::stop_port_forward_cmd,
            commands::portforward::stop_all_port_forward_cmd,
            commands::portforward::handle_exit_app,
            commands::kubecontext::list_kube_contexts,
            commands::kubecontext::list_namespaces,
            commands::kubecontext::list_services,
            commands::kubecontext::list_pods,
            commands::kubecontext::list_ports,
            commands::kubecontext::get_services_with_annotations,
            commands::portforward::deploy_and_forward_pod_cmd,
            commands::portforward::stop_proxy_forward_cmd,
            commands::httplogs::set_http_logs_cmd,
            commands::httplogs::get_http_logs_cmd,
            commands::config::get_configs_cmd,
            commands::config::insert_config_cmd,
            commands::config::delete_config_cmd,
            commands::config::get_config_cmd,
            commands::config::update_config_cmd,
            commands::config::export_configs_cmd,
            commands::config::import_configs_cmd,
            commands::config::delete_configs_cmd,
            commands::config::delete_all_configs_cmd,
            commands::window_state::open_save_dialog,
            commands::window_state::close_save_dialog,
            commands::github::import_configs_from_github,
            commands::httplogs::open_log_file,
            commands::httplogs::clear_http_logs,
            commands::httplogs::get_http_log_size,
            commands::github::store_key,
            commands::github::get_key,
            commands::github::delete_key,
            commands::window_state::toggle_pin_state,
            commands::config_state::get_config_states,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(handle_run_event);
}
