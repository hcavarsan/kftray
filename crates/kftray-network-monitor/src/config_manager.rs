use kftray_commons::models::config_model::Config;
use log::{
    error,
    info,
};

pub struct ConfigManager;

impl ConfigManager {
    pub async fn get_active_configs()
    -> Result<Vec<Config>, Box<dyn std::error::Error + Send + Sync>> {
        let config_states = kftray_commons::utils::config_state::get_configs_state().await?;
        let current_process_id = std::process::id();

        let active_config_ids: Vec<i64> = config_states
            .into_iter()
            .filter(|state| {
                state.is_running && state.process_id.is_none_or(|pid| pid == current_process_id)
            })
            .map(|state| state.config_id)
            .collect();

        if active_config_ids.is_empty() {
            return Ok(Vec::new());
        }

        let config_futures: Vec<_> = active_config_ids
            .into_iter()
            .map(|config_id| {
                tokio::spawn(
                    async move { kftray_commons::config::get_config(config_id).await.ok() },
                )
            })
            .collect();

        let mut configs = Vec::new();
        for config_future in config_futures {
            match config_future.await {
                Ok(Some(config)) => configs.push(config),
                Ok(None) => log::warn!("Config not found for an active config ID"),
                Err(e) => log::warn!("Failed to fetch config: {e}"),
            }
        }

        Ok(configs)
    }

    pub async fn restart_port_forwards(configs: Vec<Config>) {
        for protocol in ["tcp", "udp"] {
            let protocol_configs: Vec<Config> = configs
                .iter()
                .filter(|c| c.protocol == protocol)
                .cloned()
                .collect();

            if !protocol_configs.is_empty() {
                Self::restart_protocol_batch(protocol_configs, protocol).await;
            }
        }
    }

    async fn restart_protocol_batch(configs: Vec<Config>, protocol: &str) {
        info!("Restarting {} {} port forwards", configs.len(), protocol);

        let stop_tasks: Vec<_> = configs
            .iter()
            .filter_map(|config| {
                config.id.map(|config_id| {
                    tokio::spawn(async move {
                        kftray_portforward::kube::stop_port_forward(config_id.to_string()).await
                    })
                })
            })
            .collect();

        for stop_task in stop_tasks {
            if let Err(e) = stop_task.await {
                log::warn!("Stop task failed: {e}");
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        match kftray_portforward::kube::start_port_forward(configs, protocol).await {
            Ok(_) => info!("Successfully restarted {protocol} port forwards"),
            Err(e) => {
                if protocol == "udp" && e.contains("No ready pods available") {
                    log::warn!("UDP port forward restart skipped - no ready pods available: {e}");
                } else {
                    error!("Failed to restart {protocol} port forwards: {e}");
                }
            }
        }
    }
}
