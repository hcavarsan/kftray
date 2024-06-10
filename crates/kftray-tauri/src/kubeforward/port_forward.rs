use std::fs::OpenOptions;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use futures::TryStreamExt;
use kube::{
    api::Api,
    Client,
};
use tokio::net::TcpStream;
use tokio::net::UdpSocket as TokioUdpSocket;
use tokio::sync::Mutex;
use tokio::{
    io::{
        AsyncReadExt,
        AsyncWriteExt,
    },
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};
use tokio_stream::wrappers::TcpListenerStream;
use tracing::{
    error,
    info,
    trace,
};

use crate::kubeforward::logging::{
    create_log_file_path,
    log_request,
    log_response,
};
use crate::kubeforward::pod_finder::TargetPodFinder;
use crate::models::kube::HttpLogState;
use crate::models::kube::{
    PortForward,
    Target,
};

const INITIAL_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_RETRIES: usize = 5;
const BUFFER_SIZE: usize = 65536;

impl PortForward {
    pub async fn new(
        target: Target, local_port: impl Into<Option<u16>>,
        local_address: impl Into<Option<String>>, context_name: Option<String>,
        kubeconfig: Option<String>, config_id: i64, workload_type: String,
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
            workload_type,
            connection: Arc::new(Mutex::new(None)),
        })
    }

    pub fn local_port(&self) -> u16 {
        self.local_port.unwrap_or(0)
    }

    pub fn local_address(&self) -> Option<String> {
        self.local_address.clone()
    }

    pub async fn port_forward_tcp(
        self, http_log_state: tauri::State<'_, HttpLogState>,
    ) -> anyhow::Result<(u16, tokio::task::JoinHandle<()>)> {
        let local_addr = self
            .local_address()
            .unwrap_or_else(|| "127.0.0.1".to_string());

        let addr = format!("{}:{}", local_addr, self.local_port())
            .parse::<SocketAddr>()
            .expect("Invalid local address");

        let bind = TcpListener::bind(addr).await?;

        let port = bind.local_addr()?.port();

        trace!(port, "Bound to local address and port");

        let server = {
            let http_log_state = http_log_state.inner().clone();
            TcpListenerStream::new(bind).try_for_each(move |client_conn| {
                let pf = self.clone();
                let client_conn = Arc::new(Mutex::new(client_conn));
                let http_log_state = http_log_state.clone();

                async move {
                    if let Ok(peer_addr) = client_conn.lock().await.peer_addr() {
                        trace!(%peer_addr, "new connection");
                    }

                    tokio::spawn(async move {
                        if let Err(e) = pf
                            .forward_connection_with_retries(client_conn, Arc::new(http_log_state))
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
            })
        };

        Ok((
            port,
            tokio::spawn(async {
                if let Err(e) = server.await {
                    error!(error = &e as &dyn std::error::Error, "server error");
                }
            }),
        ))
    }

    async fn forward_connection_with_retries(
        self, client_conn: Arc<Mutex<TcpStream>>, http_log_state: Arc<HttpLogState>,
    ) -> anyhow::Result<()> {
        let mut retries = 0;
        let mut timeout_duration = INITIAL_TIMEOUT;

        loop {
            match self
                .clone()
                .forward_connection(client_conn.clone(), http_log_state.clone())
                .await
            {
                Ok(_) => return Ok(()),
                Err(e) if retries < MAX_RETRIES => {
                    retries += 1;
                    timeout_duration *= 2;
                    error!(
                        error = e.as_ref() as &dyn std::error::Error,
                        retries, "retrying connection"
                    );
                    tokio::time::sleep(timeout_duration).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn forward_connection(
        self, client_conn: Arc<Mutex<TcpStream>>, http_log_state: Arc<HttpLogState>,
    ) -> anyhow::Result<()> {
        let target = self.finder().find(&self.target).await?;

        let (pod_name, pod_port) = target.into_parts();

        let mut forwarder = self.pod_api.portforward(&pod_name, &[pod_port]).await?;

        let upstream_conn = forwarder
            .take_stream(pod_port)
            .context("port not found in forwarder")?;

        let local_port = self.local_port();
        let config_id = self.config_id;
        let workload_type = self.workload_type.clone();

        trace!(local_port, pod_port, pod_name = %pod_name, "forwarding connections");

        let log_file = if workload_type == "service" {
            let log_file_path = create_log_file_path(config_id, local_port)?;
            Some(Arc::new(Mutex::new(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_file_path)?,
            )))
        } else {
            None
        };

        let mut client_conn_guard = client_conn.lock().await;
        let (mut client_reader, mut client_writer) = tokio::io::split(&mut *client_conn_guard);
        let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

        let client_to_upstream = self.create_client_to_upstream_task(
            &mut client_reader,
            &mut upstream_writer,
            log_file.clone(),
            &http_log_state,
        );

        let upstream_to_client = self.create_upstream_to_client_task(
            &mut upstream_reader,
            &mut client_writer,
            log_file.clone(),
            &http_log_state,
        );

        let join_result = tokio::try_join!(client_to_upstream, upstream_to_client);

        let result = tokio::select! {
            res = async { join_result } => res,
            _ = self.detect_connection_close(client_conn.clone(), &mut upstream_reader) => {
                Err(anyhow::anyhow!("Connection closed"))
            }
        };

        match result {
            Ok(_) => {
                trace!(local_port, pod_port, pod_name = %pod_name, "connection closed normally");
            }
            Err(e) => {
                error!(
                    error = e.as_ref() as &dyn std::error::Error,
                    "connection closed with error"
                );
            }
        }

        drop(client_conn_guard);
        drop(forwarder);

        trace!(local_port, pod_port, pod_name = %pod_name, "connection fully closed");

        Ok(())
    }

    async fn detect_connection_close(
        &self, client_conn: Arc<Mutex<TcpStream>>,
        upstream_reader: &mut (impl tokio::io::AsyncRead + Unpin),
    ) -> anyhow::Result<()> {
        let mut client_buffer = [0; 1];
        let mut upstream_buffer = [0; 1];

        let mut client_conn_guard = client_conn.lock().await;
        let (mut client_reader, _) = tokio::io::split(&mut *client_conn_guard);

        loop {
            tokio::select! {
                result = client_reader.read(&mut client_buffer) => {
                    match result {
                        Ok(0) => {
                            trace!("Client connection closed");
                            return Ok(());
                        }
                        Ok(_) => continue,
                        Err(e) => {
                            error!("Error reading from client: {:?}", e);
                            return Err(e.into());
                        }
                    }
                }
                result = upstream_reader.read(&mut upstream_buffer) => {
                    match result {
                        Ok(0) => {
                            trace!("Upstream connection closed");
                            return Ok(());
                        }
                        Ok(_) => continue,
                        Err(e) => {
                            error!("Error reading from upstream: {:?}", e);
                            return Err(e.into());
                        }
                    }
                }
            }
        }
    }

    async fn create_client_to_upstream_task<'a>(
        &'a self, client_reader: &'a mut tokio::io::ReadHalf<&mut TcpStream>,
        upstream_writer: &'a mut tokio::io::WriteHalf<
            impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
        >,
        log_file: Option<Arc<Mutex<std::fs::File>>>, http_log_state: &HttpLogState,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut timeout_duration = INITIAL_TIMEOUT;

        loop {
            let n = match timeout(timeout_duration, client_reader.read(&mut buffer)).await {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => {
                    error!("Error reading from client: {:?}", e);
                    return Err(e.into());
                }
                Err(_) => {
                    error!("Timeout reading from client");
                    return Err(anyhow::anyhow!("Timeout reading from client"));
                }
            };
            if n == 0 {
                break;
            }
            if let Some(log_file) = &log_file {
                if http_log_state.get_http_logs(self.config_id).await {
                    log_request(&buffer[..n], log_file).await?;
                }
            }
            if http_log_state.get_http_logs(self.config_id).await {
                trace!("HTTP Request: {:?}", &buffer[..n]);
            }
            if let Err(e) = upstream_writer.write_all(&buffer[..n]).await {
                error!("Error writing to upstream: {:?}", e);
                return Err(e.into());
            }
            timeout_duration = INITIAL_TIMEOUT;
        }

        // Ensure the upstream_writer is properly shut down
        if let Err(e) = upstream_writer.shutdown().await {
            error!("Error shutting down upstream writer: {:?}", e);
        }

        Ok(())
    }

    async fn create_upstream_to_client_task<'a>(
        &'a self,
        upstream_reader: &'a mut tokio::io::ReadHalf<
            impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
        >,
        client_writer: &'a mut tokio::io::WriteHalf<&mut TcpStream>,
        log_file: Option<Arc<Mutex<std::fs::File>>>, http_log_state: &HttpLogState,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut timeout_duration = INITIAL_TIMEOUT;

        loop {
            let n = match timeout(timeout_duration, upstream_reader.read(&mut buffer)).await {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => {
                    error!("Error reading from upstream: {:?}", e);
                    return Err(e.into());
                }
                Err(_) => {
                    error!("Timeout reading from upstream");
                    return Err(anyhow::anyhow!("Timeout reading from upstream"));
                }
            };
            if n == 0 {
                break;
            }
            if let Some(log_file) = &log_file {
                if http_log_state.get_http_logs(self.config_id).await {
                    log_response(&buffer[..n], log_file).await?;
                }
            }
            if http_log_state.get_http_logs(self.config_id).await {
                trace!("HTTP Response: {:?}", &buffer[..n]);
            }
            if let Err(e) = client_writer.write_all(&buffer[..n]).await {
                error!("Error writing to client: {:?}", e);
                return Err(e.into());
            }
            timeout_duration = INITIAL_TIMEOUT;
        }

        // Ensure the client_writer is properly shut down
        if let Err(e) = client_writer.shutdown().await {
            error!("Error shutting down client writer: {:?}", e);
        }

        Ok(())
    }
    pub fn finder(&self) -> TargetPodFinder {
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

        info!("Local UDP socket bound to {}", local_udp_addr);

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
            let mut udp_buffer = [0u8; BUFFER_SIZE];
            let mut peer: Option<std::net::SocketAddr> = None;

            loop {
                tokio::select! {
                    // Handle incoming UDP packets and forward them to the pod via TCP
                    result = local_udp_socket_read.recv_from(&mut udp_buffer) => {
                        match result {
                            Ok((len, src)) => {
                                peer = Some(src);

                                // Encapsulate the UDP packet in a custom protocol for sending over TCP
                                let packet_len = (len as u32).to_be_bytes();
                                if let Err(e) = tcp_write.write_all(&packet_len).await {
                                    error!("Failed to write packet length to TCP stream: {:?}", e);
                                    break;
                                }
                                if let Err(e) = tcp_write.write_all(&udp_buffer[..len]).await {
                                    error!("Failed to write UDP packet to TCP stream: {:?}", e);
                                    break;
                                }
                                if let Err(e) = tcp_write.flush().await {
                                    error!("Failed to flush TCP stream: {:?}", e);
                                    break;
                                }
                            },
                            Err(e) => {
                                error!("Failed to receive from UDP socket: {:?}", e);
                                break;
                            }
                        }
                    },

                    result = Self::read_tcp_length_and_packet(&mut tcp_read) => {
                        match result {
                            Ok(Some(packet)) => {
                                if let Some(peer) = peer {
                                    if let Err(e) = local_udp_socket_write.send_to(&packet, &peer).await {
                                        error!("Failed to send UDP packet to peer: {:?}", e);
                                        break;
                                    }
                                } else {
                                    error!("No UDP peer to send to");
                                    break;
                                }
                            },
                            Ok(None) => {
                                break;
                            }
                            Err(e) => {
                                error!("Failed to read from TCP stream or send to UDP socket: {:?}", e);
                                break;
                            }
                        }
                    }
                }
            }

            // Ensure the tcp_write is properly shut down
            if let Err(e) = tcp_write.shutdown().await {
                error!("Error shutting down TCP writer: {:?}", e);
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
