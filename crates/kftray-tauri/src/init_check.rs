use std::future::Future;
use std::sync::Arc;

use kftray_commons::config::get_config;
use kftray_commons::config_state::{
    get_configs_state,
    update_config_state,
};
use kftray_commons::config_state_model::ConfigState;
use kftray_commons::models::config_model::Config;
use kftray_http_logs::HttpLogState;
use kftray_portforward::kube::deploy_and_forward_pod;
use kftray_portforward::start_port_forward;
use log::{
    debug,
    error,
    info,
    warn,
};
use netstat2::{
    get_sockets_info,
    AddressFamilyFlags,
    ProtocolFlags,
    ProtocolSocketInfo,
};
use sysinfo::{
    Pid,
    System,
};

async fn fetch_configs_in_parallel(
    running_configs: Vec<ConfigState>,
) -> Vec<(i64, Result<Config, String>)> {
    let mut config_tasks = Vec::with_capacity(running_configs.len());

    for config_state in running_configs {
        let config_id = config_state.config_id;
        let task = tokio::spawn(async move {
            let result = get_config(config_id)
                .await
                .map_err(|e| format!("Failed to retrieve config {}: {}", config_id, e));
            (config_id, result)
        });
        config_tasks.push(task);
    }

    let mut results = Vec::with_capacity(config_tasks.len());
    for task in config_tasks {
        match task.await {
            Ok((config_id, result)) => {
                results.push((config_id, result));
            }
            Err(e) => {
                error!("Task for fetching config failed: {}", e);
            }
        }
    }

    results
}

async fn run_with_timeout<T, E, F>(
    future: F, timeout_secs: u64, timeout_message: &str,
) -> Result<T, String>
where
    F: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    match tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), future).await {
        Ok(result) => result.map_err(|e| e.to_string()),
        Err(_) => Err(timeout_message.to_string()),
    }
}

pub async fn check_and_manage_ports() -> Result<(), String> {
    let running_configs = match get_configs_state().await {
        Ok(states) => states
            .into_iter()
            .filter(|state| state.is_running)
            .collect::<Vec<_>>(),
        Err(e) => {
            error!("Failed to retrieve config states: {:?}", e);
            return Err(e);
        }
    };

    if running_configs.is_empty() {
        debug!("No running port forwards found to restore");
        return Ok(());
    }

    info!("Restoring {} running port forwards", running_configs.len());

    let config_results = fetch_configs_in_parallel(running_configs).await;

    let mut port_tasks = Vec::new();
    let mut fetch_errors = Vec::new();

    for (config_id, result) in config_results {
        match result {
            Ok(config) => {
                let task = tokio::spawn(async move {
                    if let Err(err) = check_and_manage_port(config).await {
                        error!("Error checking state for config {}: {}", config_id, err);
                    }
                });
                port_tasks.push(task);
            }
            Err(e) => {
                fetch_errors.push(format!("Config ID {}: {}", config_id, e));
            }
        }
    }

    for task in port_tasks {
        match task.await {
            Ok(_) => {}
            Err(e) => error!("Port forward task failed: {}", e),
        }
    }

    if !fetch_errors.is_empty() {
        warn!(
            "Failed to retrieve some configs: {}",
            fetch_errors.join(", ")
        );
    }

    Ok(())
}

async fn check_and_manage_port(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let port = config.local_port.unwrap_or(0);

    if let Some((pid, process_name)) = find_process_by_port(port).await {
        handle_existing_process(config, port, pid, process_name).await?;
    } else {
        start_port_forwarding(config).await?;
    }

    Ok(())
}

async fn handle_existing_process(
    config: Config, port: u16, pid: i32, process_name: String,
) -> Result<(), Box<dyn std::error::Error>> {
    debug!(
        "Process '{}' (pid: {}) is using port {}.",
        process_name, pid, port
    );

    if process_name.eq_ignore_ascii_case("kftray") || process_name.eq_ignore_ascii_case("kftui") {
        debug!("Process '{}' is internal, skipping...", process_name);
    } else {
        info!(
            "External process '{}' found on port {}, updating state to 'not running'",
            process_name, port
        );
        let config_state = ConfigState {
            id: None,
            config_id: config.id.unwrap(),
            is_running: false,
        };
        update_config_state(&config_state).await?;
    }

    Ok(())
}

async fn start_port_forwarding(config: Config) -> Result<(), String> {
    let port = config.local_port.unwrap_or(0);
    debug!(
        "No process is occupying port {}. Starting port forwarding for '{}'...",
        port,
        config.alias.as_deref().unwrap_or("unknown")
    );

    let protocol = config.protocol.as_str();

    static HTTP_LOG_STATE: tokio::sync::OnceCell<Arc<HttpLogState>> =
        tokio::sync::OnceCell::const_new();
    let http_log_state = HTTP_LOG_STATE
        .get_or_init(|| async { Arc::new(HttpLogState::new()) })
        .await;

    let configs = vec![config.clone()];
    let config_id = config.id.unwrap();
    let config_alias = config
        .alias
        .clone()
        .unwrap_or_else(|| format!("ID:{}", config_id));

    let forward_future = async {
        match config.workload_type.as_deref() {
            Some("proxy") => deploy_and_forward_pod(configs, http_log_state.clone()).await,
            _ => start_port_forward(configs, protocol, http_log_state.clone()).await,
        }
    };

    let timeout_message = format!(
        "Port forwarding for '{}' timed out after 15 seconds",
        config_alias
    );
    let forward_result = run_with_timeout(forward_future, 15, &timeout_message).await;

    match forward_result {
        Ok(responses) => {
            debug!(
                "Port forwarding response for '{}': {:?}",
                config_alias, responses
            );
            let config_state = ConfigState {
                id: None,
                config_id,
                is_running: true,
            };
            update_config_state(&config_state)
                .await
                .map_err(|e| format!("Failed to update config state: {}", e))?;
        }
        Err(e) => {
            error!(
                "Failed to start port forwarding for '{}': {}",
                config_alias, e
            );
            let config_state = ConfigState {
                id: None,
                config_id,
                is_running: false,
            };
            update_config_state(&config_state)
                .await
                .map_err(|e| format!("Failed to update config state: {}", e))?;
            return Err(e);
        }
    }

    Ok(())
}

async fn find_process_by_port(port: u16) -> Option<(i32, String)> {
    if port == 0 {
        return None;
    }

    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let proto_flags = ProtocolFlags::TCP | ProtocolFlags::UDP;

    let sockets_info = match get_sockets_info(af_flags, proto_flags) {
        Ok(info) => info,
        Err(e) => {
            error!("Failed to retrieve socket information: {}", e);
            return None;
        }
    };

    for socket in sockets_info {
        match &socket.protocol_socket_info {
            ProtocolSocketInfo::Tcp(tcp_info) if tcp_info.local_port == port => {
                if let Some(&pid) = socket.associated_pids.first() {
                    let process_name = get_process_name_by_pid(pid as i32);
                    return Some((pid as i32, process_name));
                }
            }
            ProtocolSocketInfo::Udp(udp_info) if udp_info.local_port == port => {
                if let Some(&pid) = socket.associated_pids.first() {
                    let process_name = get_process_name_by_pid(pid as i32);
                    return Some((pid as i32, process_name));
                }
            }
            _ => continue,
        }
    }

    None
}

fn get_process_name_by_pid(pid: i32) -> String {
    let mut system = System::new_all();
    system.refresh_all();

    if let Some(process) = system.process(Pid::from(pid as usize)) {
        process.name().to_string_lossy().into_owned()
    } else {
        format!("PID {} not found", pid)
    }
}
