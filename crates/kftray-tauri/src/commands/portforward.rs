use std::sync::Arc;

use kftray_commons::config::get_configs;
use kftray_commons::models::config_model::Config;
use kftray_commons::models::response::CustomResponse;
use kftray_commons::utils::config_state::get_configs_state;
use kftray_http_logs::HttpLogState;
use kftray_portforward::kube::{
    deploy_and_forward_pod,
    start_port_forward,
    stop_all_port_forward,
    stop_port_forward,
    stop_proxy_forward,
};
use log::error;
use log::info;
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
    let config_id = config_id
        .parse::<i64>()
        .map_err(|e| format!("Failed to parse config_id: {}", e))?;

    stop_proxy_forward(config_id, namespace, service_name).await
}

#[tauri::command]
pub async fn handle_exit_app(app_handle: tauri::AppHandle) {
    let windows_map = app_handle.windows();

    if let Some((_, window)) = windows_map.iter().next() {
        let config_states = match get_configs_state().await {
            Ok(config_states) => config_states,
            Err(err) => {
                error!("Failed to get config states: {:?}", err);
                app_handle.exit(0);
                return;
            }
        };

        let any_running = config_states.iter().any(|config| config.is_running);

        if !any_running {
            app_handle.exit(0);
            return;
        }

        tauri::api::dialog::ask(
            Some(window),
            "Exit Kftray",
            "There are active port forwards. Do you want to stop all port forwards before closing?\n\nIf you choose 'No', the active port forwards will resume the next time you open the app.\n\nIf you choose 'Yes', the active port forwards will be stopped and the app will close.",
            move |ask_result| {
                tauri::async_runtime::spawn(async move {
                    if ask_result {
                        info!("Attempting to stop all port forwards...");
                        match stop_all_port_forward().await {
                            Ok(responses) => {
                                info!("Successfully stopped all port forwards: {:?}", responses);
                                app_handle.exit(0);
                            }
                            Err(err) => {
                                error!("Failed to stop port forwards: {:?}", err);
                                app_handle.exit(0);
                            }
                        };
                    } else {
                        info!("User chose to leave all port-forwards running.");
                        app_handle.exit(0);
                    }
                });
            },
        );
    } else {
        error!("No windows found, exiting application.");
        app_handle.exit(0);
    }
}

#[cfg(test)]
mod tests {
    use kftray_commons::models::config_model::Config;
    use kftray_commons::models::config_state_model::ConfigState;

    use super::*;

    fn create_test_configs() -> Vec<Config> {
        vec![
            Config {
                id: Some(1),
                service: Some("test-service-1".to_string()),
                namespace: "test-namespace".to_string(),
                local_port: Some(8080),
                remote_port: Some(80),
                context: "test-context".to_string(),
                protocol: "tcp".to_string(),
                ..Default::default()
            },
            Config {
                id: Some(2),
                service: Some("test-service-2".to_string()),
                namespace: "test-namespace".to_string(),
                local_port: Some(9090),
                remote_port: Some(9000),
                context: "test-context".to_string(),
                protocol: "udp".to_string(),
                ..Default::default()
            },
        ]
    }

    // Helper to create test config states
    fn create_test_config_states() -> Vec<ConfigState> {
        vec![
            ConfigState {
                id: Some(1),
                config_id: 1,
                is_running: true,
            },
            ConfigState {
                id: Some(2),
                config_id: 2,
                is_running: false,
            },
        ]
    }

    // Test the config_compare_changes function
    #[test]
    fn test_config_compare_changes() {
        let vec1 = vec![1, 2, 3];
        let vec2 = vec![1, 2, 3];
        assert!(
            config_compare_changes(&vec1, &vec2),
            "Identical vectors should return true"
        );

        let vec3 = vec![1, 2, 3, 4];
        assert!(
            !config_compare_changes(&vec1, &vec3),
            "Different length vectors should return false"
        );

        let vec4 = vec![1, 2, 4];
        assert!(
            !config_compare_changes(&vec1, &vec4),
            "Different content vectors should return false"
        );

        let vec5: Vec<i32> = vec![];
        let vec6: Vec<i32> = vec![];
        assert!(
            config_compare_changes(&vec5, &vec6),
            "Empty vectors should return true"
        );
    }

    #[test]
    fn test_config_state_conversion() {
        let configs = create_test_configs();
        let config_states = create_test_config_states();

        assert_eq!(
            configs[0].id, config_states[0].id,
            "Config and ConfigState IDs should match"
        );
        assert_eq!(
            configs[1].id, config_states[1].id,
            "Config and ConfigState IDs should match"
        );
    }
}
