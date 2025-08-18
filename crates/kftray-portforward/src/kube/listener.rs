use std::sync::Arc;

use k8s_openapi::api::core::v1::Pod;
use kftray_http_logs::HttpLogState;
use kube::Api;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{
    debug,
    error,
    info,
};

use crate::kube::http_log_watcher::HttpLogStateWatcher;
use crate::kube::models::{
    Port,
    Target,
};
use crate::kube::pod_watcher::PodWatcher;
use crate::kube::shared_client::SHARED_CLIENT_MANAGER;
use crate::kube::tcp_forwarder::TcpForwarder;
use crate::kube::udp_forwarder::UdpForwarder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Clone, Debug)]
pub struct ListenerConfig {
    pub local_address: String,
    pub local_port: u16,
    pub protocol: Protocol,
}

impl Default for ListenerConfig {
    fn default() -> Self {
        Self {
            local_address: "127.0.0.1".to_owned(),
            local_port: 0,
            protocol: Protocol::Tcp,
        }
    }
}

pub trait PortForwardStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T> PortForwardStream for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}

pub struct PortForwarder {
    namespace: Arc<str>,
    pod_watcher: Arc<PodWatcher>,
    pod_api: Api<Pod>,
    target_port: Option<u16>,
    next_portforwarder: Arc<tokio::sync::Mutex<Option<kube::api::Portforwarder>>>,
    portforward_semaphore: Arc<tokio::sync::Semaphore>,
    http_log_watcher: HttpLogStateWatcher,
    initialization_lock: Arc<tokio::sync::Mutex<bool>>,
}

impl PortForwarder {
    pub async fn new(
        namespace: &str, target: Target, context_name: Option<String>, kubeconfig: Option<String>,
        config_id: i64,
    ) -> anyhow::Result<Self> {
        let client_key =
            crate::kube::shared_client::ServiceClientKey::new(context_name, kubeconfig, config_id);
        let client = SHARED_CLIENT_MANAGER.get_client(client_key).await?;
        let pod_watcher = PodWatcher::new((*client).clone(), target.clone()).await?;

        Ok(Self {
            namespace: namespace.into(),
            pod_watcher: Arc::new(pod_watcher),
            pod_api: Api::namespaced((*client).clone(), namespace),
            target_port: None,
            next_portforwarder: Arc::new(tokio::sync::Mutex::new(None)),
            portforward_semaphore: Arc::new(tokio::sync::Semaphore::new(10)),
            http_log_watcher: HttpLogStateWatcher::new(),
            initialization_lock: Arc::new(tokio::sync::Mutex::new(false)),
        })
    }

