use std::net::{
    IpAddr,
    Ipv4Addr,
    SocketAddr,
};

use async_ssh2_lite::{
    AsyncSession,
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
use tokio::time::sleep;
use uuid::Uuid;

use crate::error::Error;
use crate::expose::config::TunnelConfig;

pub struct SshTunnel {
    config: TunnelConfig,
    session: Option<AsyncSession<TokioTcpStream>>,
}

impl SshTunnel {
    pub fn new(config: TunnelConfig) -> Self {
        Self {
            config,
            session: None,
        }
    }

    pub async fn connect_and_forward(&mut self) -> Result<(), Error> {
        let mut retries = 0;
        while retries < self.config.max_retries {
            match self.try_connect_and_forward().await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    error!("Connection attempt {} failed: {}", retries + 1, e);
                    retries += 1;
                    if retries < self.config.max_retries {
                        sleep(self.config.retry_delay).await;
                    }
                }
            }
        }
        Err(Error::Connection("Max retries exceeded".into()))
    }

    async fn try_connect_and_forward(&mut self) -> Result<(), Error> {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.config.local_port);

        let mut session = AsyncSession::<TokioTcpStream>::connect(addr, None).await?;
        session.handshake().await?;

        session
            .userauth_pubkey_file("kftray", None, &self.config.ssh_key_path, None)
            .await?;

        info!("SSH tunnel established on port {}", self.config.local_port);

        self.session = Some(session);

        let connection_handler = ConnectionHandler::new(self.config.clone());
        connection_handler
            .handle_connections(self.session.as_mut().unwrap())
            .await
    }
}

struct ConnectionHandler {
    config: TunnelConfig,
}

impl ConnectionHandler {
    fn new(config: TunnelConfig) -> Self {
        Self { config }
    }

    async fn handle_connections(
        &self, session: &mut AsyncSession<TokioTcpStream>,
    ) -> Result<(), Error> {
        info!("Starting tunnel connection handler");

        let (mut listener, _) = session
            .channel_forward_listen(self.config.remote_port, Some("0.0.0.0"), None)
            .await?;

        let mut last_activity = std::time::Instant::now();

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok(channel) => {
                            last_activity = std::time::Instant::now();
                            self.handle_new_connection(channel).await?;
                        }
                        Err(e) => return Err(Error::Connection(e.to_string())),
                    }
                }
                _ = tokio::time::sleep(self.config.keepalive_interval) => {
                    if last_activity.elapsed() > self.config.keepalive_interval {
                        debug!("Sending keepalive");
                        session.keepalive_send().await?;
                        last_activity = std::time::Instant::now();
                    }
                }
            }
        }
    }

    async fn handle_new_connection(
        &self, channel: async_ssh2_lite::AsyncChannel<TokioTcpStream>,
    ) -> Result<(), Error> {
        let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.config.remote_port);

        let local_stream = TokioTcpStream::connect(local_addr).await?;

        let config = self.config.clone();
        tokio::spawn(async move {
            if let Err(e) = Self::process_connection(channel, local_stream, config).await {
                error!("Connection error: {}", e);
            }
        });

        Ok(())
    }

    async fn process_connection(
        mut channel: async_ssh2_lite::AsyncChannel<TokioTcpStream>,
        mut local_stream: TokioTcpStream, config: TunnelConfig,
    ) -> Result<(), Error> {
        let connection_id = Uuid::new_v4();
        let mut channel_buffer = vec![0; config.buffer_size];
        let mut stream_buffer = vec![0; config.buffer_size];
        let mut bytes_transferred = 0u64;

        loop {
            tokio::select! {
                result = channel.read(&mut channel_buffer) => {
                    match result {
                        Ok(0) => break,
                        Ok(n) => {
                            bytes_transferred += n as u64;
                            local_stream.write_all(&channel_buffer[..n]).await?;
                        }
                        Err(e) => return Err(Error::Io(e)),
                    }
                }
                result = local_stream.read(&mut stream_buffer) => {
                    match result {
                        Ok(0) => break,
                        Ok(n) => {
                            bytes_transferred += n as u64;
                            channel.write_all(&stream_buffer[..n]).await?;
                        }
                        Err(e) => return Err(Error::Io(e)),
                    }
                }
            }
        }

        info!(
            "Connection {} closed. Total bytes transferred: {}",
            connection_id, bytes_transferred
        );

        Ok(())
    }
}
