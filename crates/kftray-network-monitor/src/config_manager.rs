use kftray_commons::models::config_model::Config;
use kftray_commons::models::response::CustomResponse;
use kftray_portforward::kube::stop_generation;
use log::{
    error,
    info,
};

pub struct ConfigManager;

/// Whether this process currently runs a forward for `id`. The database
/// row can still say running while a stop is in flight or has just finished;
/// only the registry says what the monitor may restart.
fn is_forward_registered(id: i64) -> bool {
    kftray_portforward::port_forward::CHILD_PROCESSES.contains_key(&id)
}

impl ConfigManager {
    pub async fn get_active_configs()
    -> Result<Vec<Config>, Box<dyn std::error::Error + Send + Sync>> {
        let config_states = kftray_commons::utils::config_state::get_configs_state().await?;
        let current_process_id = std::process::id();

        let active_config_ids: Vec<i64> = config_states
            .into_iter()
            .filter(|state| {
                state.is_running
                    && state.process_id.is_none_or(|pid| pid == current_process_id)
                    && is_forward_registered(state.config_id)
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
        // Only what this process is still running is restarted: a forward the
        // user stopped between the health check that flagged it and now must
        // stay stopped. The stop generation catches a stop landing after
        // this check: a count moved by anything other than our own stop
        // means someone else stopped it, and starting it would undo that.
        let owned: Vec<(i64, Config, u64)> = configs
            .into_iter()
            .filter_map(|config| {
                let id = config.id?;
                is_forward_registered(id).then(|| (id, config, stop_generation(id)))
            })
            .collect();
        if owned.is_empty() {
            info!("No {protocol} port forwards left to restart; they were stopped meanwhile");
            return;
        }
        info!("Restarting {} {} port forwards", owned.len(), protocol);

        let recovering: std::collections::HashSet<i64> = owned
            .iter()
            .map(|(id, _, _)| *id)
            .filter(|&id| kftray_portforward::kube::recovery_in_progress(id))
            .collect();

        let stop_tasks: Vec<_> = owned
            .iter()
            .map(|(id, _, _)| {
                let id = *id;
                let stop = tokio::spawn(async move {
                    kftray_portforward::kube::stop_port_forward(id.to_string()).await
                });
                (id, stop)
            })
            .collect();

        for (id, stop_task) in stop_tasks {
            match stop_task.await {
                Ok(Ok(_)) => {}
                // The forward is torn down either way, so the restart still
                // runs. A listener that kept the port makes it fail with the
                // port-in-use message, and this says why.
                Ok(Err(e)) => log::warn!("Stopping config {id} before its restart failed: {e}"),
                Err(e) => log::warn!("Stop task failed: {e}"),
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let configs: Vec<Config> = owned
            .into_iter()
            .filter(|(id, _, before)| stop_generation(*id) == before + 1)
            .map(|(_, config, _)| config)
            .collect();
        if configs.is_empty() {
            info!("No {protocol} port forwards left to restart; they were stopped meanwhile");
            return;
        }

        let (relay_configs, direct_configs) = partition_relay_configs(configs);

        if !direct_configs.is_empty() {
            match kftray_portforward::kube::start_port_forward(direct_configs, protocol).await {
                Ok(responses) => report_restart_outcome(&responses, protocol, "port forwards"),
                Err(e) => report_restart_failure(&e, protocol, "port forwards"),
            }
        }

        if !relay_configs.is_empty() {
            let configs_to_restart = relay_configs_to_restart(relay_configs, &recovering);

            if !configs_to_restart.is_empty() {
                match kftray_portforward::kube::deploy_and_forward_pod(configs_to_restart).await {
                    Ok(responses) => {
                        report_restart_outcome(&responses, protocol, "relay port forwards")
                    }
                    Err(e) => report_restart_failure(&e, protocol, "relay port forwards"),
                }
            }
        }
    }
}

/// Drops relay configs whose recovery was in progress before this restart
/// stopped them. `recovering` must be snapshotted before the stop pass runs,
/// since `stop_config` unconditionally clears `RECOVERY_MANAGERS` and a
/// live check here would always see it already gone.
fn relay_configs_to_restart(
    relay_configs: Vec<Config>, recovering: &std::collections::HashSet<i64>,
) -> Vec<Config> {
    relay_configs
        .into_iter()
        .filter(|config| {
            if let Some(config_id) = config.id
                && recovering.contains(&config_id)
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
        .collect()
}

fn report_restart_outcome(responses: &[CustomResponse], protocol: &str, kind: &str) {
    let failures: Vec<&CustomResponse> = responses
        .iter()
        .filter(|response| response.failed())
        .collect();
    let restarted = responses.len() - failures.len();
    if restarted > 0 {
        info!("Restarted {restarted} {protocol} {kind}");
    }
    for response in failures {
        error!(
            "Failed to restart {protocol} {kind}: config {:?} ({}/{}): {}",
            response.id, response.namespace, response.service, response.stderr
        );
    }
}

fn report_restart_failure(error: &str, protocol: &str, kind: &str) {
    error!("Failed to restart {protocol} {kind}: {error}");
}

/// Splits a restart batch into configs that need a fresh relay pod and
/// configs that forward directly. The stop pass deletes the relay of every
/// UDP and proxy forward, so only `deploy_and_forward_pod` can bring those
/// back. Matches `is_direct_tcp_forward` in `kftray-tauri`.
fn partition_relay_configs(configs: Vec<Config>) -> (Vec<Config>, Vec<Config>) {
    configs.into_iter().partition(|c| {
        let workload_type = c.workload_type.as_deref();
        let direct = workload_type == Some("expose")
            || (matches!(workload_type, None | Some("service" | "pod")) && c.protocol == "tcp");
        !direct
    })
}

#[cfg(test)]
mod tests {
    use kftray_commons::models::config_model::Config;

    use super::{
        partition_relay_configs,
        relay_configs_to_restart,
    };

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

        let (relay, direct) = partition_relay_configs(configs);

        assert_eq!(
            relay.len(),
            2,
            "all proxy configs must restart through the relay"
        );
        assert!(direct.is_empty());
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

        let (relay, direct) = partition_relay_configs(configs);

        let relay_ids: Vec<i64> = relay.iter().filter_map(|c| c.id).collect();
        assert_eq!(relay_ids, vec![2, 4, 5]);

        let direct_ids: Vec<i64> = direct.iter().filter_map(|c| c.id).collect();
        assert_eq!(direct_ids, vec![1, 3]);
    }

    #[test]
    fn empty_configs_should_produce_empty_partitions() {
        let (relay, direct) = partition_relay_configs(Vec::new());
        assert!(relay.is_empty());
        assert!(direct.is_empty());
    }

    #[test]
    fn config_with_no_workload_type_should_route_to_start_port_forward() {
        let mut config = make_config(1, "service", "tcp");
        config.workload_type = None;

        let (relay, direct) = partition_relay_configs(vec![config]);

        assert!(relay.is_empty());
        assert_eq!(direct.len(), 1);
    }

    #[test]
    fn a_config_recovering_before_the_stop_pass_is_skipped() {
        // `recovering` must be captured before `stop_port_forward` runs, or
        // every config would already be missing from `RECOVERY_MANAGERS` by
        // the time this filter looks it up, and none would ever be skipped.
        let recovering: std::collections::HashSet<i64> = [2].into_iter().collect();
        let configs = vec![
            make_config(1, "proxy", "tcp"),
            make_config(2, "proxy", "tcp"),
        ];

        let restart = relay_configs_to_restart(configs, &recovering);

        let ids: Vec<i64> = restart.iter().filter_map(|c| c.id).collect();
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn udp_service_and_pod_configs_restart_through_the_relay() {
        let configs = vec![
            make_config(1, "service", "udp"),
            make_config(2, "pod", "udp"),
            make_config(3, "service", "tcp"),
            make_config(4, "expose", "tcp"),
        ];
        let mut no_workload_udp = make_config(5, "service", "udp");
        no_workload_udp.workload_type = None;

        let (relay, direct) =
            partition_relay_configs(configs.into_iter().chain([no_workload_udp]).collect());

        let relay_ids: Vec<i64> = relay.iter().filter_map(|c| c.id).collect();
        let direct_ids: Vec<i64> = direct.iter().filter_map(|c| c.id).collect();
        assert_eq!(relay_ids, vec![1, 2, 5]);
        assert_eq!(direct_ids, vec![3, 4]);
    }

    async fn serve_forbidding_kube_api() -> (
        std::net::SocketAddr,
        std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
    ) {
        use tokio::io::{
            AsyncReadExt,
            AsyncWriteExt,
        };

        let requests = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = std::sync::Arc::clone(&requests);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let seen = std::sync::Arc::clone(&seen);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let line = request.lines().next().unwrap_or_default().to_string();
                    let (status, body) = if line.contains("/version") {
                        ("200 OK", r#"{"major":"1","minor":"30"}"#)
                    } else {
                        (
                            "403 Forbidden",
                            r#"{"kind":"Status","apiVersion":"v1","metadata":{},"status":"Failure","reason":"Forbidden","code":403,"message":"forbidden"}"#,
                        )
                    };
                    seen.lock().await.push(line);
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (addr, requests)
    }

    #[tokio::test]
    async fn restarting_a_udp_service_forward_deploys_a_new_relay() {
        use kftray_commons::models::config_state_model::ConfigState;
        use kftray_commons::test_utils::{
            EnvVarGuard,
            test_db,
        };
        use kftray_portforward::port_forward::{
            CHILD_PROCESSES,
            PROCESS_TEST_MUTEX,
            PortForwardProcess,
        };

        kftray_portforward::ssl::ensure_crypto_provider_installed();
        let _db = test_db().await;
        let _process = PROCESS_TEST_MUTEX.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());
        kftray_commons::utils::db::init().await.unwrap();
        kftray_commons::utils::migration::migrate_configs(None)
            .await
            .unwrap();

        let (api, requests) = serve_forbidding_kube_api().await;
        let kubeconfig = dir.path().join("kubeconfig");
        std::fs::write(
            &kubeconfig,
            format!(
                "apiVersion: v1\nkind: Config\ncurrent-context: dev\nclusters:\n- name: dev\n  cluster:\n    server: http://{api}\ncontexts:\n- name: dev\n  context:\n    cluster: dev\n    user: dev\nusers:\n- name: dev\n  user:\n    token: test\n"
            ),
        )
        .unwrap();

        let free_port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut config = Config {
            context: Some("dev".to_string()),
            kubeconfig: Some(kubeconfig.to_str().unwrap().to_string()),
            local_address: Some("127.0.0.1".to_string()),
            local_port: Some(free_port),
            remote_port: Some(53),
            ..make_config(0, "service", "udp")
        };
        config.id = None;
        let id = kftray_commons::config::insert_config(config.clone())
            .await
            .unwrap();
        config.id = Some(id);
        kftray_commons::utils::config_state::update_config_state(&ConfigState::new(id, true))
            .await
            .unwrap();
        let handle = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        CHILD_PROCESSES.insert(id, PortForwardProcess::new(handle, id.to_string()));

        let restart = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            super::ConfigManager::restart_port_forwards(vec![config]),
        )
        .await;
        CHILD_PROCESSES.remove(&id);

        let seen = requests.lock().await.clone();
        assert!(
            seen.iter()
                .any(|line| line.starts_with("POST /apis/apps/v1/namespaces/test-ns/deployments")),
            "no relay deployment requested, finished: {}, requests: {seen:?}",
            restart.is_ok()
        );
    }
}
