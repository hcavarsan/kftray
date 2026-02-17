use std::collections::HashMap;
use std::process::Command;

use kftray_commons::utils::settings::{
    get_auto_update_enabled, get_env_auto_sync_enabled, get_env_auto_sync_path,
    get_last_update_check, get_setting, set_auto_update_enabled, set_disconnect_timeout,
    set_env_auto_sync_enabled, set_env_auto_sync_path, set_network_monitor, set_setting,
};
use kftray_commons::utils::settings::{get_disconnect_timeout, get_network_monitor};
use log::{error, info};
use serde::Serialize;

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
    } else if kftray_network_monitor::is_running().await
        && let Err(e) = kftray_network_monitor::stop().await
    {
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

#[derive(Serialize)]
pub struct EnvAutoSyncSettings {
    pub enabled: bool,
    pub path: Option<String>,
}

#[tauri::command]
pub async fn get_env_auto_sync_settings() -> Result<EnvAutoSyncSettings, String> {
    let enabled = get_env_auto_sync_enabled().await.map_err(|e| {
        error!("Failed to get env_auto_sync_enabled: {e}");
        format!("Failed to get env_auto_sync_enabled: {e}")
    })?;

    let path = get_env_auto_sync_path().await.map_err(|e| {
        error!("Failed to get env_auto_sync_path: {e}");
        format!("Failed to get env_auto_sync_path: {e}")
    })?;

    Ok(EnvAutoSyncSettings { enabled, path })
}

#[tauri::command]
pub async fn set_env_auto_sync_settings(enabled: bool, path: Option<String>) -> Result<(), String> {
    info!("Setting env auto-sync: enabled={enabled}, path={path:?}");

    set_env_auto_sync_enabled(enabled).await.map_err(|e| {
        error!("Failed to set env_auto_sync_enabled: {e}");
        format!("Failed to set env_auto_sync_enabled: {e}")
    })?;

    let path_value = path.clone().unwrap_or_default();
    info!("Saving env_auto_sync_path to database: '{}'", path_value);
    set_env_auto_sync_path(&path_value).await.map_err(|e| {
        error!("Failed to set env_auto_sync_path: {e}");
        format!("Failed to set env_auto_sync_path: {e}")
    })?;

    // Verify the settings were saved correctly
    let verify_enabled = get_env_auto_sync_enabled().await.unwrap_or(false);
    let verify_path = get_env_auto_sync_path().await.ok().flatten();
    info!(
        "Verified saved settings: enabled={}, path={:?}",
        verify_enabled, verify_path
    );

    info!("Successfully set env auto-sync settings");
    Ok(())
}

#[derive(Serialize)]
pub struct DiagnosticResult {
    pub name: String,
    pub status: String,
    pub value: String,
    pub hint: String,
}

#[derive(Serialize)]
pub struct DiagnosticsReport {
    pub checks: Vec<DiagnosticResult>,
    pub overall_status: String,
}

fn check_command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn get_command_path(cmd: &str) -> Option<String> {
    Command::new("which")
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}

#[tauri::command]
pub async fn run_diagnostics() -> Result<DiagnosticsReport, String> {
    let mut checks = Vec::new();
    let mut has_errors = false;

    let home = std::env::var("HOME");
    checks.push(DiagnosticResult {
        name: "HOME".into(),
        status: if home.is_ok() {
            "ok".into()
        } else {
            "error".into()
        },
        value: home.clone().unwrap_or_else(|_| "<not set>".into()),
        hint: if home.is_err() {
            "HOME is required for credential files (~/.aws, ~/.kube)".into()
        } else {
            String::new()
        },
    });
    if home.is_err() {
        has_errors = true;
    }

    let path = std::env::var("PATH");
    checks.push(DiagnosticResult {
        name: "PATH".into(),
        status: if path.is_ok() {
            "ok".into()
        } else {
            "error".into()
        },
        value: path
            .clone()
            .map(|p| {
                if p.len() > 60 {
                    format!("{}...", &p[..60])
                } else {
                    p
                }
            })
            .unwrap_or_else(|_| "<not set>".into()),
        hint: if path.is_err() {
            "PATH is required to find CLI tools".into()
        } else {
            String::new()
        },
    });
    if path.is_err() {
        has_errors = true;
    }

    let kubeconfig = std::env::var("KUBECONFIG");
    let default_kubeconfig = home
        .as_ref()
        .map(|h| format!("{}/.kube/config", h))
        .unwrap_or_default();
    let kubeconfig_exists = kubeconfig
        .as_ref()
        .map(|p| std::path::Path::new(p).exists())
        .unwrap_or_else(|_| std::path::Path::new(&default_kubeconfig).exists());
    checks.push(DiagnosticResult {
        name: "KUBECONFIG".into(),
        status: if kubeconfig_exists {
            "ok".into()
        } else {
            "warning".into()
        },
        value: kubeconfig.unwrap_or_else(|_| format!("{} (default)", default_kubeconfig)),
        hint: if !kubeconfig_exists {
            "Kubeconfig file not found".into()
        } else {
            String::new()
        },
    });

    let kubectl_exists = check_command_exists("kubectl");
    checks.push(DiagnosticResult {
        name: "kubectl".into(),
        status: if kubectl_exists {
            "ok".into()
        } else {
            "warning".into()
        },
        value: get_command_path("kubectl").unwrap_or_else(|| "<not found>".into()),
        hint: if !kubectl_exists {
            "kubectl not in PATH (optional but recommended)".into()
        } else {
            String::new()
        },
    });

    let aws_exists = check_command_exists("aws");
    let aws_profile = std::env::var("AWS_PROFILE").ok();
    checks.push(DiagnosticResult {
        name: "AWS CLI".into(),
        status: if aws_exists {
            "ok".into()
        } else {
            "info".into()
        },
        value: get_command_path("aws")
            .map(|p| {
                if let Some(ref profile) = aws_profile {
                    format!("{} (profile: {})", p, profile)
                } else {
                    p
                }
            })
            .unwrap_or_else(|| "<not found>".into()),
        hint: if !aws_exists {
            "AWS CLI not found (required for EKS)".into()
        } else {
            String::new()
        },
    });

    let gcloud_exists = check_command_exists("gcloud");
    checks.push(DiagnosticResult {
        name: "gcloud CLI".into(),
        status: if gcloud_exists {
            "ok".into()
        } else {
            "info".into()
        },
        value: get_command_path("gcloud").unwrap_or_else(|| "<not found>".into()),
        hint: if !gcloud_exists {
            "gcloud CLI not found (required for GKE)".into()
        } else {
            String::new()
        },
    });

    let az_exists = check_command_exists("az");
    checks.push(DiagnosticResult {
        name: "Azure CLI".into(),
        status: if az_exists {
            "ok".into()
        } else {
            "info".into()
        },
        value: get_command_path("az").unwrap_or_else(|| "<not found>".into()),
        hint: if !az_exists {
            "Azure CLI not found (required for AKS)".into()
        } else {
            String::new()
        },
    });

    let overall_status = if has_errors {
        "error"
    } else if checks.iter().any(|c| c.status == "warning") {
        "warning"
    } else {
        "ok"
    }
    .into();

    Ok(DiagnosticsReport {
        checks,
        overall_status,
    })
}
