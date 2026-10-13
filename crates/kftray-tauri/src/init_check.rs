use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kftray_commons::config::get_config;
use kftray_commons::config_state::{
    get_configs_state,
    is_owned_by_other_live_process,
    update_config_state,
};
use kftray_commons::config_state_model::ConfigState;
use kftray_commons::models::config_model::Config;
use kftray_telemetry::{
    FailureKind,
    Operation,
    classify,
    measure,
};
use log::{
    debug,
    error,
    info,
    warn,
};
use netstat2::{
    AddressFamilyFlags,
    ProtocolFlags,
    ProtocolSocketInfo,
    get_sockets_info,
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
    /// Whether a forward for the configuration is running or currently
    /// starting (queued/pending) in this process.
    async fn is_forward_registered(&self, id: i64) -> bool;
    /// How many stops have acted on `id` in this process so far; a change
    /// means the user stopped the forward.
    fn stop_generation(&self, id: i64) -> u64;
    /// Stops a registered forward. Only meaningful when
    /// `is_forward_registered` is true for `id`.
    async fn stop_port_forward(&self, id: i64) -> Result<(), String>;
    /// Starts one configuration through the workload/protocol dispatch used
    /// by the SSL certificate restart path, the global shortcuts, and the
    /// auto-start check on launch. The only place that decides direct vs.
    /// relay dispatch, so the auto-start path cannot silently diverge from
    /// it.
    async fn dispatch_start(&self, config: &Config) -> Result<(), String>;
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

    async fn is_forward_registered(&self, id: i64) -> bool {
        kftray_portforward::port_forward::CHILD_PROCESSES.contains_key(&id)
            || kftray_portforward::kube::is_start_pending(id)
    }

    fn stop_generation(&self, id: i64) -> u64 {
        kftray_portforward::kube::stop_generation(id)
    }

    async fn stop_port_forward(&self, id: i64) -> Result<(), String> {
        kftray_portforward::kube::stop_port_forward(id.to_string())
            .await
            .map(|_| ())
    }

    async fn dispatch_start(&self, config: &Config) -> Result<(), String> {
        crate::commands::portforward::dispatch_start(config)
            .await
            .and_then(|responses| kftray_commons::models::response::batch_failure(&responses))
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
        Ok(states) => {
            let (elsewhere, running): (Vec<_>, Vec<_>) = states
                .into_iter()
                .filter(|state| state.is_running)
                .partition(is_owned_by_other_live_process);
            for state in &elsewhere {
                info!(
                    "Config {} is running in another kftray process (pid {:?}); not restoring it",
                    state.config_id, state.process_id
                );
            }
            running
        }
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
        let config_state = ConfigState::new_without_process(config.id.unwrap(), false);
        port_ops.update_config_state(&config_state).await?;
    }

    Ok(())
}

/// How long a restore waits before each further attempt when the cluster is
/// not reachable yet (network, VPN or cluster still coming up at login).
const RESTORE_RETRY_BACKOFF: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(45),
];

/// How a restore that may have waited ended without an error.
enum Restored {
    Started,
    /// While waiting to retry, the user stopped or started the forward, or
    /// another process took the row over; the row is theirs now.
    Superseded,
}

/// Failures that say the cluster is not reachable yet, as opposed to
/// something a later attempt cannot change.
fn is_transient_failure(message: &str) -> bool {
    matches!(
        classify(message),
        FailureKind::Connection | FailureKind::Timeout | FailureKind::PodNotReady
    )
}

