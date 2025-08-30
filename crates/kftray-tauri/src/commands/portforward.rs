use std::collections::HashMap;
use std::sync::Arc;

use kftray_commons::config::get_configs;
use kftray_commons::models::config_model::Config;
use kftray_commons::models::response::CustomResponse;
use kftray_commons::utils::config_state::{
    cleanup_current_process_config_states,
    get_configs_state,
};
use kftray_portforward::kube::{
    deploy_and_forward_pod,
    start_port_forward,
    stop_all_port_forward,
    stop_port_forward,
    stop_proxy_forward,
};
use log::error;
use log::info;
use serde_json::json;
use tauri::{
    AppHandle,
    Emitter,
    Manager,
    Wry,
};
use tauri_plugin_dialog::{
    DialogExt,
    MessageDialogButtons,
};
use tokio::sync::Mutex;
use tokio::time::{
    interval,
    Duration,
};

pub async fn check_and_emit_changes(app_handle: AppHandle<Wry>) {
    let mut interval = interval(Duration::from_millis(500));
    let previous_config_states = Arc::new(Mutex::new(Vec::new()));
    let previous_configs = Arc::new(Mutex::new(Vec::new()));
    let previous_active_pods = Arc::new(Mutex::new(HashMap::<String, Option<String>>::new()));

    loop {
        interval.tick().await;

        let current_config_states = match get_configs_state().await {
            Ok(states) => states,
            Err(e) => {
                error!("Failed to get config states: {e}");
                continue;
            }
        };

        let current_configs = match get_configs().await {
            Ok(configs) => configs,
            Err(e) => {
                error!("Failed to get configs: {e}");
                continue;
            }
        };
        let mut current_active_pods = HashMap::new();
        for state in &current_config_states {
            if state.is_running {
                match get_active_pod_cmd(state.config_id.to_string()).await {
                    Ok(pod_name) => {
                        current_active_pods.insert(state.config_id.to_string(), pod_name);
                    }
                    Err(_) => {
                        current_active_pods.insert(state.config_id.to_string(), None);
                    }
                }
            }
        }
        let mut prev_pods = previous_active_pods.lock().await;
        for (config_id, current_pod) in &current_active_pods {
            let should_emit = if let Some(prev_pod) = prev_pods.get(config_id) {
                prev_pod != current_pod
            } else {
                current_pod.is_some()
            };

            if should_emit {
                let payload = json!({
                    "configId": config_id,
                    "podName": current_pod
                });

                app_handle
                    .emit("active_pod_changed", payload)
                    .unwrap_or_else(|e| {
                        error!("Failed to emit active pod changed event: {e}");
                    });
            }
        }
        *prev_pods = current_active_pods;

        let mut prev_states = previous_config_states.lock().await;
        let mut prev_configs = previous_configs.lock().await;

        if !config_compare_changes(&prev_states, &current_config_states)
            || !config_compare_changes(&prev_configs, &current_configs)
        {
            app_handle
                .emit("config_state_changed", &Vec::<Config>::new())
                .unwrap_or_else(|e| {
                    error!("Failed to emit configs changed event: {e}");
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
    configs: Vec<Config>, _app_handle: tauri::AppHandle<Wry>,
) -> Result<Vec<CustomResponse>, String> {
    start_port_forward(configs.clone(), "udp").await
}

#[tauri::command]
pub async fn start_port_forward_tcp_cmd(
    configs: Vec<Config>, _app_handle: tauri::AppHandle<Wry>,
) -> Result<Vec<CustomResponse>, String> {
    start_port_forward(configs.clone(), "tcp").await
}

#[tauri::command]
pub async fn stop_all_port_forward_cmd(
    _app_handle: tauri::AppHandle<Wry>,
) -> Result<Vec<CustomResponse>, String> {
    stop_all_port_forward().await
}

#[tauri::command]
pub async fn stop_port_forward_cmd(
    config_id: String, _app_handle: tauri::AppHandle<Wry>,
) -> Result<CustomResponse, String> {
    stop_port_forward(config_id.clone()).await
}

#[tauri::command]
pub async fn deploy_and_forward_pod_cmd(
    configs: Vec<Config>, _app_handle: tauri::AppHandle<Wry>,
) -> Result<Vec<CustomResponse>, String> {
    deploy_and_forward_pod(configs.clone()).await
}

#[tauri::command]
pub async fn stop_proxy_forward_cmd(
    config_id: String, namespace: &str, service_name: String, _app_handle: tauri::AppHandle<Wry>,
) -> Result<CustomResponse, String> {
    let config_id = config_id
        .parse::<i64>()
        .map_err(|e| format!("Failed to parse config_id: {e}"))?;

    stop_proxy_forward(config_id, namespace, service_name).await
}

#[tauri::command]
pub async fn get_active_pod_cmd(config_id: String) -> Result<Option<String>, String> {
    use kftray_portforward::port_forward::CHILD_PROCESSES;

    let handle_key = format!("config:{}:service:", config_id);

    let processes = CHILD_PROCESSES.lock().await;

    for (key, process) in processes.iter() {
        if key.starts_with(&handle_key) {
            if let Some(pod_name) = process.get_current_active_pod().await {
                return Ok(Some(pod_name));
            }
        }
    }

    Ok(None)
}

#[tauri::command]
pub async fn handle_exit_app(app_handle: tauri::AppHandle<Wry>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let config_states = match get_configs_state().await {
            Ok(config_states) => config_states,
            Err(err) => {
                error!("Failed to get config states: {err:?}");
                std::process::exit(0);
            }
        };

        let any_running = config_states.iter().any(|config| config.is_running);

        if !any_running {
            if let Err(e) = cleanup_current_process_config_states().await {
                error!("Failed to cleanup config states: {e}");
            }
            std::process::exit(0);
        }

        window
            .dialog()
            .message("There are active port forwards. Do you want to stop all port forwards before closing?\n\nIf you choose 'No', the active port forwards will resume the next time you open the app.\n\nIf you choose 'Yes', the active port forwards will be stopped and the app will close.")
            .title("Exit Kftray")
            .buttons(MessageDialogButtons::YesNo)
            .show(move |response| {
                match response {
                    true => {
                        // User clicked "Yes" - stop all port forwards
                        info!("User chose to stop all port forwards before closing.");
                        tauri::async_runtime::spawn(async move {
                            match stop_all_port_forward().await {
                                Ok(responses) => {
                                    info!("Successfully stopped all port forwards: {responses:?}");
                                }
                                Err(err) => {
                                    error!("Failed to stop port forwards: {err:?}");
                                }
                            }
                            if let Err(e) = cleanup_current_process_config_states().await {
                                error!("Failed to cleanup config states: {e}");
                            }
                            std::process::exit(0);
                        });
                    }
                    false => {
                        // User clicked "No" - leave port forwards running and just exit
                        info!("User chose to leave all port-forwards running.");
                        std::process::exit(0);
                    }
                }
            });
    } else {
        error!("No windows found, exiting application.");
        if let Err(e) = cleanup_current_process_config_states().await {
            error!("Failed to cleanup config states: {e}");
        }
        std::process::exit(0);
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
                context: Some("test-context".to_string()),
                protocol: "tcp".to_string(),
                ..Default::default()
            },
            Config {
                id: Some(2),
                service: Some("test-service-2".to_string()),
                namespace: "test-namespace".to_string(),
                local_port: Some(9090),
                remote_port: Some(9000),
                context: Some("test-context".to_string()),
                protocol: "udp".to_string(),
                ..Default::default()
            },
        ]
    }

    fn create_test_config_states() -> Vec<ConfigState> {
        vec![
            ConfigState {
                id: Some(1),
                config_id: 1,
                is_running: true,
                process_id: Some(1234),
            },
            ConfigState {
                id: Some(2),
                config_id: 2,
                is_running: false,
                process_id: None,
            },
        ]
    }

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
    fn test_config_and_state_ids_match() {
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

    #[test]
    fn test_config_state_conversion() {
        let configs = create_test_configs();

        // Create config states from configs to test actual conversion
        let config_states: Vec<ConfigState> = configs
            .iter()
            .map(|config| ConfigState {
                id: config.id,
                config_id: config.id.unwrap_or_default(),
                is_running: false,
                process_id: None,
            })
            .collect();

        assert_eq!(
            configs[0].id,
            Some(config_states[0].config_id),
            "Config ID should match ConfigState config_id"
        );
        assert_eq!(
            configs[1].id,
            Some(config_states[1].config_id),
            "Config ID should match ConfigState config_id"
        );
    }
}
