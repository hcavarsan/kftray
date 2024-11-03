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

use crate::config::Config;
use crate::error::Result;
use crate::utils::state::StateManager;

pub async fn validate_config(config: &Config) -> Result<()> {
    config.validate()
}

pub async fn check_and_manage_port(config: &Config, state_manager: &StateManager) -> Result<()> {
    let port = config.local_port.unwrap_or(0);

    if let Some((pid, process_name)) = find_process_by_port(port).await {
        handle_existing_process(config, port, pid, process_name, state_manager).await?;
    }

    Ok(())
}

pub async fn find_process_by_port(port: u16) -> Option<(i32, String)> {
    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let proto_flags = ProtocolFlags::TCP | ProtocolFlags::UDP;

    let sockets_info = match get_sockets_info(af_flags, proto_flags) {
        Ok(info) => info,
        Err(e) => {
            log::error!("Failed to retrieve socket information: {}", e);
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

async fn handle_existing_process(
    config: &Config, port: u16, pid: i32, process_name: String, state_manager: &StateManager,
) -> Result<()> {
    log::info!(
        "Process '{}' (pid: {}) is using port {}",
        process_name,
        pid,
        port
    );

    if process_name.eq_ignore_ascii_case("port-forward") {
        log::info!("Process '{}' is internal, skipping...", process_name);
    } else {
        log::info!(
            "External process '{}' found, updating state to 'not running'...",
            process_name
        );
        state_manager
            .update_state(config.id.unwrap(), false)
            .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[tokio::test]
    async fn test_config_validation() {
        // Test valid config
        let valid_config = Config::builder()
            .namespace("test")
            .protocol("TCP")
            .local_port(8080)
            .build()
            .unwrap();
        assert!(validate_config(&valid_config).await.is_ok());

        // Test invalid config (empty namespace)
        let build_result = Config::builder()
            .namespace("")
            .protocol("TCP")
            .local_port(8080)
            .build();

        assert!(build_result.is_err());
        assert_eq!(
            build_result.unwrap_err().to_string(),
            Error::empty_namespace().to_string()
        );

        // Test invalid port
        let build_result = Config::builder()
            .namespace("test")
            .protocol("TCP")
            .local_port(0)
            .build();

        assert!(build_result.is_err());
        assert_eq!(
            build_result.unwrap_err().to_string(),
            Error::invalid_local_port().to_string()
        );

        // Test empty protocol
        let build_result = Config::builder()
            .namespace("test")
            .protocol("")
            .local_port(8080)
            .build();

        assert!(build_result.is_err());
        assert_eq!(
            build_result.unwrap_err().to_string(),
            Error::empty_protocol().to_string()
        );
    }
}
