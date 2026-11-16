use std::sync::Arc;

use async_trait::async_trait;
use log::info;
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
        let http_proxy = ReverseHttpProxy::new(tunnel_server.clone(), http_port);

        tokio::select! {
            result = async {
                tokio::try_join!(
                    tunnel_server.start(shutdown.clone()),
                    http_proxy.start(shutdown.clone()),
                )
            } => {
                result?;
            }
            _ = shutdown.notified() => {
                info!("Reverse proxy shutting down");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::net::TcpListener;
    use tokio::sync::Notify;

    use super::*;
    use crate::proxy::config::ProxyType;
    use crate::proxy::test_utils::wait_for_port;

    fn reverse_config(http_port: u16, websocket_port: u16) -> ProxyConfig {
        ProxyConfig::builder()
            .target_host("127.0.0.1".to_string())
            .target_port(9)
            .proxy_port(0)
            .proxy_type(ProxyType::ReverseHttp)
            .http_port(Some(http_port))
            .websocket_port(Some(websocket_port))
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn listener_failure_releases_the_other_listener() {
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = reservation.local_addr().unwrap();
        drop(reservation);

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            ReverseProxy::new().start(
                reverse_config(addr.port(), addr.port()),
                Arc::new(Notify::new()),
            ),
        )
        .await
        .expect("reverse relay kept running after a listener bind failure");

        assert!(result.is_err(), "listeners cannot share a port");
        let _listener = TcpListener::bind(addr)
            .await
            .expect("reverse relay left its other listener open");
    }

    #[tokio::test]
    async fn graceful_shutdown_releases_both_listeners() {
        let http_reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_addr = http_reservation.local_addr().unwrap();
        let ws_addr = ws_reservation.local_addr().unwrap();
        let config = reverse_config(http_addr.port(), ws_addr.port());
        drop((http_reservation, ws_reservation));
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();
        let handle =
            tokio::spawn(async move { ReverseProxy::new().start(config, shutdown_clone).await });

        assert!(
            wait_for_port(http_addr).await,
            "HTTP listener did not start"
        );
        assert!(
            wait_for_port(ws_addr).await,
            "WebSocket listener did not start"
        );
        shutdown.notify_waiters();
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("reverse relay did not shut down")
            .unwrap()
            .unwrap();

        let _http_listener = TcpListener::bind(http_addr).await.unwrap();
        let _ws_listener = TcpListener::bind(ws_addr).await.unwrap();
    }
}
