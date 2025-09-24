use std::collections::HashMap;

use kftray_commons::utils::settings::{
    get_auto_update_enabled,
    get_last_update_check,
    get_setting,
    set_auto_update_enabled,
    set_disconnect_timeout,
    set_network_monitor,
    set_setting,
};
use kftray_commons::utils::settings::{
    get_disconnect_timeout,
    get_network_monitor,
};
use log::{
    error,
    info,
};

#[tauri::command]
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

    let is_running = kftray_network_monitor::is_running().await;
    settings.insert("network_monitor_status".to_string(), is_running.to_string());

    match get_auto_update_enabled().await {
        Ok(enabled) => {
            settings.insert("auto_update_enabled".to_string(), enabled.to_string());
        }
        Err(e) => {
            error!("Failed to get auto update enabled: {e}");
            settings.insert("auto_update_enabled".to_string(), "true".to_string());
        }
    }

    match get_last_update_check().await {
        Ok(Some(timestamp)) => {
            settings.insert("last_update_check".to_string(), timestamp.to_string());
        }
        Ok(None) => {
            settings.insert("last_update_check".to_string(), "0".to_string());
        }
        Err(e) => {
            error!("Failed to get last update check: {e}");
            settings.insert("last_update_check".to_string(), "0".to_string());
        }
    }

    info!("Retrieved settings: {settings:?}");
    Ok(settings)
}

#[tauri::command]
pub async fn update_disconnect_timeout(minutes: u32) -> Result<(), String> {
    info!("Updating disconnect timeout to {minutes} minutes");

    set_disconnect_timeout(minutes).await.map_err(|e| {
        error!("Failed to update disconnect timeout: {e}");
        format!("Failed to update disconnect timeout: {e}")
    })?;

    info!("Successfully updated disconnect timeout to {minutes} minutes");
    Ok(())
}

#[tauri::command]
pub async fn get_setting_value(key: String) -> Result<Option<String>, String> {
    get_setting(&key).await.map_err(|e| {
        error!("Failed to get setting {key}: {e}");
        format!("Failed to get setting: {e}")
    })
}

#[tauri::command]
pub async fn set_setting_value(key: String, value: String) -> Result<(), String> {
    info!("Setting {key} = {value}");

    set_setting(&key, &value).await.map_err(|e| {
        error!("Failed to set setting {key}: {e}");
        format!("Failed to set setting: {e}")
    })?;

    info!("Successfully set {key} = {value}");
    Ok(())
}

#[tauri::command]
pub async fn update_network_monitor(enabled: bool) -> Result<(), String> {
    info!("Updating network monitor to {enabled}");

    set_network_monitor(enabled).await.map_err(|e| {
        error!("Failed to update network monitor setting: {e}");
        format!("Failed to update network monitor setting: {e}")
    })?;

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

#[tauri::command]
pub async fn update_auto_update_enabled(enabled: bool) -> Result<(), String> {
    info!("Updating auto update enabled to {enabled}");

    set_auto_update_enabled(enabled).await.map_err(|e| {
        error!("Failed to update auto update enabled: {e}");
        format!("Failed to update auto update enabled: {e}")
    })?;

    info!("Successfully updated auto update enabled to {enabled}");
    Ok(())
}

#[tauri::command]
pub async fn get_auto_update_status() -> Result<HashMap<String, String>, String> {
    let mut status = HashMap::new();

    match get_auto_update_enabled().await {
        Ok(enabled) => {
            status.insert("enabled".to_string(), enabled.to_string());
        }
        Err(e) => {
            error!("Failed to get auto update enabled: {e}");
            status.insert("enabled".to_string(), "true".to_string());
        }
    }

    match get_last_update_check().await {
        Ok(Some(timestamp)) => {
            status.insert("last_check".to_string(), timestamp.to_string());
        }
        Ok(None) => {
            status.insert("last_check".to_string(), "0".to_string());
        }
        Err(e) => {
            error!("Failed to get last update check: {e}");
            status.insert("last_check".to_string(), "0".to_string());
        }
    }

    Ok(status)
}
