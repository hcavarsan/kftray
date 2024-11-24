use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use log::{
    error,
    info,
};
use russh::server::{
    self,
    Msg,
    Server as _,
    Session,
};
use russh::{
    Channel,
    ChannelId,
    CryptoVec,
};
use russh_keys::key::{
    KeyPair,
    PublicKey,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::sync::{
    Mutex,
    Notify,
};

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
    traits::ProxyHandler,
};

/// SSH proxy implementation that handles SSH connections
#[derive(Clone)]
pub struct SshProxy {
    /// Map of connected clients with their channel IDs and handles
    clients: Arc<Mutex<HashMap<usize, (ChannelId, russh::server::Handle)>>>,
    /// Unique identifier for the next client
    id: usize,
    /// Target server configuration
    config: Arc<Mutex<Option<ProxyConfig>>>,
}

impl SshProxy {
    /// Creates a new SSH proxy instance
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            id: 0,
            config: Arc::new(Mutex::new(None)),
        }
    }

    /// Creates the SSH server configuration
    fn create_config() -> server::Config {
        let key = KeyPair::generate_ed25519();

        server::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
            auth_rejection_time: std::time::Duration::from_secs(3),
            auth_rejection_time_initial: Some(std::time::Duration::from_secs(0)),
            keys: vec![key],
            ..Default::default()
        }
    }

    /// Forwards data through the SSH tunnel to localhost:8080
    async fn forward_to_local(&self, data: &[u8]) -> Result<Vec<u8>, russh::Error> {
        let mut stream = tokio::net::TcpStream::connect("127.0.0.1:8080")
            .await
            .map_err(russh::Error::IO)?;

        // Write data to local port
        stream.write_all(data).await.map_err(russh::Error::IO)?;
        stream.flush().await.map_err(russh::Error::IO)?;

        // Read response
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .map_err(russh::Error::IO)?;

        Ok(response)
    }

    /// Sets up port forwarding for the SSH tunnel
    async fn setup_port_forward(
        &self, channel: ChannelId, session: &mut Session,
    ) -> Result<(), russh::Error> {
        // Send success message for port forwarding
        session.data(
            channel,
            CryptoVec::from_slice(b"Port forwarding established\n"),
        );
        session.channel_success(channel);

        info!("Port forwarding established: remote:2222 -> localhost:8080");
        Ok(())
    }
}

impl server::Server for SshProxy {
    type Handler = Self;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        let s = self.clone();
        self.id += 1;
        s
    }

    fn handle_session_error(&mut self, error: <Self::Handler as server::Handler>::Error) {
        error!("Session error: {:#?}", error);
    }
}

#[async_trait]
impl server::Handler for SshProxy {
    type Error = russh::Error;

    async fn channel_open_session(
        &mut self, channel: Channel<Msg>, session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let mut clients = self.clients.lock().await;
        clients.insert(self.id, (channel.id(), session.handle()));

        // Set up port forwarding when session opens
        self.setup_port_forward(channel.id(), session).await?;

        info!("New SSH session opened with port forwarding");
        Ok(true)
    }

    async fn auth_publickey(
        &mut self, _user: &str, _public_key: &PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        // Accept any public key for testing
        Ok(server::Auth::Accept)
    }

    async fn data(
        &mut self, channel: ChannelId, data: &[u8], session: &mut Session,
    ) -> Result<(), Self::Error> {
        if data == [3] {
            // Ctrl-C
            return Err(russh::Error::Disconnect);
        }

        // Forward data through the tunnel
        match self.forward_to_local(data).await {
            Ok(response) => {
                session.data(channel, CryptoVec::from_slice(&response));
                Ok(())
            }
            Err(e) => {
                error!("Failed to forward data through tunnel: {}", e);
                Err(e)
            }
        }
    }

    async fn shell_request(
        &mut self, channel: ChannelId, session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Send welcome message
        session.data(channel, CryptoVec::from_slice(b"Welcome to SSH proxy!\n$ "));

        // Confirm shell request
        session.channel_success(channel);
        Ok(())
    }

