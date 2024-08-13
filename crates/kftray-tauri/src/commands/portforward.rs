use std::sync::Arc;

use kftray_commons::config::get_configs;
use kftray_commons::models::config_model::Config;
use kftray_commons::models::response::CustomResponse;
use kftray_commons::utils::config_state::get_configs_state;
use kftray_portforward::core::{
    deploy_and_forward_pod,
    start_port_forward,
    stop_all_port_forward,
    stop_port_forward,
    stop_proxy_forward,
};
use kftray_portforward::models::kube::HttpLogState;
use log::error;
use tauri::AppHandle;
use tauri::Manager;
use tokio::sync::Mutex;
use tokio::time::{
    interval,
    Duration,
};

pub async fn check_and_emit_changes(app_handle: AppHandle) {
    let mut interval = interval(Duration::from_millis(500));
    let previous_config_states = Arc::new(Mutex::new(Vec::new()));
    let previous_configs = Arc::new(Mutex::new(Vec::new()));

    loop {
        interval.tick().await;

        let current_config_states = match get_configs_state().await {
            Ok(states) => states,
            Err(e) => {
                error!("Failed to get config states: {}", e);
                continue;
            }
        };

        let current_configs = match get_configs().await {
            Ok(configs) => configs,
            Err(e) => {
                error!("Failed to get configs: {}", e);
                continue;
            }
        };

        let mut prev_states = previous_config_states.lock().await;
        let mut prev_configs = previous_configs.lock().await;

        if !config_compare_changes(&prev_states, &current_config_states)
            || !config_compare_changes(&prev_configs, &current_configs)
        {
            app_handle
                .emit_all("config_state_changed", &Vec::<Config>::new())
                .unwrap_or_else(|e| {
                    error!("Failed to emit configs changed event: {}", e);
                });

            log::info!("Configs changed event emitted");

            *prev_states = current_config_states;
            *prev_configs = current_configs;
        }
    }
}

fn config_compare_changes<T: PartialEq>(prev: &[T], current: &[T]) -> bool {
    if prev.len() != current.len() {
        return false;
    }

    for (prev_item, current_item) in prev.iter().zip(current.iter()) {
        if prev_item != current_item {
            return false;
        }
    }

    true
}

#[tauri::command]
pub async fn start_port_forward_udp_cmd(
    configs: Vec<Config>, http_log_state: tauri::State<'_, HttpLogState>,
    _app_handle: tauri::AppHandle,
) -> Result<Vec<CustomResponse>, String> {
    start_port_forward(
        configs.clone(),
        "udp",
        Arc::new(http_log_state.inner().clone()),
    )
    .await
}

#[tauri::command]
pub async fn start_port_forward_tcp_cmd(
    configs: Vec<Config>, http_log_state: tauri::State<'_, HttpLogState>,
    _app_handle: tauri::AppHandle,
) -> Result<Vec<CustomResponse>, String> {
    start_port_forward(
        configs.clone(),
        "tcp",
        Arc::new(http_log_state.inner().clone()),
    )
    .await
}

#[tauri::command]
pub async fn stop_all_port_forward_cmd(
    _app_handle: tauri::AppHandle,
) -> Result<Vec<CustomResponse>, String> {
    stop_all_port_forward().await
}

#[tauri::command]
pub async fn stop_port_forward_cmd(
    config_id: String, _app_handle: tauri::AppHandle,
) -> Result<CustomResponse, String> {
    stop_port_forward(config_id.clone()).await
}

#[tauri::command]
pub async fn deploy_and_forward_pod_cmd(
    configs: Vec<Config>, http_log_state: tauri::State<'_, HttpLogState>,
    _app_handle: tauri::AppHandle,
) -> Result<Vec<CustomResponse>, String> {
    deploy_and_forward_pod(configs.clone(), Arc::new(http_log_state.inner().clone())).await
}

#[tauri::command]
pub async fn stop_proxy_forward_cmd(
    config_id: String, namespace: &str, service_name: String, _app_handle: tauri::AppHandle,
) -> Result<CustomResponse, String> {
    stop_proxy_forward(config_id.clone(), namespace, service_name).await
}
