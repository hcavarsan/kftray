use std::collections::HashMap;
use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use async_trait::async_trait;
use log::{
    error,
    info,
    warn,
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
};
use russh_keys::key::KeyPair;
use tokio::io::copy_bidirectional;
use tokio::net::{
    TcpListener,
    TcpStream,
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

/// Default timeout for inactive SSH connections in seconds
const INACTIVITY_TIMEOUT: u64 = 3600;
/// Time delay between authentication attempts in seconds
const AUTH_REJECTION_TIME: u64 = 3;
/// Maximum number of connections per client
const MAX_CONNECTIONS_PER_CLIENT: usize = 10;
/// Connection timeout in seconds
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);

/// Represents a forwarded port configuration
#[derive(Clone)]
#[allow(dead_code)]
struct ForwardedPort {
    /// Address the port is bound to
    bind_address: String,
    /// Port number being forwarded
    port: u32,
}

/// Stores information about a connected SSH client
#[derive(Clone)]
#[allow(dead_code)]
struct ClientInfo {
    /// Handle to the SSH server for this client
    handle: russh::server::Handle,
    /// ID of the currently active channel, if any
    channel_id: Option<ChannelId>,
    /// Map of forwarded ports for this client
    forwarded_ports: HashMap<(String, u32), ForwardedPort>,
    /// Number of active connections for this client
    connection_count: usize,
    /// Time of the last connection attempt for this client
    last_connection: Instant,
}

/// SSH proxy implementation that handles SSH connections and port forwarding
#[derive(Clone)]
pub struct SshProxy {
    /// Map of connected clients indexed by client ID
    clients: Arc<Mutex<HashMap<usize, ClientInfo>>>,
    /// List of spawned async tasks
    tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    /// Unique identifier for this proxy instance
    id: usize,
    /// Counter for generating next client ID
    next_id: Arc<Mutex<usize>>,
}

impl Default for SshProxy {
    fn default() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            tasks: Arc::new(Mutex::new(Vec::new())),
            id: 0,
            next_id: Arc::new(Mutex::new(0)),
        }
    }
}

impl SshProxy {
    /// Creates a new SSH proxy instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates the SSH server configuration with default settings
    fn create_config() -> server::Config {
        let key = KeyPair::generate_ed25519();
        server::Config {
            inactivity_timeout: Some(Duration::from_secs(INACTIVITY_TIMEOUT)),
            auth_rejection_time: Duration::from_secs(AUTH_REJECTION_TIME),
            auth_rejection_time_initial: Some(Duration::from_secs(0)),
            keys: vec![key],
            methods: russh::MethodSet::NONE | russh::MethodSet::PUBLICKEY,
            window_size: 2097152,
            maximum_packet_size: 32768,
            preferred: russh::Preferred::default(),
            ..Default::default()
        }
    }
}

impl server::Server for SshProxy {
    type Handler = Self;

    /// Creates a new client handler with a unique ID
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        let mut s = self.clone();
        s.id = {
            let mut next_id = self.next_id.try_lock().expect("Failed to lock next_id");
            let id = *next_id;
            *next_id += 1;
            id
        };
        info!("Created new SSH client handler with ID: {}", s.id);
        s
    }
}

#[async_trait]
impl server::Handler for SshProxy {
    type Error = russh::Error;

    /// Handles authentication requests using the 'none' method
    async fn auth_none(&mut self, user: &str) -> Result<server::Auth, Self::Error> {
        info!("Accepting none authentication for user: {}", user);
        Ok(server::Auth::Accept)
    }

    /// Handles authentication requests using the 'publickey' method
    async fn auth_publickey(
        &mut self, user: &str, public_key: &russh_keys::key::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        info!("Attempting public key authentication for user: {}", user);

        // TODO: Implement proper key validation against authorized_keys
        // For now, we'll continue accepting connections but log the attempt
        warn!(
            "Public key authentication not properly implemented - accepting connection for testing"
        );
        info!("Public key fingerprint: {}", public_key.fingerprint());

        Ok(server::Auth::Accept)
    }

