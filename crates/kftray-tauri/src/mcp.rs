//! MCP Server lifecycle management for kftray.
//!
//! This module handles starting and stopping the MCP server from within
//! the Tauri application.

use std::net::{
    IpAddr,
    Ipv4Addr,
    SocketAddr,
};
use std::sync::Arc;

use lazy_static::lazy_static;
use log::{
    error,
    info,
};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

lazy_static! {
    /// Global state for the MCP server
    static ref MCP_SERVER: Arc<RwLock<Option<McpServerState>>> =
        Arc::new(RwLock::new(None));
}

struct McpServerState {
    handle: JoinHandle<()>,
    port: u16,
}

/// Check if the MCP server is currently running
pub async fn is_running() -> bool {
    let state = MCP_SERVER.read().await;
    if let Some(ref server) = *state {
        !server.handle.is_finished()
    } else {
        false
    }
}

/// Start the MCP server on the specified port
pub async fn start(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::time::Duration;

    // Check if already running
    {
        let state = MCP_SERVER.read().await;
        if let Some(ref server) = *state
            && !server.handle.is_finished()
        {
            if server.port == port {
                info!("MCP server already running on port {}", port);
                return Ok(());
            } else {
                // Different port, need to restart
                drop(state);
                stop().await?;
            }
        }
    }

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);

    info!("Starting MCP server on http://{}", addr);

    let handle = tokio::spawn(async move {
        if let Err(e) = kftray_mcp::server::start_server(addr).await {
            error!("MCP server error: {}", e);
        }
    });

    let mut state = MCP_SERVER.write().await;
    *state = Some(McpServerState { handle, port });

    // Wait briefly for server to start, then verify it's running
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Perform health check with retries
    let mut healthy = false;
    for _ in 0..5 {
        if health_check(port).await {
            healthy = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if healthy {
        info!("MCP server started successfully on port {}", port);
    } else {
        info!("MCP server started on port {} (health check pending)", port);
    }

    Ok(())
}

/// Stop the MCP server if it's running
pub async fn stop() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut state = MCP_SERVER.write().await;

    if let Some(server) = state.take() {
        info!("Stopping MCP server on port {}", server.port);
        server.handle.abort();
        info!("MCP server stopped");
    }

    Ok(())
}

/// Get the current port the MCP server is running on, if any
pub async fn get_running_port() -> Option<u16> {
    let state = MCP_SERVER.read().await;
    if let Some(ref server) = *state
        && !server.handle.is_finished()
    {
        return Some(server.port);
    }
    None
}

/// Initialize the MCP server based on saved settings
/// Call this during app startup
pub async fn init_from_settings() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use kftray_commons::utils::settings::{
        get_mcp_server_enabled,
        get_mcp_server_port,
    };

    let enabled = get_mcp_server_enabled().await.unwrap_or(false);

    if enabled {
        let port = get_mcp_server_port().await.unwrap_or(3000);
        info!("MCP server enabled in settings, starting on port {}", port);
        start(port).await?;
    } else {
        info!("MCP server disabled in settings");
    }

    Ok(())
}

/// Check if the MCP server is healthy by attempting to connect to the port
pub async fn health_check(port: u16) -> bool {
    use std::time::Duration;

    use tokio::net::TcpStream;
    use tokio::time::timeout;

    let addr = format!("127.0.0.1:{}", port);

    match timeout(Duration::from_secs(2), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}
