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

pub async fn check_and_manage_ports() -> Result<(), String> {
    let config_states = match get_configs_state().await {
        Ok(states) => states,
        Err(e) => {
            error!("Failed to retrieve config states: {:?}", e);
            return Err(e);
        }
    };

    let running_configs = config_states
        .into_iter()
        .filter(|state| state.is_running)
        .collect::<Vec<_>>();

    let mut config_tasks = Vec::new();
    for config_state in running_configs {
        let task = tokio::spawn(async move {
            match get_config(config_state.config_id).await {
                Ok(config) => Some((config_state.config_id, config)),
                Err(_) => {
                    error!(
                        "Could not retrieve config with ID {}",
                        config_state.config_id
                    );
                    None
                }
            }
        });
        config_tasks.push(task);
    }

    let mut configs = Vec::new();
    for task in config_tasks {
        if let Ok(Some((config_id, config))) = task.await {
            configs.push((config_id, config));
        }
    }

    let mut port_tasks = Vec::new();
    for (config_id, config) in configs {
        let task = tokio::spawn(async move {
            if let Err(err) = check_and_manage_port(config).await {
                error!("Error check state for config {}: {}", config_id, err);
            }
        });
        port_tasks.push(task);
    }

    for task in port_tasks {
        let _ = task.await;
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
    info!(
        "Process '{}' (pid: {}) is using port {}.",
        process_name, pid, port
    );

    if process_name.eq_ignore_ascii_case("kftray") || process_name.eq_ignore_ascii_case("kftui") {
        info!("Process '{}' is internal, skipping...", process_name);
    } else {
        info!(
            "External process '{}' found, updating state to 'not running'...",
            process_name
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
    info!(
        "No process is occupying port {}. Starting port forwarding...",
        config.local_port.unwrap_or(0)
    );

    let protocol = config.protocol.as_str();

    static HTTP_LOG_STATE: tokio::sync::OnceCell<Arc<HttpLogState>> =
        tokio::sync::OnceCell::const_new();
    let http_log_state = HTTP_LOG_STATE
        .get_or_init(|| async { Arc::new(HttpLogState::new()) })
        .await;

    let configs = vec![config.clone()];

    let forward_future = async {
        match config.workload_type.as_deref() {
            Some("proxy") => deploy_and_forward_pod(configs, http_log_state.clone()).await,
            _ => start_port_forward(configs, protocol, http_log_state.clone()).await,
        }
    };

    let forward_result =
        match tokio::time::timeout(tokio::time::Duration::from_secs(15), forward_future).await {
            Ok(result) => result,
            Err(_) => {
                error!(
                    "Port forwarding for {:?} timed out after 15 seconds",
                    config.alias
                );
                let config_state = ConfigState {
                    id: None,
                    config_id: config.id.unwrap(),
                    is_running: false,
                };
                update_config_state(&config_state).await?;
                return Err("Port forwarding timed out".to_string());
            }
        };

    match forward_result {
        Ok(responses) => {
            debug!(
                "Port forwarding response for {:?}: {:?}",
                config.alias, responses
            );
            let config_state = ConfigState {
                id: None,
                config_id: config.id.unwrap(),
                is_running: true,
            };
            update_config_state(&config_state).await?;
        }
        Err(e) => {
            error!(
                "Failed to start port forwarding for {:?}: {}",
                config.alias, e
            );
            let config_state = ConfigState {
                id: None,
                config_id: config.id.unwrap(),
                is_running: false,
            };
            update_config_state(&config_state).await?;
        }
    }

    Ok(())
}

async fn find_process_by_port(port: u16) -> Option<(i32, String)> {
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
