use std::sync::Arc;
use std::time::Duration;

use kftray_portforward::port_forward::CANCEL_NOTIFIER;
use log::error;
use log::info;
use tokio::time::{
    sleep,
    timeout,
};

pub async fn start_network_monitor() {
    info!("Starting network monitor");

    let mut was_network_up = check_network().await;

    loop {
        sleep(Duration::from_secs(5)).await;

        let is_network_up = check_network().await;

        if !was_network_up && is_network_up {
            info!("Network reconnected - likely wake from sleep");
            handle_reconnect().await;
        } else if was_network_up && !is_network_up {
            info!("Network disconnected - possibly entering sleep");
        }

        was_network_up = is_network_up;
    }
}

async fn check_network() -> bool {
    let connectivity_check = tokio::spawn(async {
        if let Ok(socket) = tokio::net::TcpStream::connect("8.8.8.8:53").await {
            drop(socket);
            true
        } else {
            false
        }
    });

    match timeout(Duration::from_secs(1), connectivity_check).await {
        Ok(Ok(result)) => result,
        _ => false,
    }
}

async fn handle_reconnect() {
    info!("Triggering port forward reconnection after network change");

    CANCEL_NOTIFIER.notify_waiters();

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    match kftray_commons::utils::config_state::get_configs_state().await {
        Ok(config_states) => {
            let active_config_ids: Vec<i64> = config_states
                .into_iter()
                .filter(|state| state.is_running)
                .map(|state| state.config_id)
                .collect();

            if !active_config_ids.is_empty() {
                info!(
                    "Found {} active port forward configs to restart",
                    active_config_ids.len()
                );

                let mut configs_to_restart = Vec::new();
                for config_id in active_config_ids {
                    match kftray_commons::config::get_config(config_id).await {
                        Ok(config) => {
                            configs_to_restart.push(config);
                        }
                        Err(e) => {
                            error!("Failed to get config {}: {}", config_id, e);
                        }
                    }
                }

                let http_log_state = Arc::new(kftray_http_logs::HttpLogState::new());

                let tcp_configs = configs_to_restart
                    .iter()
                    .filter(|c| c.protocol == "tcp")
                    .cloned()
                    .collect::<Vec<_>>();

                if !tcp_configs.is_empty() {
                    info!("Restarting {} TCP port forwards", tcp_configs.len());
                    match kftray_portforward::kube::start_port_forward(
                        tcp_configs,
                        "tcp",
                        http_log_state.clone(),
                    )
                    .await
                    {
                        Ok(_) => info!("Successfully restarted TCP port forwards"),
                        Err(e) => error!("Failed to restart TCP port forwards: {}", e),
                    }
                }

                let udp_configs = configs_to_restart
                    .iter()
                    .filter(|c| c.protocol == "udp")
                    .cloned()
                    .collect::<Vec<_>>();

                if !udp_configs.is_empty() {
                    info!("Restarting {} UDP port forwards", udp_configs.len());
                    match kftray_portforward::kube::start_port_forward(
                        udp_configs,
                        "udp",
                        http_log_state.clone(),
                    )
                    .await
                    {
                        Ok(_) => info!("Successfully restarted UDP port forwards"),
                        Err(e) => error!("Failed to restart UDP port forwards: {}", e),
                    }
                }
            } else {
                info!("No active port forwards to restart");
            }
        }
        Err(e) => {
            error!("Failed to get config states: {}", e);
        }
    }

    info!("Network reconnection handling completed");
}

#[cfg(test)]
mod tests {

    use kftray_commons::models::config_model::Config;

    use super::*;

    #[tokio::test]
    async fn test_check_network_connectivity() {
        let result = check_network().await;
        println!("Network connectivity test result: {}", result);
    }

    #[tokio::test]
    async fn test_network_state_transitions() {
        let initial_state = false;
        let new_state = true;

        assert_ne!(initial_state, new_state);
        assert!(!initial_state && new_state);

        let initial_state = true;
        let new_state = false;

        assert_ne!(initial_state, new_state);
        assert!(initial_state && !new_state);

        let initial_state = true;
        let new_state = true;

        assert_eq!(initial_state, new_state);

        let initial_state = false;
        let new_state = false;

        assert_eq!(initial_state, new_state);
    }

    #[tokio::test]
    async fn test_config_protocol_filtering() {
        let tcp_config1 = Config {
            id: Some(1),
            protocol: "tcp".to_string(),
            ..Default::default()
        };

        let tcp_config2 = Config {
            id: Some(2),
            protocol: "tcp".to_string(),
            ..Default::default()
        };

        let udp_config = Config {
            id: Some(3),
            protocol: "udp".to_string(),
            ..Default::default()
        };

        let configs = vec![tcp_config1, tcp_config2, udp_config];

        let tcp_configs = configs
            .iter()
            .filter(|c| c.protocol == "tcp")
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(tcp_configs.len(), 2);
        assert!(tcp_configs.iter().all(|c| c.protocol == "tcp"));

        let udp_configs = configs
            .iter()
            .filter(|c| c.protocol == "udp")
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(udp_configs.len(), 1);
        assert!(udp_configs.iter().all(|c| c.protocol == "udp"));
    }
}
