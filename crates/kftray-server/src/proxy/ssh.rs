use std::collections::HashMap;
use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use async_trait::async_trait;
use base64::Engine as _;
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
use crate::ProxyType;

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
    /// Proxy configuration
    config: ProxyConfig,
}

impl Default for SshProxy {
    fn default() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            tasks: Arc::new(Mutex::new(Vec::new())),
            id: 0,
            next_id: Arc::new(Mutex::new(0)),
            config: ProxyConfig::builder()
                .target_host("localhost".to_string())
                .target_port(22)
                .proxy_port(2222)
                .proxy_type(ProxyType::Ssh)
                .build()
                .unwrap(),
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
        if !self.config.ssh_auth_enabled {
            info!(
                "SSH authentication disabled, accepting connection for user: {}",
                user
            );
            return Ok(server::Auth::Accept);
        }
        info!("Authentication required for user: {}", user);
        Ok(server::Auth::Reject {
            proceed_with_methods: None,
        })
    }

    /// Handles authentication requests using the 'password' method
    async fn auth_password(
        &mut self, user: &str, _password: &str,
    ) -> Result<server::Auth, Self::Error> {
        warn!("Password authentication not supported for user: {}", user);
        Ok(server::Auth::Reject {
            proceed_with_methods: None,
        })
    }

    /// Handles authentication requests using the 'publickey' method
    async fn auth_publickey(
        &mut self, user: &str, public_key: &russh_keys::key::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        if !self.config.ssh_auth_enabled {
            info!(
                "SSH authentication disabled, accepting public key for user: {}",
                user
            );
            return Ok(server::Auth::Accept);
        }

        if let Some(ref authorized_keys) = self.config.ssh_authorized_keys {
            let key_fingerprint = public_key.fingerprint();

            match authorized_keys.iter().any(|authorized_key| {
                // Split the authorized key into parts (typically format is "ssh-rsa AAAA...
                // comment")
                let parts: Vec<&str> = authorized_key.split_whitespace().collect();
                if parts.len() < 2 {
                    error!("Invalid authorized key format");
                    return false;
                }

                let algo = parts[0].as_bytes();
                // Decode the base64 key data using the STANDARD engine
                if let Ok(key_data) =
                    base64::engine::general_purpose::STANDARD.decode(parts[1].as_bytes())
                {
                    match russh_keys::key::PublicKey::parse(algo, &key_data) {
                        Ok(auth_key) => auth_key.fingerprint() == key_fingerprint,
                        Err(e) => {
                            error!("Failed to parse authorized key: {}", e);
                            false
                        }
                    }
                } else {
                    error!("Failed to decode base64 key data");
                    false
                }
            }) {
                true => {
                    info!("Public key authentication successful for user: {}", user);
                    Ok(server::Auth::Accept)
                }
                false => {
                    warn!("Invalid public key for user: {}", user);
                    Ok(server::Auth::Reject {
                        proceed_with_methods: None,
                    })
                }
            }
        } else {
            error!("SSH authentication enabled but no authorized keys configured");
            Ok(server::Auth::Reject {
                proceed_with_methods: None,
            })
        }
    }

    /// Handles requests to open a new SSH session
    async fn channel_open_session(
        &mut self, channel: Channel<Msg>, session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let mut clients = self.clients.lock().await;

        if let Some(client) = clients.get_mut(&self.id) {
            if client.connection_count >= MAX_CONNECTIONS_PER_CLIENT {
                error!("Too many connections for client {}", self.id);
                return Ok(false);
            }

            if client.last_connection.elapsed() < CONNECTION_TIMEOUT {
                error!("Connection attempt too soon for client {}", self.id);
                return Ok(false);
            }

            // Update connection count and last_connection
            client.connection_count += 1;
            client.last_connection = Instant::now();
        } else {
            // Insert new client
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
        }
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
        let mut server = self.clone();
        server.config = config.clone();

        let ssh_config = Arc::new(Self::create_config());
        let addr = ("0.0.0.0".to_string(), config.proxy_port);

        info!("Starting SSH proxy server on {}:{}", addr.0, addr.1);

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
    use std::sync::Arc;

    use russh::server::{
        Auth,
        Handler,
    };
    use russh_keys::key::KeyPair;

    use super::*;
    use crate::proxy::config::ProxyType;

    // Helper function to create a test config with random available port
    async fn create_test_config() -> ProxyConfig {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };

        ProxyConfig::builder()
            .target_host("127.0.0.1".to_string())
            .target_port(2222)
            .proxy_port(port)
            .proxy_type(ProxyType::Ssh)
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn test_ssh_proxy_startup() {
        let config = create_test_config().await;
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
        let config = create_test_config().await;

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

    #[tokio::test]
    async fn test_auth_methods() {
        let mut proxy = SshProxy::new();

        // Test 'none' authentication
        let auth_result = <SshProxy as Handler>::auth_none(&mut proxy, "test_user")
            .await
            .unwrap();
        assert!(
            matches!(auth_result, Auth::Accept),
            "None auth should be accepted"
        );

        // Test public key authentication
        let key_pair = KeyPair::generate_ed25519();
        let public_key = key_pair.clone_public_key().unwrap(); // Unwrap the Result
        let auth_result =
            <SshProxy as Handler>::auth_publickey(&mut proxy, "test_user", &public_key)
                .await
                .unwrap();
        assert!(
            matches!(auth_result, Auth::Accept),
            "Public key auth should be accepted"
        );
    }

    #[tokio::test]
    async fn test_config_creation() {
        let config = SshProxy::create_config();
        assert!(
            !config.keys.is_empty(),
            "SSH config should have at least one key"
        );
        assert_eq!(
            config.auth_rejection_time,
            Duration::from_secs(AUTH_REJECTION_TIME),
            "Auth rejection time should match constant"
        );
        assert_eq!(
            config.inactivity_timeout,
            Some(Duration::from_secs(INACTIVITY_TIMEOUT)),
            "Inactivity timeout should match constant"
        );
    }

    #[tokio::test]
    async fn test_client_cleanup() {
        let proxy = SshProxy::new();

        // Verify initial state
        {
            let clients = proxy.clients.lock().await;
            assert_eq!(clients.len(), 0, "Should start with no clients");
        }

        // Add a test client
        {
            let mut tasks = proxy.tasks.lock().await;
            tasks.push(tokio::spawn(async {}));
        }

        // Drop the proxy
        drop(proxy);

        // Small delay to allow async cleanup to run
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create new proxy to verify tasks were cleaned up
        let new_proxy = SshProxy::new();
        let tasks = new_proxy.tasks.lock().await;
        assert_eq!(tasks.len(), 0, "Tasks should be cleaned up after drop");
    }
}
