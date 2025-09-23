#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::Arc;

use log::{
    error,
    info,
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::validation::alert_multiple_configs;
mod commands;
mod init_check;
mod logging;
pub mod shortcut;
pub mod shortcut_manager;
mod tray;
mod validation;
mod window;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::SaveDialogState;
use tauri::Manager;
use tauri_plugin_global_shortcut::{
    Code,
    Modifiers,
    Shortcut,
};
use tokio::runtime::Runtime;

use crate::commands::portforward::check_and_emit_changes;
use crate::init_check::RealPortOperations;
use crate::shortcut::parse_shortcut_string;
use crate::tray::{
    TrayPositionState,
    create_tray_icon,
    handle_run_event,
    handle_window_event,
};

fn main() {
    let _ = logging::setup_logging();

    let _ = fix_path_env::fix();

    kftray_portforward::ssl::ensure_crypto_provider_installed();

    let positioning_active = Arc::new(AtomicBool::new(false));
    let pinned = Arc::new(AtomicBool::new(false));
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
        std::env::set_var("__GL_THREADED_OPTIMIZATIONS", "0");
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
    }

    let app = tauri::Builder::default()
        .manage(SaveDialogState::default())
        .manage(TrayPositionState::default())
        .manage(AppState {
            positioning_active: positioning_active.clone(),
            pinned: pinned.clone(),
            runtime: runtime.clone(),
        })
        .setup(move |app| {
            if let Err(e) = shortcut_manager::setup_shortcut_manager(app) {
                error!("Failed to setup shortcut manager: {}", e);
            }
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

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

            #[cfg(not(debug_assertions))]
            {
                let app_handle_clone = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                    match kftray_commons::utils::settings::get_auto_update_enabled().await {
                        Ok(true) => {
                            info!("Auto-update enabled, checking for updates on startup...");
                            match crate::commands::updater::check_for_updates_silent(
                                app_handle_clone.clone(),
                            )
                            .await
                            {
                                Ok(true) => {
                                    info!("Update available on startup, showing update dialog");
                                    if let Err(e) = crate::commands::updater::check_for_updates(
                                        app_handle_clone,
                                    )
                                    .await
                                    {
                                        error!("Failed to show update dialog: {e}");
                                    }
                                }
                                Ok(false) => {
                                    info!("No updates available on startup");
                                }
                                Err(e) => {
                                    error!("Update check failed on startup: {e}");
                                }
                            }
                        }
                        Ok(false) => {
                            info!("Auto-update disabled, skipping startup update check");
                        }
                        Err(e) => {
                            error!("Failed to get auto-update setting, defaulting to enabled: {e}");
                            info!("Checking for updates on startup...");
                            match crate::commands::updater::check_for_updates_silent(
                                app_handle_clone.clone(),
                            )
                            .await
                            {
                                Ok(true) => {
                                    info!("Update available on startup, showing update dialog");
                                    if let Err(e) = crate::commands::updater::check_for_updates(
                                        app_handle_clone,
                                    )
                                    .await
                                    {
                                        error!("Failed to show update dialog: {e}");
                                    }
                                }
                                Ok(false) => {
                                    info!("No updates available on startup");
                                }
                                Err(e) => {
                                    error!("Update check failed on startup: {e}");
                                }
                            }
                        }
                    }
                });
            }

            let window = app.get_webview_window("main").unwrap();
            #[cfg(debug_assertions)]
            {
                window.open_devtools();
                window.show().unwrap();
                window.set_focus().unwrap();
                window.set_always_on_top(true).unwrap();
                pinned.store(true, Ordering::SeqCst);
            }

            if pinned.load(Ordering::SeqCst) {
                window.set_always_on_top(true).unwrap();
            }

            #[cfg(not(target_os = "linux"))]
            {
                // Register global shortcut from settings (non-Linux only)
                let app_handle_for_shortcut = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    match kftray_commons::utils::settings::get_global_shortcut().await {
                        Ok(shortcut_str) => {
                            if let Some(shortcut) = parse_shortcut_string(&shortcut_str) {
                                if let Err(e) =
                                    app_handle_for_shortcut.global_shortcut().register(shortcut)
                                {
                                    error!(
                                        "Failed to register global shortcut {shortcut_str}: {e:?}"
                                    );
                                } else {
                                    info!("Registered global shortcut: {shortcut_str}");
                                }
                            } else {
                                error!("Failed to parse global shortcut: {shortcut_str}");
                            }
                        }
                        Err(e) => {
                            error!("Failed to load global shortcut setting: {e}");
                            let default_shortcut = Shortcut::new(
                                Some(Modifiers::CONTROL | Modifiers::SHIFT),
                                Code::F1,
                            );
                            if let Err(e) = app_handle_for_shortcut
                                .global_shortcut()
                                .register(default_shortcut)
                            {
                                error!("Failed to register default global shortcut: {e:?}");
                            }
                        }
                    }
                });
            }

            #[cfg(target_os = "linux")]
            {
                // Initialize Linux global shortcuts from database
                let app_handle_for_linux_shortcut = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    use crate::shortcut_manager::ShortcutManagerState;

                    let state = app_handle_for_linux_shortcut.state::<ShortcutManagerState>();

                    match kftray_commons::utils::settings::get_global_shortcut().await {
                        Ok(shortcut_str) => {
                            let app_handle_clone = app_handle_for_linux_shortcut.clone();
                            let result = state.register_shortcut(
                                "toggle_window".to_string(),
                                shortcut_str.clone(),
                                move || {
                                    if let Some(window) = app_handle_clone.get_webview_window("main") {
                                        crate::window::toggle_window_visibility(&window);
                                    }
                                }
                            );

                            match result {
                                Ok(_) => {
                                    info!("Linux global shortcut initialized from database: {}", shortcut_str);
                                }
                                Err(e) => {
                                    error!("Failed to initialize Linux global shortcut '{}': {}", shortcut_str, e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to load Linux global shortcut setting: {}", e);
                            // Try default shortcut
                            let app_handle_clone = app_handle_for_linux_shortcut.clone();
                            let result = state.register_shortcut(
                                "toggle_window".to_string(),
                                "ctrl+shift+k".to_string(),
                                move || {
                                    if let Some(window) = app_handle_clone.get_webview_window("main") {
                                        crate::window::toggle_window_visibility(&window);
                                    }
                                }
                            );

                            match result {
                                Ok(_) => {
                                    info!("Linux global shortcut initialized with default: ctrl+shift+k");
                                }
                                Err(e) => {
                                    error!("Failed to initialize default Linux global shortcut: {}", e);
                                }
                            }
                        }
                    }
                });
            }

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
        .plugin(tauri_plugin_http::init());

    #[cfg(not(target_os = "linux"))]
    let app = app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app_handle, _shortcut, event| {
                use tauri_plugin_global_shortcut::ShortcutState;

                if event.state() == ShortcutState::Pressed
                    && let Some(window) = app_handle.get_webview_window("main")
                {
                    crate::window::toggle_window_visibility(&window);
                }
            })
            .build(),
    );

    let app = app
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            commands::settings::update_auto_update_enabled,
            commands::settings::get_auto_update_status,
            commands::ssl::get_ssl_settings,
            commands::ssl::set_ssl_settings,
            commands::ssl::regenerate_certificate,
            commands::ssl::get_certificate_info,
            commands::ssl::list_certificates,
            commands::ssl::remove_certificate,
            commands::ssl::is_ssl_enabled,
            commands::ssl::enable_ssl,
            commands::ssl::disable_ssl,
            commands::ssl::get_ssl_cert_validity,
            commands::ssl::set_ssl_cert_validity,
            commands::ssl::get_ssl_auto_regenerate,
            commands::ssl::set_ssl_auto_regenerate,
            commands::updater::check_for_updates,
            commands::updater::check_for_updates_silent,
            commands::updater::get_version_info,
            commands::updater::install_update_silent,
            commands::settings::get_global_shortcut_cmd,
            commands::settings::set_global_shortcut_cmd,
            commands::settings::update_global_shortcut,
            shortcut_manager::register_global_shortcut,
            shortcut_manager::unregister_global_shortcut,
            shortcut_manager::get_registered_shortcuts,
            shortcut_manager::test_shortcut_format,
            shortcut_manager::check_linux_permissions,
            shortcut_manager::try_fix_linux_permissions,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(handle_run_event);
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::AtomicBool,
    };

    use super::*;

    #[test]
    fn test_app_state_creation() {
        let positioning_active = Arc::new(AtomicBool::new(false));
        let pinned = Arc::new(AtomicBool::new(false));
        let runtime = Arc::new(Runtime::new().expect("Failed to create a Tokio runtime"));

        let app_state = AppState {
            positioning_active: positioning_active.clone(),
            pinned: pinned.clone(),
            runtime: runtime.clone(),
        };

        assert!(!app_state.positioning_active.load(Ordering::SeqCst));
        assert!(!app_state.pinned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_save_dialog_state() {
        let state = SaveDialogState::default();
        assert!(!state.is_open.load(Ordering::SeqCst));
    }

    #[test]
    fn test_pin_state() {
        let pinned = Arc::new(AtomicBool::new(false));
        assert!(!pinned.load(Ordering::SeqCst));

        pinned.store(true, Ordering::SeqCst);
        assert!(pinned.load(Ordering::SeqCst));
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
