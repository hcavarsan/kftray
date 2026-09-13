use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use dashmap::{
    DashMap,
    mapref::entry::Entry,
};
use futures::{
    StreamExt,
    TryStreamExt,
    stream,
};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{
        Pod,
        PodSpec,
        Probe,
        TCPSocketAction,
    },
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kftray_commons::{
    models::{
        config_model::Config,
        response::CustomResponse,
    },
    utils::{
        config_dir::{
            get_pod_manifest_path,
            get_proxy_deployment_manifest_path,
        },
        db_mode::DatabaseMode,
        manifests::{
            pod_manifest_is_customized,
            proxy_deployment_manifest_exists,
        },
    },
};
use kube::Client;
use kube::api::{
    Api,
    DeleteParams,
    PostParams,
};
use kube_runtime::WatchStreamExt;
use log::{
    error,
    info,
};
use rand::distr::{
    Alphanumeric,
    SampleString,
};
use tokio_util::sync::CancellationToken;

use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};

pub(super) static STARTING_PROXIES: std::sync::LazyLock<DashMap<i64, CancellationToken>> =
    std::sync::LazyLock::new(DashMap::new);

pub(super) struct PendingStart {
    id: i64,
    cancellation: CancellationToken,
}

impl PendingStart {
    pub(super) fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }
}

impl PendingStart {
    fn new(id: i64) -> Result<Self, String> {
        match STARTING_PROXIES.entry(id) {
            Entry::Occupied(_) => Err(format!("Startup is already in progress for config {id}")),
            Entry::Vacant(entry) => {
                let cancellation = CancellationToken::new();
                entry.insert(cancellation.clone());
                Ok(Self { id, cancellation })
            }
        }
    }
}

impl Drop for PendingStart {
    fn drop(&mut self) {
        STARTING_PROXIES.remove(&self.id);
        crate::kube::proxy_recovery::remove_recovery_lock(self.id);
    }
}

/// Name prefix shared by every proxy resource this user creates. Config ids come
/// from a local database and are not unique inside a namespace, so anything that
/// selects resources by `config_id` must also match this prefix.
///
/// The username is reduced to at most 16 ASCII alphanumeric characters: the
/// full name is also used as an `app` label value, which Kubernetes caps at 63
/// characters and restricts to ASCII, so a long or non-ASCII username would
/// make every create request fail.
pub(crate) fn proxy_resource_prefix() -> String {
    let username: String = whoami::username()
        .unwrap_or_else(|_| "unknown".to_string())
        .to_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(16)
        .collect();
    format!("kftray-forward-{username}-")
}

/// Label carried by every proxy resource this installation creates.
pub(crate) const INSTALLATION_LABEL: &str = "installation_id";

/// Selector that matches only this installation's resources for `config_id`.
/// Two machines can hold the same local config id under the same username, so
/// `config_id` alone is not an ownership test.
pub(crate) async fn proxy_owner_selector(config_id: &str) -> Result<String, String> {
    Ok(format!(
        "config_id={config_id},{INSTALLATION_LABEL}={}",
        kftray_commons::utils::config_dir::installation_id().await?
    ))
}

pub(crate) async fn tag_installation(
    labels: &mut Option<std::collections::BTreeMap<String, String>>,
) -> Result<(), String> {
    labels
        .get_or_insert_with(std::collections::BTreeMap::new)
        .insert(
            INSTALLATION_LABEL.to_owned(),
            kftray_commons::utils::config_dir::installation_id()
                .await?
                .to_owned(),
        );
    Ok(())
}

#[derive(Clone, Copy)]
struct ProxyStartOptions<'a> {
    mode: DatabaseMode,
    ssl_override: bool,
    cancellation: &'a CancellationToken,
}

pub async fn deploy_and_forward_pod(configs: Vec<Config>) -> Result<Vec<CustomResponse>, String> {
    deploy_and_forward_pod_with_mode(configs, DatabaseMode::File, false).await
}

