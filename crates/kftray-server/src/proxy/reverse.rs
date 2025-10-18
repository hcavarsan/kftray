use std::sync::Arc;

use async_trait::async_trait;
use log::{
    error,
    info,
};
use tokio::sync::Notify;

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
    reverse_http::ReverseHttpProxy,
    traits::ProxyHandler,
    websocket_server::WebSocketTunnelServer,
};

#[derive(Clone)]
pub struct ReverseProxy;

impl ReverseProxy {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReverseProxy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProxyHandler for ReverseProxy {
    async fn start(&self, config: ProxyConfig, shutdown: Arc<Notify>) -> Result<(), ProxyError> {
        let ws_port = config.websocket_port.unwrap_or(9999);
        let http_port = config.http_port.unwrap_or(8080);

        info!(
            "Starting reverse proxy: HTTP on port {}, WebSocket on port {}",
            http_port, ws_port
        );

        let tunnel_server = Arc::new(WebSocketTunnelServer::new(ws_port));
        let tunnel_clone = tunnel_server.clone();

        let shutdown_clone = shutdown.clone();
        let ws_handle = tokio::spawn(async move {
            if let Err(e) = tunnel_clone.start(shutdown_clone).await {
                error!("WebSocket server error: {}", e);
            }
        });

        let http_proxy = ReverseHttpProxy::new(tunnel_server, http_port);
        let http_handle = tokio::spawn(async move {
            if let Err(e) = http_proxy.start().await {
                error!("HTTP proxy error: {}", e);
            }
        });

        shutdown.notified().await;
        info!("Reverse proxy shutting down");

        ws_handle.abort();
        http_handle.abort();

        Ok(())
    }
}
