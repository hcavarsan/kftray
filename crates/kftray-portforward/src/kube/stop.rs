use std::collections::{
    HashMap,
    HashSet,
};
use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use futures::stream::{
    self,
    StreamExt,
};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kftray_commons::config_model::Config;
use kftray_commons::{
    config::get_config_with_mode,
    models::{
        config_state_model::ConfigState,
        response::CustomResponse,
    },
    utils::{
        config::read_configs_with_mode,
        config_state::{
            get_configs_state_with_mode,
            update_config_state_with_mode,
        },
        db_mode::DatabaseMode,
        timeout_manager::cancel_timeout_for_forward,
    },
};
use kube::Client;
use kube::api::{
    Api,
    DeleteParams,
    ListParams,
};
use tokio::task::spawn_blocking;
use tokio::time::timeout;
use tracing::{
    info,
    warn,
};

use crate::hostsfile::{
    remove_host_entry,
    remove_ssl_host_entry,
};
use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};
use crate::port_forward::CHILD_PROCESSES;
#[cfg(test)]
use crate::port_forward::PortForwardProcess;

/// One tracked cleanup target.
#[derive(Clone)]
struct PendingTarget {
    config: Config,
    /// Set while a create request's outcome is unknown. Until it expires, an
    /// empty resource list is not proof of cleanup: the API server may still be
    /// persisting an object whose request was abandoned.
    uncertain_until: Option<Instant>,
}

impl PendingTarget {
    fn is_uncertain(&self, now: Instant) -> bool {
        self.uncertain_until.is_some_and(|until| now < until)
    }
}

lazy_static::lazy_static! {
    /// Configurations whose cleanup has not been confirmed, kept so a later
    /// stop can retry against the resources that actually exist. The database
    /// row is not a substitute: it can be edited or deleted while a forward
    /// runs. One id can hold several entries, because an edited config that was
    /// restarted describes different resources than the ones an earlier failed
    /// cleanup left behind.
    static ref PENDING_CLEANUP: dashmap::DashMap<i64, Vec<PendingTarget>> = dashmap::DashMap::new();
}

/// How long an abandoned create is assumed to still be in flight.
const UNCERTAIN_CREATE_WINDOW: Duration = Duration::from_secs(120);

/// Two cleanup targets are the same when they name the same resources.
///
/// The protocol is part of that identity: `delete_cluster_resources` treats a
/// UDP service config as owning relay resources and the otherwise identical TCP
/// config as a no-op, so collapsing them would let the no-op forget the relay.
/// The local address and domain alias are part of it too, since local cleanup
/// releases exactly those.
fn same_resources(left: &Config, right: &Config) -> bool {
    left.namespace == right.namespace
        && left.context == right.context
        && left.kubeconfig == right.kubeconfig
        && left.workload_type == right.workload_type
        && left.protocol == right.protocol
        && left.service == right.service
        && left.local_address == right.local_address
        && left.domain_enabled == right.domain_enabled
}

/// Records a configuration whose resources exist but whose startup did not
/// finish, so stop-all still reaches them. Dropping a startup future (the
/// terminal's shutdown drain, an aborted task) skips its own rollback.
pub(crate) fn record_pending_cleanup(id: i64, config: Config) {
    record_target(id, config, None);
}

fn record_target(id: i64, config: Config, uncertain_until: Option<Instant>) {
    let mut entries = PENDING_CLEANUP.entry(id).or_default();
    if let Some(existing) = entries
        .iter_mut()
        .find(|entry| same_resources(&entry.config, &config))
    {
        existing.uncertain_until = match (existing.uncertain_until, uncertain_until) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        return;
    }
    entries.push(PendingTarget {
        config,
        uncertain_until,
    });
}

/// Drops one recorded target, leaving any other resources for this id tracked.
pub(crate) fn forget_pending_cleanup(id: i64, config: &Config) {
    let mut empty = false;
    if let Some(mut entries) = PENDING_CLEANUP.get_mut(&id) {
        entries.retain(|entry| !same_resources(&entry.config, config));
        empty = entries.is_empty();
    }
    if empty {
        PENDING_CLEANUP.remove(&id);
    }
}