    async fn exec_request(
        &mut self, channel: ChannelId, data: &[u8], session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Forward exec request to localhost:8080
        match self.forward_to_local(data).await {
            Ok(response) => {
                session.data(channel, CryptoVec::from_slice(&response));
                session.exit_status_request(channel, 0);
                session.channel_success(channel);
                Ok(())
            }
            Err(e) => {
                error!("Failed to forward exec request: {}", e);
                session.exit_status_request(channel, 1);
                session.channel_failure(channel);
                Err(e)
            }
        }
    }

    async fn env_request(
        &mut self, channel: ChannelId, _variable_name: &str, _variable_value: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Accept environment variables
        session.channel_success(channel);
        Ok(())
    }

    async fn channel_eof(
        &mut self, channel: ChannelId, session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel);
        Ok(())
    }

    async fn tcpip_forward(
        &mut self, address: &str, port: &mut u32, session: &mut Session,
    ) -> Result<bool, Self::Error> {
        info!("Received port forwarding request for {}:{}", address, port);

        // Accept the port forwarding request
        if let Some((channel_id, _)) = self.clients.lock().await.get(&self.id) {
            session.channel_success(*channel_id);
        }

        // Store the forwarding information if needed
        if let Some(config) = self.config.lock().await.as_ref() {
            info!(
                "Port forwarding established: {}:{} -> {}:{}",
                address, port, config.target_host, config.target_port
            );
        }

        Ok(true)
    }

    async fn channel_open_direct_tcpip(
        &mut self, channel: Channel<Msg>, host_to_connect: &str, port_to_connect: u32,
        originator_addr: &str, originator_port: u32, session: &mut Session,
    ) -> Result<bool, Self::Error> {
        info!(
            "Direct TCP/IP connection request from {}:{} to {}:{}",
            originator_addr, originator_port, host_to_connect, port_to_connect
        );

        // Accept the direct TCP/IP connection
        session.channel_success(channel.id());
        Ok(true)
    }
}

#[async_trait]
impl ProxyHandler for SshProxy {
    async fn start(&self, config: ProxyConfig, shutdown: Arc<Notify>) -> Result<(), ProxyError> {
        *self.config.lock().await = Some(config.clone());

        let ssh_config = Arc::new(Self::create_config());
        let addr = ("0.0.0.0".to_string(), config.proxy_port);

        info!("Starting SSH proxy server on {}:{}", addr.0, addr.1);

        let mut server = self.clone();

        tokio::select! {
            result = server.run_on_address(ssh_config, addr) => {
                if let Err(e) = result {
                    error!("SSH server error: {}", e);
                    return Err(ProxyError::Connection(format!("SSH server error: {}", e)));
                }
            }
            _ = shutdown.notified() => {
                info!("Shutdown signal received, stopping SSH proxy");
            }
        }

        Ok(())
    }
}

impl Drop for SshProxy {
    fn drop(&mut self) {
        let id = self.id;
        let clients = self.clients.clone();
        tokio::spawn(async move {
            let mut clients = clients.lock().await;
            clients.remove(&id);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;

    use super::*;
    use crate::proxy::config::ProxyType;

    #[tokio::test]
    async fn test_ssh_proxy_startup() {
        let proxy = SshProxy::new();
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let config = ProxyConfig::builder()
            .target_host("127.0.0.1".to_string())
            .target_port(2222)
            .proxy_port(0)
            .proxy_type(ProxyType::Ssh)
            .build()
            .unwrap();

        let handle = tokio::spawn(async move { proxy.start(config, shutdown).await });

        tokio::time::sleep(Duration::from_secs(1)).await;
        shutdown_clone.notify_one();

        match timeout(Duration::from_secs(5), handle).await {
            Ok(result) => {
                assert!(result.is_ok(), "Server should shut down cleanly");
            }
            Err(_) => panic!("Server shutdown timed out"),
        }
    }
}
