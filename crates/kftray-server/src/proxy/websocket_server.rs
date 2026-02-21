use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::{
    SinkExt,
    StreamExt,
};
use log::{
    debug,
    error,
    info,
    warn,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use tokio::sync::{
    Notify,
    RwLock,
    mpsc,
    oneshot,
};
use tokio_tungstenite::{
    accept_async,
    tungstenite::Message,
};

use crate::models::tunnel_protocol::TunnelMessage;

const WS_PING_INTERVAL: Duration = Duration::from_secs(30);

const WS_PONG_TIMEOUT: Duration = Duration::from_secs(10);

pub struct WebSocketTunnelServer {
    tunnel: Arc<RwLock<Option<TunnelConnection>>>,
    ws_port: u16,
}

struct TunnelConnection {
    request_tx: mpsc::UnboundedSender<TunnelMessage>,
    pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<TunnelMessage>>>>,
}

impl WebSocketTunnelServer {
    pub fn new(ws_port: u16) -> Self {
        Self {
            tunnel: Arc::new(RwLock::new(None)),
            ws_port,
        }
    }

    pub async fn start(&self, shutdown: Arc<Notify>) -> Result<(), String> {
        let addr = format!("0.0.0.0:{}", self.ws_port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("Failed to bind WebSocket server: {}", e))?;

        info!("WebSocket tunnel server listening on {}", addr);

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, addr)) => {
                            info!("WebSocket connection from {}", addr);
                            let tunnel = self.tunnel.clone();
                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_tunnel_connection(tunnel, stream).await {
                                    error!("Tunnel connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => error!("Failed to accept WebSocket connection: {}", e),
                    }
                }
                _ = shutdown.notified() => {
                    info!("WebSocket server shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_tunnel_connection(
        tunnel: Arc<RwLock<Option<TunnelConnection>>>, stream: TcpStream,
    ) -> Result<(), String> {
        Self::handle_tunnel_connection_with_keepalive(
            tunnel,
            stream,
            WS_PING_INTERVAL,
            WS_PONG_TIMEOUT,
        )
        .await
    }

    async fn handle_tunnel_connection_with_keepalive(
        tunnel: Arc<RwLock<Option<TunnelConnection>>>, stream: TcpStream,
        ping_interval_duration: Duration, pong_timeout_duration: Duration,
    ) -> Result<(), String> {
        let ws_stream = accept_async(stream)
            .await
            .map_err(|e| format!("WebSocket handshake failed: {}", e))?;

        info!("WebSocket tunnel established");

        let (mut ws_write, mut ws_read) = ws_stream.split();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel::<TunnelMessage>();
        let pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<TunnelMessage>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        {
            let mut tunnel_lock = tunnel.write().await;
            *tunnel_lock = Some(TunnelConnection {
                request_tx,
                pending_responses: pending_responses.clone(),
            });
        }

        let mut ping_interval = tokio::time::interval_at(
            tokio::time::Instant::now() + ping_interval_duration,
            ping_interval_duration,
        );
        let mut awaiting_pong = false;
        let pong_timeout = tokio::time::sleep(Duration::MAX);
        tokio::pin!(pong_timeout);

        loop {
            tokio::select! {
                msg = ws_read.next() => {
                    match msg {
                        Some(Ok(Message::Binary(data))) => {
                            match TunnelMessage::deserialize(&data) {
                                Ok(tunnel_msg) => {
                                    debug!("Received tunnel message: {:?}", tunnel_msg);

                                    match &tunnel_msg {
                                        TunnelMessage::HttpResponse { id, .. } => {
                                            let id_clone = id.clone();
                                            let mut pending =
                                                pending_responses.write().await;
                                            if let Some(sender) =
                                                pending.remove(&id_clone)
                                            {
                                                if sender.send(tunnel_msg).is_err() {
                                                    warn!(
                                                        "Failed to send response to waiting handler for request {}",
                                                        id_clone
                                                    );
                                                }
                                            } else {
                                                warn!(
                                                    "Received response for unknown request ID: {}",
                                                    id_clone
                                                );
                                            }
                                        }
                                        TunnelMessage::Pong => {
                                            debug!("Received application-level pong");
                                        }
                                        TunnelMessage::Error { id, .. } => {
                                            error!("Tunnel error for request {:?}", id);
                                            if let Some(id_val) = id {
                                                let id_clone = id_val.clone();
                                                let mut pending =
                                                    pending_responses.write().await;
                                                if let Some(sender) =
                                                    pending.remove(&id_clone)
                                                {
                                                    let _ = sender.send(tunnel_msg);
                                                }
                                            }
                                        }
                                        _ => {
                                            warn!("Unexpected message type received");
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to deserialize tunnel message: {}",
                                        e
                                    );
                                }
                            }
                        }
                        Some(Ok(Message::Pong(_))) => {
                            debug!("Received WebSocket Pong — keepalive confirmed");
                            awaiting_pong = false;
                        }
                        Some(Ok(Message::Ping(_))) => {}
                        Some(Ok(Message::Close(_))) => {
                            info!("WebSocket closed by client");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            info!("WebSocket stream ended");
                            break;
                        }
                        _ => {}
                    }
                }
                Some(msg) = request_rx.recv() => {
                    match msg.serialize() {
                        Ok(data) => {
                            if let Err(e) =
                                ws_write.send(Message::Binary(data.into())).await
                            {
                                error!(
                                    "Failed to send message through WebSocket: {}",
                                    e
                                );
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to serialize message: {}", e);
                        }
                    }
                }
                _ = ping_interval.tick() => {
                    if awaiting_pong {
                        warn!(
                            "No WebSocket Pong received before next ping — closing tunnel"
                        );
                        break;
                    }
                    if let Err(e) =
                        ws_write.send(Message::Ping(vec![].into())).await
                    {
                        error!("Failed to send WebSocket Ping: {}", e);
                        break;
                    }
                    debug!("Sent WebSocket Ping keepalive");
                    awaiting_pong = true;
                    pong_timeout.as_mut().reset(
                        tokio::time::Instant::now() + pong_timeout_duration,
                    );
                }
                _ = &mut pong_timeout, if awaiting_pong => {
                    warn!(
                        "WebSocket Pong not received within {}s — closing tunnel",
                        pong_timeout_duration.as_secs()
                    );
                    break;
                }
            }
        }

        info!("Tunnel connection closed");

        {
            let mut tunnel_lock = tunnel.write().await;
            *tunnel_lock = None;
        }

        {
            let mut pending = pending_responses.write().await;
            let count = pending.len();
            for (id, sender) in pending.drain() {
                let _ = sender.send(TunnelMessage::Error {
                    id: Some(id),
                    message: "tunnel closed".to_string(),
                });
            }
            if count > 0 {
                info!(
                    "Drained {} pending response(s) with tunnel-closed error",
                    count
                );
            }
        }

        Ok(())
    }

    pub async fn send_request(
        &self, id: String, method: String, path: String, headers: HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<TunnelMessage, String> {
        let tunnel_lock = self.tunnel.read().await;
        let tunnel = tunnel_lock.as_ref().ok_or("No active tunnel connection")?;

        let (response_tx, response_rx) = oneshot::channel();

        {
            let mut pending = tunnel.pending_responses.write().await;
            pending.insert(id.clone(), response_tx);
        }

        let request_msg = TunnelMessage::HttpRequest {
            id: id.clone(),
            method,
            path,
            headers,
            body,
        };

        tunnel
            .request_tx
            .send(request_msg)
            .map_err(|e| format!("Failed to send request: {}", e))?;

        match tokio::time::timeout(Duration::from_secs(30), response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err("Response channel closed".to_string()),
            Err(_) => {
                let mut pending = tunnel.pending_responses.write().await;
                pending.remove(&id);
                Err("Request timeout".to_string())
            }
        }
    }

    pub async fn is_connected(&self) -> bool {
        self.tunnel.read().await.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use futures::StreamExt;
    use tokio::net::TcpListener;
    use tokio::sync::RwLock;
    use tokio_tungstenite::tungstenite::Message;

    use super::*;

    #[tokio::test]
    async fn websocket_ping_sent_and_pong_keeps_connection_alive() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let tunnel: Arc<RwLock<Option<TunnelConnection>>> = Arc::new(RwLock::new(None));
        let tunnel_clone = tunnel.clone();

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            WebSocketTunnelServer::handle_tunnel_connection_with_keepalive(
                tunnel_clone,
                stream,
                Duration::from_millis(200),
                Duration::from_secs(5),
            )
            .await
        });

        let (mut ws_client, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}", addr.port()))
                .await
                .unwrap();

        let msg = tokio::time::timeout(Duration::from_secs(3), ws_client.next())
            .await
            .expect("Timed out waiting for Ping")
            .expect("Stream ended")
            .expect("WebSocket error");
        assert!(
            matches!(msg, Message::Ping(_)),
            "Expected Ping, got {msg:?}"
        );

        let msg = tokio::time::timeout(Duration::from_secs(3), ws_client.next())
            .await
            .expect("Timed out waiting for second Ping — pong may not have been handled")
            .expect("Stream ended")
            .expect("WebSocket error");
        assert!(
            matches!(msg, Message::Ping(_)),
            "Expected second Ping, got {msg:?}"
        );

        drop(ws_client);
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn websocket_closes_tunnel_when_pong_not_received_within_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let tunnel: Arc<RwLock<Option<TunnelConnection>>> = Arc::new(RwLock::new(None));
        let tunnel_clone = tunnel.clone();

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            WebSocketTunnelServer::handle_tunnel_connection_with_keepalive(
                tunnel_clone,
                stream,
                Duration::from_millis(200),
                Duration::from_millis(100),
            )
            .await
        });

        let (_ws_client, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}", addr.port()))
                .await
                .unwrap();

        let result = tokio::time::timeout(Duration::from_secs(5), server_handle)
            .await
            .expect("Server handler did not complete — pong timeout may not be working");

        let inner = result.expect("Server handler panicked");
        assert!(inner.is_ok(), "Server handler returned error: {inner:?}");
    }

    #[tokio::test]
    async fn pending_callers_receive_error_when_tunnel_closes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let tunnel: Arc<RwLock<Option<TunnelConnection>>> = Arc::new(RwLock::new(None));
        let tunnel_clone = tunnel.clone();

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            WebSocketTunnelServer::handle_tunnel_connection_with_keepalive(
                tunnel_clone,
                stream,
                Duration::from_secs(300),
                Duration::from_secs(300),
            )
            .await
        });

        let (ws_client, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}", addr.port()))
                .await
                .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let pending_ref = {
            let tunnel_lock = tunnel.read().await;
            tunnel_lock.as_ref().unwrap().pending_responses.clone()
        };
        let (tx, rx) = oneshot::channel::<TunnelMessage>();
        {
            let mut pending = pending_ref.write().await;
            pending.insert("drain-test-1".to_string(), tx);
        }

        drop(ws_client);

        let result = tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .expect("Timed out — caller would hang forever without drain fix")
            .expect("Channel closed without sending error response");

        match result {
            TunnelMessage::Error { id, message } => {
                assert_eq!(id.as_deref(), Some("drain-test-1"));
                assert!(
                    message.contains("tunnel closed"),
                    "Expected 'tunnel closed' error, got: {message}"
                );
            }
            other => panic!("Expected TunnelMessage::Error, got: {other:?}"),
        }

        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn pending_responses_empty_after_tunnel_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let tunnel: Arc<RwLock<Option<TunnelConnection>>> = Arc::new(RwLock::new(None));
        let tunnel_clone = tunnel.clone();

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            WebSocketTunnelServer::handle_tunnel_connection_with_keepalive(
                tunnel_clone,
                stream,
                Duration::from_secs(300),
                Duration::from_secs(300),
            )
            .await
        });

        let (ws_client, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}", addr.port()))
                .await
                .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let pending_ref = {
            let tunnel_lock = tunnel.read().await;
            tunnel_lock.as_ref().unwrap().pending_responses.clone()
        };
        {
            let mut pending = pending_ref.write().await;
            let (tx1, _rx1) = oneshot::channel::<TunnelMessage>();
            let (tx2, _rx2) = oneshot::channel::<TunnelMessage>();
            let (tx3, _rx3) = oneshot::channel::<TunnelMessage>();
            pending.insert("leak-test-1".to_string(), tx1);
            pending.insert("leak-test-2".to_string(), tx2);
            pending.insert("leak-test-3".to_string(), tx3);
            assert_eq!(pending.len(), 3, "Expected 3 pending entries before close");
        }

        drop(ws_client);

        let _ = tokio::time::timeout(Duration::from_secs(5), server_handle)
            .await
            .expect("Server handler did not complete")
            .expect("Server handler panicked");

        let pending = pending_ref.read().await;
        assert!(
            pending.is_empty(),
            "Expected pending_responses to be empty after tunnel close, but had {} entries",
            pending.len()
        );
    }
}