fn pending_cleanup_targets(id: i64) -> Vec<PendingTarget> {
    PENDING_CLEANUP
        .get(&id)
        .map(|entry| entry.value().clone())
        .unwrap_or_default()
}

/// Records created cluster resources so a dropped startup future still leaves a
/// trail for stop-all.
///
/// The record is uncertain until [`confirm`](Self::confirm) observes the
/// create's outcome: a request abandoned in flight can still be persisted, and
/// forgetting it on one empty list would leave it running untracked.
pub(crate) struct ClusterResourceGuard {
    id: i64,
    config: Option<Config>,
}

impl ClusterResourceGuard {
    pub(crate) fn arm(id: i64, config: Config) -> Self {
        record_target(
            id,
            config.clone(),
            Some(Instant::now() + UNCERTAIN_CREATE_WINDOW),
        );
        Self {
            id,
            config: Some(config),
        }
    }

    /// The create returned, so the record describes a resource that either
    /// exists or never will.
    pub(crate) fn confirm(&self) {
        if let Some(config) = &self.config
            && let Some(mut entries) = PENDING_CLEANUP.get_mut(&self.id)
            && let Some(entry) = entries
                .iter_mut()
                .find(|entry| same_resources(&entry.config, config))
        {
            entry.uncertain_until = None;
        }
    }

    pub(crate) fn disarm(mut self) {
        if let Some(config) = self.config.take() {
            forget_pending_cleanup(self.id, &config);
        }
    }
}

impl Drop for ClusterResourceGuard {
    fn drop(&mut self) {
        if let Some(config) = self.config.take() {
            record_pending_cleanup(self.id, config);
        }
    }
}

