#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::Arc;

use log::{error, info};

use crate::validation::alert_multiple_configs;
mod commands;
mod glibc_detector;
mod init_check;
mod shortcuts;
mod tray;
mod validation;
mod window;
mod x11_init;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::SaveDialogState;
use tauri::Manager;
use tokio::runtime::Runtime;

use crate::commands::portforward::check_and_emit_changes;
#[cfg(target_os = "linux")]
use crate::glibc_detector::get_updater_target_platform;
use crate::init_check::RealPortOperations;
use crate::shortcuts::setup_shortcut_integration;
use crate::tray::{TrayPositionState, create_tray_icon, handle_run_event, handle_window_event};

fn main() {
    // CRITICAL: Must be called before ANY other code that might touch X11.
    // This prevents crashes with "[xcb] Most likely this is a multi-threaded
    // client and XInitThreads has not been called" on Linux X11 systems.
    x11_init::init_x11_threads();

    if let Err(e) = fix_path_env::fix_all_vars() {
        log::warn!("fix_path_env::fix_all_vars failed: {e}");
    }

    #[cfg(unix)]
    if std::env::var("HOME").is_err()
        && let Some(home) = dirs::home_dir()
    {
        unsafe { std::env::set_var("HOME", home) };
    }

    kftray_portforward::ssl::ensure_crypto_provider_installed();

    let positioning_active = Arc::new(AtomicBool::new(false));
    let pinned = Arc::new(AtomicBool::new(false));
    let runtime = Arc::new(Runtime::new().expect("Failed to create a Tokio runtime"));

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
            let app_handle = app.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = setup_shortcut_integration(app_handle).await {
                    error!("Failed to setup shortcut integration: {}", e);
                }
            });

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

            let app_handle_logs = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) =
                    crate::commands::logs::cleanup_old_logs_on_startup(app_handle_logs).await
                {
                    log::warn!("Failed to cleanup old logs on startup: {e}");
                }
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

            tauri::async_runtime::spawn(async move {
                use std::time::Duration;

                use kftray_portforward::kube::cleanup_stale_timeout_entries;
                use kftray_portforward::kube::shared_client::SHARED_CLIENT_MANAGER;

                let mut interval = tokio::time::interval(Duration::from_secs(3600));
                loop {
                    interval.tick().await;
                    cleanup_stale_timeout_entries().await;
                    SHARED_CLIENT_MANAGER.cleanup_expired();
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

            info!("Shortcut initialization is now handled by the new shortcuts integration");

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
        .plugin(tauri_plugin_notification::init())
        .plugin({
            let log_filename = format!(
                "kftray_{}",
                jiff::Zoned::now().strftime("%Y-%m-%d_%H-%M-%S")
            );
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some(log_filename),
                    }),
                ])
                .level(log::LevelFilter::Info)
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .max_file_size(5_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build()
        });

    let app = app
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin({
            #[cfg(target_os = "linux")]
            {
                let custom_target = get_updater_target_platform();
                info!("Using custom updater target: {}", custom_target);
                tauri_plugin_updater::Builder::new()
                    .target(custom_target)
                    .build()
            }

            #[cfg(not(target_os = "linux"))]
            {
                tauri_plugin_updater::Builder::new().build()
            }
        })
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
            commands::settings::run_diagnostics,
            commands::settings::get_env_auto_sync_settings,
            commands::settings::set_env_auto_sync_settings,
            commands::logs::get_log_info,
            commands::logs::get_log_contents,
            commands::logs::get_log_contents_json,
            commands::logs::clear_logs,
            commands::logs::generate_diagnostic_report,
            commands::logs::open_log_directory,
            commands::logs::list_log_files,
            commands::logs::cleanup_old_logs,
            commands::logs::delete_log_file,
            commands::logs::get_log_settings,
            commands::logs::set_log_settings,
            commands::logs::open_log_viewer_window_cmd,
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
            commands::shortcuts::create_shortcut,
            commands::shortcuts::get_shortcuts,
            commands::shortcuts::update_shortcut,
            commands::shortcuts::delete_shortcut,
            commands::shortcuts::validate_shortcut_key,
            commands::shortcuts::get_available_actions,
            commands::shortcuts::create_config_shortcut,
            commands::shortcuts::get_shortcuts_by_config,
            commands::shortcuts::test_shortcut_format_v2,
            commands::shortcuts::normalize_shortcut_key,
            commands::shortcuts::check_shortcut_conflicts,
            commands::shortcuts::get_platform_status,
            commands::shortcuts::try_fix_platform_permissions,
            commands::server_resources::list_all_kftray_resources,
            commands::server_resources::delete_kftray_resource,
            commands::server_resources::cleanup_all_kftray_resources,
            commands::server_resources::cleanup_orphaned_kftray_resources,
            commands::env_export::export_env_cmd,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(handle_run_event);
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, atomic::AtomicBool};

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
}
