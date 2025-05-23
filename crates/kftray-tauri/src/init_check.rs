use std::sync::Arc;

use async_trait::async_trait;
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

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait PortOperations: Send + Sync {
    async fn get_configs_state(&self) -> Result<Vec<ConfigState>, String>;
    async fn get_config(&self, id: i64) -> Result<Config, String>;
    async fn update_config_state(&self, state: &ConfigState) -> Result<(), String>;
    async fn find_process_by_port(&self, port: u16) -> Option<(i32, String)>;
    async fn start_port_forward(
        &self, configs: Vec<Config>, protocol: &str, http_log_state: Arc<HttpLogState>,
    ) -> Result<Vec<String>, String>;
    async fn deploy_and_forward_pod(
        &self, configs: Vec<Config>, http_log_state: Arc<HttpLogState>,
    ) -> Result<Vec<String>, String>;
}

pub struct RealPortOperations;

#[async_trait]
impl PortOperations for RealPortOperations {
    async fn get_configs_state(&self) -> Result<Vec<ConfigState>, String> {
        get_configs_state().await
    }

    async fn get_config(&self, id: i64) -> Result<Config, String> {
        get_config(id).await
    }

    async fn update_config_state(&self, state: &ConfigState) -> Result<(), String> {
        update_config_state(state).await
    }

    async fn find_process_by_port(&self, port: u16) -> Option<(i32, String)> {
        find_process_by_port_internal(port).await
    }

    async fn start_port_forward(
        &self, configs: Vec<Config>, protocol: &str, http_log_state: Arc<HttpLogState>,
    ) -> Result<Vec<String>, String> {
        start_port_forward(configs, protocol, http_log_state)
            .await
            .map(|responses| responses.into_iter().map(|r| format!("{r:?}")).collect())
    }

    async fn deploy_and_forward_pod(
        &self, configs: Vec<Config>, http_log_state: Arc<HttpLogState>,
    ) -> Result<Vec<String>, String> {
        deploy_and_forward_pod(configs, http_log_state)
            .await
            .map(|responses| responses.into_iter().map(|r| format!("{r:?}")).collect())
    }
}

async fn fetch_configs_in_parallel(
    port_ops: Arc<dyn PortOperations>, running_configs: Vec<ConfigState>,
) -> Vec<(i64, Result<Config, String>)> {
    let mut config_tasks = Vec::with_capacity(running_configs.len());

    for config_state in running_configs {
        let config_id = config_state.config_id;
        let port_ops = Arc::clone(&port_ops);
        let task = tokio::spawn(async move {
            let result = port_ops
                .get_config(config_id)
                .await
                .map_err(|e| format!("Failed to retrieve config {config_id}: {e}"));
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
                error!("Task for fetching config failed: {e}");
            }
        }
    }

    results
}

