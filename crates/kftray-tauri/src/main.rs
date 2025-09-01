#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::{
    Arc,
    Mutex,
};

use log::{
    error,
    info,
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::validation::alert_multiple_configs;
mod commands;
mod init_check;
mod logging;
mod tray;
mod validation;
mod window;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::SaveDialogState;
use tauri::Manager;
use tokio::runtime::Runtime;

use crate::commands::portforward::check_and_emit_changes;
use crate::init_check::RealPortOperations;
use crate::tray::{
    TrayPositionState,
    create_tray_icon,
    handle_run_event,
    handle_window_event,
};

fn main() {
    let _ = logging::setup_logging();

    let _ = fix_path_env::fix();

    // Tray will be created in setup function for Tauri v2
    let is_moving = Arc::new(Mutex::new(false));
    let is_plugin_moving = Arc::new(AtomicBool::new(false));
    let is_pinned = Arc::new(AtomicBool::new(false));
    let runtime = Arc::new(Runtime::new().expect("Failed to create a Tokio runtime"));

    // TODO: Remove this workaround when the tauri issue is resolved
    // tauri Issue: https://github.com/tauri-apps/tauri/issues/9394
    // kftray Issue: https://github.com/hcavarsan/kftray/issues/366
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    unsafe {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }

    let app = tauri::Builder::default()
        .manage(SaveDialogState::default())
        .manage(TrayPositionState::default())
        .manage(AppState {
            is_moving: is_moving.clone(),
            is_plugin_moving: is_plugin_moving.clone(),
            is_pinned: is_pinned.clone(),
            runtime: runtime.clone(),
        })
        // HttpLogState removed - using direct database access
        .setup(move |app| {
            let app_handle = app.app_handle();
            let app_handle_clone2 = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) =
                    kftray_commons::utils::config::clean_all_custom_hosts_entries().await
                {
                    error!("Failed to clean custom hosts entries: {e}");
                }

                if let Err(e) = kftray_commons::utils::db::init().await {
                    error!("Failed to initialize database: {e}");
                }

                if let Err(e) = kftray_commons::utils::migration::migrate_configs(None).await {
                    error!("Database migration failed during setup: {e}");
                }
            });

            tauri::async_runtime::spawn(async move {
                alert_multiple_configs(app_handle_clone2).await;
            });

            let port_ops = Arc::new(RealPortOperations);
            let port_ops_clone = Arc::clone(&port_ops);

            tauri::async_runtime::spawn(async move {
                info!("Starting port management checks");
                if let Err(e) = init_check::check_and_manage_ports(port_ops_clone).await {
                    error!("Error in port management: {e}");
                }
            });

            let app_handle_clone = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                check_and_emit_changes(app_handle_clone).await;
            });

            tauri::async_runtime::spawn(async move {
                if let Ok(enabled) = kftray_commons::utils::settings::get_network_monitor().await
                    && enabled
                    && let Err(e) = kftray_network_monitor::start().await
                {
                    error!("Failed to start network monitor: {e}");
                }
            });

            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            let window = app.get_webview_window("main").unwrap();
            #[cfg(debug_assertions)]
            {
                window.open_devtools();
                window.show().unwrap();
                window.set_focus().unwrap();
                window.set_always_on_top(true).unwrap();
                is_pinned.store(true, Ordering::SeqCst);
            }

            if is_pinned.load(Ordering::SeqCst) {
                window.set_always_on_top(true).unwrap();
            }

            // Register global shortcut using Tauri v2 plugin API
            app.global_shortcut()
                .register("CmdOrCtrl+Shift+F1")
                .unwrap_or_else(|err| error!("Failed to register global shortcut: {err:?}"));

            // Create tray icon for Tauri v2
            if let Err(e) = create_tray_icon(app) {
                error!("Failed to create tray icon: {e}");
            }

            Ok(())
        })
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
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
            commands::portforward::get_active_pod_cmd,
            commands::httplogs::set_http_logs_cmd,
            commands::httplogs::get_http_logs_cmd,
            commands::httplogs::get_http_logs_config_cmd,
            commands::httplogs::update_http_logs_config_cmd,
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
            commands::helper::install_helper,
            commands::helper::remove_helper,
            commands::helper::allocate_local_address_cmd,
            commands::helper::release_local_address_cmd,
            commands::settings::get_settings,
            commands::settings::update_disconnect_timeout,
            commands::settings::update_network_monitor,
            commands::settings::get_setting_value,
            commands::settings::set_setting_value,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(handle_run_event);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    #[test]
    fn test_app_state_creation() {
        let is_moving = Arc::new(Mutex::new(false));
        let is_plugin_moving = Arc::new(AtomicBool::new(false));
        let is_pinned = Arc::new(AtomicBool::new(false));
        let runtime = Arc::new(Runtime::new().expect("Failed to create a Tokio runtime"));

        let app_state = AppState {
            is_moving: is_moving.clone(),
            is_plugin_moving: is_plugin_moving.clone(),
            is_pinned: is_pinned.clone(),
            runtime: runtime.clone(),
        };

        assert!(!*app_state.is_moving.lock().unwrap());
        assert!(!app_state.is_plugin_moving.load(Ordering::SeqCst));
        assert!(!app_state.is_pinned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_save_dialog_state() {
        let state = SaveDialogState::default();
        assert!(!state.is_open.load(Ordering::SeqCst));
    }

    #[test]
    fn test_pin_state() {
        let is_pinned = Arc::new(AtomicBool::new(false));
        assert!(!is_pinned.load(Ordering::SeqCst));

        is_pinned.store(true, Ordering::SeqCst);
        assert!(is_pinned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_moving_state() {
        let is_moving = Arc::new(Mutex::new(false));
        assert!(!*is_moving.lock().unwrap());

        *is_moving.lock().unwrap() = true;
        assert!(*is_moving.lock().unwrap());
    }

    #[test]
    fn test_plugin_moving_state() {
        let is_plugin_moving = Arc::new(AtomicBool::new(false));
        assert!(!is_plugin_moving.load(Ordering::SeqCst));

        is_plugin_moving.store(true, Ordering::SeqCst);
        assert!(is_plugin_moving.load(Ordering::SeqCst));
    }

    #[test]
    fn test_setup_logging() {
        let result = logging::setup_logging();
        assert!(result.is_ok());
    }
}
