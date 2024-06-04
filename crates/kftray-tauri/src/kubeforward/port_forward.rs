use std::fs::{
    self,
    OpenOptions,
};
use std::io::Write;
use std::path::PathBuf;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc,
        Mutex,
    },
};

use anyhow::Context;
use futures::TryStreamExt;
use hostsfile::HostsBuilder;
use kube::{
    api::{
        Api,
        DeleteParams,
        ListParams,
    },
    Client,
};
use lazy_static::lazy_static;
use tokio::{
    io::{
        AsyncReadExt,
        AsyncWriteExt,
    },
    net::{
        TcpListener,
        UdpSocket as TokioUdpSocket,
    },
    task::JoinHandle,
};
use tokio_stream::wrappers::TcpListenerStream;

use crate::{
    config,
    kubeforward::{
        port_forward::Target as TargetImpl,
        vx::Pod,
    },
    models::{
        config::Config,
        kube::{
            AnyReady,
            PodSelection,
            Port,
            PortForward,
            Target,
            TargetPod,
            TargetPodFinder,
            TargetSelector,
        },
        response::CustomResponse,
    },
};
impl PortForward {
    pub async fn new(
        target: Target, local_port: impl Into<Option<u16>>,
        local_address: impl Into<Option<String>>, context_name: Option<String>,
        kubeconfig: Option<String>, config_id: i64,
    ) -> anyhow::Result<Self> {
        let client = if let Some(ref context_name) = context_name {
            crate::kubeforward::kubecontext::create_client_with_specific_context(
                kubeconfig,
                context_name,
            )
            .await?
        } else {
            Client::try_default().await?
        };

        let namespace = target.namespace.name_any();

        Ok(Self {
            target,
            local_port: local_port.into(),
            local_address: local_address.into(),
            pod_api: Api::namespaced(client.clone(), &namespace),
            svc_api: Api::namespaced(client, &namespace),
            context_name,
            config_id,
        })
    }

    fn local_port(&self) -> u16 {
        self.local_port.unwrap_or(0)
    }

    fn local_address(&self) -> Option<String> {
        self.local_address.clone()
    }

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

    async fn forward_connection(self, client_conn: tokio::net::TcpStream) -> anyhow::Result<()> {
        let target = self.finder().find(&self.target).await?;

        let (pod_name, pod_port) = target.into_parts();

        let mut forwarder = self.pod_api.portforward(&pod_name, &[pod_port]).await?;

        let upstream_conn = forwarder
            .take_stream(pod_port)
            .context("port not found in forwarder")?;

        let local_port = self.local_port();
        let config_id = self.config_id;

        tracing::debug!(local_port, pod_port, pod_name, "forwarding connections");

        let log_file_path = {
            let mut path = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
            path.push(".kftray/sniff");
            fs::create_dir_all(&path)?;
            path.push(format!("{}_{}.log", config_id, local_port));
            path
        };

        let log_file = Arc::new(tokio::sync::Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file_path)?,
        ));

        let (mut client_reader, mut client_writer) = tokio::io::split(client_conn);
        let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

        let log_file_clone = Arc::clone(&log_file);
        let client_to_upstream = async move {
            let mut buffer = [0; 1024];
            loop {
                let n = client_reader.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                if let Ok(request) = std::str::from_utf8(&buffer[..n]) {
                    let mut log_file = log_file_clone.lock().await;
                    writeln!(log_file, "HTTP Request: {}", request)
                        .expect("Failed to write to log file");
                }
                upstream_writer.write_all(&buffer[..n]).await?;
            }
            Ok::<(), anyhow::Error>(())
        };

        let log_file_clone = Arc::clone(&log_file);
        let upstream_to_client = async move {
            let mut buffer = [0; 1024];
            loop {
                let n = upstream_reader.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                if let Ok(response) = std::str::from_utf8(&buffer[..n]) {
                    let mut log_file = log_file_clone.lock().await;
                    writeln!(log_file, "HTTP Response: {}", response)
                        .expect("Failed to write to log file");
                }
                client_writer.write_all(&buffer[..n]).await?;
            }
            Ok::<(), anyhow::Error>(())
        };

        tokio::try_join!(client_to_upstream, upstream_to_client)?;

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