    async fn resolve_target_port(&self, target: &Target) -> anyhow::Result<u16> {
        match &target.port {
            Port::Number(port) => {
                let port_u16 = u16::try_from(*port)
                    .map_err(|_| anyhow::anyhow!("Port number {} is out of range", port))?;
                Ok(port_u16)
            }
            Port::Name(port_name) => {
                let selected_pod = self
                    .pod_watcher
                    .wait_for_ready_pod(tokio::time::Duration::from_secs(5))
                    .await
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No ready pods available to resolve port name '{}'",
                            port_name
                        )
                    })?;
                Ok(selected_pod.port_number)
            }
        }
    }

    pub async fn initialize(&mut self, target: &Target) -> anyhow::Result<()> {
        let mut init_lock = self.initialization_lock.lock().await;
        if *init_lock {
            return Ok(());
        }

        let target_port = self.resolve_target_port(target).await?;
        self.target_port = Some(target_port);

        let first_portforwarder = self.create_portforwarder(target_port).await?;
        {
            let mut next = self.next_portforwarder.lock().await;
            *next = Some(first_portforwarder);
        }

        for _ in 2..=3 {
            self.spawn_next_portforwarder(target_port);
        }

        *init_lock = true;
        info!(
            "Initialized port forwarder for port {} with 1 ready + 2 creating",
            target_port
        );
        Ok(())
    }

    pub async fn get_stream(&self) -> anyhow::Result<Box<dyn PortForwardStream>> {
        let init_lock = self.initialization_lock.lock().await;
        if !*init_lock {
            return Err(anyhow::anyhow!(
                "Port forwarder not initialized - call initialize() first"
            ));
        }
        drop(init_lock);

        let target_port = self.target_port.ok_or_else(|| {
            anyhow::anyhow!("Port forwarder not initialized - call initialize() first")
        })?;

        let mut next_pf = self.next_portforwarder.lock().await;
        let mut portforwarder = next_pf.take();
        drop(next_pf);

        if portforwarder.is_none() {
            portforwarder = Some(self.create_portforwarder(target_port).await?);
        }

        let stream = self
            .get_stream_with_retry(portforwarder.unwrap(), target_port)
            .await?;
        self.spawn_next_portforwarder(target_port);

        Ok(Box::new(stream))
    }

    async fn get_stream_with_retry(
        &self, mut portforwarder: kube::api::Portforwarder, target_port: u16,
    ) -> anyhow::Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> {
        if let Some(stream) = portforwarder.take_stream(target_port) {
            if let Some(error_future) = portforwarder.take_error(target_port) {
                tokio::spawn(async move {
                    if let Some(error_msg) = error_future.await {
                        debug!("Portforwarder error detected: {}", error_msg);
                    }
                });
            }
            return Ok(stream);
        }

        let mut retry_portforwarder = self.create_portforwarder(target_port).await?;

        match retry_portforwarder.take_stream(target_port) {
            Some(stream) => {
                if let Some(error_future) = retry_portforwarder.take_error(target_port) {
                    tokio::spawn(async move {
                        if let Some(error_msg) = error_future.await {
                            debug!("Retry portforwarder error: {}", error_msg);
                        }
                    });
                }
                Ok(stream)
            }
            None => Err(anyhow::anyhow!("Failed to get stream after retry")),
        }
    }

    async fn create_portforwarder(
        &self, target_port: u16,
    ) -> anyhow::Result<kube::api::Portforwarder> {
        let _permit = self
            .portforward_semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("Semaphore closed"))?;

        let selected_pod = self
            .pod_watcher
            .wait_for_ready_pod(tokio::time::Duration::from_secs(3))
            .await
            .ok_or_else(|| anyhow::anyhow!("No ready pods available"))?;

        for attempt in 1..=2 {
            let result = tokio::time::timeout(
                tokio::time::Duration::from_secs(3),
                self.pod_api
                    .portforward(&selected_pod.pod_name, &[target_port]),
            )
            .await;

            match result {
                Ok(Ok(portforwarder)) => return Ok(portforwarder),
                Ok(Err(e)) => {
                    if e.to_string().contains("404") && attempt == 1 {
                        debug!(
                            "Portforward attempt {} failed with 404, retrying in 2s",
                            attempt
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    return Err(anyhow::anyhow!("Failed to create portforwarder: {}", e));
                }
                Err(e) => return Err(anyhow::anyhow!("Portforward timeout: {}", e)),
            }
        }

        Err(anyhow::anyhow!(
            "Failed to create portforwarder after 2 attempts"
        ))
    }

    fn spawn_next_portforwarder(&self, target_port: u16) {
        let pod_watcher = Arc::clone(&self.pod_watcher);
        let pod_api = self.pod_api.clone();
        let next_pf = self.next_portforwarder.clone();

        tokio::spawn(async move {
            let mut guard = next_pf.lock().await;
            if guard.is_some() {
                return;
            }

            if let Some(selected_pod) = pod_watcher
                .wait_for_ready_pod(tokio::time::Duration::from_secs(5))
                .await
            {
                for attempt in 1..=2 {
                    let result = tokio::time::timeout(
                        tokio::time::Duration::from_secs(3),
                        pod_api.portforward(&selected_pod.pod_name, &[target_port]),
                    )
                    .await;

                    match result {
                        Ok(Ok(portforwarder)) => {
                            *guard = Some(portforwarder);
                            return;
                        }
                        Ok(Err(e)) => {
                            if e.to_string().contains("404") && attempt == 1 {
                                debug!(
                                    "Spawn portforward attempt {} failed with 404, retrying in 2s",
                                    attempt
                                );
                                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                continue;
                            }
                            debug!("Spawn portforward failed: {}", e);
                            return;
                        }
                        Err(e) => {
                            debug!("Spawn portforward timeout: {}", e);
                            return;
                        }
                    }
                }
            }
        });
    }

    pub async fn handle_tcp_listener(
        self: Arc<Self>, listener: TcpListener, http_log_state: Arc<HttpLogState>, config_id: i64,
        workload_type: String, port: u16, cancellation_token: CancellationToken,
    ) -> anyhow::Result<()> {
        let initial_logging_enabled = http_log_state
            .get_http_logs(config_id)
            .await
            .unwrap_or_default();

        self.http_log_watcher
            .set_http_logs(config_id, initial_logging_enabled)
            .await?;

        let http_log_watcher_clone = self.http_log_watcher.clone();
        let http_log_state_clone = http_log_state.clone();
        let sync_cancel_token = cancellation_token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = http_log_watcher_clone
                            .sync_from_external_state(&http_log_state_clone, config_id)
                            .await
                        {
                            debug!("Sync error for config {}: {}", config_id, e);
                        }
                    }
                    _ = sync_cancel_token.cancelled() => {
                        debug!("HTTP log sync task cancelled for config {}", config_id);
                        break;
                    }
                }
            }
        });

        let tcp_forwarder = TcpForwarder::new(config_id, workload_type);

        let forwarder_clone = Arc::clone(&self);
        let cancel_token = cancellation_token.clone();

        loop {
            let (client_conn, client_addr) = tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok(connection) => connection,
                        Err(e) => {
                            error!("Accept failed: {}", e);
                            break;
                        }
                    }
                }
                _ = cancel_token.cancelled() => {
                    break;
                }
            };

            let forwarder = Arc::clone(&forwarder_clone);
            let mut tcp_forwarder = tcp_forwarder.clone();
            let http_log_watcher_clone = Arc::new(self.http_log_watcher.clone());
            let http_log_state_clone = http_log_state.clone();
            let cancel_token_clone = cancel_token.clone();
            tokio::spawn(async move {
                let upstream_stream = match forwarder.get_stream().await {
                    Ok(stream) => stream,
                    Err(e) => {
                        error!("Failed to create stream for {}: {}", client_addr, e);
                        return;
                    }
                };

                if let Err(e) = tcp_forwarder
                    .forward_streams(
                        client_conn,
                        upstream_stream,
                        client_addr,
                        cancel_token_clone,
                        http_log_watcher_clone,
                        http_log_state_clone,
                        port,
                    )
                    .await
                {
                    error!("Forward failed for {}: {}", client_addr, e);
                }
            });
        }

        Ok(())
    }

    pub async fn start_listener(
        self: Arc<Self>, listener_config: ListenerConfig, http_log_state: Arc<HttpLogState>,
        config_id: i64, workload_type: String, cancellation_token: CancellationToken,
    ) -> anyhow::Result<(u16, JoinHandle<anyhow::Result<()>>)> {
        if let Err(e) =
            crate::network_utils::ensure_loopback_address(&listener_config.local_address).await
        {
            return Err(anyhow::anyhow!("Network config failed: {}", e));
        }

        match listener_config.protocol {
            Protocol::Tcp => {
                self.start_tcp_listener(
                    listener_config,
                    http_log_state,
                    config_id,
                    workload_type,
                    cancellation_token,
                )
                .await
            }
            Protocol::Udp => {
                self.start_udp_listener(
                    listener_config,
                    http_log_state,
                    config_id,
                    workload_type,
                    cancellation_token,
                )
                .await
            }
        }
    }

    async fn start_tcp_listener(
        self: Arc<Self>, listener_config: ListenerConfig, http_log_state: Arc<HttpLogState>,
        config_id: i64, workload_type: String, cancellation_token: CancellationToken,
    ) -> anyhow::Result<(u16, JoinHandle<anyhow::Result<()>>)> {
        let ip = listener_config
            .local_address
            .parse::<std::net::IpAddr>()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Invalid IP address '{}': {}",
                    listener_config.local_address,
                    e
                )
            })?;
        let addr = std::net::SocketAddr::new(ip, listener_config.local_port);

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bind TCP listener to {}: {}", addr, e))?;
        let port = listener.local_addr()?.port();

        let handle = tokio::spawn(async move {
            self.handle_tcp_listener(
                listener,
                http_log_state,
                config_id,
                workload_type,
                port,
                cancellation_token,
            )
            .await
        });

        Ok((port, handle))
    }

    async fn start_udp_listener(
        self: Arc<Self>, listener_config: ListenerConfig, _http_log_state: Arc<HttpLogState>,
        _config_id: i64, workload_type: String, cancellation_token: CancellationToken,
    ) -> anyhow::Result<(u16, JoinHandle<anyhow::Result<()>>)> {
        if workload_type == "service" || workload_type == "proxy" {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }

        let test_addr = format!(
            "{}:{}",
            listener_config.local_address, listener_config.local_port
        );
        let _test_socket = tokio::net::UdpSocket::bind(&test_addr)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bind local UDP socket: {}", e))?;
        drop(_test_socket);

        let upstream_stream = self
            .get_stream()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get upstream connection for UDP: {}", e))?;

        let (port, handle) = UdpForwarder::bind_and_forward(
            listener_config.local_address,
            listener_config.local_port,
            upstream_stream,
            cancellation_token,
        )
        .await?;

        let result_handle = tokio::spawn(async move {
            handle
                .await
                .map_err(|e| anyhow::anyhow!("UDP forwarding task failed: {}", e))?;
            Ok(())
        });

        Ok((port, result_handle))
    }

    pub fn get_http_log_watcher(&self) -> &HttpLogStateWatcher {
        &self.http_log_watcher
    }

    pub async fn set_http_logging(&self, config_id: i64, enabled: bool) -> anyhow::Result<()> {
        self.http_log_watcher
            .set_http_logs(config_id, enabled)
            .await
    }

    pub async fn get_http_logging(&self, config_id: i64) -> bool {
        self.http_log_watcher.get_http_logs(config_id).await
    }

    pub async fn shutdown(&self) {
        info!(
            "Shutting down port forwarder for namespace: {}",
            self.namespace.as_ref()
        );

        let mut next_pf = self.next_portforwarder.lock().await;
        if let Some(portforwarder) = next_pf.take() {
            drop(portforwarder);
        }
    }
}
