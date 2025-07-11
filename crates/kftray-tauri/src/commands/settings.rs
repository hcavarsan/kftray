use std::collections::HashMap;

use kftray_commons::utils::settings::{
    get_disconnect_timeout,
    get_setting,
    set_disconnect_timeout,
    set_setting,
};
use log::{
    error,
    info,
};
use tauri::command;

#[command]
pub async fn get_settings() -> Result<HashMap<String, String>, String> {
    let mut settings = HashMap::new();

    match get_disconnect_timeout().await {
        Ok(Some(timeout)) => {
            settings.insert(
                "disconnect_timeout_minutes".to_string(),
                timeout.to_string(),
            );
        }
        Ok(None) => {
            settings.insert("disconnect_timeout_minutes".to_string(), "0".to_string());
        }
        Err(e) => {
            error!("Failed to get disconnect timeout: {e}");
            settings.insert("disconnect_timeout_minutes".to_string(), "0".to_string());
        }
    }

    info!("Retrieved settings: {settings:?}");
    Ok(settings)
}

#[command]
pub async fn update_disconnect_timeout(minutes: u32) -> Result<(), String> {
    info!("Updating disconnect timeout to {minutes} minutes");

    set_disconnect_timeout(minutes).await.map_err(|e| {
        error!("Failed to update disconnect timeout: {e}");
        format!("Failed to update disconnect timeout: {e}")
    })?;

    info!("Successfully updated disconnect timeout to {minutes} minutes");
    Ok(())
}

#[command]
pub async fn get_setting_value(key: String) -> Result<Option<String>, String> {
    get_setting(&key).await.map_err(|e| {
        error!("Failed to get setting {key}: {e}");
        format!("Failed to get setting: {e}")
    })
}

#[command]
pub async fn set_setting_value(key: String, value: String) -> Result<(), String> {
    info!("Setting {key} = {value}");

    set_setting(&key, &value).await.map_err(|e| {
        error!("Failed to set setting {key}: {e}");
        format!("Failed to set setting: {e}")
    })?;

    info!("Successfully set {key} = {value}");
    Ok(())
}