pub(super) type RegisteredBatch = (Vec<(Config, PendingStart)>, Vec<(Config, String)>);

/// Registers every config in [`STARTING_PROXIES`] before any work is buffered.
/// Registration has to be eager: `buffer_unordered` only polls a window of the
/// batch, and a stop-all that snapshots the map while the tail is still
/// unpolled would let those configs start after the snapshot.
pub(super) fn register_start_batch(configs: Vec<Config>) -> RegisteredBatch {
    let mut queued = Vec::new();
    let mut rejected = Vec::new();
    for config in configs {
        match config.id.ok_or_else(|| "Config has no ID".to_string()) {
            Ok(id) => match PendingStart::new(id) {
                Ok(startup) => queued.push((config, startup)),
                Err(error) => rejected.push((config, error)),
            },
            Err(error) => rejected.push((config, error)),
        }
    }
    (queued, rejected)
}

pub async fn deploy_and_forward_pod_with_mode(
    configs: Vec<Config>, mode: DatabaseMode, ssl_override: bool,
) -> Result<Vec<CustomResponse>, String> {
    let (queued, rejected) = register_start_batch(configs);

    let mut responses: Vec<CustomResponse> = stream::iter(queued)
        .map(|(config, startup)| async move {
            match process_single_proxy_config(config.clone(), startup, mode, ssl_override).await {
                Ok(response) => response,
                Err(error) => super::start::start_failure_response(&config, error),
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;
    responses.extend(
        rejected
            .into_iter()
            .map(|(config, error)| super::start::start_failure_response(&config, error)),
    );
    Ok(responses)
}

async fn process_single_proxy_config(
    config: Config, startup: PendingStart, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, String> {
    let id = startup.id;

    let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
    let guard = tokio::select! {
        biased;
        _ = startup.cancellation.cancelled() => {
            return Err(format!("Proxy startup cancelled for config {id}"));
        }
        guard = lock.lock() => guard,
    };
    let result = if crate::port_forward::CHILD_PROCESSES.contains_key(&id) {
        Err(format!(
            "Port forwarding is already running for config {id}"
        ))
    } else {
        start_proxy_config(config, mode, ssl_override, &startup.cancellation).await
    };
    drop(guard);
    drop(lock);
    result
}

pub(super) async fn start_proxy_config(
    mut config: Config, mode: DatabaseMode, ssl_override: bool, cancellation: &CancellationToken,
) -> Result<CustomResponse, String> {
    let protocol = config.protocol.to_ascii_lowercase();
    if !matches!(protocol.as_str(), "tcp" | "udp") {
        return Err(format!("Unsupported proxy protocol: {protocol}"));
    }
    let client_key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());

    let shared_client = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err("Proxy startup cancelled".to_string()),
        result = SHARED_CLIENT_MANAGER.get_connection(client_key) => result.map_err(|e| {
            error!("Failed to get shared Kubernetes client: {e}");
            e.to_string()
        })?,
    };
    let client = shared_client.client.clone();

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();

    let random_string: String = Alphanumeric
        .sample_string(&mut rand::rng(), 6)
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .collect();

    let hashed_name = format!(
        "{}{protocol}-{timestamp}-{random_string}",
        proxy_resource_prefix()
    )
    .to_lowercase();

    let config_id_str = config
        .id
        .map_or_else(|| "default".into(), |id| id.to_string());

    let remote_address = config
        .remote_address
        .take()
        .filter(|address| !address.is_empty())
        .or_else(|| config.service.clone().filter(|service| !service.is_empty()))
        .ok_or("A proxy destination address or service is required")?;
    let remote_port = config
        .remote_port
        .filter(|port| *port > 0)
        .ok_or("A proxy destination port is required")?;
    let service_name = config
        .service
        .clone()
        .filter(|service| !service.is_empty())
        .unwrap_or_else(|| remote_address.clone());
    config.remote_address = Some(remote_address.clone());

    let mut values: HashMap<String, String> = HashMap::new();
    values.insert("hashed_name".to_string(), hashed_name.clone());
    values.insert("config_id".to_string(), config_id_str.clone());
    values.insert("service_name".to_string(), service_name);
    values.insert("remote_address".to_string(), remote_address);
    values.insert("remote_port".to_string(), remote_port.to_string());
    values.insert("local_port".to_string(), remote_port.to_string());
    values.insert("protocol".to_string(), protocol.clone());

    let use_deployment = should_use_deployment_manifest();
    let options = ProxyStartOptions {
        mode,
        ssl_override,
        cancellation,
    };

    if use_deployment {
        process_deployment_proxy(
            client,
            &mut config,
            &hashed_name,
            &config_id_str,
            &values,
            &protocol,
            options,
        )
        .await
    } else {
        process_pod_proxy(
            client,
            &mut config,
            &hashed_name,
            &values,
            &protocol,
            options,
        )
        .await
    }
}

fn relay_container_index(spec: &PodSpec) -> usize {
    spec.containers
        .iter()
        .position(|container| {
            container
                .env
                .as_ref()
                .is_some_and(|env| env.iter().any(|variable| variable.name == "LOCAL_PORT"))
        })
        .unwrap_or(0)
}

pub(crate) fn relay_container_name(spec: &PodSpec) -> Option<String> {
    spec.containers
        .get(relay_container_index(spec))
        .map(|container| container.name.clone())
}

/// Adds a startup probe to the relay container and returns its name.
///
/// A customized manifest is left alone: the probe is a kubelet TCP check
/// against the pod IP, and a custom relay may only listen on loopback inside
/// the pod, which the port forward can still reach but the probe cannot.
fn prepare_relay_startup(
    spec: &mut PodSpec, port: u16, customized: bool,
) -> Result<String, String> {
    let index = relay_container_index(spec);
    let container = spec
        .containers
        .get_mut(index)
        .ok_or("Proxy manifest must contain a container")?;
    if customized {
        return Ok(container.name.clone());
    }
    container.startup_probe.get_or_insert_with(|| Probe {
        tcp_socket: Some(TCPSocketAction {
            port: IntOrString::Int(i32::from(port)),
            ..Default::default()
        }),
        period_seconds: Some(1),
        timeout_seconds: Some(1),
        failure_threshold: Some(30),
        ..Default::default()
    });
    Ok(container.name.clone())
}

pub(crate) fn relay_started(pod: Option<&Pod>, container_name: &str) -> bool {
    pod.is_some_and(|pod| {
        pod.metadata.deletion_timestamp.is_none()
            && pod.status.as_ref().is_some_and(|status| {
                status.phase.as_deref() == Some("Running")
                    && status
                        .container_statuses
                        .as_ref()
                        .is_some_and(|containers| {
                            containers.iter().any(|container| {
                                container.name == container_name && container.started == Some(true)
                            })
                        })
            })
    })
}

async fn wait_for_relay_startup(
    pods: &Api<Pod>, pod_name: &str, container_name: &str, cancellation: &CancellationToken,
) -> Result<(), String> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err("Proxy startup cancelled".to_string()),
        result = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            kube_runtime::wait::await_condition(pods.clone(), pod_name, |pod: Option<&Pod>| {
                relay_started(pod, container_name)
            }),
        ) => result
            .map_err(|_| format!("Timed out waiting for proxy listener in pod {pod_name}"))?
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}

