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
use tokio::net::TcpListener;
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
    if port == 0 {
        return Err("MCP server port cannot be 0".into());
    }

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

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("cannot listen on {addr}: {e}"))?;

    info!("Starting MCP server on http://{}", addr);

    let handle = tokio::spawn(async move {
        if let Err(e) = kftray_mcp::server::serve(listener).await {
            error!("MCP server error: {}", e);
        }
    });

    *MCP_SERVER.write().await = Some(McpServerState { handle, port });

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

#[cfg(test)]
pub(crate) static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_fails_when_the_port_is_already_taken() {
        let _mcp = TEST_LOCK.lock().await;
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();

        let err = start(port)
            .await
            .expect_err("start should fail on a busy port");

        assert!(err.to_string().contains(&port.to_string()), "{err}");
    }

    #[tokio::test]
    async fn start_rejects_port_zero() {
        let _mcp = TEST_LOCK.lock().await;
        assert!(start(0).await.is_err());
        assert!(!is_running().await);
    }
}
