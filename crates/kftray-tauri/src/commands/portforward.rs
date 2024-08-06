use std::sync::Arc;

use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
use kftray_commons::models::response::CustomResponse;
use kftray_portforward::core::{
    deploy_and_forward_pod,
    start_port_forward,
    stop_all_port_forward,
    stop_port_forward,
    stop_proxy_forward,
};
use kftray_portforward::models::kube::HttpLogState;
use tauri::Manager;

async fn emit_config_state(
    app_handle: &tauri::AppHandle, config_id: i64, is_running: bool,
) -> Result<(), String> {
    let config_state = ConfigState {
        id: None,
        config_id,
        is_running,
    };
    app_handle
        .emit_all("config_state_changed", &config_state)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_port_forward_udp_cmd(
    configs: Vec<Config>, http_log_state: tauri::State<'_, HttpLogState>,
    app_handle: tauri::AppHandle,
) -> Result<Vec<CustomResponse>, String> {
    let result = start_port_forward(
        configs.clone(),
        "udp",
        Arc::new(http_log_state.inner().clone()),
    )
    .await;
    for config in configs {
        if let Some(config_id) = config.id {
            emit_config_state(&app_handle, config_id, true).await?;
        }
    }
    result
}

#[tauri::command]
pub async fn start_port_forward_tcp_cmd(
    configs: Vec<Config>, http_log_state: tauri::State<'_, HttpLogState>,
    app_handle: tauri::AppHandle,
) -> Result<Vec<CustomResponse>, String> {
    let result = start_port_forward(
        configs.clone(),
        "tcp",
        Arc::new(http_log_state.inner().clone()),
    )
    .await;
    for config in configs {
        if let Some(config_id) = config.id {
            emit_config_state(&app_handle, config_id, true).await?;
        }
    }
    result
}

#[tauri::command]
pub async fn stop_all_port_forward_cmd(
    app_handle: tauri::AppHandle,
) -> Result<Vec<CustomResponse>, String> {
    let result = stop_all_port_forward().await;
    // Assuming you have a way to get all config IDs
    let all_config_ids: Vec<i64> = vec![]; // Replace with actual logic to get all config IDs
    for config_id in all_config_ids {
        emit_config_state(&app_handle, config_id, false).await?;
    }
    result
}

#[tauri::command]
pub async fn stop_port_forward_cmd(
    config_id: String, app_handle: tauri::AppHandle,
) -> Result<CustomResponse, String> {
    let config_id_i64 = config_id.parse::<i64>().map_err(|e| e.to_string())?;
    let result = stop_port_forward(config_id.clone()).await;
    emit_config_state(&app_handle, config_id_i64, false).await?;
    result
}

#[tauri::command]
pub async fn deploy_and_forward_pod_cmd(
    configs: Vec<Config>, http_log_state: tauri::State<'_, HttpLogState>,
    app_handle: tauri::AppHandle,
) -> Result<Vec<CustomResponse>, String> {
    let result =
        deploy_and_forward_pod(configs.clone(), Arc::new(http_log_state.inner().clone())).await;
    for config in configs {
        if let Some(config_id) = config.id {
            emit_config_state(&app_handle, config_id, true).await?;
        }
    }
    result
}

#[tauri::command]
pub async fn stop_proxy_forward_cmd(
    config_id: String, namespace: &str, service_name: String, app_handle: tauri::AppHandle,
) -> Result<CustomResponse, String> {
    let config_id_i64 = config_id.parse::<i64>().map_err(|e| e.to_string())?;
    let result = stop_proxy_forward(config_id.clone(), namespace, service_name).await;
    emit_config_state(&app_handle, config_id_i64, false).await?;
    result
}
