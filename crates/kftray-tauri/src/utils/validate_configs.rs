extern crate dirs;
extern crate tauri;

use std::env;
use std::path::PathBuf;

use tauri::api::dialog::{
    MessageDialogBuilder,
    MessageDialogKind,
};
use tauri::{
    async_runtime::spawn_blocking,
    AppHandle,
};
#[derive(Clone)]
struct ConfigLocation {
    path: PathBuf,
    origin: String,
}

impl ConfigLocation {
    fn new(path: PathBuf, origin: String) -> Self {
        ConfigLocation { path, origin }
    }
}

fn detect_multiple_configs() -> (Vec<ConfigLocation>, Option<ConfigLocation>) {
    let mut config_locations = Vec::new();
    let mut active_config: Option<ConfigLocation> = None;

    if let Ok(config_dir) = env::var("KFTRAY_CONFIG") {
        let path = PathBuf::from(&config_dir);
        if path.is_dir() {
            let config = ConfigLocation::new(path.clone(), "KFTRAY_CONFIG".into());
            config_locations.push(config.clone());
            active_config = Some(config);
        }
    }

    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME") {
        let mut path = PathBuf::from(&xdg_config_home);
        path.push("kftray");
        if path.is_dir() {
            let config = ConfigLocation::new(path.clone(), "XDG_CONFIG_HOME".into());
            config_locations.push(config.clone());
            if active_config.is_none() {
                active_config = Some(config);
            }
        }
    }

    if let Ok(home_dir) = env::var("HOME") {
        let mut path = PathBuf::from(&home_dir);
        path.push(".kftray");
        if path.is_dir() {
            let config = ConfigLocation::new(path.clone(), "HOME".into());
            config_locations.push(config.clone());
            if active_config.is_none() {
                active_config = Some(config);
            }
        }
    }

    (config_locations, active_config)
}

async fn show_alert_dialog(
    app_handle: AppHandle, configs: Vec<ConfigLocation>, active_config: Option<ConfigLocation>,
) {
    let msg = configs
        .into_iter()
        .map(|config| format!(" * {}: {}", config.origin, config.path.display()))
        .collect::<Vec<_>>()
        .join("\n");

    let active_config_msg = if let Some(active) = active_config {
        format!(
            "Active Configuration:\n * {}: {}\n\n",
            active.origin,
            active.path.display()
        )
    } else {
        "Active Configuration:\n * No active configuration detected.\n\n".to_string()
    };

    let full_message = format!(
        "Multiple configuration directories have been detected in the following locations:\n\n{}\n\n\
        {}\n\
        Environment Variables Checked:\n\
        - HOME: {}\n\
        - KFTRAY_CONFIG: {}\n\
        - XDG_CONFIG_HOME: {}\n\n\
        To resolve this issue, please:\n\
        1. Move or delete the extra configuration directories.\n\
        2. Ensure that the remaining directory is in the correct location.\n\n\
        Recommended Directory:\n\
        * {}\n",
        msg,
        active_config_msg,
        env::var("HOME").unwrap_or_else(|_| "Not set".to_string()),
        env::var("KFTRAY_CONFIG").unwrap_or_else(|_| "Not set".to_string()),
        env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| "Not set".to_string()),
        dirs::home_dir().map_or("<home_directory_not_found>".to_string(), |p| p.join(".kftray").display().to_string())
    );

    // Use the app handle to trigger the dialog on the main thread
    spawn_blocking(move || {
        let _ = app_handle.run_on_main_thread(move || {
            MessageDialogBuilder::new("Multiple Configuration Directories Detected", &full_message)
                .kind(MessageDialogKind::Warning)
                .show(|_| {});
        });
    })
    .await
    .unwrap();
}

pub async fn alert_multiple_configs(app_handle: AppHandle) {
    let (configs, active_config) = detect_multiple_configs();
    if configs.len() > 1 {
        show_alert_dialog(app_handle, configs, active_config).await;
    }
}