/// Starts the config, retrying after each wait in `backoff` while the
/// failure is transient. Returns the last error once the waits run out or
/// the failure is not transient. Run inside `measure`, so one restore is one
/// AutoStart report, for its final outcome.
async fn dispatch_until_reachable(
    port_ops: &Arc<dyn PortOperations>, config: &Config, config_id: i64, backoff: &[Duration],
) -> Result<Restored, String> {
    let mut stops_before = None;
    let mut waits = backoff.iter();
    loop {
        let error = match port_ops.dispatch_start(config).await {
            Ok(()) => return Ok(Restored::Started),
            Err(error) => error,
        };
        let Some(wait) = waits.next().filter(|_| is_transient_failure(&error)) else {
            return Err(error);
        };
        // Read after the first failure, not before the first attempt: a stop
        // that lands earlier either cancels that attempt (not transient) or
        // clears the row, which `restore_superseded` also checks.
        let stops_before = *stops_before.get_or_insert_with(|| port_ops.stop_generation(config_id));
        warn!("Config {config_id} is not reachable yet ({error}); retrying in {wait:?}");
        tokio::time::sleep(*wait).await;
        if restore_superseded(port_ops, config_id, stops_before).await {
            info!("Config {config_id} changed while waiting to retry; not restoring it");
            return Ok(Restored::Superseded);
        }
    }
}

/// Whether the intent to restore `config_id` no longer holds after a wait:
/// the user stopped or started it, or another live process owns the row.
async fn restore_superseded(
    port_ops: &Arc<dyn PortOperations>, config_id: i64, stops_before: u64,
) -> bool {
    if port_ops.stop_generation(config_id) != stops_before
        || port_ops.is_forward_registered(config_id).await
    {
        return true;
    }
    match port_ops.get_configs_state().await {
        Ok(states) => states
            .iter()
            .find(|state| state.config_id == config_id)
            .is_none_or(|state| !state.is_running || is_owned_by_other_live_process(state)),
        Err(e) => {
            debug!("Could not read config states while waiting to retry {config_id}: {e}");
            false
        }
    }
}

async fn start_port_forwarding(
    port_ops: Arc<dyn PortOperations>, config: Config,
) -> Result<(), String> {
    start_port_forwarding_with_backoff(port_ops, config, &RESTORE_RETRY_BACKOFF).await
}

