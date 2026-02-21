use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use log::{
    error,
    info,
    warn,
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
        let mut ws_handle = tokio::spawn(async move {
            if let Err(e) = tunnel_clone.start(shutdown_clone).await {
                error!("WebSocket server error: {}", e);
            }
        });

        let http_proxy = ReverseHttpProxy::new(tunnel_server, http_port);
        let shutdown_http = shutdown.clone();
        let mut http_handle = tokio::spawn(async move {
            if let Err(e) = http_proxy.start(shutdown_http).await {
                error!("HTTP proxy error: {}", e);
            }
        });

        shutdown.notified().await;
        info!("Reverse proxy shutting down");

        shutdown.notify_waiters();

        tokio::select! {
            _ = &mut ws_handle => {
                info!("WebSocket task completed gracefully");
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                warn!("WebSocket task did not shut down within 5s, aborting");
                ws_handle.abort();
            }
        }

        tokio::select! {
            _ = &mut http_handle => {
                info!("HTTP proxy task completed gracefully");
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                warn!("HTTP proxy task did not shut down within 5s, aborting");
                http_handle.abort();
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::Notify;

    #[tokio::test]
    async fn graceful_shutdown_should_complete_before_abort() {
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();

        let mut handle = tokio::spawn(async move {
            let _ = started_tx.send(());
            shutdown_clone.notified().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        started_rx.await.unwrap();
        shutdown.notify_waiters();

        let start = std::time::Instant::now();
        let completed_gracefully;
        tokio::select! {
            result = &mut handle => {
                completed_gracefully = true;
                assert!(result.is_ok(), "task should complete successfully");
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                handle.abort();
                completed_gracefully = false;
            }
        }

        assert!(completed_gracefully, "task should complete before timeout");
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "graceful shutdown should be fast"
        );
    }

    #[tokio::test]
    async fn graceful_shutdown_should_abort_after_timeout_for_stuck_task() {
        let mut handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });

        let shutdown = Arc::new(Notify::new());
        shutdown.notify_waiters();

        let start = std::time::Instant::now();
        let was_aborted;
        tokio::select! {
            _ = &mut handle => {
                was_aborted = false;
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                handle.abort();
                was_aborted = true;
            }
        }

        assert!(was_aborted, "stuck task should require abort after timeout");
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "abort should happen shortly after timeout"
        );
    }
}
