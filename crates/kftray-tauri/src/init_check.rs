use std::sync::Arc;

use kftray_commons::config::get_config;
use kftray_commons::config_state::{
    get_configs_state,
    update_config_state,
};
use kftray_commons::config_state_model::ConfigState;
use kftray_commons::models::config_model::Config;
use kftray_portforward::deploy_and_forward_pod;
use kftray_portforward::models::kube::HttpLogState;
use kftray_portforward::start_port_forward;
use log::{
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

    for config_state in config_states {
        if config_state.is_running {
            match get_config(config_state.config_id).await {
                Ok(config) => {
                    if let Err(err) = check_and_manage_port(config).await {
                        error!(
                            "Error check state for config {}: {}",
                            config_state.config_id, err
                        );
                    }
                }
                Err(_) => {
                    error!(
                        "Could not retrieve config with ID {}",
                        config_state.config_id
                    );
                }
            }
        }
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
    info!("No process is occupying the port. Starting port forwarding...");

    let protocol = config.protocol.as_str();
    let http_log_state = Arc::new(HttpLogState::new());

    info!(
        "Starting workload type '{:?}' for config: {:?}",
        config.workload_type, config.alias
    );

    let configs = vec![config.clone()];
    let forward_result = match config.workload_type.as_deref() {
        Some("proxy") => deploy_and_forward_pod(configs, http_log_state).await,
        _ => start_port_forward(configs, protocol, http_log_state).await,
    };

    match forward_result {
        Ok(responses) => {
            for response in responses {
                info!("Port forwarding response: {:?}", response);
            }
            let config_state = ConfigState {
                id: None,
                config_id: config.id.unwrap(),
                is_running: true,
            };
            update_config_state(&config_state).await?;
        }
        Err(e) => {
            error!("Failed to start port forwarding: {}", e);
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