async fn start_port_forwarding_with_backoff(
    port_ops: Arc<dyn PortOperations>, config: Config, backoff: &[Duration],
) -> Result<(), String> {
    let port = config.local_port.unwrap_or(0);
    debug!(
        "No process is occupying port {}. Starting port forwarding for '{}'...",
        port,
        config.alias.as_deref().unwrap_or("unknown")
    );

    let config_id = config.id.unwrap();
    let config_alias = config
        .alias
        .clone()
        .unwrap_or_else(|| format!("ID:{config_id}"));

    let result = measure(
        Operation::AutoStart,
        dispatch_until_reachable(&port_ops, &config, config_id, backoff),
    )
    .await;
    match result {
        Ok(Restored::Started) => {
            let config_state = ConfigState::new(config_id, true);
            port_ops.update_config_state(&config_state).await?;
            Ok(())
        }
        Ok(Restored::Superseded) => Ok(()),
        Err(e) => {
            let error_msg = format!("Failed to start port forwarding for '{config_alias}': {e}");
            error!("{error_msg}");

            // A start that lost to a concurrent one (a shortcut, a click) fails
            // with the forward live and its state written by the backend;
            // marking the row stopped here would erase that forward's
            // ownership and let another process treat it as idle.
            if port_ops.is_forward_registered(config_id).await {
                debug!("Config {config_id} is forwarding already; leaving its state as it is");
                return Ok(());
            }

            // The guard above only covers this process. A concurrent start
            // can also be owned by another live kftray/kftui process, whose
            // config_state row already reflects that ownership; clearing it
            // here would wipe that process's row out from under it. A row
            // that cannot be read at all is left untouched too: a read
            // failure says nothing about who owns the config, and clearing
            // it on that guess could erase another process's ownership just
            // as wrongly as skipping the check entirely would.
            match port_ops.get_configs_state().await {
                Ok(states) => {
                    if let Some(state) = states.iter().find(|s| s.config_id == config_id)
                        && is_owned_by_other_live_process(state)
                    {
                        debug!(
                            "Config {config_id} is owned by another live process (pid {:?}); \
                             leaving its state as it is",
                            state.process_id
                        );
                        return Err(error_msg);
                    }
                    let config_state = ConfigState::new_without_process(config_id, false);
                    port_ops.update_config_state(&config_state).await?;
                }
                Err(e) => {
                    debug!(
                        "Could not read config states while handling the start failure for \
                         {config_id}: {e}; leaving its state as it is"
                    );
                }
            }

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

            if local_port == port
                && let Some(&pid) = socket.associated_pids.first()
            {
                let process_name = get_process_name_by_pid(pid as i32);
                return Some((pid as i32, process_name));
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
            process_id: None,
            ..Default::default()
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
        let config = create_test_config(1, 8080, "tcp", Some("service"));

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

        mock.expect_dispatch_start().times(1).returning(|_| Ok(()));

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
        let config = create_test_config(1, 0, "tcp", Some("service"));

        mock.expect_find_process_by_port()
            .with(eq(0))
            .times(1)
            .returning(|_| None);

        mock.expect_dispatch_start().times(1).returning(|_| Ok(()));

        mock.expect_update_config_state()
            .times(1)
            .returning(|_| Ok(()));

        let result = check_and_manage_port(Arc::new(mock), config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_port_forwarding_success() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start().times(1).returning(|_| Ok(()));

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

        mock.expect_dispatch_start().times(1).returning(|_| Ok(()));

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
    async fn a_start_that_lost_to_a_live_forward_leaves_its_state_alone() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start()
            .times(1)
            .returning(|_| Err("Port forwarding is already running for config 1".to_string()));
        mock.expect_is_forward_registered()
            .with(eq(1))
            .times(1)
            .returning(|_| true);
        mock.expect_update_config_state().times(0);

        let result = start_port_forwarding(Arc::new(mock), config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_port_forwarding_failure() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start()
            .times(1)
            .returning(|_| Err("Port forwarding failed".to_string()));
        mock.expect_is_forward_registered()
            .times(1)
            .returning(|_| false);
        mock.expect_get_configs_state()
            .times(1)
            .returning(|| Ok(vec![]));

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
    async fn a_state_read_error_while_handling_a_start_failure_does_not_clear_the_row() {
        // Regression: falling through the `if let Ok(states) = ...` guard on
        // a state-read error used to reach the unconditional clear below it,
        // marking the config stopped on a read failure that says nothing
        // about who owns it.
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start()
            .times(1)
            .returning(|_| Err("Port forwarding failed".to_string()));
        mock.expect_is_forward_registered()
            .times(1)
            .returning(|_| false);
        mock.expect_get_configs_state()
            .times(1)
            .returning(|| Err("database is locked".to_string()));
        mock.expect_update_config_state().times(0);

        let result = start_port_forwarding(Arc::new(mock), config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_start_port_forwarding_update_state_error() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start().times(1).returning(|_| Ok(()));

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

    #[tokio::test]
    async fn a_start_failure_with_a_stale_row_from_this_process_still_clears() {
        // The row's process_id equals this process's own pid: that is
        // stale data from an earlier attempt in this same process, not
        // another live owner, so it must still be cleared.
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start()
            .times(1)
            .returning(|_| Err("Port forwarding failed".to_string()));
        mock.expect_is_forward_registered()
            .times(1)
            .returning(|_| false);
        mock.expect_get_configs_state().times(1).returning(|| {
            Ok(vec![ConfigState {
                id: None,
                config_id: 1,
                is_running: true,
                process_id: Some(std::process::id()),
                ..Default::default()
            }])
        });
        mock.expect_update_config_state()
            .with(function(|state: &ConfigState| {
                state.config_id == 1 && !state.is_running
            }))
            .times(1)
            .returning(|_| Ok(()));

        let result = start_port_forwarding(Arc::new(mock), config).await;
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_start_owned_by_another_live_process_leaves_its_config_state_row_alone() {
        // Regression: the ownership guard only covered this process
        // (`is_forward_registered`); a restore that failed because
        // another live kftray/kftui process owned the config still wiped
        // that process's config_state row.
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("failed to spawn helper process");
        let other_pid = child.id();

        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start().times(1).returning(move |_| {
            Err(format!(
                "Config 1 is being forwarded by another kftray process ({other_pid})"
            ))
        });
        mock.expect_is_forward_registered()
            .times(1)
            .returning(|_| false);
        mock.expect_get_configs_state().times(1).returning(move || {
            Ok(vec![ConfigState {
                id: None,
                config_id: 1,
                is_running: true,
                process_id: Some(other_pid),
                ..Default::default()
            }])
        });
        mock.expect_update_config_state().times(0);

        let result = start_port_forwarding(Arc::new(mock), config).await;
        assert!(result.is_err());

        // The zombie stays alive until reaped; clean it up now that the
        // check ran.
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_row_owned_by_another_live_process_is_not_restored() {
        // Regression: every running row was restored without looking at who
        // recorded it, so each row another live kftray process was already
        // forwarding went through a start that its ownership guard refused,
        // and the refusal was reported as an autostart failure.
        use std::sync::mpsc;

        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("failed to spawn helper process");
        let other_pid = child.id();

        let foreign = ConfigState {
            config_id: 1,
            is_running: true,
            process_id: Some(other_pid),
            ..Default::default()
        };
        let own = create_config_state(2, true);
        let (started_tx, started_rx) = mpsc::channel();
        let (written_tx, written_rx) = mpsc::channel();

        let mut mock = MockPortOperations::new();
        mock.expect_get_configs_state()
            .returning(move || Ok(vec![foreign.clone(), own.clone()]));
        mock.expect_get_config().returning(|id| {
            Ok(create_test_config(
                id,
                8080 + id as u16,
                "tcp",
                Some("service"),
            ))
        });
        mock.expect_find_process_by_port().returning(|_| None);
        mock.expect_dispatch_start().returning(move |config| {
            started_tx.send(config.id.unwrap()).unwrap();
            Ok(())
        });
        mock.expect_update_config_state().returning(move |state| {
            written_tx.send(state.config_id).unwrap();
            Ok(())
        });

        let result = check_and_manage_ports(Arc::new(mock)).await;

        let _ = child.kill();
        let _ = child.wait();
        assert!(result.is_ok());
        assert_eq!(started_rx.try_iter().collect::<Vec<_>>(), vec![2]);
        assert_eq!(written_rx.try_iter().collect::<Vec<_>>(), vec![2]);
    }

    const NOT_REACHABLE: &str =
        "error trying to connect: tcp connect error: Connection refused (os error 111)";
    const TEST_BACKOFF: [Duration; 2] = [Duration::from_millis(1), Duration::from_millis(1)];

    fn marks_running(state: &ConfigState) -> bool {
        state.config_id == 1 && state.is_running
    }

    fn marks_stopped(state: &ConfigState) -> bool {
        state.config_id == 1 && !state.is_running
    }

    #[tokio::test]
    async fn a_restore_that_finds_the_cluster_unreachable_is_retried_until_it_starts() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));
        let mut attempts = mockall::Sequence::new();

        mock.expect_dispatch_start()
            .times(1)
            .in_sequence(&mut attempts)
            .returning(|_| Err(NOT_REACHABLE.to_string()));
        mock.expect_dispatch_start()
            .times(1)
            .in_sequence(&mut attempts)
            .returning(|_| Ok(()));
        mock.expect_stop_generation().returning(|_| 0);
        mock.expect_is_forward_registered().returning(|_| false);
        mock.expect_get_configs_state()
            .returning(|| Ok(vec![create_config_state(1, true)]));
        mock.expect_update_config_state()
            .with(function(marks_running))
            .times(1)
            .returning(|_| Ok(()));

        let result =
            start_port_forwarding_with_backoff(Arc::new(mock), config, &TEST_BACKOFF).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn a_restore_gives_up_after_the_backoff_and_marks_the_config_stopped() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start()
            .times(TEST_BACKOFF.len() + 1)
            .returning(|_| Err(NOT_REACHABLE.to_string()));
        mock.expect_stop_generation().returning(|_| 0);
        mock.expect_is_forward_registered().returning(|_| false);
        mock.expect_get_configs_state()
            .returning(|| Ok(vec![create_config_state(1, true)]));
        mock.expect_update_config_state()
            .with(function(marks_stopped))
            .times(1)
            .returning(|_| Ok(()));

        let result =
            start_port_forwarding_with_backoff(Arc::new(mock), config, &TEST_BACKOFF).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn only_transient_failures_are_retried() {
        for message in [
            "Config 1 is being forwarded by another kftray process (4242); stop it there",
            "Port forwarding is already running for config 1",
            "Failed to read kubeconfig from /home/user/.kube/config",
            "Config has no ID",
            "Failed to bind 127.0.0.1:80: Permission denied (os error 13)",
            "Failed to bind 127.0.0.1:8080: Address already in use (os error 98)",
        ] {
            let mut mock = MockPortOperations::new();
            let config = create_test_config(1, 8080, "tcp", Some("service"));

            mock.expect_dispatch_start()
                .times(1)
                .returning(move |_| Err(message.to_string()));
            mock.expect_stop_generation().times(0);
            mock.expect_is_forward_registered().returning(|_| false);
            mock.expect_get_configs_state().returning(|| Ok(vec![]));
            mock.expect_update_config_state()
                .with(function(marks_stopped))
                .times(1)
                .returning(|_| Ok(()));

            let result =
                start_port_forwarding_with_backoff(Arc::new(mock), config, &TEST_BACKOFF).await;
            assert_eq!(
                result,
                Err(format!(
                    "Failed to start port forwarding for 'test-config-1': {message}"
                ))
            );
        }
    }

    #[tokio::test]
    async fn a_user_stop_while_waiting_to_retry_cancels_the_restore() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));
        let stop_reads = std::sync::atomic::AtomicU64::new(0);

        mock.expect_dispatch_start()
            .times(1)
            .returning(|_| Err(NOT_REACHABLE.to_string()));
        mock.expect_stop_generation()
            .returning(move |_| stop_reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
        mock.expect_update_config_state().times(0);

        let result =
            start_port_forwarding_with_backoff(Arc::new(mock), config, &TEST_BACKOFF).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn a_row_cleared_while_waiting_to_retry_cancels_the_restore() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start()
            .times(1)
            .returning(|_| Err(NOT_REACHABLE.to_string()));
        mock.expect_stop_generation().returning(|_| 0);
        mock.expect_is_forward_registered().returning(|_| false);
        mock.expect_get_configs_state()
            .returning(|| Ok(vec![create_config_state(1, false)]));
        mock.expect_update_config_state().times(0);

        let result =
            start_port_forwarding_with_backoff(Arc::new(mock), config, &TEST_BACKOFF).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn a_user_start_while_waiting_to_retry_cancels_the_restore() {
        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start()
            .times(1)
            .returning(|_| Err(NOT_REACHABLE.to_string()));
        mock.expect_stop_generation().returning(|_| 0);
        mock.expect_is_forward_registered().returning(|_| true);
        mock.expect_update_config_state().times(0);

        let result =
            start_port_forwarding_with_backoff(Arc::new(mock), config, &TEST_BACKOFF).await;
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn another_process_claiming_the_row_while_waiting_ends_the_retry() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("failed to spawn helper process");
        let other_pid = child.id();

        let mut mock = MockPortOperations::new();
        let config = create_test_config(1, 8080, "tcp", Some("service"));

        mock.expect_dispatch_start()
            .times(1)
            .returning(|_| Err(NOT_REACHABLE.to_string()));
        mock.expect_stop_generation().returning(|_| 0);
        mock.expect_is_forward_registered().returning(|_| false);
        mock.expect_get_configs_state().returning(move || {
            Ok(vec![ConfigState {
                config_id: 1,
                is_running: true,
                process_id: Some(other_pid),
                ..Default::default()
            }])
        });
        mock.expect_update_config_state().times(0);

        let result =
            start_port_forwarding_with_backoff(Arc::new(mock), config, &TEST_BACKOFF).await;

        let _ = child.kill();
        let _ = child.wait();
        assert!(result.is_ok());
    }
}
