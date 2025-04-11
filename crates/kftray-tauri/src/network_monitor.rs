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
    use super::*;

    #[tokio::test]
    async fn test_check_network_connectivity() {
        // This is a basic connectivity test - will pass or fail based on actual network
        let _result = check_network().await;
        // Not asserting result as it depends on actual network connectivity
    }
}
