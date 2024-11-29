use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use anyhow::Context;
use bytes::Bytes;
use futures::TryStreamExt;
use kftray_commons::logging::{
    create_log_file_path,
    Logger,
};
use kube::{
    api::Api,
    Client,
};
use lazy_static::lazy_static;
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::{
    TcpListener,
    TcpStream,
    UdpSocket as TokioUdpSocket,
};
use tokio::sync::{
    Mutex,
    Notify,
};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_stream::wrappers::TcpListenerStream;
use tracing::{
    error,
    info,
    trace,
};

use crate::models::kube::HttpLogState;
use crate::models::kube::{
    PortForward,
    Target,
};
use crate::pod_finder::TargetPodFinder;

const BUFFER_SIZE: usize = 131072;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

lazy_static! {
    pub static ref CHILD_PROCESSES: Arc<StdMutex<HashMap<String, JoinHandle<()>>>> =
        Arc::new(StdMutex::new(HashMap::new()));
    pub static ref CANCEL_NOTIFIER: Arc<Notify> = Arc::new(Notify::new());
}

impl PortForward {
    pub async fn new(
        target: Target, local_port: impl Into<Option<u16>>,
        local_address: impl Into<Option<String>>, context_name: Option<String>,
        kubeconfig: Option<String>, config_id: i64, workload_type: String,
    ) -> anyhow::Result<Self> {
        let (client, _, _) = if let Some(ref context_name) = context_name {
            crate::client::create_client_with_specific_context(kubeconfig, Some(context_name))
                .await?
        } else {
            (Some(Client::try_default().await?), None, Vec::new())
        };

        let client = client.ok_or_else(|| {
            anyhow::anyhow!(
                "Client not created for context '{}'",
                context_name.clone().unwrap_or_default()
            )
        })?;

        let namespace = target.namespace.name_any();

        Ok(Self {
            target,
            local_port: local_port.into(),
            local_address: local_address.into(),
            pod_api: Api::namespaced(client.clone(), &namespace),
            svc_api: Api::namespaced(client, &namespace),
            context_name,
            config_id,
            workload_type,
            connection: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn port_forward_tcp(
        self, http_log_state: Arc<HttpLogState>,
    ) -> anyhow::Result<(u16, JoinHandle<()>)> {
        let target = self.finder().find(&self.target).await?;
        let (pod_name, pod_port) = target.into_parts();
        let pod_port = u16::try_from(pod_port).context("Invalid port number")?;

        let mut forwarder = self.pod_api.portforward(&pod_name, &[pod_port]).await?;
        let _upstream_conn = forwarder
            .take_stream(pod_port)
            .context("port not found in forwarder")?;

        let listener = self.create_tcp_listener().await?;
        let port = listener.local_addr()?.port();

        let server_handle = self.spawn_tcp_server(listener, http_log_state);

        Ok((port, server_handle))
    }

    async fn create_tcp_listener(&self) -> anyhow::Result<TcpListener> {
        let addr = self.get_bind_address()?;
        let listener = TcpListener::bind(addr).await?;
        trace!(port = ?listener.local_addr()?, "Bound to local address and port");
        Ok(listener)
    }

    fn spawn_tcp_server(
        self, listener: TcpListener, http_log_state: Arc<HttpLogState>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let server = TcpListenerStream::new(listener).try_for_each(|client_conn| {
                let pf = self.clone();
                let client_conn = Arc::new(Mutex::new(client_conn));
                let http_log_state = http_log_state.clone();

                async move {
                    if let Ok(peer_addr) = client_conn.lock().await.peer_addr() {
                        trace!(%peer_addr, "new connection");
                    }

                    {
                        let conn = client_conn.lock().await;
                        conn.set_nodelay(true)?;
                    }

                    let cancel_notifier = CANCEL_NOTIFIER.clone();

                    tokio::spawn(async move {
                        if let Err(e) = pf
                            .handle_tcp_connection(client_conn, http_log_state, cancel_notifier)
                            .await
                        {
                            error!(
                                error = e.as_ref() as &dyn std::error::Error,
                                "failed to forward connection"
                            );
                        }
                    });

                    Ok(())
                }
            });

            if let Err(e) = server.await {
                error!(error = &e as &dyn std::error::Error, "server error");
            }
        })
    }

    async fn handle_tcp_connection(
        &self, client_conn: Arc<Mutex<TcpStream>>, http_log_state: Arc<HttpLogState>,
        cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let target = self.finder().find(&self.target).await?;
        let (pod_name, pod_port) = target.into_parts();
        let pod_port = u16::try_from(pod_port).context("Invalid port number")?;

        // Get pod IP for connection
        let pod = self.pod_api.get(&pod_name).await?;
        let pod_ip = pod
            .status
            .and_then(|s| s.pod_ip)
            .context("Pod IP not found")?;

        info!(
            "Connecting to pod {} ({}:{}) for forwarding",
            pod_name, pod_ip, pod_port
        );

        let mut forwarder = self.pod_api.portforward(&pod_name, &[pod_port]).await?;
        let upstream_conn = forwarder
            .take_stream(pod_port)
            .context("port not found in forwarder")?;

        let logger = self.create_logger().await?;
        let request_id = Arc::new(Mutex::new(None));

        let mut client_conn_guard = client_conn.lock().await;
        let (mut client_reader, mut client_writer) = tokio::io::split(&mut *client_conn_guard);
        let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

        let client_to_upstream = self.process_client_to_upstream(
            &mut client_reader,
            &mut upstream_writer,
            logger.as_ref(),
            &http_log_state,
            Arc::clone(&request_id),
            cancel_notifier.clone(),
        );

        let upstream_to_client = self.process_upstream_to_client(
            &mut upstream_reader,
            &mut client_writer,
            logger.as_ref(),
            &http_log_state,
            Arc::clone(&request_id),
            cancel_notifier.clone(),
        );

        tokio::try_join!(client_to_upstream, upstream_to_client)?;

        Ok(())
    }

    pub async fn port_forward_udp(self) -> anyhow::Result<(u16, JoinHandle<()>)> {
        let local_socket = self.create_udp_socket().await?;
        let local_port = local_socket.local_addr()?.port();

        let handle = self.spawn_udp_handler(local_socket);

        Ok((local_port, handle))
    }

    async fn create_udp_socket(&self) -> anyhow::Result<Arc<TokioUdpSocket>> {
        let addr = self.get_bind_address()?;
        let socket = TokioUdpSocket::bind(&addr)
            .await
            .context("Failed to bind local UDP socket")?;

        info!("Local UDP socket bound to {}", addr);
        Ok(Arc::new(socket))
    }

    fn spawn_udp_handler(self, local_socket: Arc<TokioUdpSocket>) -> JoinHandle<()> {
        tokio::spawn(async move {
            if let Err(e) = self.handle_udp_forwarding(local_socket).await {
                error!("UDP forwarding error: {}", e);
            }
        })
    }

    async fn handle_udp_forwarding(&self, local_socket: Arc<TokioUdpSocket>) -> anyhow::Result<()> {
        let target = self.finder().find(&self.target).await?;
        let (pod_name, pod_port) = target.into_parts();
        let pod_port = u16::try_from(pod_port).context("Invalid port number")?;

        let mut forwarder = self
            .pod_api
            .portforward(&pod_name, &[pod_port])
            .await
            .context("Failed to start port forwarding to pod")?;

        let stream = forwarder
            .take_stream(pod_port)
            .context("port not found in forwarder")?;

        let (mut tcp_read, mut tcp_write) = tokio::io::split(stream);

        self.process_udp_forwarding(local_socket, &mut tcp_read, &mut tcp_write)
            .await
    }

    fn get_bind_address(&self) -> anyhow::Result<SocketAddr> {
        let local_addr = self
            .local_address
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let addr = format!("{}:{}", local_addr, self.local_port.unwrap_or(0))
            .parse()
            .context("Invalid bind address")?;
        Ok(addr)
    }

    async fn create_logger(&self) -> anyhow::Result<Option<Logger>> {
        if self.workload_type == "service" || self.workload_type == "pod" {
            let log_file_path =
                create_log_file_path(self.config_id, self.local_port.unwrap_or(0)).await?;
            Ok(Some(Logger::new(log_file_path).await?))
        } else {
            Ok(None)
        }
    }

    fn finder(&self) -> TargetPodFinder {
        TargetPodFinder {
            pod_api: &self.pod_api,
            svc_api: &self.svc_api,
        }
    }

    async fn process_client_to_upstream(
        &self, client_reader: &mut (impl AsyncReadExt + Unpin),
        upstream_writer: &mut (impl AsyncWriteExt + Unpin), logger: Option<&Logger>,
        http_log_state: &HttpLogState, request_id: Arc<Mutex<Option<String>>>,
        cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = vec![0; BUFFER_SIZE];

        loop {
            tokio::select! {
                n = timeout(DEFAULT_TIMEOUT, client_reader.read(&mut buffer)) => {
                    match n {
                        Ok(Ok(0)) => break,
                        Ok(Ok(n)) => {
                            self.handle_client_data(&buffer[..n], upstream_writer, logger, http_log_state, &request_id).await?;
                        }
                        Ok(Err(e)) => return Err(e.into()),
                        Err(_) => return Err(anyhow::anyhow!("Client read timeout")),
                    }
                }
                _ = cancel_notifier.notified() => break,
            }
        }

        upstream_writer.shutdown().await?;
        Ok(())
    }

    async fn process_upstream_to_client(
        &self, upstream_reader: &mut (impl AsyncReadExt + Unpin),
        client_writer: &mut (impl AsyncWriteExt + Unpin), logger: Option<&Logger>,
        http_log_state: &HttpLogState, request_id: Arc<Mutex<Option<String>>>,
        cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = vec![0; BUFFER_SIZE];

        loop {
            tokio::select! {
                n = timeout(DEFAULT_TIMEOUT, upstream_reader.read(&mut buffer)) => {
                    match n {
                        Ok(Ok(0)) => break,
                        Ok(Ok(n)) => {
                            self.handle_upstream_data(&buffer[..n], client_writer, logger, http_log_state, &request_id).await?;
                        }
                        Ok(Err(e)) => return Err(e.into()),
                        Err(_) => return Err(anyhow::anyhow!("Upstream read timeout")),
                    }
                }
                _ = cancel_notifier.notified() => break,
            }
        }

        client_writer.shutdown().await?;
        Ok(())
    }

    async fn handle_client_data(
        &self, data: &[u8], upstream_writer: &mut (impl AsyncWriteExt + Unpin),
        logger: Option<&Logger>, http_log_state: &HttpLogState,
        request_id: &Arc<Mutex<Option<String>>>,
    ) -> anyhow::Result<()> {
        if http_log_state.get_http_logs(self.config_id).await {
            if let Some(logger) = logger {
                let mut req_id_guard = request_id.lock().await;
                let new_request_id = logger.log_request(Bytes::from(data.to_vec())).await;
                *req_id_guard = Some(new_request_id);
            }
        }

        upstream_writer.write_all(data).await?;
        Ok(())
    }

    async fn handle_upstream_data(
        &self, data: &[u8], client_writer: &mut (impl AsyncWriteExt + Unpin),
        logger: Option<&Logger>, http_log_state: &HttpLogState,
        request_id: &Arc<Mutex<Option<String>>>,
    ) -> anyhow::Result<()> {
        if http_log_state.get_http_logs(self.config_id).await {
            if let Some(logger) = logger {
                let req_id_guard = request_id.lock().await;
                if let Some(req_id) = &*req_id_guard {
                    logger
                        .log_response(Bytes::from(data.to_vec()), req_id.clone())
                        .await;
                }
            }
        }

        client_writer.write_all(data).await?;
        Ok(())
    }

    async fn process_udp_forwarding(
        &self, local_socket: Arc<TokioUdpSocket>, tcp_read: &mut (impl AsyncReadExt + Unpin),
        tcp_write: &mut (impl AsyncWriteExt + Unpin),
    ) -> anyhow::Result<()> {
        let mut udp_buffer = [0u8; BUFFER_SIZE];
        let mut peer: Option<std::net::SocketAddr> = None;

        loop {
            tokio::select! {
                result = local_socket.recv_from(&mut udp_buffer) => {
                    match result {
                        Ok((len, src)) => {
                            peer = Some(src);
                            self.handle_udp_to_tcp(tcp_write, &udp_buffer[..len]).await?;
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                result = Self::read_tcp_packet(tcp_read) => {
                    match result? {
                        Some(packet) => {
                            if let Some(peer) = peer {
                                local_socket.send_to(&packet, &peer).await?;
                            }
                        }
                        None => break,
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_udp_to_tcp(
        &self, tcp_write: &mut (impl AsyncWriteExt + Unpin), data: &[u8],
    ) -> anyhow::Result<()> {
        let packet_len = (data.len() as u32).to_be_bytes();
        tcp_write.write_all(&packet_len).await?;
        tcp_write.write_all(data).await?;
        tcp_write.flush().await?;
        Ok(())
    }

    async fn read_tcp_packet(
        tcp_read: &mut (impl AsyncReadExt + Unpin),
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let mut len_bytes = [0u8; 4];
        if tcp_read.read_exact(&mut len_bytes).await.is_err() {
            return Ok(None);
        }

        let len = u32::from_be_bytes(len_bytes) as usize;
        let mut packet = vec![0u8; len];

        if tcp_read.read_exact(&mut packet).await.is_err() {
            return Ok(None);
        }

        Ok(Some(packet))
    }
}