async fn process_deployment_proxy(
    client: Client, config: &mut Config, hashed_name: &str, config_id_str: &str,
    values: &HashMap<String, String>, protocol: &str, options: ProxyStartOptions<'_>,
) -> Result<CustomResponse, String> {
    let manifest_path = get_proxy_deployment_manifest_path().map_err(|e| e.to_string())?;
    let mut file = File::open(manifest_path).map_err(|e| e.to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;

    let rendered_json = render_json_template_owned(&contents, values);
    let mut deployment: Deployment =
        serde_json::from_str(&rendered_json).map_err(|e| e.to_string())?;
    tag_installation(&mut deployment.metadata.labels).await?;
    if let Some(spec) = deployment.spec.as_mut() {
        spec.selector
            .match_labels
            .get_or_insert_with(std::collections::BTreeMap::new)
            .insert(
                INSTALLATION_LABEL.to_owned(),
                kftray_commons::utils::config_dir::installation_id()
                    .await?
                    .to_owned(),
            );
        tag_installation(&mut spec.template.metadata.get_or_insert_default().labels).await?;
    }
    let spec = deployment
        .spec
        .as_mut()
        .and_then(|spec| spec.template.spec.as_mut())
        .ok_or("Proxy deployment must contain a pod specification")?;
    let container_name = prepare_relay_startup(
        spec,
        config
            .remote_port
            .ok_or("A proxy destination port is required")?,
        kftray_commons::utils::manifests::deployment_manifest_is_customized(),
    )?;
    if options.cancellation.is_cancelled() {
        return Err("Proxy startup cancelled".to_string());
    }

    let deployments: Api<Deployment> = Api::namespaced(client.clone(), &config.namespace);

    // Armed before the request: the API server can create the resource and
    // still leave us without a response, and a dropped startup future never
    // reaches the rollback below.
    let mut guard = crate::kube::stop::ClusterResourceGuard::arm(
        config.id.unwrap_or_default(),
        Config {
            service: Some(hashed_name.to_string()),
            ..config.clone()
        },
    )
    .await;
    // Deliberately not raced against cancellation: abandoning a create in
    // flight leaves an unknown outcome, and a cleanup pass that lists before
    // the object is persisted would forget it. Bounded instead, so a stalled
    // request still releases the lifecycle lock.
    match create_proxy_resource(&deployments, &deployment).await {
        CreateOutcome::Settled(Ok(())) => guard.confirm(),
        // A definitive rejection means nothing was created, so there is nothing
        // for a later cleanup pass to find.
        CreateOutcome::Settled(Err(error)) => {
            guard.disarm().await;
            return Err(error);
        }
        // No answer: the object may still appear, so the record stays uncertain
        // and a later cleanup pass retries it.
        CreateOutcome::Unknown(error) => return Err(error),
    }
    let result: Result<CustomResponse, String> = async {
        let pods: Api<Pod> = Api::namespaced(client, &config.namespace);
        let label_selector = format!(
            "app={hashed_name},{}",
            proxy_owner_selector(config_id_str).await?
        );
        wait_for_relay_pod(
            &pods,
            &label_selector,
            &container_name,
            options.cancellation,
        )
        .await?;
        config.service = Some(hashed_name.to_string());
        let response = super::start::start_config_cancellable(
            config.clone(),
            protocol,
            options.mode,
            options.ssl_override,
            Some(options.cancellation),
        )
        .await
        .map_err(|error| format!("Failed to start port forwarding: {error}"))?;
        crate::kube::proxy_recovery::spawn_recovery_manager(
            config.clone(),
            crate::kube::proxy_recovery::ProxyType::Deployment,
            options.mode,
            options.ssl_override,
        );
        Ok(response)
    }
    .await;
    if let Err(error) = result {
        match delete_proxy_resource(&deployments, hashed_name).await {
            Ok(()) => guard.disarm().await,
            Err(cleanup) => {
                // The guard stays armed so stop-all retries this deletion.
                return Err(format!(
                    "{error}; failed to delete proxy deployment: {cleanup}"
                ));
            }
        }
        return Err(error);
    }
    guard.disarm().await;
    result
}

/// The outcome of a create request.
enum CreateOutcome {
    /// The API server answered: the resource exists, or it definitively
    /// rejected the request.
    Settled(Result<(), String>),
    /// The client stopped waiting, or the transport failed. The API server may
    /// still persist the object, so the cleanup record must stay uncertain.
    Unknown(String),
}

/// Creates one proxy resource under a deadline.
async fn create_proxy_resource<K>(api: &Api<K>, resource: &K) -> CreateOutcome
where
    K: Clone + serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug,
{
    const CREATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    match tokio::time::timeout(CREATE_TIMEOUT, api.create(&PostParams::default(), resource)).await {
        Ok(Ok(_)) => CreateOutcome::Settled(Ok(())),
        // Only a definitive rejection proves the object was not created. A
        // server-side timeout or an unavailable API server can be answered
        // while the request is still being applied.
        Ok(Err(kube::Error::Api(response))) => {
            if matches!(response.code, 408 | 429 | 500 | 502 | 503 | 504) {
                CreateOutcome::Unknown(response.message.clone())
            } else {
                CreateOutcome::Settled(Err(response.message.clone()))
            }
        }
        Ok(Err(error)) => CreateOutcome::Unknown(error.to_string()),
        Err(_) => CreateOutcome::Unknown("Timed out creating the proxy resource".to_string()),
    }
}

/// Deletes one proxy resource under a deadline. Rollback must not inherit the
/// stall that caused the failure: the lifecycle lock is still held and a stop
/// is waiting on it.
async fn delete_proxy_resource<K>(api: &Api<K>, name: &str) -> Result<(), String>
where
    K: Clone + serde::de::DeserializeOwned + std::fmt::Debug,
{
    const ROLLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

    let dp = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::default()
    };
    match tokio::time::timeout(ROLLBACK_TIMEOUT, api.delete(name, &dp)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(kube::Error::Api(response))) if response.code == 404 => Ok(()),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err(format!("timed out deleting {name}")),
    }
}