pub async fn check_and_manage_ports(port_ops: Arc<dyn PortOperations>) -> Result<(), String> {
    let running_configs = match port_ops.get_configs_state().await {
        Ok(states) => states
            .into_iter()
            .filter(|state| state.is_running)
            .collect::<Vec<_>>(),
        Err(e) => {
            error!("Failed to retrieve config states: {e:?}");
            return Err(e);
        }
    };

    if running_configs.is_empty() {
        debug!("No running port forwards found to restore");
        return Ok(());
    }

    info!("Restoring {} running port forwards", running_configs.len());

    let config_results = fetch_configs_in_parallel(Arc::clone(&port_ops), running_configs).await;

    let mut port_tasks = Vec::new();
    let mut fetch_errors = Vec::new();

    for (config_id, result) in config_results {
        match result {
            Ok(config) => {
                let port_ops = Arc::clone(&port_ops);
                let task = tokio::spawn(async move {
                    if let Err(err) = check_and_manage_port(port_ops, config).await {
                        error!("Error checking state for config {config_id}: {err}");
                    }
                });
                port_tasks.push(task);
            }
            Err(e) => {
                fetch_errors.push(format!("Config ID {config_id}: {e}"));
            }
        }
    }

    for task in port_tasks {
        match task.await {
            Ok(_) => {}
            Err(e) => error!("Port forward task failed: {e}"),
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

async fn check_and_manage_port(
    port_ops: Arc<dyn PortOperations>, config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let port = config.local_port.unwrap_or(0);

    if let Some((pid, process_name)) = port_ops.find_process_by_port(port).await {
        handle_existing_process(Arc::clone(&port_ops), config, port, pid, process_name).await?;
    } else {
        start_port_forwarding(port_ops, config).await?;
    }

    Ok(())
}

async fn handle_existing_process(
    port_ops: Arc<dyn PortOperations>, config: Config, port: u16, pid: i32, process_name: String,
) -> Result<(), Box<dyn std::error::Error>> {
    debug!("Process '{process_name}' (pid: {pid}) is using port {port}.");

    if process_name.eq_ignore_ascii_case("kftray") || process_name.eq_ignore_ascii_case("kftui") {
        debug!("Process '{process_name}' is internal, skipping...");
    } else {
        info!(
            "External process '{process_name}' found on port {port}, updating state to 'not running'"
        );
        let config_state = ConfigState {
            id: None,
            config_id: config.id.unwrap(),
            is_running: false,
        };
        port_ops.update_config_state(&config_state).await?;
    }

    Ok(())
}

async fn start_port_forwarding(
    port_ops: Arc<dyn PortOperations>, config: Config,
) -> Result<(), String> {
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
        .unwrap_or_else(|| format!("ID:{config_id}"));

    let result = match config.workload_type.as_deref() {
        Some("proxy") => {
            port_ops
                .deploy_and_forward_pod(configs, http_log_state.clone())
                .await
        }
        _ => {
            port_ops
                .start_port_forward(configs, protocol, http_log_state.clone())
                .await
        }
    };

    match result {
        Ok(_) => {
            let config_state = ConfigState {
                id: None,
                config_id,
                is_running: true,
            };
            port_ops.update_config_state(&config_state).await?;
            Ok(())
        }
        Err(e) => {
            let error_msg = format!("Failed to start port forwarding for '{config_alias}': {e}");
            error!("{error_msg}");

            let config_state = ConfigState {
                id: None,
                config_id,
                is_running: false,
            };
            port_ops.update_config_state(&config_state).await?;

            Err(error_msg)
        }
    }
}

async fn find_process_by_port_internal(port: u16) -> Option<(i32, String)> {
    if port == 0 {
        return None;
    }

    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let proto_flags = ProtocolFlags::TCP | ProtocolFlags::UDP;

    if let Ok(sockets_info) = get_sockets_info(af_flags, proto_flags) {
        for socket in sockets_info {
            let local_port = match &socket.protocol_socket_info {
                ProtocolSocketInfo::Tcp(tcp_info) => tcp_info.local_port,
                ProtocolSocketInfo::Udp(udp_info) => udp_info.local_port,
            };

            if local_port == port {
                if let Some(&pid) = socket.associated_pids.first() {
                    let process_name = get_process_name_by_pid(pid as i32);
                    return Some((pid as i32, process_name));
                }
            }
        }
    }

    None // Return None if no process found on the port
}

fn get_process_name_by_pid(pid: i32) -> String {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, false);

    system
        .process(Pid::from(pid as usize))
        .map(|process| process.name().to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use mockall::predicate::*;

    use super::*;

    fn create_test_config(
        id: i64, port: u16, protocol: &str, workload_type: Option<&str>,
    ) -> Config {
        Config {
            id: Some(id),
            alias: Some(format!("test-config-{id}")),
            local_port: Some(port),
            protocol: protocol.to_string(),
            workload_type: workload_type.map(String::from),
            ..Default::default()
        }
    }

    fn create_config_state(id: i64, is_running: bool) -> ConfigState {
        ConfigState {
            id: None,
            config_id: id,
            is_running,
        }
    }

    #[tokio::test]
    async fn test_check_and_manage_ports_no_running_configs() {
        let mut mock = MockPortOperations::new();
        mock.expect_get_configs_state()
            .times(1)
            .returning(|| Ok(vec![]));

        let result = check_and_manage_ports(Arc::new(mock)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_manage_ports_with_configs() {
        let mut mock = MockPortOperations::new();
        let config_states = vec![create_config_state(1, true)];
        let config = create_test_config(1, 8080, "tcp", None);

        mock.expect_get_configs_state()
            .times(1)
            .returning(move || Ok(config_states.clone()));

        mock.expect_get_config()
            .with(eq(1))
            .times(1)
            .returning(move |_| Ok(config.clone()));

        mock.expect_find_process_by_port()
            .with(eq(8080))
            .times(1)
            .returning(|_| None);

        mock.expect_start_port_forward()
            .times(1)
            .returning(|_, _, _| Ok(vec!["Port forwarding started".to_string()]));

        mock.expect_update_config_state()
            .times(1)
            .returning(|_| Ok(()));

        let result = check_and_manage_ports(Arc::new(mock)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_manage_ports_get_states_error() {
        let mut mock = MockPortOperations::new();
        mock.expect_get_configs_state()
            .times(1)
            .returning(|| Err("Failed to get config states".to_string()));

        let result = check_and_manage_ports(Arc::new(mock)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Failed to get config states");
    }

    #[tokio::test]
    async fn test_check_and_manage_ports_get_config_error() {
        let mut mock = MockPortOperations::new();
        let config_states = vec![create_config_state(1, true)];

        mock.expect_get_configs_state()
            .times(1)
            .returning(move || Ok(config_states.clone()));

        mock.expect_get_config()
            .with(eq(1))
            .times(1)
            .returning(|id| Err(format!("Failed to get config {id}")));

        let result = check_and_manage_ports(Arc::new(mock)).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_manage_port_existing_process() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", None);

        mock.expect_find_process_by_port()
            .with(eq(8080))
            .times(1)
            .returning(|_| Some((1234, "nginx".to_string())));

        mock.expect_update_config_state()
            .with(function(|state: &ConfigState| {
                state.config_id == 1 && !state.is_running
            }))
            .times(1)
            .returning(|_| Ok(()));

        let result = check_and_manage_port(Arc::new(mock), config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_manage_port_internal_process() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", None);

        mock.expect_find_process_by_port()
            .with(eq(8080))
            .times(1)
            .returning(|_| Some((1234, "kftray".to_string())));

        let result = check_and_manage_port(Arc::new(mock), config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_manage_port_kftui_process() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", None);

        mock.expect_find_process_by_port()
            .with(eq(8080))
            .times(1)
            .returning(|_| Some((1234, "kftui".to_string())));

        let result = check_and_manage_port(Arc::new(mock), config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_manage_port_update_state_error() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", None);

        mock.expect_find_process_by_port()
            .with(eq(8080))
            .times(1)
            .returning(|_| Some((1234, "nginx".to_string())));

        mock.expect_update_config_state()
            .times(1)
            .returning(|_| Err("Failed to update state".to_string()));

        let result = check_and_manage_port(Arc::new(mock), config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_check_and_manage_port_zero_port() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 0, "tcp", None);

        mock.expect_find_process_by_port()
            .with(eq(0))
            .times(1)
            .returning(|_| None);

        mock.expect_start_port_forward()
            .times(1)
            .returning(|_, _, _| Ok(vec!["Port forwarding started".to_string()]));

        mock.expect_update_config_state()
            .times(1)
            .returning(|_| Ok(()));

        let result = check_and_manage_port(Arc::new(mock), config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_port_forwarding_success() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", None);

        mock.expect_start_port_forward()
            .times(1)
            .returning(|_, _, _| Ok(vec!["Port forwarding started".to_string()]));

        mock.expect_update_config_state()
            .with(function(|state: &ConfigState| {
                state.config_id == 1 && state.is_running
            }))
            .times(1)
            .returning(|_| Ok(()));

        let result = start_port_forwarding(Arc::new(mock), config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_port_forwarding_proxy_workload() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("proxy"));

        mock.expect_deploy_and_forward_pod()
            .times(1)
            .returning(|_, _| Ok(vec!["Proxy pod deployed and forwarded".to_string()]));

        mock.expect_update_config_state()
            .with(function(|state: &ConfigState| {
                state.config_id == 1 && state.is_running
            }))
            .times(1)
            .returning(|_| Ok(()));

        let result = start_port_forwarding(Arc::new(mock), config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_port_forwarding_failure() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", None);

        mock.expect_start_port_forward()
            .times(1)
            .returning(|_, _, _| Err("Port forwarding failed".to_string()));

        mock.expect_update_config_state()
            .with(function(|state: &ConfigState| {
                state.config_id == 1 && !state.is_running
            }))
            .times(1)
            .returning(|_| Ok(()));

        let result = start_port_forwarding(Arc::new(mock), config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_start_port_forwarding_update_state_error() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", None);

        mock.expect_start_port_forward()
            .times(1)
            .returning(|_, _, _| Ok(vec!["Port forwarding started".to_string()]));

        mock.expect_update_config_state()
            .times(1)
            .returning(|_| Err("Failed to update state".to_string()));

        let result = start_port_forwarding(Arc::new(mock), config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fetch_configs_in_parallel() {
        let mut mock = MockPortOperations::new();
        let config_states = vec![create_config_state(1, true), create_config_state(2, true)];

        let config1 = create_test_config(1, 8080, "tcp", None);
        let config2 = create_test_config(2, 9090, "tcp", None);

        mock.expect_get_config()
            .with(eq(1))
            .times(1)
            .returning(move |_| Ok(config1.clone()));

        mock.expect_get_config()
            .with(eq(2))
            .times(1)
            .returning(move |_| Ok(config2.clone()));

        let results = fetch_configs_in_parallel(Arc::new(mock), config_states).await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, result)| result.is_ok()));

        let config_ids: Vec<i64> = results.iter().map(|(id, _)| *id).collect();
        assert!(config_ids.contains(&1));
        assert!(config_ids.contains(&2));
    }

    #[tokio::test]
    async fn test_find_process_by_port_internal() {
        let result = find_process_by_port_internal(0).await;
        assert!(result.is_none());
    }
}
