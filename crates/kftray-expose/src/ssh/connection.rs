use std::net::{
    IpAddr,
    Ipv4Addr,
    SocketAddr,
};

use async_ssh2_lite::{
    AsyncChannel,
    AsyncListener,
    TokioTcpStream,
};
use log::{
    debug,
    error,
    info,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use uuid::Uuid;

use crate::{
    config::TunnelConfig,
    error::*,
};

pub struct ConnectionHandler {
    config: TunnelConfig,
}

impl ConnectionHandler {
    pub fn new(config: TunnelConfig) -> Self {
        Self { config }
    }

    pub async fn handle_connections(
        &self, remote_listener: &mut AsyncListener<TokioTcpStream>,
    ) -> TunnelResult<()> {
        info!("Starting tunnel connection handler");
        debug!(
            "Listening for incoming connections on remote port {}",
            self.config.remote_port
        );

        let mut connection_count = 0;
        let mut last_activity = std::time::Instant::now();

        loop {
            tokio::select! {
                accept_result = remote_listener.accept() => {
                    match accept_result {
                        Ok(channel) => {
                            connection_count += 1;
                            last_activity = std::time::Instant::now();

                            self.spawn_connection_handler(
                                channel,
                                connection_count,
                            ).await?;
                        }
                        Err(e) => {
                            error!("Failed to accept channel: {}", e);
                            return Err(e.into());
                        }
                    }
                }
                _ = tokio::time::sleep(self.config.keepalive_interval) => {
                    if last_activity.elapsed() > self.config.keepalive_interval {
                        debug!("Sending keepalive");
                        last_activity = std::time::Instant::now();
                    }
                }
            }
        }
    }

    async fn spawn_connection_handler(
        &self, channel: AsyncChannel<TokioTcpStream>, connection_count: u32,
    ) -> TunnelResult<()> {
        info!(
            "New connection accepted (#{}) - setting up tunnel",
            connection_count
        );

        let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.config.remote_port);

        debug!("Attempting to connect to local service at {}", local_addr);

        match TokioTcpStream::connect(local_addr).await {
            Ok(local_stream) => {
                info!(
                    "Local connection established for tunnel #{}",
                    connection_count
                );
                let config = self.config.clone();
                tokio::spawn(Self::handle_tunnel_connection(
                    channel,
                    local_stream,
                    local_addr,
                    config,
                ));
                Ok(())
            }
            Err(e) => {
                error!(
                    "Failed to connect to local service for tunnel #{}: {}",
                    connection_count, e
                );
                Err(e.into())
            }
        }
    }

    async fn handle_tunnel_connection(
        mut channel: AsyncChannel<TokioTcpStream>, mut local_stream: TokioTcpStream,
        _addr: SocketAddr, config: TunnelConfig,
    ) {
        let connection_id = Uuid::new_v4();
        info!("Starting tunnel connection handler {}", connection_id);

        let mut channel_buf = vec![0; config.buffer_size];
        let mut stream_buf = vec![0; config.buffer_size];
        let mut bytes_forwarded = 0u64;

        loop {
            tokio::select! {
                result = channel.read(&mut channel_buf) => {
                    if !Self::handle_channel_read(
                        &mut local_stream,
                        &channel_buf,
                        result,
                        connection_id,
                        &mut bytes_forwarded,
                    ).await {
                        break;
                    }
                }
                result = local_stream.read(&mut stream_buf) => {
                    if !Self::handle_stream_read(
                        &mut channel,
                        &stream_buf,
                        result,
                        connection_id,
                        &mut bytes_forwarded,
                    ).await {
                        break;
                    }
                }
            }
        }

        info!(
            "Connection {} closed. Total bytes forwarded: {}",
            connection_id, bytes_forwarded
        );
        if let Err(e) = channel.close().await {
            error!(
                "Error closing channel for connection {}: {}",
                connection_id, e
            );
        }
    }

    async fn handle_channel_read(
        local_stream: &mut TokioTcpStream, channel_buf: &[u8], result: std::io::Result<usize>,
        connection_id: Uuid, bytes_forwarded: &mut u64,
    ) -> bool {
        match result {
            Ok(0) => {
                debug!("Channel EOF received for connection {}", connection_id);
                false
            }
            Ok(n) => {
                *bytes_forwarded += n as u64;
                debug!(
                    "Forwarding {} bytes from remote to local (total: {})",
                    n, bytes_forwarded
                );
                match local_stream.write_all(&channel_buf[..n]).await {
                    Ok(_) => true,
                    Err(e) => {
                        error!(
                            "Failed to write to local stream for connection {}: {}",
                            connection_id, e
                        );
                        false
                    }
                }
            }
            Err(e) => {
                error!("Channel read error for connection {}: {}", connection_id, e);
                false
            }
        }
    }

    async fn handle_stream_read(
        channel: &mut AsyncChannel<TokioTcpStream>, stream_buf: &[u8],
        result: std::io::Result<usize>, connection_id: Uuid, bytes_forwarded: &mut u64,
    ) -> bool {
        match result {
            Ok(0) => {
                debug!("Local stream EOF received for connection {}", connection_id);
                false
            }
            Ok(n) => {
                *bytes_forwarded += n as u64;
                debug!(
                    "Forwarding {} bytes from local to remote (total: {})",
                    n, bytes_forwarded
                );
                match channel.write_all(&stream_buf[..n]).await {
                    Ok(_) => true,
                    Err(e) => {
                        error!(
                            "Failed to write to channel for connection {}: {}",
                            connection_id, e
                        );
                        false
                    }
                }
            }
            Err(e) => {
                error!(
                    "Local stream read error for connection {}: {}",
                    connection_id, e
                );
                false
            }
        }
    }
}
