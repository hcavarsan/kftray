use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::{
    SinkExt,
    StreamExt,
};
use kftray_commons::models::tunnel_protocol::TunnelMessage;
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
                request_tx: request_tx.clone(),
                pending_responses: pending_responses.clone(),
            });
        }

        let send_task = tokio::spawn(async move {
            while let Some(msg) = request_rx.recv().await {
                match msg.serialize() {
                    Ok(data) => {
                        if let Err(e) = ws_write.send(Message::Binary(data.into())).await {
                            error!("Failed to send message through WebSocket: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize message: {}", e);
                    }
                }
            }
        });

        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Binary(data)) => match TunnelMessage::deserialize(&data) {
                    Ok(tunnel_msg) => {
                        debug!("Received tunnel message: {:?}", tunnel_msg);

                        match &tunnel_msg {
                            TunnelMessage::HttpResponse { id, .. } => {
                                let id_clone = id.clone();
                                let mut pending = pending_responses.write().await;
                                if let Some(sender) = pending.remove(&id_clone) {
                                    if sender.send(tunnel_msg).is_err() {
                                        warn!(
                                            "Failed to send response to waiting handler for request {}",
                                            id_clone
                                        );
                                    }
                                } else {
                                    warn!("Received response for unknown request ID: {}", id_clone);
                                }
                            }
                            TunnelMessage::Pong => {
                                debug!("Received pong");
                            }
                            TunnelMessage::Error { id, .. } => {
                                error!("Tunnel error for request {:?}", id);
                                if let Some(id_val) = id {
                                    let id_clone = id_val.clone();
                                    let mut pending = pending_responses.write().await;
                                    if let Some(sender) = pending.remove(&id_clone) {
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
                        error!("Failed to deserialize tunnel message: {}", e);
                    }
                },
                Ok(Message::Ping(_)) => {
                    // Pings are automatically handled by tungstenite
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by client");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        info!("Tunnel connection closed");
        {
            let mut tunnel_lock = tunnel.write().await;
            *tunnel_lock = None;
        }

        send_task.abort();

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
                // Timeout - clean up pending response
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
