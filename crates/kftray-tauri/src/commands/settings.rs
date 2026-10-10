use std::collections::HashMap;
use std::process::Command;

use kftray_commons::utils::settings::{
    get_auto_update_enabled,
    get_last_update_check,
    get_mcp_server_enabled,
    get_mcp_server_port,
    get_performance_enabled,
    get_setting,
    get_telemetry_enabled,
    set_auto_update_enabled,
    set_disconnect_timeout,
    set_mcp_server_enabled,
    set_mcp_server_port,
    set_network_monitor,
    set_setting,
};
use kftray_commons::utils::settings::{
    get_disconnect_timeout,
    get_network_monitor,
};
use kftray_telemetry::{
    Operation,
    measure,
};
use log::{
    error,
    info,
};
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

    match get_telemetry_enabled().await {
        Ok(Some(enabled)) => {
            settings.insert("telemetry_enabled".to_string(), enabled.to_string());
        }
        Ok(None) => {}
        Err(e) => {
            error!("Failed to get telemetry enabled: {e}");
        }
    }

    match get_performance_enabled().await {
        Ok(Some(enabled)) => {
            settings.insert("performance_enabled".to_string(), enabled.to_string());
        }
        Ok(None) => {}
        Err(e) => {
            error!("Failed to get performance enabled: {e}");
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

pub(crate) fn truncate_for_display(value: String, max_bytes: usize) -> String {
    if value.len() > max_bytes {
        format!("{}...", &value[..value.floor_char_boundary(max_bytes)])
    } else {
        value
    }
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
            .map(|p| truncate_for_display(p, 60))
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

#[tauri::command]
pub async fn get_mcp_server_status() -> Result<HashMap<String, String>, String> {
    let mut status = HashMap::new();

    match get_mcp_server_enabled().await {
        Ok(enabled) => {
            status.insert("enabled".to_string(), enabled.to_string());
        }
        Err(e) => {
            error!("Failed to get MCP server enabled: {e}");
            status.insert("enabled".to_string(), "false".to_string());
        }
    }

    match get_mcp_server_port().await {
        Ok(port) => {
            status.insert("port".to_string(), port.to_string());
        }
        Err(e) => {
            error!("Failed to get MCP server port: {e}");
            status.insert("port".to_string(), "3000".to_string());
        }
    }

    let is_running = crate::mcp::is_running().await;
    status.insert("running".to_string(), is_running.to_string());

    info!("Retrieved MCP server status: {status:?}");
    Ok(status)
}

#[tauri::command]
pub async fn update_mcp_server_enabled(enabled: bool) -> Result<(), String> {
    info!("Updating MCP server enabled to {enabled}");

    set_mcp_server_enabled(enabled).await.map_err(|e| {
        error!("Failed to update MCP server enabled: {e}");
        format!("Failed to update MCP server enabled: {e}")
    })?;

    if enabled {
        let port = get_mcp_server_port().await.unwrap_or(3000);
        if let Err(e) = measure(Operation::McpStart, crate::mcp::start(port)).await {
            error!("Failed to start MCP server: {e}");
            return Err(format!("Failed to start MCP server: {e}"));
        }
    } else if crate::mcp::is_running().await
        && let Err(e) = crate::mcp::stop().await
    {
        error!("Failed to stop MCP server: {e}");
        return Err(format!("Failed to stop MCP server: {e}"));
    }

    info!("Successfully updated MCP server enabled to {enabled}");
    Ok(())
}

#[tauri::command]
pub async fn update_mcp_server_port(port: u16) -> Result<(), String> {
    info!("Updating MCP server port to {port}");

    // Validate port range (1-65535, with 0 being invalid)
    if port == 0 {
        return Err("Port cannot be 0".to_string());
    }

    // Warn about privileged ports (below 1024)
    if port < 1024 {
        info!(
            "Warning: Port {port} is a privileged port (< 1024), may require elevated permissions"
        );
    }

    let enabled = get_mcp_server_enabled().await.map_err(|e| {
        error!("Failed to read MCP server enabled: {e}");
        format!("Failed to read MCP server enabled: {e}")
    })?;

    if enabled && let Err(e) = measure(Operation::McpStart, crate::mcp::start(port)).await {
        error!("Failed to start MCP server: {e}");
        return Err(format!("Failed to start MCP server: {e}"));
    }

    set_mcp_server_port(port).await.map_err(|e| {
        error!("Failed to update MCP server port: {e}");
        format!("Failed to update MCP server port: {e}")
    })?;

    info!("Successfully updated MCP server port to {port}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_for_display_keeps_short_values() {
        assert_eq!(truncate_for_display("/usr/bin".into(), 60), "/usr/bin");
    }

    #[test]
    fn truncate_for_display_cuts_ascii_at_the_limit() {
        assert_eq!(truncate_for_display("abcdef".into(), 4), "abcd...");
    }

    #[test]
    fn truncate_for_display_does_not_split_a_multibyte_character() {
        let path = format!("{}/home/joão/.local/bin:/usr/bin", "x".repeat(50));
        let cut = path.find('ã').unwrap() + 1;

        assert_eq!(
            truncate_for_display(path.clone(), cut),
            format!("{}...", &path[..cut - 1])
        );
    }

    async fn use_test_db() -> kftray_commons::test_utils::TestDb {
        let db = kftray_commons::test_utils::test_db().await;
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        kftray_commons::utils::db::create_db_table(&pool)
            .await
            .unwrap();
        kftray_commons::utils::migration::migrate_configs(Some(&pool))
            .await
            .unwrap();
        kftray_commons::utils::db::set_db_pool(std::sync::Arc::new(pool));
        db
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[tokio::test]
    async fn changing_the_port_starts_an_enabled_mcp_server_that_failed_to_bind() {
        let _db = use_test_db().await;
        let _mcp = crate::mcp::TEST_LOCK.lock().await;
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        set_mcp_server_port(taken.local_addr().unwrap().port())
            .await
            .unwrap();

        assert!(update_mcp_server_enabled(true).await.is_err());
        assert!(!crate::mcp::is_running().await);

        let port = free_port();
        update_mcp_server_port(port).await.unwrap();

        assert!(crate::mcp::is_running().await);
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("MCP server should accept connections on the new port");
        crate::mcp::stop().await.unwrap();
    }

    #[tokio::test]
    async fn a_port_change_that_cannot_bind_keeps_the_saved_port() {
        let _db = use_test_db().await;
        let _mcp = crate::mcp::TEST_LOCK.lock().await;
        let running = free_port();
        set_mcp_server_port(running).await.unwrap();
        update_mcp_server_enabled(true).await.unwrap();

        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = taken.local_addr().unwrap().port();
        let result = update_mcp_server_port(busy).await;

        let saved = get_mcp_server_port().await.unwrap();
        let still_running = crate::mcp::is_running().await;
        crate::mcp::stop().await.unwrap();
        assert!(result.is_err());
        assert_eq!(saved, running, "the busy port {busy} was saved");
        assert!(still_running);
    }
}