    /// Handles requests to open a new SSH session
    async fn channel_open_session(
        &mut self, channel: Channel<Msg>, session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let mut clients = self.clients.lock().await;

        if let Some(client) = clients.get(&self.id) {
            if client.connection_count >= MAX_CONNECTIONS_PER_CLIENT {
                error!("Too many connections for client {}", self.id);
                return Ok(false);
            }

            if client.last_connection.elapsed() < CONNECTION_TIMEOUT {
                error!("Connection attempt too soon for client {}", self.id);
                return Ok(false);
            }
        }

        clients.insert(
            self.id,
            ClientInfo {
                channel_id: Some(channel.id()),
                handle: session.handle(),
                forwarded_ports: HashMap::new(),
                connection_count: 1,
                last_connection: Instant::now(),
            },
        );

        Ok(true)
    }
    /// Handles TCP port forwarding requests
    async fn tcpip_forward(
        &mut self, address: &str, port: &mut u32, session: &mut Session,
    ) -> Result<bool, Self::Error> {
        info!(
            "Port forwarding request for {}:{} from client {}",
            address, port, self.id
        );

        let bind_addr = format!("{}:{}", address, port);
        match TcpListener::bind(&bind_addr).await {
            Ok(listener) => {
                info!(
                    "Successfully bound listener to {} for client {}",
                    bind_addr, self.id
                );

                // Store forwarding information
                {
                    let mut clients = self.clients.lock().await;
                    if let Some(client_info) = clients.get_mut(&self.id) {
                        client_info.forwarded_ports.insert(
                            (address.to_string(), *port),
                            ForwardedPort {
                                bind_address: address.to_string(),
                                port: *port,
                            },
                        );
                    }
                }

                session.request_success();

                let handle = session.handle();
                let client_id = self.id;
                let port = *port;
                let address = address.to_string();

                let accept_task = tokio::spawn(async move {
                    while let Ok((inbound, addr)) = listener.accept().await {
                        info!(
                            "New connection accepted from {} on tunnel for client {}",
                            addr, client_id
                        );

                        let _ = inbound.set_nodelay(true);
                        let handle = handle.clone();
                        let address = address.clone();

                        tokio::spawn(async move {
                            match handle
                                .channel_open_forwarded_tcpip(
                                    address,
                                    port,
                                    addr.ip().to_string(),
                                    addr.port() as u32,
                                )
                                .await
                            {
                                Ok(channel) => {
                                    let mut channel_stream = channel.into_stream();
                                    let mut inbound = inbound;

                                    if let Err(e) =
                                        copy_bidirectional(&mut inbound, &mut channel_stream).await
                                    {
                                        error!("Connection error: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to open forwarded-tcpip channel for {}: {}",
                                        addr, e
                                    );
                                }
                            }
                        });
                    }
                });

                let mut tasks = self.tasks.lock().await;
                tasks.push(accept_task);

                Ok(true)
            }
            Err(e) => {
                error!("Failed to bind {}: {}", bind_addr, e);
                session.request_failure();
                Ok(false)
            }
        }
    }

    async fn channel_open_direct_tcpip(
        &mut self, channel: Channel<Msg>, host_to_connect: &str, port_to_connect: u32,
        _originator_address: &str, _originator_port: u32, session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let channel_id = channel.id();
        session.channel_success(channel_id);

        let target_addr = format!("{}:{}", host_to_connect, port_to_connect);
        match TcpStream::connect(&target_addr).await {
            Ok(target_stream) => {
                let _ = target_stream.set_nodelay(true);
                let tasks = self.tasks.clone();

                let handle = tokio::spawn(async move {
                    let mut channel_stream = channel.into_stream();
                    let mut target_stream = target_stream;

                    if let Err(e) =
                        copy_bidirectional(&mut target_stream, &mut channel_stream).await
                    {
                        error!("Direct-tcpip tunnel error: {}", e);
                    }
                });

                tasks.lock().await.push(handle);
                Ok(true)
            }
            Err(e) => {
                error!("Failed to connect to {}: {}", target_addr, e);
                session.channel_open_failure(
                    channel_id,
                    russh::ChannelOpenFailure::ConnectFailed,
                    &format!("Failed to connect: {}", e),
                    "en-US",
                );
                Ok(false)
            }
        }
    }
}

#[async_trait]
impl ProxyHandler for SshProxy {
    async fn start(&self, config: ProxyConfig, shutdown: Arc<Notify>) -> Result<(), ProxyError> {
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
                info!("Shutdown signal received");
            }
        }

        Ok(())
    }
}

impl Drop for SshProxy {
    fn drop(&mut self) {
        let id = self.id;
        let clients = self.clients.clone();
        let tasks = self.tasks.clone();

        tokio::spawn(async move {
            info!("Cleaning up resources for client {}", id);
            let mut tasks = tasks.lock().await;
            for task in tasks.drain(..) {
                task.abort();
            }

            if let Ok(mut clients) = clients.try_lock() {
                clients.remove(&id);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::config::ProxyType;

    #[tokio::test]
    async fn test_ssh_proxy_startup() {
        let config = ProxyConfig::builder()
            .target_host("127.0.0.1".to_string())
            .target_port(2222)
            .proxy_port(0)
            .proxy_type(ProxyType::Ssh)
            .build()
            .unwrap();

        let proxy = SshProxy::new();
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let handle = tokio::spawn(async move { proxy.start(config, shutdown).await });

        tokio::time::sleep(Duration::from_secs(1)).await;
        shutdown_clone.notify_one();

        match tokio::time::timeout(Duration::from_secs(5), handle).await {
            Ok(result) => {
                assert!(result.unwrap().is_ok(), "Server should shut down cleanly");
            }
            Err(_) => panic!("Server shutdown timed out"),
        }
    }

    #[tokio::test]
    async fn test_ssh_proxy_connection_handling() {
        let _ = env_logger::try_init();

        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };

        let config = ProxyConfig::builder()
            .target_host("127.0.0.1".to_string())
            .target_port(2222)
            .proxy_port(port)
            .proxy_type(ProxyType::Ssh)
            .build()
            .unwrap();

        let proxy = SshProxy::new();
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let server_handle = tokio::spawn(async move { proxy.start(config, shutdown).await });

        tokio::time::sleep(Duration::from_millis(100)).await;
        shutdown_clone.notify_one();

        match tokio::time::timeout(Duration::from_secs(5), server_handle).await {
            Ok(result) => assert!(result.unwrap().is_ok(), "Server should shut down cleanly"),
            Err(_) => panic!("Server shutdown timed out"),
        }
    }
}
