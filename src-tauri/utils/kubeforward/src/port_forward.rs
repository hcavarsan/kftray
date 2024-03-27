use anyhow::Context;
use futures::TryStreamExt;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::net::UdpSocket as TokioUdpSocket;
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

use hostsfile::HostsBuilder;

#[derive(Clone)]
#[allow(dead_code)]
pub struct PortForward {
    target: crate::Target,
    local_port: Option<u16>,
    local_address: Option<String>,
    pod_api: Api<Pod>,
    svc_api: Api<Service>,
    context_name: Option<String>,
}

impl PortForward {
    pub async fn new(
        target: crate::Target,
        local_port: impl Into<Option<u16>>,
        local_address: impl Into<Option<String>>,
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
            local_address: local_address.into(),
            pod_api: Api::namespaced(client.clone(), &namespace),
            svc_api: Api::namespaced(client, &namespace),
            context_name, // Store the context name if provided
        })
    }

    fn local_port(&self) -> u16 {
        self.local_port.unwrap_or(0)
    }

    fn local_address(&self) -> Option<String> {
        self.local_address.clone()
    }

    /// Runs the port forwarding proxy until a SIGINT signal is received.
    pub async fn port_forward(self) -> anyhow::Result<(u16, tokio::task::JoinHandle<()>)> {
        let local_addr = self
            .local_address()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let addr = format!("{}:{}", local_addr, self.local_port())
            .parse::<SocketAddr>()
            .expect("Invalid local address");

        let bind = TcpListener::bind(addr).await?;
        let port = bind.local_addr()?.port();
        tracing::trace!(port, "Bound to local address and port");

        let server = TcpListenerStream::new(bind).try_for_each(move |client_conn| {
            let pf = self.clone();

            async move {
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

    pub async fn port_forward_udp(self) -> anyhow::Result<(u16, JoinHandle<()>)> {
        let local_address = self
            .local_address()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let local_udp_addr = format!("{}:{}", local_address, self.local_port());
        let local_udp_socket = Arc::new(
            TokioUdpSocket::bind(&local_udp_addr)
                .await
                .context("Failed to bind local UDP socket")?,
        );
        let local_port = local_udp_socket.local_addr()?.port();
        tracing::info!("Local UDP socket bound to {}", local_udp_addr);

        let target = self.finder().find(&self.target).await?;
        let (pod_name, pod_port) = target.into_parts();

        // Start port forwarding to the pod via TCP
        let mut port_forwarder = self
            .pod_api
            .portforward(&pod_name, &[pod_port])
            .await
            .context("Failed to start port forwarding to pod")?;

        let (mut tcp_read, mut tcp_write) = tokio::io::split(
            port_forwarder
                .take_stream(pod_port)
                .context("port not found in forwarder")?,
        );

        let local_udp_socket_read = local_udp_socket.clone();
        let local_udp_socket_write = local_udp_socket;

        let handle = tokio::spawn(async move {
            let mut udp_buffer = [0u8; 65535]; // Maximum UDP packet size
            let mut peer: Option<std::net::SocketAddr> = None;

            loop {
                tokio::select! {
                    // Handle incoming UDP packets and forward them to the pod via TCP
                    result = local_udp_socket_read.recv_from(&mut udp_buffer) => {
                        match result {
                            Ok((len, src)) => {
                                peer = Some(src); // Store the peer address

                                // Encapsulate the UDP packet in a custom protocol for sending over TCP
                                let packet_len = (len as u32).to_be_bytes();
                                if let Err(e) = tcp_write.write_all(&packet_len).await {
                                    tracing::error!("Failed to write packet length to TCP stream: {:?}", e);
                                    break;
                                }
                                if let Err(e) = tcp_write.write_all(&udp_buffer[..len]).await {
                                    tracing::error!("Failed to write UDP packet to TCP stream: {:?}", e);
                                    break;
                                }
                                if let Err(e) = tcp_write.flush().await {
                                    tracing::error!("Failed to flush TCP stream: {:?}", e);
                                    break;
                                }
                            },
                            Err(e) => {
                                tracing::error!("Failed to receive from UDP socket: {:?}", e);
                                break;
                            }
                        }
                    },

                    result = Self::read_tcp_length_and_packet(&mut tcp_read) => {
                        match result {
                            Ok(Some(packet)) => {
                                if let Some(peer) = peer {
                                    if let Err(e) = local_udp_socket_write.send_to(&packet, &peer).await {
                                        tracing::error!("Failed to send UDP packet to peer: {:?}", e);
                                        break;
                                    }
                                } else {
                                    tracing::error!("No UDP peer to send to");
                                    break;
                                }
                            },
                            Ok(None) => {
                                break;
                            }
                            Err(e) => {
                                tracing::error!("Failed to read from TCP stream or send to UDP socket: {:?}", e);
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok((local_port, handle))
    }

    pub async fn read_tcp_length_and_packet(
        tcp_read: &mut (impl AsyncReadExt + Unpin),
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let mut len_bytes = [0u8; 4];
        if tcp_read.read_exact(&mut len_bytes).await.is_err() {
            // If there's an error reading (which includes EOF), return None
            return Ok(None);
        }
        let len = u32::from_be_bytes(len_bytes) as usize;

        let mut packet = vec![0u8; len];
        if tcp_read.read_exact(&mut packet).await.is_err() {
            // If there's an error reading the packet (which includes EOF), return None
            return Ok(None);
        }
        Ok(Some(packet))
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

#[derive(serde::Serialize, serde::Deserialize, Debug)]
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
    protocol: String,
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
        protocol: String,
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
            protocol,
        }
    }
}

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct Config {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    pub namespace: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub context: String,
    pub workload_type: String,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_enabled: Option<bool>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            id: None,
            service: Some("default-service".to_string()),
            namespace: "default-namespace".to_string(),
            local_port: 1234,
            remote_port: 5678,
            context: "default-context".to_string(),
            workload_type: "default-workload".to_string(),
            protocol: "tcp".to_string(),
            remote_address: Some("default-remote-address".to_string()),
            local_address: Some("127.0.0.1".to_string()),
            domain_enabled: Some(false),
            alias: Some("default-alias".to_string()),
        }
    }
}

#[tauri::command]
pub async fn start_port_udp_forward(configs: Vec<Config>) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();

    for config in configs {
        let selector = crate::TargetSelector::ServiceName(config.service.clone().unwrap());
        let remote_port = crate::Port::from(config.remote_port as i32);
        let context_name = Some(config.context.clone());
        log::info!("Remote Port: {}", config.remote_port);
        log::info!("Local Port: {}", config.local_port);

        let namespace = config.namespace.clone();
        let target = crate::Target::new(selector, remote_port, namespace);

        log::debug!("Attempting to forward to service: {:?}", &config.service);
        let local_address_clone = config.local_address.clone();
        let port_forward =
            PortForward::new(target, config.local_port, local_address_clone, context_name)
                .await
                .map_err(|e| {
                    log::error!("Failed to create PortForward: {:?}", e);
                    e.to_string()
                })?;

        let (actual_local_port, handle) = port_forward.port_forward_udp().await.map_err(|e| {
            log::error!("Failed to start UDP port forwarding: {:?}", e);
            e.to_string()
        })?;

        log::info!(
            "UDP port forwarding is set up on local port: {:?} for service: {:?}",
            actual_local_port,
            &config.service
        );

        // Store the JoinHandle to the global child processes map.
        CHILD_PROCESSES.lock().unwrap().insert(
            format!(
                "{}_{}",
                config.id.unwrap().to_string(),
                config.service.clone().unwrap_or_default()
            ),
            handle,
        );

        if config.domain_enabled.unwrap_or_default() {
            let hostfile_comment = format!(
                "kftray custom host for {} - {}",
                config.service.clone().unwrap_or_default(),
                config.id.unwrap_or_default()
            );

            let mut hosts_builder = HostsBuilder::new(hostfile_comment);

            if let Some(service_name) = &config.service {
                if let Some(local_address) = &config.local_address {
                    if let Ok(ip_addr) = local_address.parse::<std::net::IpAddr>() {
                        hosts_builder
                            .add_hostname(ip_addr, config.alias.clone().unwrap_or_default());
                        if let Err(e) = hosts_builder.write() {
                            log::error!(
                                "Failed to write to the hostfile for {}: {}",
                                service_name,
                                e
                            );
                        }
                    } else {
                        log::warn!("Invalid IP address format: {}", local_address);
                    }
                }
            }
        }
        // Append a new CustomResponse to responses collection.
        responses.push(CustomResponse {
            id: config.id,
            service: config.service.clone().unwrap(),
            namespace: config.namespace,
            local_port: actual_local_port,
            remote_port: config.remote_port,
            context: config.context.clone(),
            protocol: config.protocol.clone(),
            stdout: format!(
                "UDP forwarding from 127.0.0.1:{} -> {}:{}",
                actual_local_port,
                config.remote_port,
                config.service.unwrap()
            ),
            stderr: String::new(),
            status: 0,
        });
    }

    if !responses.is_empty() {
        log::info!("UDP port forwarding responses generated successfully.");
    }

    Ok(responses)
}

#[tauri::command]
pub async fn start_port_forward(configs: Vec<Config>) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();

    for config in configs {
        let selector = crate::TargetSelector::ServiceName(config.service.clone().unwrap());
        let remote_port = crate::Port::from(config.remote_port as i32);
        let context_name = Some(config.context.clone());
        log::info!("Remote Port: {}", config.remote_port);
        log::info!("Local Port: {}", config.remote_port);
        log::info!("Local Address: {:?}", config.local_address);

        let namespace = config.namespace.clone();
        let target = crate::Target::new(selector, remote_port, namespace);

        log::debug!("Attempting to forward to service: {:?}", &config.service);
        let port_forward = PortForward::new(
            target,
            config.local_port,
            config.local_address.clone(),
            context_name.clone(),
        )
        .await
        .map_err(|e| {
            log::error!("Failed to create PortForward: {:?}", e);
            e.to_string()
        })?;

        let (actual_local_port, handle) = port_forward.port_forward().await.map_err(|e| {
            log::error!("Failed to start port forwarding: {:?}", e);
            e.to_string()
        })?;

        log::info!(
            "Port forwarding is set up on local port: {:?} for service: {:?}",
            actual_local_port,
            &config.service
        );
        // Store the JoinHandle to the global child processes map.
        CHILD_PROCESSES.lock().unwrap().insert(
            format!(
                "{}_{}",
                config.id.unwrap().to_string(),
                config.service.clone().unwrap_or_default()
            ),
            handle,
        );

        if config.domain_enabled.unwrap_or_default() {
            let hostfile_comment = format!(
                "kftray custom host for {} - {}",
                config.service.clone().unwrap_or_default(),
                config.id.unwrap_or_default()
            );

            let mut hosts_builder = HostsBuilder::new(hostfile_comment);

            if let Some(service_name) = &config.service {
                if let Some(local_address) = &config.local_address {
                    if let Ok(ip_addr) = local_address.parse::<std::net::IpAddr>() {
                        hosts_builder
                            .add_hostname(ip_addr, config.alias.clone().unwrap_or_default());
                        if let Err(e) = hosts_builder.write() {
                            log::error!(
                                "Failed to write to the hostfile for {}: {}",
                                service_name,
                                e
                            );
                        }
                    } else {
                        log::warn!("Invalid IP address format: {}", local_address);
                    }
                }
            }
        }

        // Append a new CustomResponse to responses collection.
        responses.push(CustomResponse {
            id: config.id,
            service: config.service.clone().unwrap(),
            namespace: config.namespace,
            local_port: actual_local_port,
            remote_port: config.remote_port,
            context: config.context.clone(),
            protocol: config.protocol.clone(),
            stdout: format!(
                "Forwarding from 127.0.0.1:{} -> {}:{}",
                actual_local_port,
                config.remote_port,
                config.service.unwrap()
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

    // Drain the global hashmap to take ownership of all tasks
    let handle_map: HashMap<String, tokio::task::JoinHandle<()>> =
        CHILD_PROCESSES.lock().unwrap().drain().collect();

    for (composite_key, handle) in handle_map.iter() {
        let ids: Vec<&str> = composite_key.split('_').collect();
        if ids.len() != 2 {
            log::error!(
                "Invalid composite key format encountered: {}",
                composite_key
            );
            continue;
        }
        let config_id = ids[0];
        let service_id = ids[1];

        let hostfile_comment = format!("kftray custom host for {} - {}", service_id, config_id);
        let hosts_builder = HostsBuilder::new(&hostfile_comment);

        hosts_builder.write().map_err(|e| {
            log::error!("Failed to write to the hostfile for {}: {}", service_id, e);
            e.to_string()
        })?;

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
                                String::new(),
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
                                String::new(),
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
    _service_name: String,
    config_id: String,
) -> Result<CustomResponse, String> {
    let child_processes = CHILD_PROCESSES.lock().unwrap();
    let key_to_remove = child_processes
        .keys()
        .find(|key| key.starts_with(&format!("{}_", config_id)));

    match key_to_remove {
        Some(key) => {
            let composite_key = key.clone();
            drop(child_processes);

            let mut child_processes = CHILD_PROCESSES.lock().unwrap();
            if let Some(handle) = child_processes.remove(&composite_key) {
                handle.abort();
                let (config_id, service_name) = composite_key.split_once('_').unwrap_or(("", ""));
                let hostfile_comment =
                    format!("kftray custom host for {} - {}", service_name, config_id);
                let hosts_builder = HostsBuilder::new(&hostfile_comment);

                hosts_builder.write().map_err(|e| {
                    log::error!(
                        "Failed to write to the hostfile for {}: {}",
                        service_name,
                        e
                    );
                    e.to_string()
                })?;

                Ok(CustomResponse {
                    id: None,
                    service: service_name.to_string(),
                    namespace: String::new(),
                    local_port: 0,
                    remote_port: 0,
                    context: String::new(),
                    stdout: String::from("Service port forwarding has been stopped"),
                    stderr: String::new(),
                    status: 0,
                    protocol: String::new(),
                })
            } else {
                Err(format!(
                    "Failed to stop port forwarding process for config_id '{}'",
                    config_id
                ))
            }
        }
        None => Err(format!(
            "No port forwarding process found for config_id '{}'",
            config_id
        )),
    }
}
