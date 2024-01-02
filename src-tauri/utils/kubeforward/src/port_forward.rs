use anyhow::Context;
use futures::TryStreamExt;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use tokio_stream::wrappers::TcpListenerStream;

use crate::{
    pod_selection::{AnyReady, PodSelection},
    vx::{Pod, Service},
};
use kube::{
    api::{Api, DeleteParams, ListParams},
    Client,
};

#[derive(Clone)]
#[allow(dead_code)]
pub struct PortForward {
    target: crate::Target,
    local_port: Option<u16>,
    pod_api: Api<Pod>,
    svc_api: Api<Service>,
    context_name: Option<String>,
}

impl PortForward {
    pub async fn new(
        target: crate::Target,
        local_port: impl Into<Option<u16>>,
        context_name: Option<String>,
    ) -> anyhow::Result<Self> {
        // Check if context_name was provided and create a Kubernetes client
        let client = if let Some(ref context_name) = context_name {
            crate::kubecontext::create_client_with_specific_context(context_name).await?
        } else {
            // Use default context (or whatever client creation logic you prefer)
            Client::try_default().await?
        };
        let namespace = target.namespace.name_any();

        Ok(Self {
            target,
            local_port: local_port.into(),
            pod_api: Api::namespaced(client.clone(), &namespace),
            svc_api: Api::namespaced(client, &namespace),
            context_name, // Store the context name if provided
        })
    }

    fn local_port(&self) -> u16 {
        self.local_port.unwrap_or(0)
    }

    /// Runs the port forwarding proxy until a SIGINT signal is received.
    pub async fn port_forward(self) -> anyhow::Result<(u16, tokio::task::JoinHandle<()>)> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.local_port()));

        let bind = TcpListener::bind(addr).await?;
        let port = bind.local_addr()?.port();
        tracing::trace!(port, "Bound to local port");

        let server = TcpListenerStream::new(bind).try_for_each(move |client_conn| {
            let pf = self.clone();

            async {
                let client_conn = client_conn;
                if let Ok(peer_addr) = client_conn.peer_addr() {
                    tracing::trace!(%peer_addr, "new connection");
                }

                tokio::spawn(async move {
                    if let Err(e) = pf.forward_connection(client_conn).await {
                        tracing::error!(
                            error = e.as_ref() as &dyn std::error::Error,
                            "failed to forward connection"
                        );
                    }
                });

                Ok(())
            }
        });

        Ok((
            port,
            tokio::spawn(async {
                if let Err(e) = server.await {
                    tracing::error!(error = &e as &dyn std::error::Error, "server error");
                }
            }),
        ))
    }
    async fn forward_connection(
        self,
        mut client_conn: tokio::net::TcpStream,
    ) -> anyhow::Result<()> {
        let target = self.finder().find(&self.target).await?;
        let (pod_name, pod_port) = target.into_parts();

        let mut forwarder = self.pod_api.portforward(&pod_name, &[pod_port]).await?;

        let mut upstream_conn = forwarder
            .take_stream(pod_port)
            .context("port not found in forwarder")?;

        let local_port = self.local_port();

        tracing::debug!(local_port, pod_port, pod_name, "forwarding connections");

        if let Err(error) =
            tokio::io::copy_bidirectional(&mut client_conn, &mut upstream_conn).await
        {
            tracing::trace!(local_port, pod_port, pod_name, ?error, "connection error");
        }

        drop(upstream_conn);
        forwarder.join().await?;
        tracing::debug!(local_port, pod_port, pod_name, "connection closed");
        Ok(())
    }
    fn finder(&self) -> TargetPodFinder {
        TargetPodFinder {
            pod_api: &self.pod_api,
            svc_api: &self.svc_api,
        }
    }
}

