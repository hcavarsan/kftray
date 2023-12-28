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
    api::{Api, ListParams},
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
            crate::TargetSelector::ServiceName(name) => {
                match self.svc_api.get(name).await {
                    Ok(service) => {
                        if let Some(selector) = service.spec.and_then(|spec| spec.selector) {
                            let label_selector_str = selector
                                .iter()
                                .map(|(key, value)| format!("{}={}", key, value))
                                .collect::<Vec<_>>()
                                .join(",");

                            let pods = self.pod_api.list(&ListParams::default().labels(&label_selector_str)).await?;
                            let pod = ready_pod.select(&pods.items, &label_selector_str)?;
                            target.find(pod, None)
                        } else {
                            Err(anyhow::anyhow!("No selector found for service '{}'", name))
                        }
                    },
                    Err(kube::Error::Api(kube::error::ErrorResponse { code: 404, .. })) => {
                        let label_selector_str = format!("app={}", name);
                        let pods = self.pod_api.list(&ListParams::default().labels(&label_selector_str)).await?;
                        let pod = ready_pod.select(&pods.items, &label_selector_str)?;
                        target.find(pod, None)
                    },
                    Err(e) => Err(anyhow::anyhow!("Error finding service '{}': {}", name, e))
                }
            }
        }
    }
}


lazy_static! {
    static ref CHILD_PROCESSES: Arc<Mutex<HashMap<String, JoinHandle<()>>>> =
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

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct Config {
    id: Option<i64>,
    service: String,
    namespace: String,
    local_port: u16,
    remote_port: u16,
    context: String,
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

        // Store the JoinHandle to the global child processes map.
        CHILD_PROCESSES
            .lock()
            .unwrap()
            .insert(config.service.clone(), handle);

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
    let mut responses = Vec::new();
    let child_processes = std::mem::take(&mut *CHILD_PROCESSES.lock().unwrap());

    for (service, handle) in child_processes {
        handle.abort(); // Stop the port forwarding task

        responses.push(CustomResponse {
            id: None,
            service,
            namespace: String::new(),
            local_port: 0,
            remote_port: 0,
            context: String::new(),
            stdout: String::from("Port forwarding has been stopped"),
            stderr: String::new(),
            status: 0,
        });
    }

    Ok(responses)
}

#[tauri::command]
pub fn kill_all_processes() -> Result<(), String> {
    let mut child_processes = CHILD_PROCESSES.lock().unwrap();
    for (_, handle) in child_processes.drain() {
        handle.abort(); // Use abort() to cancel the running async task
    }
    Ok(())
}

#[tauri::command]
pub fn quit_app(window: tauri::Window) {
    println!("quit_app called");
    window.close().unwrap();
    let _ = kill_all_processes();
}

#[tauri::command]
pub async fn stop_port_forward(service_name: String) -> Result<CustomResponse, String> {
    let mut child_processes = CHILD_PROCESSES.lock().unwrap();

    if let Some(handle) = child_processes.remove(&service_name) {
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
