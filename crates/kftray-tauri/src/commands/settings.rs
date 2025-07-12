use std::collections::HashMap;

use kftray_commons::utils::settings::{
    get_disconnect_timeout,
    get_network_monitor,
    get_setting,
    set_disconnect_timeout,
    set_network_monitor,
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

    match get_network_monitor().await {
        Ok(enabled) => {
            settings.insert("network_monitor".to_string(), enabled.to_string());
        }
        Err(e) => {
            error!("Failed to get network monitor: {e}");
            settings.insert("network_monitor".to_string(), "true".to_string());
        }
    }

    // Add network monitor status
    let is_running = kftray_network_monitor::is_running().await;
    settings.insert("network_monitor_status".to_string(), is_running.to_string());

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

#[command]
pub async fn update_network_monitor(enabled: bool) -> Result<(), String> {
    info!("Updating network monitor to {enabled}");

    // Save setting to database
    set_network_monitor(enabled).await.map_err(|e| {
        error!("Failed to update network monitor setting: {e}");
        format!("Failed to update network monitor setting: {e}")
    })?;

    // Control network monitor at runtime
    if enabled {
        if let Err(e) = kftray_network_monitor::restart().await {
            error!("Failed to start network monitor: {e}");
            return Err(format!("Failed to start network monitor: {e}"));
        }
    } else if let Err(e) = kftray_network_monitor::stop().await {
        error!("Failed to stop network monitor: {e}");
        return Err(format!("Failed to stop network monitor: {e}"));
    }

    info!("Successfully updated network monitor to {enabled}");
    Ok(())
}