async fn wait_for_relay_pod(
    pods: &Api<Pod>, label_selector: &str, container_name: &str, cancellation: &CancellationToken,
) -> Result<String, String> {
    let watcher = kube_runtime::watcher(
        pods.clone(),
        kube_runtime::watcher::Config::default().labels(label_selector),
    )
    .applied_objects();
    futures::pin_mut!(watcher);
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err("Proxy startup cancelled".to_string()),
        result = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            async {
                while let Some(pod) = watcher.try_next().await.map_err(|error| error.to_string())? {
                    if relay_started(Some(&pod), container_name) {
                        return pod
                            .metadata
                            .name
                            .ok_or_else(|| "Proxy pod has no name".to_string());
                    }
                }
                Err("Proxy pod watch ended before the relay started".to_string())
            },
        ) => result,
    };
    match result {
        Ok(result) => result,
        Err(_) => Err("Timed out waiting for the proxy deployment relay to start".to_string()),
    }
}

async fn process_pod_proxy(
    client: Client, config: &mut Config, hashed_name: &str, values: &HashMap<String, String>,
    protocol: &str, options: ProxyStartOptions<'_>,
) -> Result<CustomResponse, String> {
    let manifest_path = get_pod_manifest_path().map_err(|e| e.to_string())?;
    let mut file = File::open(manifest_path).map_err(|e| e.to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;

    let rendered_json = render_json_template_owned(&contents, values);
    let mut pod: Pod = serde_json::from_str(&rendered_json).map_err(|e| e.to_string())?;
    tag_installation(&mut pod.metadata.labels).await?;
    let spec = pod
        .spec
        .as_mut()
        .ok_or("Proxy pod must contain a pod specification")?;
    let container_name = prepare_relay_startup(
        spec,
        config
            .remote_port
            .ok_or("A proxy destination port is required")?,
        pod_manifest_is_customized(),
    )?;
    if options.cancellation.is_cancelled() {
        return Err("Proxy startup cancelled".to_string());
    }

    let pods: Api<Pod> = Api::namespaced(client.clone(), &config.namespace);

    let mut guard = crate::kube::stop::ClusterResourceGuard::arm(
        config.id.unwrap_or_default(),
        Config {
            service: Some(hashed_name.to_string()),
            ..config.clone()
        },
    )
    .await;
    match create_proxy_resource(&pods, &pod).await {
        CreateOutcome::Settled(Ok(())) => guard.confirm(),
        CreateOutcome::Settled(Err(error)) => {
            guard.disarm().await;
            return Err(error);
        }
        CreateOutcome::Unknown(error) => return Err(error),
    }
    let result: Result<CustomResponse, String> = async {
        wait_for_relay_startup(&pods, hashed_name, &container_name, options.cancellation).await?;
        config.service = Some(hashed_name.to_string());
        let response = super::start::start_config_cancellable(
            config.clone(),
            protocol,
            options.mode,
            options.ssl_override,
            Some(options.cancellation),
        )
        .await
        .map_err(|error| format!("Failed to start port forwarding: {error}"))?;
        crate::kube::proxy_recovery::spawn_recovery_manager(
            config.clone(),
            crate::kube::proxy_recovery::ProxyType::BarePod,
            options.mode,
            options.ssl_override,
        );
        Ok(response)
    }
    .await;
    if let Err(error) = result {
        match delete_proxy_resource(&pods, hashed_name).await {
            Ok(()) => guard.disarm().await,
            Err(cleanup) => {
                // The guard stays armed so stop-all retries this deletion.
                return Err(format!("{error}; failed to delete proxy pod: {cleanup}"));
            }
        }
        return Err(error);
    }
    guard.disarm().await;
    result
}

pub async fn stop_proxy_forward_with_mode(
    config_id: i64, _namespace: &str, service_name: String,
    mode: kftray_commons::utils::db_mode::DatabaseMode,
) -> Result<CustomResponse, String> {
    info!("Stopping proxy forward for service: {service_name}");
    super::stop::stop_port_forward_with_mode(config_id.to_string(), mode)
        .await
        .map_err(|e| {
            error!("Failed to stop port forwarding for service '{service_name}': {e}");
            e
        })
}

pub async fn stop_proxy_forward(
    config_id: i64, _namespace: &str, service_name: String,
) -> Result<CustomResponse, String> {
    info!("Stopping proxy forward for service: {service_name}");
    super::stop::stop_port_forward_with_mode(
        config_id.to_string(),
        kftray_commons::utils::db_mode::DatabaseMode::File,
    )
    .await
    .map_err(|e| {
        error!("Failed to stop port forwarding for service '{service_name}': {e}");
        e
    })
}

fn should_use_deployment_manifest() -> bool {
    if pod_manifest_is_customized() {
        info!("Using legacy Pod manifest (custom detected)");
        return false;
    }

    if proxy_deployment_manifest_exists() {
        info!("Using new Deployment manifest");
        return true;
    }

    info!("Using legacy Pod manifest (Deployment not available)");
    false
}

fn render_json_template_owned(template: &str, values: &HashMap<String, String>) -> String {
    let mut rendered_template = template.to_string();

    for (key, value) in values.iter() {
        rendered_template = rendered_template.replace(&format!("{{{key}}}"), value);
    }

    rendered_template
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use kftray_commons::models::config_model::Config;

    use super::*;

    #[test]
    fn every_batch_config_registers_before_any_is_buffered() {
        let ids: Vec<i64> = (930_100..930_130).collect();
        let configs: Vec<Config> = ids
            .iter()
            .map(|id| Config {
                id: Some(*id),
                ..Default::default()
            })
            .collect();

        let (queued, errors) = register_start_batch(configs);

        assert!(errors.is_empty());
        assert_eq!(queued.len(), ids.len());
        for id in &ids {
            assert!(
                STARTING_PROXIES.contains_key(id),
                "config {id} must be cancellable by stop-all before its turn in the buffer"
            );
        }
        drop(queued);
        for id in &ids {
            assert!(!STARTING_PROXIES.contains_key(id));
        }
    }

    #[test]
    fn test_render_json_template_owned() {
        let template = r#"{
            "name": "{hashed_name}",
            "config_id": "{config_id}",
            "service": "{service_name}",
            "port": {remote_port}
        }"#;

        let mut values = HashMap::new();
        values.insert("hashed_name".to_string(), "test-pod".to_string());
        values.insert("config_id".to_string(), "123".to_string());
        values.insert("service_name".to_string(), "test-service".to_string());
        values.insert("remote_port".to_string(), "8080".to_string());

        let rendered = render_json_template_owned(template, &values);

        assert!(rendered.contains("\"name\": \"test-pod\""));
        assert!(rendered.contains("\"config_id\": \"123\""));
        assert!(rendered.contains("\"service\": \"test-service\""));
        assert!(rendered.contains("\"port\": 8080"));
    }

    #[test]
    fn test_render_json_template_owned_with_missing_values() {
        let template = r#"{
            "name": "{hashed_name}",
            "config_id": "{config_id}",
            "missing": "{missing_value}"
        }"#;

        let mut values = HashMap::new();
        values.insert("hashed_name".to_string(), "test-pod".to_string());
        values.insert("config_id".to_string(), "123".to_string());

        let rendered = render_json_template_owned(template, &values);

        assert!(rendered.contains("\"name\": \"test-pod\""));
        assert!(rendered.contains("\"config_id\": \"123\""));
        assert!(rendered.contains("\"missing\": \"{missing_value}\""));
    }

    #[test]
    fn test_render_json_template_owned_with_empty_values() {
        let template = r#"{"name": "{hashed_name}"}"#;
        let values = HashMap::new();

        let rendered = render_json_template_owned(template, &values);
        assert_eq!(rendered, r#"{"name": "{hashed_name}"}"#);
    }

    #[test]
    fn test_render_json_template_owned_complex() {
        let template = r#"{
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": "{hashed_name}",
                "labels": {
                    "app": "kftray-forward",
                    "config_id": "{config_id}"
                }
            },
            "spec": {
                "containers": [
                    {
                        "name": "proxy",
                        "image": "alpine:latest",
                        "command": ["/bin/sh"],
                        "args": ["-c", "while true; do sleep 60; done"],
                        "ports": [
                            {
                                "containerPort": {remote_port},
                                "protocol": "{protocol}"
                            }
                        ]
                    }
                ]
            }
        }"#;

        let mut values = HashMap::new();
        values.insert("hashed_name".to_string(), "test-pod-abc123".to_string());
        values.insert("config_id".to_string(), "456".to_string());
        values.insert("remote_port".to_string(), "9090".to_string());
        values.insert("protocol".to_string(), "TCP".to_string());

        let rendered = render_json_template_owned(template, &values);

        assert!(rendered.contains("\"name\": \"test-pod-abc123\""));
        assert!(rendered.contains("\"config_id\": \"456\""));
        assert!(rendered.contains("\"containerPort\": 9090"));
        assert!(rendered.contains("\"protocol\": \"TCP\""));
    }

    #[tokio::test]
    async fn test_deploy_and_forward_pod_empty_config() {
        let configs = Vec::new();

        let result = deploy_and_forward_pod(configs).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_deploy_and_forward_pod_invalid_kubeconfig() {
        let config = Config {
            id: Some(1),
            context: Some("invalid-context".to_string()),
            kubeconfig: Some("invalid-kubeconfig".to_string()),
            namespace: "default".to_string(),
            service: Some("test-service".to_string()),
            alias: None,
            local_port: Some(8080),
            remote_port: Some(8080),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            target: None,
            local_address: None,
            auto_loopback_address: false,
            remote_address: None,
            domain_enabled: None,
            http_logs_enabled: Some(false),
            http_logs_max_file_size: Some(10 * 1024 * 1024),
            http_logs_retention_days: Some(7),
            http_logs_auto_cleanup: Some(true),
            exposure_type: None,
            cert_manager_enabled: None,
            cert_issuer: None,
            cert_issuer_kind: None,
            ingress_class: None,
            ingress_annotations: None,
        };

        let responses = deploy_and_forward_pod(vec![config]).await.unwrap();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].id, Some(1));
        assert_ne!(responses[0].status, 0);
        assert!(!responses[0].stderr.is_empty());
    }

    #[test]
    fn running_relay_waits_for_listener_startup() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "relay"},
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "name": "relay", "started": false, "ready": false,
                    "restartCount": 0, "image": "relay", "imageID": "relay"
                }]
            }
        }))
        .unwrap();
        assert!(!relay_started(Some(&pod), "relay"));
    }

    #[test]
    fn started_relay_does_not_require_sidecar_readiness() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "relay"},
            "status": {
                "phase": "Running",
                "conditions": [{"type": "Ready", "status": "False"}],
                "containerStatuses": [
                    {"name": "relay", "started": true, "ready": true,
                     "restartCount": 0, "image": "relay", "imageID": "relay"},
                    {"name": "sidecar", "started": true, "ready": false,
                     "restartCount": 0, "image": "sidecar", "imageID": "sidecar"}
                ]
            }
        }))
        .unwrap();
        assert!(relay_started(Some(&pod), "relay"));
    }

    #[tokio::test]
    async fn test_stop_proxy_forward_invalid_config() {
        let result = stop_proxy_forward(999, "default", "nonexistent-service".to_string()).await;
        assert!(result.is_err());
    }

    async fn assert_startup_wait_is_cancelled(id: i64, listener_wait: bool) {
        let _isolation = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let startup = PendingStart::new(id).unwrap();
        let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
        let guard = lock.lock_owned().await;
        let (service, mut requests) = tower_test::mock::pair::<
            http::Request<kube::client::Body>,
            http::Response<kube::client::Body>,
        >();
        let pods = Api::namespaced(Client::new(service, "default"), "default");
        let waiter = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let _guard = guard;
            if listener_wait {
                wait_for_relay_startup(&pods, "relay", "relay", &startup.cancellation).await
            } else {
                wait_for_relay_pod(&pods, "app=relay", "relay", &startup.cancellation)
                    .await
                    .map(|_| ())
            }
        }));
        let (_request, _pending_response) = requests.next_request().await.unwrap();
        if listener_wait {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                crate::kube::stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory),
            )
            .await
            .unwrap();
            assert!(result.is_err());
        } else {
            let responses = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                crate::kube::stop_all_port_forward_with_mode(DatabaseMode::Memory),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(responses.iter().any(|response| response.id == Some(id)));
        }
        assert!(waiter.await.unwrap().is_err());
        assert!(!STARTING_PROXIES.contains_key(&id));
    }

    #[tokio::test]
    async fn stop_all_cancels_proxy_pod_discovery() {
        assert_startup_wait_is_cancelled(420_001, false).await;
    }

    #[tokio::test]
    async fn stop_cancels_proxy_listener_readiness() {
        assert_startup_wait_is_cancelled(420_002, true).await;
    }

    #[tokio::test]
    async fn relay_discovery_skips_pods_whose_relay_has_not_started() {
        let (service, mut requests) = tower_test::mock::pair::<
            http::Request<kube::client::Body>,
            http::Response<kube::client::Body>,
        >();
        let pods = Api::namespaced(Client::new(service, "default"), "default");
        let started = serde_json::json!({
            "phase": "Running",
            "containerStatuses": [{
                "name": "relay", "started": true, "ready": true,
                "restartCount": 0, "image": "relay", "imageID": "relay"
            }]
        });
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let (_, send) = requests.next_request().await.unwrap();
            send.send_response(http::Response::builder().body(kube::client::Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "metadata":{"resourceVersion":"1"},
                    "items":[
                        {"metadata":{"name":"terminating-pod","deletionTimestamp":"2026-09-11T00:00:00Z"},
                         "status": started},
                        {"metadata":{"name":"scheduling-pod"},"status":{"phase":"Pending"}},
                        {"metadata":{"name":"replacement-pod"},"status": started}
                    ]
                })).unwrap()
            )).unwrap());
        }));
        let cancellation = CancellationToken::new();
        let selected = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            wait_for_relay_pod(&pods, "app=relay", "relay", &cancellation),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(selected, "replacement-pod");
        server.await.unwrap();
    }
}
