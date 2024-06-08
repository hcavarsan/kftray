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
use tokio::net::UdpSocket as TokioUdpSocket;
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

use crate::kubeforward::logging::{
    create_log_file_path,
    log_request,
    log_response,
};
use crate::kubeforward::pod_finder::TargetPodFinder;
use crate::models::kube::{
    PortForward,
    Target,
};

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
        })
    }

    pub fn local_port(&self) -> u16 {
        self.local_port.unwrap_or(0)
    }

    pub fn local_address(&self) -> Option<String> {
        self.local_address.clone()
    }

    pub async fn port_forward_tcp(self) -> anyhow::Result<(u16, tokio::task::JoinHandle<()>)> {
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

    pub async fn forward_connection(
        self, client_conn: tokio::net::TcpStream,
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

        tracing::debug!(local_port, pod_port, pod_name = %pod_name, "forwarding connections");

        let log_file = if workload_type == "service" {
            let log_file_path = create_log_file_path(config_id, local_port)?;
            Some(Arc::new(tokio::sync::Mutex::new(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_file_path)?,
            )))
        } else {
            None
        };

        let (mut client_reader, mut client_writer) = tokio::io::split(client_conn);
        let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

        let client_to_upstream = self.create_client_to_upstream_task(
            &mut client_reader,
            &mut upstream_writer,
            log_file.clone(),
        );

        let upstream_to_client = self.create_upstream_to_client_task(
            &mut upstream_reader,
            &mut client_writer,
            log_file.clone(),
        );

        tokio::try_join!(client_to_upstream, upstream_to_client)?;

        forwarder.join().await?;

        tracing::debug!(local_port, pod_port, pod_name = %pod_name, "connection closed");

        Ok(())
    }

    async fn create_client_to_upstream_task<'a>(
        &'a self, client_reader: &'a mut tokio::io::ReadHalf<tokio::net::TcpStream>,
        upstream_writer: &'a mut tokio::io::WriteHalf<
            impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
        >,
        log_file: Option<Arc<tokio::sync::Mutex<std::fs::File>>>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; 65536];
        loop {
            let n = match timeout(Duration::from_secs(30), client_reader.read(&mut buffer)).await {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => {
                    tracing::error!("Error reading from client: {:?}", e);
                    return Err(e.into());
                }
                Err(_) => {
                    tracing::error!("Timeout reading from client");
                    return Err(anyhow::anyhow!("Timeout reading from client"));
                }
            };
            if n == 0 {
                break;
            }
            if let Some(log_file) = &log_file {
                log_request(&buffer[..n], log_file).await?;
            }
            if let Err(e) = upstream_writer.write_all(&buffer[..n]).await {
                tracing::error!("Error writing to upstream: {:?}", e);
                return Err(e.into());
            }
        }
        Ok(())
    }

    async fn create_upstream_to_client_task<'a>(
        &'a self,
        upstream_reader: &'a mut tokio::io::ReadHalf<
            impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
        >,
        client_writer: &'a mut tokio::io::WriteHalf<tokio::net::TcpStream>,
        log_file: Option<Arc<tokio::sync::Mutex<std::fs::File>>>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; 65536]; // Increased buffer size
        loop {
            let n = match timeout(Duration::from_secs(30), upstream_reader.read(&mut buffer)).await
            {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => {
                    tracing::error!("Error reading from upstream: {:?}", e);
                    return Err(e.into());
                }
                Err(_) => {
                    tracing::error!("Timeout reading from upstream");
                    return Err(anyhow::anyhow!("Timeout reading from upstream"));
                }
            };
            if n == 0 {
                break;
            }
            if let Some(log_file) = &log_file {
                log_response(&buffer[..n], log_file).await?;
            }
            if let Err(e) = client_writer.write_all(&buffer[..n]).await {
                tracing::error!("Error writing to client: {:?}", e);
                return Err(e.into());
            }
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
