extern crate native_dialog;
use std::env;
use std::path::PathBuf;

use native_dialog::MessageDialog;
use native_dialog::MessageType;

fn detect_multiple_configs() -> Vec<PathBuf> {
    let mut config_paths = Vec::new();

    if let Some(home_dir) = dirs::home_dir() {
        let mut path = home_dir.clone();
        path.push(".kftray/configs.db");
        if path.exists() {
            config_paths.push(path);
        }
    }

    if let Ok(config_dir) = env::var("KFTRAY_CONFIG") {
        let mut path = PathBuf::from(config_dir);
        path.push("configs.db");
        if path.exists() {
            config_paths.push(path);
        }
    }

    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME") {
        let mut path = PathBuf::from(xdg_config_home);
        path.push("kftray/configs.db");
        if path.exists() {
            config_paths.push(path);
        }
    }

    config_paths
}

fn show_alert_dialog(paths: Vec<PathBuf>) {
    let msg = paths
        .into_iter()
        .map(|path| format!(" * {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n");

    let full_message = format!(
        "Multiple configuration files 'configs.db' have been detected in the following locations:\n\n{}\n\n\
        This can cause unexpected behavior. Please ensure only one 'configs.db' file exists. \n\n\
        To resolve this issue, you can:\n\
        1. Move or delete the extra 'configs.db' files.\n\
        2. Ensure that the remaining file is in the correct directory.\n\n\
        Recommended directory: '{}'\n",
        msg,
        dirs::home_dir().map_or("<home_directory_not_found>".to_string(), |p| p.join(".kftray").display().to_string())
    );

    MessageDialog::new()
        .set_type(MessageType::Warning)
        .set_title("Multiple Configuration Files Detected")
        .set_text(&full_message)
        .show_alert()
        .unwrap();
}

pub fn alert_multiple_configs() {
    let configs = detect_multiple_configs();
    if configs.len() > 1 {
        show_alert_dialog(configs);
    }
}