#[derive(Clone)]
struct TargetPodFinder<'a> {
    pod_api: &'a Api<Pod>,
    svc_api: &'a Api<Service>,
}
impl<'a> TargetPodFinder<'a> {
    pub(crate) async fn find(&self, target: &crate::Target) -> anyhow::Result<crate::TargetPod> {
        let ready_pod = AnyReady {};
        match &target.selector {
            crate::TargetSelector::ServiceName(name) => match self.svc_api.get(name).await {
                Ok(service) => {
                    if let Some(selector) = service.spec.and_then(|spec| spec.selector) {
                        let label_selector_str = selector
                            .iter()
                            .map(|(key, value)| format!("{}={}", key, value))
                            .collect::<Vec<_>>()
                            .join(",");

                        let pods = self
                            .pod_api
                            .list(&ListParams::default().labels(&label_selector_str))
                            .await?;
                        let pod = ready_pod.select(&pods.items, &label_selector_str)?;
                        target.find(pod, None)
                    } else {
                        Err(anyhow::anyhow!("No selector found for service '{}'", name))
                    }
                }
                Err(kube::Error::Api(kube::error::ErrorResponse { code: 404, .. })) => {
                    let label_selector_str = format!("app={}", name);
                    let pods = self
                        .pod_api
                        .list(&ListParams::default().labels(&label_selector_str))
                        .await?;
                    let pod = ready_pod.select(&pods.items, &label_selector_str)?;
                    target.find(pod, None)
                }
                Err(e) => Err(anyhow::anyhow!("Error finding service '{}': {}", name, e)),
            },
        }
    }
}

lazy_static! {
    pub static ref CHILD_PROCESSES: Arc<Mutex<HashMap<String, JoinHandle<()>>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

#[derive(serde::Serialize)]
pub struct CustomResponse {
    id: Option<i64>,
    service: String,
    namespace: String,
    local_port: u16,
    remote_port: u16,
    context: String,
    stdout: String,
    stderr: String,
    status: i32,
}

impl CustomResponse {
    pub fn new(
        id: Option<i64>,
        service: String,
        namespace: String,
        local_port: u16,
        remote_port: u16,
        context: String,
        stdout: String,
        stderr: String,
        status: i32,
    ) -> Self {
        CustomResponse {
            id,
            service,
            namespace,
            local_port,
            remote_port,
            context,
            stdout,
            stderr,
            status,
        }
    }
}

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct Config {
    pub id: Option<i64>,
    pub service: String,
    pub namespace: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub context: String,
    pub workload_type: String,
    pub remote_address: Option<String>,
    pub alias: Option<String>,
}

#[tauri::command]
pub async fn start_port_forward(configs: Vec<Config>) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();

    for config in configs {
        let selector = crate::TargetSelector::ServiceName(config.service.clone());
        let remote_port = crate::Port::from(config.remote_port as i32);
        let context_name = Some(config.context.clone());
        log::info!("Remote Port: {}", config.remote_port);
        log::info!("Local Port: {}", config.remote_port);

        let namespace = config.namespace.clone();
        let target = crate::Target::new(selector, remote_port, namespace);

        log::debug!("Attempting to forward to service: {}", &config.service);
        let port_forward = PortForward::new(target, config.local_port, context_name)
            .await
            .map_err(|e| {
                log::error!("Failed to create PortForward: {}", e);
                e.to_string()
            })?;

        let (actual_local_port, handle) = port_forward.port_forward().await.map_err(|e| {
            log::error!("Failed to start port forwarding: {}", e);
            e.to_string()
        })?;

        log::info!(
            "Port forwarding is set up on local port: {} for service: {}",
            actual_local_port,
            &config.service
        );
        println!(
            "Port forwarding is set up on local port: {} for service: {} for config_id: {}",
            actual_local_port,
            &config.service,
            config.id.unwrap()
        );
        // Store the JoinHandle to the global child processes map.
        CHILD_PROCESSES
            .lock()
            .unwrap()
            .insert(config.id.unwrap().to_string(), handle);

        // Append a new CustomResponse to responses collection.
        responses.push(CustomResponse {
            id: config.id,
            service: config.service.clone(),
            namespace: config.namespace, // Safe to use here as we cloned before
            local_port: actual_local_port,
            remote_port: config.remote_port,
            context: config.context.clone(),
            stdout: format!(
                "Forwarding from 127.0.0.1:{} -> {}:{}",
                actual_local_port, config.remote_port, config.service
            ),
            stderr: String::new(),
            status: 0,
        });
    }

    if !responses.is_empty() {
        log::info!("Port forwarding responses generated successfully.");
    }

    Ok(responses)
}

