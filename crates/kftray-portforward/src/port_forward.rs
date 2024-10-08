use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use anyhow::Context;
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
use tokio::net::TcpStream;
use tokio::net::UdpSocket as TokioUdpSocket;
use tokio::sync::Mutex;
use tokio::sync::Notify;
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
    debug,
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

lazy_static! {
    pub static ref CHILD_PROCESSES: Arc<StdMutex<HashMap<String, JoinHandle<()>>>> =
        Arc::new(StdMutex::new(HashMap::new()));
    pub static ref CANCEL_NOTIFIER: Arc<Notify> = Arc::new(Notify::new());
}

const BUFFER_SIZE: usize = 131072;

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
            context_name: context_name.clone(),
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
        self, http_log_state: Arc<HttpLogState>,
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
            let cancel_notifier = CANCEL_NOTIFIER.clone();
            let http_log_state = http_log_state.clone();
            TcpListenerStream::new(bind).try_for_each(move |client_conn| {
                let pf = self.clone();
                let client_conn = Arc::new(Mutex::new(client_conn));
                let http_log_state = http_log_state.clone();
                let cancel_notifier = cancel_notifier.clone();
                async move {
                    if let Ok(peer_addr) = client_conn.lock().await.peer_addr() {
                        trace!(%peer_addr, "new connection");
                    }

                    {
                        let conn = client_conn.lock().await;
                        conn.set_nodelay(true)?;
                    }

                    let cancel_notifier_clone = cancel_notifier.clone();

                    tokio::spawn(async move {
                        if let Err(e) = pf
                            .forward_connection(client_conn, http_log_state, cancel_notifier_clone)
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

    pub fn finder(&self) -> TargetPodFinder {
        TargetPodFinder {
            pod_api: &self.pod_api,
            svc_api: &self.svc_api,
        }
    }

    async fn forward_connection(
        self, client_conn: Arc<Mutex<TcpStream>>, http_log_state: Arc<HttpLogState>,
        cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let target = self.finder().find(&self.target).await?;

        debug!("Forwarding connection to target pod");
        debug!("Target pod: {:?}", target);

        let (pod_name, pod_port) = target.into_parts();

        debug!("Pod name: {}", pod_name);
        debug!("Pod port: {}", pod_port);

        let mut forwarder = self.pod_api.portforward(&pod_name, &[pod_port]).await?;

        debug!("Forwarder created");

        let upstream_conn = forwarder
            .take_stream(pod_port)
            .context("port not found in forwarder")?;

        let local_port = self.local_port();
        debug!("Local port: {}", local_port);
        let config_id = self.config_id;
        debug!("Config ID: {}", config_id);
        let workload_type = self.workload_type.clone();
        debug!("Workload type: {}", workload_type);

        trace!(local_port, pod_port, pod_name = %pod_name, "forwarding connections");

        let logger = if workload_type == "service" || workload_type == "pod" {
            let log_file_path = create_log_file_path(config_id, local_port).await?;
            let logger = Logger::new(log_file_path).await?;
            Some(logger)
        } else {
            None
        };

        debug!("Logger created");
        debug!("Logger: {:?}", logger);

        let request_id = Arc::new(Mutex::new(None));

        debug!("Request ID created");
        debug!("Request ID: {:?}", request_id);

        let mut client_conn_guard = client_conn.lock().await;
        client_conn_guard.set_nodelay(true)?;
        let (mut client_reader, mut client_writer) = tokio::io::split(&mut *client_conn_guard);

        let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

        let client_to_upstream = self.create_client_to_upstream_task(
            &mut client_reader,
            &mut upstream_writer,
            logger.clone(),
            &http_log_state,
            Arc::clone(&request_id),
            cancel_notifier.clone(),
        );

        let upstream_to_client = self.create_upstream_to_client_task(
            &mut upstream_reader,
            &mut client_writer,
            logger.clone(),
            &http_log_state,
            Arc::clone(&request_id),
            cancel_notifier.clone(),
        );

        let join_result = tokio::try_join!(client_to_upstream, upstream_to_client);

        let result = tokio::select! {
            res = async { join_result } => res,
            _ = self.detect_connection_close(client_conn.clone(), &mut upstream_reader, cancel_notifier) => {
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
        drop(upstream_reader);
        trace!(local_port, pod_port, pod_name = %pod_name, "connection fully closed");

        Ok(())
    }

    async fn create_client_to_upstream_task<'a>(
        &'a self, client_reader: &'a mut tokio::io::ReadHalf<&mut TcpStream>,
        upstream_writer: &'a mut tokio::io::WriteHalf<
            impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
        >,
        logger: Option<Logger>, http_log_state: &HttpLogState,
        request_id: Arc<Mutex<Option<String>>>, cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut timeout_duration = Duration::from_secs(600);
        let mut request_buffer = Vec::new();

        loop {
            tokio::select! {
                n = timeout(timeout_duration, client_reader.read(&mut buffer)) => {
                    let n = match n {
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

                    trace!("Read {} bytes from client", n);
                    request_buffer.extend_from_slice(&buffer[..n]);

                        if http_log_state.get_http_logs(self.config_id).await {
                            if let Some(logger) = &logger {
                                let mut req_id_guard = request_id.lock().await;
                                let new_request_id =
                                    logger.log_request(request_buffer.clone().into()).await;
                                trace!("Generated new request ID: {}", new_request_id);
                                *req_id_guard = Some(new_request_id);
                            }
                        }

                        if let Err(e) = upstream_writer.write_all(&request_buffer).await {
                            error!("Error writing to upstream: {:?}", e);
                            return Err(e.into());
                        }
                        request_buffer.clear();
                },

                _ = cancel_notifier.notified() => {
                    trace!("Client to upstream task cancelled");
                    break;
                }
            }

            timeout_duration = Duration::from_secs(600);
        }

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
        client_writer: &'a mut tokio::io::WriteHalf<&mut TcpStream>, logger: Option<Logger>,
        http_log_state: &HttpLogState, request_id: Arc<Mutex<Option<String>>>,
        cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut timeout_duration = Duration::from_secs(600);
        let mut response_buffer = Vec::new();

        loop {
            tokio::select! {
                n = timeout(timeout_duration, upstream_reader.read(&mut buffer)) => {
                    let n = match n {
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

                    trace!("Read {} bytes from upstream", n);
                    response_buffer.extend_from_slice(&buffer[..n]);

                        if http_log_state.get_http_logs(self.config_id).await {
                            if let Some(logger) = &logger {
                                let req_id_guard = request_id.lock().await;
                                if let Some(req_id) = &*req_id_guard {
                                    trace!("Logging response for request ID: {}", req_id);
                                    logger
                                        .log_response(response_buffer.clone().into(), req_id.clone())
                                        .await;
                                }
                            }
                        }

                        if let Err(e) = client_writer.write_all(&response_buffer).await {
                            error!("Error writing to client: {:?}", e);
                            return Err(e.into());
                        }

                        response_buffer.clear();


                    timeout_duration = Duration::from_secs(600);
                },
                _ = cancel_notifier.notified() => {
                    trace!("Upstream to client task cancelled");
                    break;
                }
            }
        }

        if let Err(e) = client_writer.shutdown().await {
            error!("Error shutting down client writer: {:?}", e);
        }

        Ok(())
    }
    async fn detect_connection_close(
        &self, client_conn: Arc<Mutex<TcpStream>>,
        upstream_reader: &mut (impl tokio::io::AsyncRead + Unpin), cancel_notifier: Arc<Notify>,
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
                            debug!("Client connection closed");
                            return Ok(());
                        }
                        Ok(_) => continue,
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(e) => {
                            error!("Error reading from client: {:?}", e);
                            return Err(e.into());
                        }
                    }
                }
                result = upstream_reader.read(&mut upstream_buffer) => {
                    match result {
                        Ok(0) => {
                            debug!("Upstream connection closed");
                            return Ok(());
                        }
                        Ok(_) => continue,
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(e) => {
                            error!("Error reading from upstream: {:?}", e);
                            return Err(e.into());
                        }
                    }
                }
                _ = cancel_notifier.notified() => {
                    debug!("Cancellation signal received");
                    return Ok(());
                }
            }
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
                    result = local_udp_socket_read.recv_from(&mut udp_buffer) => {
                        match result {
                            Ok((len, src)) => {
                                peer = Some(src);

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
