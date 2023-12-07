

use anyhow::Context;
use futures::TryStreamExt;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use lazy_static::lazy_static;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::task::JoinHandle;

use crate::{
    pod_selection::{AnyReady, PodSelection},
    vx::{Pod, Service},
};
use kube::{
    api::{Api, ListParams},
    Client,
	ResourceExt
};


#[derive(Clone)]
pub struct PortForward {
    target: crate::Target,
    local_port: Option<u16>,
    pod_api: Api<Pod>,
    svc_api: Api<Service>,
}

impl PortForward {
    pub async fn new(
        target: crate::Target,
        local_port: impl Into<Option<u16>>,
    ) -> anyhow::Result<Self> {
        let client = Client::try_default().await?;
        let namespace = target.namespace.name_any();

        Ok(Self {
            target,
            local_port: local_port.into(),
            pod_api: Api::namespaced(client.clone(), &namespace),
            svc_api: Api::namespaced(client, &namespace),
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

        let server = TcpListenerStream::new(bind)
            .try_for_each(move |client_conn| {
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
        let pod_api = self.pod_api;
        let svc_api = self.svc_api;
        let ready_pod = AnyReady {};

        match &target.selector {
			crate::TargetSelector::ServiceName(name) => {
				let services = svc_api.list(&ListParams::default()).await?;
				let service = services
					.items
					.into_iter()
					.find(|s| s.name_any() == *name)
					.ok_or(anyhow::anyhow!("Service '{}' not found", name))?;

				if let Some(selector) = &service.spec.as_ref().and_then(|spec| spec.selector.clone()) {
					// Convert the service's selector map into a comma-separated string of "key=value" pairs
					let label_selector_str = selector.iter()
						.map(|(key, value)| format!("{}={}", key, value))
						.collect::<Vec<_>>()
						.join(",");

					let pods = pod_api.list(&ListParams::default().labels(&label_selector_str)).await?;
					let pod = ready_pod.select(&pods.items, &label_selector_str)?;

					target.find(pod, None)
				} else {
					Err(anyhow::anyhow!("No selector found for service '{}'", name))
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

        log::info!("Remote Port: {}", config.remote_port);
		log::info!("Local Port: {}", config.remote_port);

        let namespace = config.namespace.clone();
        let target = crate::Target::new(selector, remote_port, namespace);

        log::debug!("Attempting to forward to service: {}", &config.service);
        let port_forward = PortForward::new(target, config.local_port).await.map_err(|e| {
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
        CHILD_PROCESSES.lock().unwrap().insert(config.service.clone(), handle);

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
pub async fn stop_port_forward() -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();
    // Acquire the lock and retrieve all child processes handles
    let child_processes = std::mem::take(&mut *CHILD_PROCESSES.lock().unwrap());

    // Iterate through all child process handles and abort them
    for (service, handle) in child_processes {
        handle.abort(); // Stop the port forwarding task

        // Create a response object for each service that was stopped
        responses.push(CustomResponse {
            id: None, // id is not applicable here since we're stopping services
            service,
            namespace: String::new(), // Namespace information is not available here
            local_port: 0, // Local port information is not available here
            remote_port: 0, // Remote port information is not available here
            context: String::new(), // Context information is not available here
            stdout: String::from("Port forwarding has been stopped"), // Indicate that forwarding was stopped
            stderr: String::new(), // No error message since we're stopping the service
            status: 0, // A simple status code, you might want to include more detail based on your application logic
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