impl<'a> TargetPodFinder<'a> {
    pub(crate) async fn find(&self, target: &Target) -> anyhow::Result<TargetPod> {
        let ready_pod = AnyReady {};

        match &target.selector {
            TargetSelector::ServiceName(name) => match self.svc_api.get(name).await {
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

#[tauri::command]
pub async fn start_port_udp_forward(configs: Vec<Config>) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();
    let mut errors = Vec::new();
    let mut child_handles = Vec::new();

    for config in configs.iter() {
        let selector = TargetSelector::ServiceName(config.service.clone().unwrap());
        let remote_port = Port::from(config.remote_port as i32);
        let context_name = Some(config.context.clone());
        let kubeconfig = Some(config.kubeconfig.clone());
        let namespace = config.namespace.clone();
        let target = TargetImpl::new(selector, remote_port, namespace);

        log::info!("Remote Port: {}", config.remote_port);
        log::info!("Local Port: {}", config.local_port);
        log::debug!("Attempting to forward to service: {:?}", &config.service);

        let local_address_clone = config.local_address.clone();

        match PortForward::new(
            target,
            config.local_port,
            local_address_clone,
            context_name,
            kubeconfig.flatten(),
            config.id.unwrap_or_default(),
        )
        .await
        {
            Ok(port_forward) => match port_forward.port_forward_udp().await {
                Ok((actual_local_port, handle)) => {
                    log::info!(
                        "UDP port forwarding is set up on local port: {:?} for service: {:?}",
                        actual_local_port,
                        &config.service
                    );

                    let handle_key = format!(
                        "{}_{}",
                        config.id.unwrap(),
                        config.service.clone().unwrap_or_default()
                    );
                    CHILD_PROCESSES
                        .lock()
                        .unwrap()
                        .insert(handle_key.clone(), handle);
                    child_handles.push(handle_key.clone()); // Clone handle_key for tracking

                    if config.domain_enabled.unwrap_or_default() {
                        let hostfile_comment = format!(
                            "kftray custom host for {} - {}",
                            config.service.clone().unwrap_or_default(),
                            config.id.unwrap_or_default()
                        );

                        let mut hosts_builder = HostsBuilder::new(hostfile_comment);

                        if let Some(service_name) = &config.service {
                            if let Some(local_address) = &config.local_address {
                                match local_address.parse::<std::net::IpAddr>() {
                                    Ok(ip_addr) => {
                                        hosts_builder.add_hostname(
                                            ip_addr,
                                            config.alias.clone().unwrap_or_default(),
                                        );
                                        if let Err(e) = hosts_builder.write() {
                                            let error_message = format!(
                                                "Failed to write to the hostfile for {}: {}",
                                                service_name, e
                                            );
                                            log::error!("{}", &error_message);
                                            errors.push(error_message);

                                            // Abort the child process due to critical error
                                            if let Some(handle) =
                                                CHILD_PROCESSES.lock().unwrap().remove(&handle_key)
                                            {
                                                handle.abort();
                                            }
                                            continue;
                                        }
                                    }
                                    Err(_) => {
                                        let warning_message =
                                            format!("Invalid IP address format: {}", local_address);
                                        log::warn!("{}", &warning_message);
                                        errors.push(warning_message);
                                    }
                                }
                            }
                        }
                    }

                    responses.push(CustomResponse {
                        id: config.id,
                        service: config.service.clone().unwrap(),
                        namespace: config.namespace.clone(),
                        local_port: actual_local_port,
                        remote_port: config.remote_port,
                        context: config.context.clone(),
                        protocol: config.protocol.clone(),
                        stdout: format!(
                            "UDP forwarding from 127.0.0.1:{} -> {}:{}",
                            actual_local_port,
                            config.remote_port,
                            config.service.clone().unwrap()
                        ),
                        stderr: String::new(),
                        status: 0,
                    });
                }
                Err(e) => {
                    let error_message = format!(
                        "Failed to start UDP port forwarding for service {}: {}",
                        config.service.clone().unwrap_or_default(),
                        e
                    );
                    log::error!("{}", &error_message);
                    errors.push(error_message);
                }
            },
            Err(e) => {
                let error_message = format!(
                    "Failed to create PortForward for service {}: {}",
                    config.service.clone().unwrap_or_default(),
                    e
                );
                log::error!("{}", &error_message);
                errors.push(error_message);
            }
        }
    }

    if !errors.is_empty() {
        // Abort all child processes if any critical error occurred
        for handle_key in child_handles {
            if let Some(handle) = CHILD_PROCESSES.lock().unwrap().remove(&handle_key) {
                handle.abort();
            }
        }
        return Err(errors.join("\n"));
    }

    if !responses.is_empty() {
        log::info!("UDP port forwarding responses generated successfully.");
    }

    Ok(responses)
}

#[tauri::command]
pub async fn start_port_forward(configs: Vec<Config>) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();
    let mut errors = Vec::new();
    let mut child_handles = Vec::new();

    for config in configs.iter() {
        let selector = TargetSelector::ServiceName(config.service.clone().unwrap());
        let remote_port = Port::from(config.remote_port as i32);
        let context_name = Some(config.context.clone());
        let kubeconfig = Some(config.kubeconfig.clone());
        let namespace = config.namespace.clone();
        let target = TargetImpl::new(selector, remote_port, namespace);

        log::info!("Remote Port: {}", config.remote_port);
        log::info!("Local Port: {}", config.local_port);
        log::info!("Local Address: {:?}", config.local_address);

        match PortForward::new(
            target,
            config.local_port,
            config.local_address.clone(),
            context_name.clone(),
            kubeconfig.clone().flatten(),
            config.id.unwrap_or_default(),
        )
        .await
        {
            Ok(port_forward) => match port_forward.port_forward().await {
                Ok((actual_local_port, handle)) => {
                    log::info!(
                        "Port forwarding is set up on local port: {:?} for service: {:?}",
                        actual_local_port,
                        &config.service
                    );

                    let handle_key = format!(
                        "{}_{}",
                        config.id.unwrap(),
                        config.service.clone().unwrap_or_default()
                    );
                    CHILD_PROCESSES
                        .lock()
                        .unwrap()
                        .insert(handle_key.clone(), handle);
                    child_handles.push(handle_key.clone());

                    if config.domain_enabled.unwrap_or_default() {
                        let hostfile_comment = format!(
                            "kftray custom host for {} - {}",
                            config.service.clone().unwrap_or_default(),
                            config.id.unwrap_or_default()
                        );

                        let mut hosts_builder = HostsBuilder::new(hostfile_comment);

                        if let Some(service_name) = &config.service {
                            if let Some(local_address) = &config.local_address {
                                match local_address.parse::<std::net::IpAddr>() {
                                    Ok(ip_addr) => {
                                        hosts_builder.add_hostname(
                                            ip_addr,
                                            config.alias.clone().unwrap_or_default(),
                                        );
                                        if let Err(e) = hosts_builder.write() {
                                            let error_message = format!(
                                                "Failed to write to the hostfile for {}: {}",
                                                service_name, e
                                            );
                                            log::error!("{}", &error_message);
                                            errors.push(error_message);

                                            if let Some(handle) =
                                                CHILD_PROCESSES.lock().unwrap().remove(&handle_key)
                                            {
                                                handle.abort();
                                            }
                                            continue;
                                        }
                                    }
                                    Err(_) => {
                                        let warning_message =
                                            format!("Invalid IP address format: {}", local_address);
                                        log::warn!("{}", &warning_message);
                                    }
                                }
                            }
                        }
                    }

                    responses.push(CustomResponse {
                        id: config.id,
                        service: config.service.clone().unwrap(),
                        namespace: config.namespace.clone(),
                        local_port: actual_local_port,
                        remote_port: config.remote_port,
                        context: config.context.clone(),
                        protocol: config.protocol.clone(),
                        stdout: format!(
                            "Forwarding from 127.0.0.1:{} -> {}:{}",
                            actual_local_port,
                            config.remote_port,
                            config.service.clone().unwrap()
                        ),
                        stderr: String::new(),
                        status: 0,
                    });
                }
                Err(e) => {
                    let error_message = format!("Failed to start port forwarding: {}", e);
                    log::error!("{}", &error_message);
                    errors.push(error_message);
                }
            },
            Err(e) => {
                let error_message = format!("Failed to create PortForward for service: {}", e);
                log::error!("{}", &error_message);
                errors.push(error_message);
            }
        }
    }

    if !errors.is_empty() {
        for handle_key in child_handles {
            if let Some(handle) = CHILD_PROCESSES.lock().unwrap().remove(&handle_key) {
                handle.abort();
            }
        }
        return Err(errors.join("\n"));
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

    let configs_result = config::get_configs().await;
    if let Err(e) = configs_result {
        return Err(format!("Failed to retrieve configs: {}", e));
    }
    let configs = configs_result.unwrap();

    for (composite_key, handle) in handle_map.iter() {
        let ids: Vec<&str> = composite_key.split('_').collect();
        if ids.len() != 2 {
            log::error!(
                "Invalid composite key format encountered: {}",
                composite_key
            );
            continue;
        }

        let config_id_str = ids[0];
        let service_id = ids[1];
        let config_id_parsed = config_id_str.parse::<i64>().unwrap_or_default();

        let config = configs
            .iter()
            .find(|c| c.id.map_or(false, |id| id == config_id_parsed));
        if let Some(config) = config {
            if config.domain_enabled.unwrap_or_default() {
                let hostfile_comment =
                    format!("kftray custom host for {} - {}", service_id, config_id_str);
                let hosts_builder = HostsBuilder::new(&hostfile_comment);

                if let Err(e) = hosts_builder.write() {
                    log::error!("Failed to write to the hostfile for {}: {}", service_id, e);
                    responses.push(CustomResponse {
                        id: Some(config_id_parsed),
                        service: service_id.to_string(),
                        namespace: String::new(),
                        local_port: 0,
                        remote_port: 0,
                        context: String::new(),
                        protocol: String::new(),
                        stdout: String::new(),
                        stderr: e.to_string(),
                        status: 1,
                    });
                    continue;
                }
            }
        } else {
            log::warn!("Config with id '{}' not found.", config_id_str);
        }

        log::info!(
            "Aborting port forwarding task for config_id: {}",
            config_id_str
        );
        handle.abort();

        // Delete pods
        let pods: Api<Pod> = Api::all(client.clone());
        let lp = ListParams::default().labels(&format!("config_id={}", config_id_str));
        log::info!(
            "Listing pods with label selector: config_id={}",
            config_id_str
        );
        let pod_list = pods.list(&lp).await.map_err(|e| {
            log::error!("Error listing pods for config_id {}: {}", config_id_str, e);
            e.to_string()
        })?;

        let username = whoami::username();
        let pod_prefix = format!("kftray-forward-{}", username);

        for pod in pod_list.items.into_iter() {
            if let Some(pod_name) = pod.metadata.name {
                log::info!("Found pod: {}", pod_name);
                if pod_name.starts_with(&pod_prefix) {
                    log::info!("Deleting pod: {}", pod_name);
                    let namespace = pod
                        .metadata
                        .namespace
                        .clone()
                        .unwrap_or_else(|| "default".to_string());
                    let pods_in_namespace: Api<Pod> = Api::namespaced(client.clone(), &namespace);
                    let dp = DeleteParams {
                        grace_period_seconds: Some(0),
                        ..DeleteParams::default()
                    };
                    if let Err(e) = pods_in_namespace.delete(&pod_name, &dp).await {
                        log::error!(
                            "Failed to delete pod {} in namespace {}: {}",
                            pod_name,
                            namespace,
                            e
                        );
                    } else {
                        log::info!("Successfully deleted pod: {}", pod_name);
                    }
                }
            }
        }

        responses.push(CustomResponse {
            id: Some(config_id_parsed),
            service: service_id.to_string(),
            namespace: String::new(),
            local_port: 0,
            remote_port: 0,
            context: String::new(),
            protocol: String::new(),
            stdout: String::from("Service port forwarding has been stopped"),
            stderr: String::new(),
            status: 0,
        });
    }

    log::info!(
        "Port forward stopping process completed with {} responses",
        responses.len()
    );

    Ok(responses)
}

#[tauri::command]
pub async fn stop_port_forward(
    _service_name: String, config_id: String,
) -> Result<CustomResponse, String> {
    let composite_key = {
        let child_processes = CHILD_PROCESSES.lock().unwrap();
        child_processes
            .keys()
            .find(|key| key.starts_with(&format!("{}_", config_id)))
            .map(|key| key.to_string())
    };

    if let Some(composite_key) = composite_key {
        let handle = {
            let mut child_processes = CHILD_PROCESSES.lock().unwrap();
            child_processes.remove(&composite_key)
        };

        if let Some(handle) = handle {
            handle.abort();

            let (config_id_str, service_name) = composite_key.split_once('_').unwrap_or(("", ""));
            let config_id_parsed = config_id_str.parse::<i64>().unwrap_or_default();

            let configs_result = config::get_configs().await;
            match configs_result {
                Ok(configs) => {
                    let config = configs
                        .iter()
                        .find(|c| c.id.map_or(false, |id| id == config_id_parsed));

                    if let Some(config) = config {
                        if config.domain_enabled.unwrap_or_default() {
                            let hostfile_comment = format!(
                                "kftray custom host for {} - {}",
                                service_name, config_id_str
                            );

                            let hosts_builder = HostsBuilder::new(hostfile_comment);

                            hosts_builder.write().map_err(|e| {
                                log::error!(
                                    "Failed to remove from the hostfile for {}: {}",
                                    service_name,
                                    e
                                );

                                e.to_string()
                            })?;
                        }
                    } else {
                        log::warn!("Config with id '{}' not found.", config_id_str);
                    }

                    Ok(CustomResponse {
                        id: None,
                        service: service_name.to_string(),
                        namespace: String::new(),
                        local_port: 0,
                        remote_port: 0,
                        context: String::new(),
                        protocol: String::new(),
                        stdout: String::from("Service port forwarding has been stopped"),
                        stderr: String::new(),
                        status: 0,
                    })
                }
                Err(e) => Err(format!("Failed to retrieve configs: {}", e)),
            }
        } else {
            Err(format!(
                "Failed to stop port forwarding process for config_id '{}'",
                config_id
            ))
        }
    } else {
        Err(format!(
            "No port forwarding process found for config_id '{}'",
            config_id
        ))
    }
}