#[tauri::command]
pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    log::info!("Attempting to stop all port forwards");
    let mut responses = Vec::new();
    let client = Client::try_default().await.map_err(|e| {
        log::error!("Failed to create Kubernetes client: {}", e);
        e.to_string()
    })?;

    let handle_map: HashMap<String, tokio::task::JoinHandle<()>> =
        { CHILD_PROCESSES.lock().unwrap().drain().collect() };

    for (config_id, handle) in handle_map.iter() {
        log::info!("Aborting port forwarding task for config_id: {}", config_id);
        handle.abort();
    }

    let pods: Api<Pod> = Api::all(client.clone());
    for config_id in handle_map.keys() {
        log::info!("Fetching pods for config_id: {}", config_id);
        let lp = ListParams::default().labels(&format!("config_id={}", config_id));
        let pod_list = match pods.list(&lp).await {
            Ok(pods) => pods,
            Err(e) => {
                log::error!("Error fetching pods for config_id {}: {}", config_id, e);
                continue;
            }
        };

        log::info!(
            "Found {} pods for config_id: {}",
            pod_list.items.len(),
            config_id
        );
        let username = whoami::username();
        for pod in pod_list.items {
            if let Some(pod_name) = pod.metadata.name.clone() {
                if pod_name.starts_with(&format!("kftray-forward-{}", username)) {
                    log::info!("Deleting pod: {}", pod_name);
                    let namespace = pod.metadata.namespace.as_deref().unwrap_or_default();
                    let namespaced_pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

                    let dp = DeleteParams {
                        grace_period_seconds: Some(0),
                        propagation_policy: Some(kube::api::PropagationPolicy::Background),
                        ..Default::default()
                    };

                    match namespaced_pods.delete(&pod_name, &dp).await {
                        Ok(_) => {
                            log::info!("Successfully deleted pod: {}", pod_name);
                            responses.push(CustomResponse::new(
                                config_id.parse().ok(),
                                pod_name.clone(),
                                namespace.to_string(),
                                0,
                                0,
                                String::new(),
                                format!("Deleted pod {}", pod_name),
                                String::new(),
                                0,
                            ));
                        }
                        Err(e) => {
                            log::error!("Failed to delete pod {}: {}", pod_name, e);
                            responses.push(CustomResponse::new(
                                config_id.parse().ok(),
                                pod_name.clone(),
                                namespace.to_string(),
                                0,
                                0,
                                String::new(),
                                format!("Failed to delete pod {}", pod_name),
                                e.to_string(),
                                1,
                            ));
                        }
                    }
                } else {
                    log::info!(
                        "Pod {} does not match the username prefix, skipping",
                        pod_name
                    );
                }
            }
        }
    }

    log::info!(
        "Port forward stopping process completed with {} responses",
        responses.len()
    );
    Ok(responses)
}

#[tauri::command]
pub async fn stop_port_forward(
    service_name: String,
    config_id: String,
) -> Result<CustomResponse, String> {
    let mut child_processes = CHILD_PROCESSES.lock().unwrap();
    println!(
        "Attempting to stop port forwarding for service: {} and config_id: {}",
        service_name, config_id
    );
    if let Some(handle) = child_processes.remove(&config_id) {
        handle.abort();

        Ok(CustomResponse {
            id: None,
            service: service_name,
            namespace: String::new(),
            local_port: 0,
            remote_port: 0,
            context: String::new(),
            stdout: String::from("Service port forwarding has been stopped"),
            stderr: String::new(),
            status: 0,
        })
    } else {
        Err(format!(
            "No port forwarding process found for service '{}'",
            service_name
        ))
    }
}