/// Releases the loopback address and host entries one target describes.
async fn release_local_resources(id: i64, config: &Config) -> Result<(), String> {
    if let Some(address) = &config.local_address
        && crate::network_utils::is_custom_loopback_address(address)
    {
        release_address_with_fallback(address).await;
    }
    let mut errors = Vec::new();
    if config.domain_enabled.unwrap_or_default()
        && let Err(error) = remove_host_entry(&id.to_string())
    {
        errors.push(error.to_string());
    }
    if let Err(error) = remove_ssl_host_entry(&id.to_string()) {
        errors.push(error.to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

async fn delete_cluster_resources(id: i64, config: &Config) -> Result<(), String> {
    if config.workload_type.as_deref() != Some("expose")
        && config.workload_type.as_deref() != Some("proxy")
        && config.protocol != "udp"
    {
        return Ok(());
    }
    let key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
    let connection = SHARED_CLIENT_MANAGER
        .get_connection(key)
        .await
        .map_err(|error| error.to_string())?;
    if config.workload_type.as_deref() == Some("expose") {
        crate::expose::kubernetes::delete_expose_resources(
            connection.client.clone(),
            &config.namespace,
            &id.to_string(),
        )
        .await
    } else {
        delete_proxy_cluster_resources(connection.client.clone(), &config.namespace, id).await
    }
}

/// Synchronous helper function to release address via helper service.
/// Must be called from spawn_blocking to avoid blocking the tokio runtime.
fn try_release_address_sync(address: &str) -> Result<(), String> {
    let app_id = "com.kftray.app".to_string();

    let socket_path =
        kftray_helper::communication::get_default_socket_path().map_err(|e| e.to_string())?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err("Helper service is not available".to_string());
    }

    let command = kftray_helper::messages::RequestCommand::Address(
        kftray_helper::messages::AddressCommand::Release {
            address: address.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(response) => match response.result {
            kftray_helper::messages::RequestResult::Success => Ok(()),
            kftray_helper::messages::RequestResult::Error(error) => Err(error),
            _ => Err("Unexpected response format".to_string()),
        },
        Err(e) => Err(e.to_string()),
    }
}

/// Release address with timeout. Skips osascript fallback to avoid blocking on
/// user interaction. Address cleanup is not critical - addresses will be freed
/// on system restart.
async fn release_address_with_fallback(address: &str) {
    const ADDRESS_RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

    let address_owned = address.to_string();

    // Wrap blocking helper service call in spawn_blocking with timeout
    let result = timeout(ADDRESS_RELEASE_TIMEOUT, async {
        let addr = address_owned.clone();
        spawn_blocking(move || try_release_address_sync(&addr)).await
    })
    .await;

    match result {
        Ok(Ok(Ok(_))) => {
            info!("Successfully released address via helper: {}", address);
        }
        Ok(Ok(Err(e))) => {
            // Helper service returned an error - skip fallback (osascript blocks for user
            // input)
            warn!(
                "Failed to release address {} via helper: {}. Skipping fallback to avoid blocking.",
                address, e
            );
        }
        Ok(Err(e)) => {
            // spawn_blocking panicked
            warn!(
                "Address release task panicked for {}: {}. Skipping.",
                address, e
            );
        }
        Err(_) => {
            // Timeout elapsed
            warn!(
                "Address release timed out for {} after {:?}. Skipping.",
                address, ADDRESS_RELEASE_TIMEOUT
            );
        }
    }
}

pub(crate) async fn delete_proxy_cluster_resources(
    client: Client, namespace: &str, config_id: i64,
) -> Result<(), String> {
    let prefix = crate::kube::proxy::proxy_resource_prefix();
    let owned = ListParams::default().labels(&crate::kube::proxy::proxy_owner_selector(
        &config_id.to_string(),
    )?);
    // Relays created before the installation label existed carry only
    // app/config_id. They can only have come from this user on this machine, so
    // the name prefix still identifies them, and an installation that upgraded
    // would otherwise treat an empty labelled list as proof of cleanup. A
    // resource created by another installation after the upgrade always carries
    // the label and is excluded here.
    let legacy = ListParams::default().labels(&format!(
        "config_id={config_id},!{}",
        crate::kube::proxy::INSTALLATION_LABEL
    ));
    let dp = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::default()
    };
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let deployments: Api<Deployment> = Api::namespaced(client, namespace);
    let selectors = [&owned, &legacy];
    let delete_pods = delete_prefixed(&pods, &selectors, &prefix, &dp);
    let delete_deployments = delete_prefixed(&deployments, &selectors, &prefix, &dp);
    let (pods, deployments) = tokio::join!(delete_pods, delete_deployments);
    let errors: Vec<_> = [pods, deployments]
        .into_iter()
        .filter_map(Result::err)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

async fn delete_prefixed<K>(
    api: &Api<K>, selectors: &[&ListParams], prefix: &str, dp: &DeleteParams,
) -> Result<(), String>
where
    K: Clone + serde::de::DeserializeOwned + std::fmt::Debug + kube::Resource,
{
    let mut errors = Vec::new();
    for lp in selectors {
        let list = api.list(lp).await.map_err(|error| error.to_string())?;
        for item in list.items {
            let Some(name) = item.meta().name.clone() else {
                continue;
            };
            if !name.starts_with(prefix) {
                continue;
            }
            if let Err(error) = api.delete(&name, dp).await
                && !matches!(&error, kube::Error::Api(response) if response.code == 404)
            {
                errors.push(error.to_string());
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    stop_all_port_forward_with_mode(DatabaseMode::File).await
}

/// Cancels every startup and recovery attempt without waiting for them.
///
/// Shutdown calls this before draining so in-flight startups observe
/// cancellation cooperatively and run their own rollback, instead of being
/// dropped mid-create by an abort deadline.
pub fn cancel_all_startups() {
    for entry in crate::kube::proxy::STARTING_PROXIES.iter() {
        entry.value().cancel();
    }
    for entry in crate::kube::proxy_recovery::RECOVERY_MANAGERS.iter() {
        entry.value().cancel();
    }
    for entry in CHILD_PROCESSES.iter() {
        entry.value().cancel();
    }
}

pub async fn stop_all_port_forward_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<CustomResponse>, String> {
    let mut ids: HashSet<i64> = CHILD_PROCESSES.iter().map(|entry| *entry.key()).collect();
    for entry in crate::kube::proxy::STARTING_PROXIES.iter() {
        entry.value().cancel();
        ids.insert(*entry.key());
    }
    for entry in crate::kube::proxy_recovery::RECOVERY_MANAGERS.iter() {
        entry.value().cancel();
        ids.insert(*entry.key());
    }
    for entry in CHILD_PROCESSES.iter() {
        entry.value().cancel();
    }
    for entry in crate::kube::proxy_recovery::RECOVERY_LOCKS.iter() {
        if Arc::strong_count(entry.value()) > 1 {
            ids.insert(*entry.key());
        }
    }
    ids.extend(PENDING_CLEANUP.iter().map(|entry| *entry.key()));

    let configs_result = read_configs_with_mode(mode).await;
    let states_result = get_configs_state_with_mode(mode).await;
    if let Ok(states) = &states_result {
        ids.extend(
            states
                .iter()
                .filter(|state| state.is_running)
                .map(|state| state.config_id),
        );
    }
    let configs: HashMap<_, _> = configs_result
        .as_ref()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|config| config.id.map(|id| (id, config)))
        .collect();
    let mut responses: Vec<CustomResponse> = stream::iter(ids)
        .map(|id| {
            let config = configs.get(&id).copied();
            async move {
                match stop_config(id, config, mode).await {
                    Ok(response) => response,
                    Err(error) => stop_response(id, config, Some(error)),
                }
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;

    if let Err(error) = configs_result {
        warn!("Could not read configs while stopping every forward: {error}");
        responses.push(enumeration_failure(format!(
            "Could not read configs, so some forwards may not have been cleaned up: {error}"
        )));
    }
    if let Err(error) = states_result {
        warn!("Could not read config states while stopping every forward: {error}");
        responses.push(enumeration_failure(format!(
            "Could not read config states, so persisted forwards may have been missed: {error}"
        )));
    }
    Ok(responses)
}

/// Failure response that is not tied to a single config, used when stop-all
/// cannot enumerate everything it was supposed to stop.
fn enumeration_failure(error: String) -> CustomResponse {
    CustomResponse {
        id: None,
        service: String::new(),
        namespace: String::new(),
        local_port: 0,
        remote_port: 0,
        context: String::new(),
        protocol: String::new(),
        stdout: String::new(),
        status: 1,
        stderr: error,
    }
}

pub async fn stop_port_forward(config_id: String) -> Result<CustomResponse, String> {
    stop_port_forward_with_mode(config_id, DatabaseMode::File).await
}

pub async fn stop_port_forward_with_mode(
    config_id: String, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
    let id = config_id
        .parse::<i64>()
        .map_err(|_| "Invalid config ID".to_string())?;
    if let Some(startup) = crate::kube::proxy::STARTING_PROXIES.get(&id) {
        startup.cancel();
    }
    if let Some(manager) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&id) {
        manager.cancel();
    }
    if let Some(process) = CHILD_PROCESSES.get(&id) {
        process.cancel();
    }
    let config = get_config_with_mode(id, mode).await;
    let response = stop_config(id, config.as_ref().ok(), mode).await;
    match (config, response) {
        // The cleanup error describes what is still running; the lookup error
        // only says the row is gone, which is not the actionable part.
        (Err(lookup), Err(cleanup)) => Err(format!("{cleanup}; {lookup}")),
        (_, response) => response,
    }
}

async fn stop_config(
    id: i64, config: Option<&Config>, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
    if let Some(startup) = crate::kube::proxy::STARTING_PROXIES.get(&id) {
        startup.cancel();
    }
    if let Some(manager) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&id) {
        manager.cancel();
    }
    let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
    let (guard, was_starting) = match lock.try_lock() {
        Ok(guard) => (guard, false),
        Err(_) => (lock.lock().await, true),
    };
    if let Some(startup) = crate::kube::proxy::STARTING_PROXIES.get(&id) {
        startup.cancel();
    }
    if let Some((_, manager)) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.remove(&id) {
        manager.cancel();
    }
    let process = CHILD_PROCESSES.remove(&id);
    let existed = process.is_some();
    let mut retained = None;
    if let Some((_, mut process)) = process {
        retained = process.config().cloned();
        // Recorded before the first await: the process is already out of
        // CHILD_PROCESSES, so aborting this stop, as the terminal's shutdown
        // drain does, would otherwise drop the only snapshot of its resources.
        if let Some(config) = &retained {
            record_pending_cleanup(id, config.clone());
        }
        process.cleanup_and_abort().await;
    }
    // The snapshot taken at startup describes the resources that actually
    // exist. The database record can have been edited since without stopping
    // the forward, which would point cleanup at the new destination.
    let pending = pending_cleanup_targets(id);
    let refreshed = if retained.is_none() && pending.is_empty() && was_starting {
        get_config_with_mode(id, mode).await.ok()
    } else {
        None
    };
    let config = retained
        .as_ref()
        .or(refreshed.as_ref())
        .or(config)
        .or(pending.first().map(|target| &target.config));
    cancel_timeout_for_forward(id).await;

    let result = if let Some(config) = config {
        // Every distinct set of resources this id ever created, not just the
        // current one: an edited config that was restarted does not describe
        // the resources an earlier failed cleanup left behind.
        let mut targets = vec![PendingTarget {
            config: config.clone(),
            uncertain_until: pending
                .iter()
                .find(|target| same_resources(&target.config, config))
                .and_then(|target| target.uncertain_until),
        }];
        targets.extend(
            pending
                .iter()
                .filter(|target| !same_resources(&target.config, config))
                .cloned(),
        );

        let mut errors: Vec<String> = Vec::new();
        let mut settled: Vec<Config> = Vec::new();
        let now = Instant::now();
        for target in &targets {
            let cluster = delete_cluster_resources(id, &target.config).await;
            // Local resources are released per target too: an edited row can
            // name a different loopback address than the one still bound.
            let local = release_local_resources(id, &target.config).await;
            match (cluster, local) {
                (Ok(()), Ok(())) => {
                    // An abandoned create may still be persisting, so one empty
                    // list is not proof. The record survives, and the stop
                    // reports incomplete cleanup rather than marking the config
                    // stopped while a relay may still appear.
                    if target.is_uncertain(now) {
                        record_target(id, target.config.clone(), target.uncertain_until);
                        errors.push(format!(
                            "A create request for config {id} was never answered, so its cluster \
                             resources are still being reconciled"
                        ));
                    } else {
                        settled.push(target.config.clone());
                    }
                }
                (cluster, local) => {
                    // The only remaining record of where these resources live:
                    // the database row can be edited or deleted while a forward
                    // runs.
                    record_target(id, target.config.clone(), target.uncertain_until);
                    if let Err(error) = cluster {
                        SHARED_CLIENT_MANAGER.invalidate_client(&ServiceClientKey::new(
                            target.config.context.clone(),
                            target.config.kubeconfig.clone(),
                        ));
                        errors.push(error);
                    }
                    if let Err(error) = local {
                        errors.push(error);
                    }
                }
            }
        }
        for target in settled {
            forget_pending_cleanup(id, &target);
        }
        if errors.is_empty() {
            let state = ConfigState::new(id, false);
            if let Err(error) = update_config_state_with_mode(&state, mode).await {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(stop_response(id, Some(config), None))
        } else {
            Err(errors.join("; "))
        }
    } else if existed {
        Err(format!(
            "Stopped the local process for config {id} but could not load its configuration, so \
             cluster resources, loopback addresses and host entries were left in place"
        ))
    } else {
        Err(format!(
            "No port forwarding process found for config_id '{id}'"
        ))
    };
    drop(guard);
    drop(lock);
    crate::kube::proxy_recovery::remove_recovery_lock(id);
    result
}

fn stop_response(id: i64, config: Option<&Config>, error: Option<String>) -> CustomResponse {
    CustomResponse {
        id: Some(id),
        service: config
            .and_then(|config| config.service.clone())
            .unwrap_or_default(),
        namespace: config
            .map(|config| config.namespace.clone())
            .unwrap_or_default(),
        local_port: config
            .and_then(|config| config.local_port)
            .unwrap_or_default(),
        remote_port: config
            .and_then(|config| config.remote_port)
            .unwrap_or_default(),
        context: config
            .and_then(|config| config.context.clone())
            .unwrap_or_default(),
        protocol: config
            .map(|config| config.protocol.clone())
            .unwrap_or_default(),
        stdout: if error.is_none() {
            "Port forwarding has been stopped".to_string()
        } else {
            String::new()
        },
        status: i32::from(error.is_some()),
        stderr: error.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::{
        TcpListener,
        UdpSocket,
    };

    use super::*;

    /// Mirrors the snapshot `start_config` stores: a plain local forward with
    /// no cluster resources, loopback address or host entry to release.
    fn local_config(id: i64) -> Config {
        Config {
            id: Some(id),
            namespace: "default".to_string(),
            service: Some("test-service".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            ..Config::default()
        }
    }

    #[tokio::test]
    async fn stop_releases_listener_and_websocket_owner_before_returning() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_011;
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp_address = tcp.local_addr().unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_address = udp.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _tcp = tcp;
            std::future::pending::<anyhow::Result<()>>().await
        });
        let websocket = tokio::spawn(async move {
            let _udp = udp;
            std::future::pending::<()>().await;
        });
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_ws_client_handle(websocket);
        process.set_config(local_config(id));
        CHILD_PROCESSES.insert(id, process);

        stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .unwrap();

        let _tcp = TcpListener::bind(tcp_address).await.unwrap();
        let _udp = UdpSocket::bind(udp_address).await.unwrap();
    }

    #[tokio::test]
    async fn stop_uses_the_retained_config_when_the_database_lookup_fails() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_051;
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_config(Config {
            id: Some(id),
            workload_type: Some("proxy".to_string()),
            protocol: "tcp".to_string(),
            namespace: "default".to_string(),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/isolated-test-kubeconfig".to_string()),
            ..Config::default()
        });
        CHILD_PROCESSES.insert(id, process);

        let error = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .expect_err("cluster cleanup must be attempted from the retained config");

        assert!(!CHILD_PROCESSES.contains_key(&id));
        assert!(
            !error.contains("could not load its configuration"),
            "the retained snapshot should have supplied the cleanup metadata: {error}"
        );
    }

    #[tokio::test]
    async fn stop_all_releases_every_transport_in_memory_mode() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mut addresses = Vec::new();
        for id in [410_021, 410_022] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            addresses.push(listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                let _listener = listener;
                std::future::pending::<anyhow::Result<()>>().await
            });
            let mut process = PortForwardProcess::new(task, id.to_string());
            process.set_config(local_config(id));
            CHILD_PROCESSES.insert(id, process);
        }
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_address = udp.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _udp = udp;
            std::future::pending::<anyhow::Result<()>>().await
        });
        let mut process = PortForwardProcess::new(task, "410023".to_string());
        process.set_config(local_config(410_023));
        CHILD_PROCESSES.insert(410_023, process);

        let responses = stop_all_port_forward_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        for id in [410_021, 410_022, 410_023] {
            assert!(
                responses
                    .iter()
                    .any(|response| response.id == Some(id) && response.status == 0)
            );
        }
        for address in addresses {
            let _listener = TcpListener::bind(address).await.unwrap();
        }
        let _udp = UdpSocket::bind(udp_address).await.unwrap();
    }

    #[tokio::test]
    async fn failed_cluster_cleanup_keeps_the_config_marked_running() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = Config {
            namespace: "default".to_string(),
            service: Some("expose-target".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("expose".to_string()),
            ..Config::default()
        };
        let id =
            kftray_commons::utils::config::insert_config_with_mode(config, DatabaseMode::Memory)
                .await
                .unwrap();
        update_config_state_with_mode(&ConfigState::new(id, true), DatabaseMode::Memory)
            .await
            .unwrap();
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        CHILD_PROCESSES.insert(id, PortForwardProcess::new(task, id.to_string()));

        let error = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .expect_err("an unreachable cluster must fail the stop");
        assert!(!error.is_empty());

        let states = get_configs_state_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert!(
            states
                .iter()
                .any(|state| state.config_id == id && state.is_running),
            "orphaned cluster resources must keep the config retryable"
        );
    }

    #[tokio::test]
    async fn a_failed_cleanup_keeps_its_snapshot_after_the_config_is_edited() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = Config {
            namespace: "original-namespace".to_string(),
            service: Some("proxy-target".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };
        let id = kftray_commons::utils::config::insert_config_with_mode(
            config.clone(),
            DatabaseMode::Memory,
        )
        .await
        .unwrap();
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_config(Config {
            id: Some(id),
            ..config
        });
        CHILD_PROCESSES.insert(id, process);

        stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .expect_err("an unreachable cluster must fail the stop");

        // The row is gone and the process is no longer registered, so the
        // retained snapshot is the only thing that still knows the namespace.
        kftray_commons::utils::config::delete_config_with_mode(id, DatabaseMode::Memory)
            .await
            .unwrap();
        let pending = pending_cleanup_targets(id);
        assert_eq!(
            pending.len(),
            1,
            "a failed cleanup must keep its snapshot for the next stop"
        );
        assert_eq!(pending[0].config.namespace, "original-namespace");

        let responses = stop_all_port_forward_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert!(
            responses.iter().any(|response| response.id == Some(id)),
            "stop-all must retry a configuration whose cleanup never finished"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn a_dropped_startup_leaves_its_resources_for_stop_all() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_130;
        let config = Config {
            id: Some(id),
            namespace: "default".to_string(),
            service: Some("kftray-forward-abc".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };

        // Models a startup future dropped after it created cluster resources:
        // its own rollback never runs, so the trail is all stop-all has.
        record_pending_cleanup(id, config);

        let responses = stop_all_port_forward_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert!(
            responses.iter().any(|response| response.id == Some(id)),
            "stop-all must reach resources left behind by a dropped startup"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn an_uncertain_create_survives_an_empty_cleanup_pass() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_160;
        let config = Config {
            id: Some(id),
            namespace: "default".to_string(),
            service: Some("kftray-forward-pending".to_string()),
            protocol: "tcp".to_string(),
            // A plain service forward: cluster cleanup is a successful no-op,
            // which is exactly the "empty list" the create could outlive.
            workload_type: Some("service".to_string()),
            ..Config::default()
        };
        let guard = ClusterResourceGuard::arm(id, config.clone());
        // Dropped without `confirm`: the create request was abandoned, so the
        // resource may still be persisting.
        drop(guard);

        let _ = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory).await;

        assert!(
            pending_cleanup_targets(id)
                .iter()
                .any(|target| target.config.service.as_deref() == Some("kftray-forward-pending")),
            "an unconfirmed create must not be forgotten on one empty pass"
        );

        // Once the create's outcome is observed, the same pass settles it.
        let guard = ClusterResourceGuard::arm(id, config.clone());
        guard.confirm();
        std::mem::forget(guard);
        let _ = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory).await;
        assert!(
            pending_cleanup_targets(id).is_empty(),
            "a confirmed target is forgotten once cleanup succeeds"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn a_restart_after_failed_cleanup_keeps_both_resource_sets() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_140;
        let orphaned = Config {
            id: Some(id),
            namespace: "old-namespace".to_string(),
            service: Some("kftray-forward-old".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };
        record_pending_cleanup(id, orphaned.clone());

        // The config was edited and restarted as a plain TCP forward, whose own
        // cleanup is a no-op and must not discard the orphaned proxy.
        let restarted = Config {
            id: Some(id),
            namespace: "new-namespace".to_string(),
            service: Some("plain-service".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            ..Config::default()
        };
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_config(restarted);
        CHILD_PROCESSES.insert(id, process);

        let stopped = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory).await;

        assert!(
            stopped.is_err(),
            "the orphaned proxy must still be attempted, not silently skipped"
        );
        let still_pending = pending_cleanup_targets(id);
        assert!(
            still_pending
                .iter()
                .any(|target| target.config.namespace == "old-namespace"),
            "cleaning the restarted config must not forget the earlier resources"
        );
        PENDING_CLEANUP.remove(&id);
    }
}
