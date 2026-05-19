#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::sync::Arc;

use log::{
    error,
    info,
};

use crate::validation::alert_multiple_configs;
#[cfg(target_os = "linux")]
mod appimage_wayland_fixup;
mod commands;
mod glibc_detector;
mod init_check;
mod mcp;
mod shortcuts;
mod tray;
#[cfg(target_os = "linux")]
mod tray_linux;
#[cfg(target_os = "windows")]
mod tray_theme;
mod validation;
mod window;
mod window_size;
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
use crate::tray::{
    TrayPositionState,
    create_tray_icon,
    handle_run_event,
    handle_window_event,
};

/// Sets an environment variable only if it isn't already defined.
/// Respects user overrides for power users who know their system works
/// with different settings.
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn set_default_env(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        unsafe { std::env::set_var(key, value) };
    }
}

fn init_file_logger() -> anyhow::Result<()> {
    use kftray_commons::utils::config_dir::get_app_log_path;

    let log_path = get_app_log_path().map_err(|e| anyhow::anyhow!(e))?;
    let log_dir = log_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("log path has no parent directory"))?
        .to_path_buf();
    let basename = format!(
        "kftray_{}",
        jiff::Zoned::now().strftime("%Y-%m-%d_%H-%M-%S")
    );

    // When RUST_LOG is set, mirror the same level to stdout so terminal output
    // reflects the requested verbosity (e.g. RUST_LOG=trace mise run dev).
    let stdout_duplicate = match std::env::var("RUST_LOG")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        s if s.contains("trace") => flexi_logger::Duplicate::Trace,
        s if s.contains("debug") => flexi_logger::Duplicate::Debug,
        s if s.contains("warn") => flexi_logger::Duplicate::Warn,
        s if s.contains("error") => flexi_logger::Duplicate::Error,
        _ => flexi_logger::Duplicate::Info,
    };

    flexi_logger::Logger::try_with_env_or_str("info")?
        .log_to_file(
            flexi_logger::FileSpec::default()
                .directory(log_dir)
                .basename(basename),
        )
        .duplicate_to_stdout(stdout_duplicate)
        .rotate(
            flexi_logger::Criterion::Size(5_000_000),
            flexi_logger::Naming::Numbers,
            flexi_logger::Cleanup::KeepLogFiles(1),
        )
        .format(flexi_logger::detailed_format)
        .start()?;

    // Bridge `log` crate events (tungstenite, tokio-tungstenite, etc.) into
    // `tracing` so a single subscriber handles everything.
    // max_level matches RUST_LOG or falls back to Debug to avoid filtering
    // out log:: calls before tracing's EnvFilter can evaluate them.
    let max_log_level = match std::env::var("RUST_LOG")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        s if s.contains("trace") => log::LevelFilter::Trace,
        s if s.contains("debug") => log::LevelFilter::Debug,
        _ => log::LevelFilter::Info,
    };
    tracing_log::LogTracer::builder()
        .with_max_level(max_log_level)
        .init()
        .ok();

    // Single tracing subscriber for both tracing:: and log:: events.
    // RUST_LOG controls the filter (e.g. tungstenite=trace,kube=debug).
    let rust_log = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let filter = tracing_subscriber::EnvFilter::try_new(&rust_log)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_level(true)
        .without_time() // flexi_logger already adds timestamps
        .try_init()
        .ok(); // ok() because another subscriber may already be installed

    Ok(())
}

fn main() {
    // CRITICAL: WebKit/GPU env vars must be set before any library init.
    // These prevent blank-window bugs on Linux (EGL_BAD_PARAMETER, DMA-BUF
    // renderer crashes). Only set if the user hasn't already provided a value.
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        set_default_env("__GL_THREADED_OPTIMIZATIONS", "0");
        set_default_env("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        set_default_env("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        set_default_env("__NV_DISABLE_EXPLICIT_SYNC", "1");
    }

    // Attempt LD_PRELOAD re-exec for AppImage on Wayland before any GTK init.
    #[cfg(target_os = "linux")]
    appimage_wayland_fixup::maybe_reexec();

    // Must be called before any X11 interaction to prevent xcb threading crashes.
    x11_init::init_x11_threads();

    if let Err(e) = init_file_logger() {
        eprintln!("failed to initialize file logger: {e}");
    }

    if let Err(e) = fix_path_env::fix_all_vars() {
        log::warn!("fix_path_env::fix_all_vars failed: {e}");
    }

    #[cfg(unix)]
    if std::env::var("HOME").is_err()
        && let Some(home) = dirs::home_dir()
    {
        unsafe { std::env::set_var("HOME", home) };
    }

    kftray_ssl::install_default_keyring_store();
    kftray_ssl::ensure_crypto_provider_installed();

    let positioning_active = Arc::new(AtomicBool::new(false));
    let pinned = Arc::new(AtomicBool::new(false));
    let runtime = Arc::new(Runtime::new().expect("Failed to create a Tokio runtime"));

    let app = tauri::Builder::default()
        .manage(SaveDialogState::default())
        .manage(TrayPositionState::default())
        .manage(AppState {
            positioning_active: positioning_active,
            pinned: pinned.clone(),
            runtime: runtime,
        })
        .setup(move |app| {
            let app_handle = app.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = setup_shortcut_integration(app_handle).await {
                    error!("Failed to setup shortcut integration: {e}");
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
                if let Err(e) = commands::logs::cleanup_old_logs_on_startup(app_handle_logs).await {
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
                if let Err(e) = mcp::init_from_settings().await {
                    error!("Failed to initialize MCP server: {e}");
                }
            });

            tauri::async_runtime::spawn(async move {
                use std::time::Duration;

                use kftray_kube::kube::cleanup_stale_timeout_entries;
                use kftray_kube::registry::PORT_FORWARD_REGISTRY;

                let mut interval = tokio::time::interval(Duration::from_secs(3600));
                loop {
                    interval.tick().await;
                    cleanup_stale_timeout_entries().await;
                    PORT_FORWARD_REGISTRY.cleanup_expired_clients();
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
        .plugin(tauri_plugin_notification::init());

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
            commands::settings::get_mcp_server_status,
            commands::settings::update_mcp_server_enabled,
            commands::settings::update_mcp_server_port,
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
}
