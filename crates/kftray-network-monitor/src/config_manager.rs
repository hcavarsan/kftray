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

        let (proxy_configs, other_configs) = partition_configs_by_workload(configs);

        if !other_configs.is_empty() {
            match kftray_portforward::kube::start_port_forward(other_configs, protocol).await {
                Ok(_) => info!("Successfully restarted {protocol} port forwards"),
                Err(e) => {
                    if protocol == "udp" && e.contains("No ready pods available") {
                        log::warn!(
                            "UDP port forward restart skipped - no ready pods available: {e}"
                        );
                    } else {
                        error!("Failed to restart {protocol} port forwards: {e}");
                    }
                }
            }
        }

        if !proxy_configs.is_empty() {
            // Filter out proxy configs that already have recovery in progress
            let configs_to_restart: Vec<Config> = proxy_configs
                .into_iter()
                .filter(|config| {
                    if let Some(config_id) = config.id
                        && kftray_portforward::kube::proxy_recovery::RECOVERY_LOCKS
                            .contains_key(&config_id)
                    {
                        info!(
                            "Skipping network monitor restart for config {} \
                             \u{2014} recovery already in progress",
                            config_id
                        );
                        return false;
                    }
                    true
                })
                .collect();

            if !configs_to_restart.is_empty() {
                match kftray_portforward::kube::deploy_and_forward_pod(configs_to_restart).await {
                    Ok(_) => info!("Successfully restarted {protocol} proxy port forwards"),
                    Err(e) => {
                        error!("Failed to restart {protocol} proxy port forwards: {e}");
                    }
                }
            }
        }
    }
}

fn partition_configs_by_workload(configs: Vec<Config>) -> (Vec<Config>, Vec<Config>) {
    configs
        .into_iter()
        .partition(|c| c.workload_type.as_deref() == Some("proxy"))
}

#[cfg(test)]
mod tests {
    use kftray_commons::models::config_model::Config;

    use super::partition_configs_by_workload;

    fn make_config(id: i64, workload_type: &str, protocol: &str) -> Config {
        Config {
            id: Some(id),
            workload_type: Some(workload_type.to_string()),
            protocol: protocol.to_string(),
            namespace: "test-ns".to_string(),
            service: Some(format!("svc-{id}")),
            ..Config::default()
        }
    }

    #[test]
    fn proxy_configs_should_route_to_deploy_and_forward_pod() {
        let configs = vec![
            make_config(1, "proxy", "tcp"),
            make_config(2, "proxy", "udp"),
        ];

        let (proxy, other) = partition_configs_by_workload(configs);

        assert_eq!(
            proxy.len(),
            2,
            "all proxy configs must be in the proxy partition"
        );
        assert!(
            other.is_empty(),
            "no configs should be in the other partition"
        );
        assert!(
            proxy
                .iter()
                .all(|c| c.workload_type.as_deref() == Some("proxy")),
            "all partitioned configs must have workload_type=proxy"
        );
    }

    #[test]
    fn service_configs_should_route_to_start_port_forward() {
        let configs = vec![
            make_config(1, "service", "tcp"),
            make_config(2, "pod", "tcp"),
            make_config(3, "service", "udp"),
        ];

        let (proxy, other) = partition_configs_by_workload(configs);

        assert!(
            proxy.is_empty(),
            "no configs should be in the proxy partition"
        );
        assert_eq!(
            other.len(),
            3,
            "all service/pod configs must be in the other partition"
        );
    }

    #[test]
    fn mixed_configs_should_partition_correctly() {
        let configs = vec![
            make_config(1, "service", "tcp"),
            make_config(2, "proxy", "tcp"),
            make_config(3, "pod", "tcp"),
            make_config(4, "proxy", "udp"),
            make_config(5, "service", "udp"),
        ];

        let (proxy, other) = partition_configs_by_workload(configs);

        assert_eq!(proxy.len(), 2);
        assert_eq!(other.len(), 3);

        let proxy_ids: Vec<i64> = proxy.iter().filter_map(|c| c.id).collect();
        assert_eq!(proxy_ids, vec![2, 4]);

        let other_ids: Vec<i64> = other.iter().filter_map(|c| c.id).collect();
        assert_eq!(other_ids, vec![1, 3, 5]);
    }

    #[test]
    fn empty_configs_should_produce_empty_partitions() {
        let (proxy, other) = partition_configs_by_workload(Vec::new());
        assert!(proxy.is_empty());
        assert!(other.is_empty());
    }

    #[test]
    fn config_with_no_workload_type_should_route_to_start_port_forward() {
        let mut config = make_config(1, "service", "tcp");
        config.workload_type = None;

        let (proxy, other) = partition_configs_by_workload(vec![config]);

        assert!(proxy.is_empty());
        assert_eq!(other.len(), 1);
    }

    #[test]
    fn proxy_udp_configs_are_not_skipped_on_no_ready_pods() {
        // This test verifies that proxy UDP configs are routed to deploy_and_forward_pod()
        // and NOT subject to the "No ready pods available" skip logic that applies to
        // non-proxy UDP configs in start_port_forward().
        //
        // The partition logic at line 84 separates proxy from other configs completely.
        // Proxy configs bypass the skip logic at lines 90-93 and go directly to
        // deploy_and_forward_pod() at line 101.

        let proxy_udp_config = make_config(1, "proxy", "udp");
        let service_udp_config = make_config(2, "service", "udp");

        let (proxy, other) = partition_configs_by_workload(vec![proxy_udp_config, service_udp_config]);

        // Verify proxy UDP config is in the proxy partition (will go to deploy_and_forward_pod)
        assert_eq!(
            proxy.len(),
            1,
            "proxy UDP config must be in proxy partition (not subject to skip logic)"
        );
        assert_eq!(
            proxy[0].workload_type.as_deref(),
            Some("proxy"),
            "proxy partition must contain proxy workload_type"
        );

        // Verify service UDP config is in the other partition (subject to skip logic)
        assert_eq!(
            other.len(),
            1,
            "service UDP config must be in other partition (subject to skip logic)"
        );
        assert_eq!(
            other[0].workload_type.as_deref(),
            Some("service"),
            "other partition must contain service workload_type"
        );
    }

}
