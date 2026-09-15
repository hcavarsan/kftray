use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use kftray_commons::config::get_configs;
use kftray_commons::models::config_model::Config;
use kftray_commons::models::response::CustomResponse;
use kftray_commons::utils::config_state::{
    cleanup_current_process_config_states_with_mode,
    get_configs_state,
};
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_portforward::kube::{
    deploy_and_forward_pod,
    reconcile_pending_cleanup,
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
    Duration,
    interval,
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
        let all_active_pods = kftray_portforward::port_forward::active_pods().await;
        let mut current_active_pods = HashMap::new();
        for state in &current_config_states {
            if state.is_running {
                let pod_name = all_active_pods.get(&state.config_id).cloned().flatten();
                current_active_pods.insert(state.config_id.to_string(), pod_name);
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

/// Bounds the exit-time reconciliation below: a stop or a pending-create
/// wait up against lifecycle locks and cluster deletes, and a stalled one
/// must not keep the process from exiting. Mirrors kftui's
/// `CLEANUP_RECONCILE_TIMEOUT`.
const CLEANUP_RECONCILE_TIMEOUT: Duration =
    kftray_portforward::kube::UNCERTAIN_CREATE_WINDOW.saturating_mul(2);

/// Whether a config is started as a plain TCP port-forward rather than
/// deployed through a relay pod: expose and tcp service/pod configs take
/// the direct path, everything else (proxy, udp service/pod) goes through
/// the relay. A missing workload type is treated like `service`/`pod`,
/// matching kftui and the old auto-start check. Shared by `dispatch_start`
/// and the auto-start check on launch so the two cannot silently diverge on
/// which configs get which treatment.
pub(crate) fn is_direct_tcp_forward(workload_type: Option<&str>, protocol: &str) -> bool {
    workload_type == Some("expose")
        || (matches!(workload_type, None | Some("service" | "pod")) && protocol == "tcp")
}

/// Starts one configuration through the dispatch its kind uses. Shared by
/// the global shortcut handlers and the SSL certificate restart path so the
/// two cannot silently diverge on which configs get which treatment.
pub(crate) async fn dispatch_start(config: &Config) -> Result<Vec<CustomResponse>, String> {
    if is_direct_tcp_forward(config.workload_type.as_deref(), &config.protocol) {
        start_port_forward(vec![config.clone()], "tcp").await
    } else {
        deploy_and_forward_pod(vec![config.clone()]).await
    }
}

/// Stops one configuration, regardless of its kind: `stop_port_forward` looks
/// the row up by id and unwinds whatever it started (direct forward, proxy
/// relay, or expose), so callers no longer need to branch on workload type
/// to pick a stop command. Shared by the global shortcut handlers so stop
/// and toggle cannot silently diverge from that.
pub(crate) async fn dispatch_stop(config: &Config) -> Result<CustomResponse, String> {
    stop_port_forward(config.id.unwrap_or(0).to_string()).await
}

/// Reconciles anything this process still owes a cluster delete for, then
/// clears its rows from `config_state` so the next launch does not see them
/// as still running. Mirrors kftui's shutdown sequence.
async fn reconcile_and_cleanup_on_exit() {
    let still_owed = reconcile_pending_cleanup(
        DatabaseMode::File,
        CLEANUP_RECONCILE_TIMEOUT,
        &HashSet::new(),
    )
    .await;
    if !still_owed.is_empty() {
        error!(
            "Cleanup for configuration(s) {still_owed:?} did not complete; they stay marked \
             running and are retried on the next stop"
        );
    }
    if let Err(e) =
        cleanup_current_process_config_states_with_mode(DatabaseMode::File, &still_owed).await
    {
        error!("Failed to cleanup config states: {e}");
    }
}

#[tauri::command]
pub async fn start_port_forward_udp_cmd(
    configs: Vec<Config>, _app_handle: tauri::AppHandle<Wry>,
) -> Result<Vec<CustomResponse>, String> {
    start_port_forward(configs, "udp").await
}

#[tauri::command]
pub async fn start_port_forward_tcp_cmd(
    configs: Vec<Config>, _app_handle: tauri::AppHandle<Wry>,
) -> Result<Vec<CustomResponse>, String> {
    start_port_forward(configs, "tcp").await
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
    stop_port_forward(config_id).await
}

#[tauri::command]
pub async fn deploy_and_forward_pod_cmd(
    configs: Vec<Config>, _app_handle: tauri::AppHandle<Wry>,
) -> Result<Vec<CustomResponse>, String> {
    deploy_and_forward_pod(configs).await
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

    let config_id = config_id
        .parse::<i64>()
        .map_err(|e| format!("Invalid config ID: {e}"))?;
    let forwarder = CHILD_PROCESSES
        .get(&config_id)
        .and_then(|process| process.direct_forwarder.clone());
    match forwarder {
        Some(forwarder) => Ok(forwarder.get_current_active_pod().await),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn handle_exit_app(app_handle: tauri::AppHandle<Wry>) {
    match app_handle.get_webview_window("main") {
        Some(window) => {
            let config_states = match get_configs_state().await {
                Ok(config_states) => config_states,
                Err(err) => {
                    error!("Failed to get config states: {err:?}");
                    std::process::exit(0);
                }
            };

            let any_running = config_states.iter().any(|config| config.is_running);

            if !any_running {
                reconcile_and_cleanup_on_exit().await;
                // Stop MCP server if running
                if let Err(e) = crate::mcp::stop().await {
                    error!("Failed to stop MCP server: {e}");
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
                            reconcile_and_cleanup_on_exit().await;
                            // Stop MCP server if running
                            if let Err(e) = crate::mcp::stop().await {
                                error!("Failed to stop MCP server: {e}");
                            }
                            std::process::exit(0);
                        });
                    }
                    false => {
                        // User clicked "No" - leave port forwards running and just exit
                        info!("User chose to leave all port-forwards running.");
                        // Stop MCP server if running
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = crate::mcp::stop().await {
                                error!("Failed to stop MCP server: {e}");
                            }
                            std::process::exit(0);
                        });
                    }
                }
            });
        }
        _ => {
            error!("No windows found, exiting application.");
            reconcile_and_cleanup_on_exit().await;
            // Stop MCP server if running
            if let Err(e) = crate::mcp::stop().await {
                error!("Failed to stop MCP server: {e}");
            }
            std::process::exit(0);
        }
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
                ..Default::default()
            },
            ConfigState {
                id: Some(2),
                config_id: 2,
                is_running: false,
                process_id: None,
                ..Default::default()
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
                ..Default::default()
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

    #[test]
    fn none_workload_type_dispatches_like_service_or_pod() {
        // Regression: a config with no workload_type (the common case for
        // configs created before the field existed, and for every kftui
        // config) used to be treated as a relay deploy for both protocols,
        // while kftui and the old auto-start check treated it like
        // `service`/`pod`: a direct TCP forward, a relay for UDP.
        assert!(
            is_direct_tcp_forward(None, "tcp"),
            "a None workload type with tcp must take the direct forward path, like service/pod"
        );
        assert!(
            !is_direct_tcp_forward(None, "udp"),
            "a None workload type with udp must still go through the relay, like service/pod"
        );
    }

    #[test]
    fn service_and_pod_workload_types_match_none() {
        assert_eq!(
            is_direct_tcp_forward(Some("service"), "tcp"),
            is_direct_tcp_forward(None, "tcp")
        );
        assert_eq!(
            is_direct_tcp_forward(Some("pod"), "udp"),
            is_direct_tcp_forward(None, "udp")
        );
    }

    #[test]
    fn expose_is_always_direct_and_proxy_never_is() {
        assert!(is_direct_tcp_forward(Some("expose"), "udp"));
        assert!(!is_direct_tcp_forward(Some("proxy"), "tcp"));
    }
}
