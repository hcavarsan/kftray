use std::collections::HashMap;
use std::sync::{
    Arc,
    Mutex,
    PoisonError,
};
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
    Semaphore,
    mpsc,
    oneshot,
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{
            Callback,
            ErrorResponse,
            Request,
            Response,
        },
        http::HeaderValue,
    },
};

use crate::models::tunnel_protocol::{
    HALF_CLOSE_HEADER,
    STREAM_WINDOW,
    TCP_MODE_PATH,
    TUNNEL_MODE_HEADER,
    TunnelMessage,
};

const WS_PING_INTERVAL: Duration = Duration::from_secs(30);

const WS_PONG_TIMEOUT: Duration = Duration::from_secs(10);

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct WebSocketTunnelServer {
    tunnel: Arc<RwLock<Option<TunnelConnection>>>,
    ws_port: u16,
}

struct StreamRoute {
    data: mpsc::UnboundedSender<StreamChunk>,
    credit: Arc<Semaphore>,
}

pub enum StreamChunk {
    Data(Vec<u8>),
    End,
}

type StreamSenders = Arc<Mutex<HashMap<String, StreamRoute>>>;

struct TunnelConnection {
    request_tx: mpsc::UnboundedSender<TunnelMessage>,
    pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<TunnelMessage>>>>,
    streams: StreamSenders,
    carries_tcp: bool,
    half_close: bool,
}

impl TunnelConnection {
    fn add_stream(&self, id: &str) -> (mpsc::UnboundedReceiver<StreamChunk>, Arc<Semaphore>) {
        let (data, data_rx) = mpsc::unbounded_channel();
        let credit = Arc::new(Semaphore::new(STREAM_WINDOW));
        self.streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                id.to_owned(),
                StreamRoute {
                    data,
                    credit: credit.clone(),
                },
            );
        (data_rx, credit)
    }
}

/// The relay end of an upgraded connection or a TCP connection. Dropping it
/// tells the client to close its side, unless the client closed first.
pub struct TunnelStream {
    id: String,
    incoming: mpsc::UnboundedReceiver<StreamChunk>,
    credit: Arc<Semaphore>,
    request_tx: mpsc::UnboundedSender<TunnelMessage>,
    streams: StreamSenders,
    half_close: bool,
}

impl TunnelStream {
    pub async fn recv(&mut self) -> Option<StreamChunk> {
        self.incoming.recv().await
    }

    pub fn half_close(&self) -> bool {
        self.half_close
    }

    pub fn credit(&self) -> Arc<Semaphore> {
        self.credit.clone()
    }

    pub fn send(&self, data: Vec<u8>) -> bool {
        self.request_tx
            .send(TunnelMessage::StreamData {
                id: self.id.clone(),
                data,
            })
            .is_ok()
    }

    pub fn return_credit(&self, bytes: usize) -> bool {
        self.request_tx
            .send(TunnelMessage::StreamCredit {
                id: self.id.clone(),
                bytes,
            })
            .is_ok()
    }

    pub fn end(&self) -> bool {
        self.request_tx
            .send(TunnelMessage::StreamEnd {
                id: self.id.clone(),
            })
            .is_ok()
    }
}

/// Waits for credit on a stream and takes up to `max` bytes of it. The
/// stream ends through its data channel, so a closed semaphore never
/// resolves and cannot win over data still queued for the browser.
pub async fn take_credit(credit: &Semaphore, max: usize) -> usize {
    match credit.acquire().await {
        Ok(first) => {
            first.forget();
            1 + credit.forget_permits(max - 1)
        }
        Err(_) => std::future::pending().await,
    }
}

impl Drop for TunnelStream {
    fn drop(&mut self) {
        let open = self
            .streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.id)
            .is_some();
        if open {
            let _ = self.request_tx.send(TunnelMessage::StreamClose {
                id: self.id.clone(),
            });
        }
    }
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
        let mut carries_tcp = false;
        let mut half_close = false;
        let ws_stream = accept_hdr_async(
            stream,
            TunnelModeHandshake {
                carries_tcp: &mut carries_tcp,
                half_close: &mut half_close,
            },
        )
        .await
        .map_err(|e| format!("WebSocket handshake failed: {}", e))?;

        info!(
            "WebSocket tunnel established ({})",
            if carries_tcp { "tcp" } else { "http" }
        );

        let (mut ws_write, mut ws_read) = ws_stream.split();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel::<TunnelMessage>();
        let pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<TunnelMessage>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let streams = StreamSenders::default();

        {
            let mut tunnel_lock = tunnel.write().await;
            *tunnel_lock = Some(TunnelConnection {
                request_tx,
                pending_responses: pending_responses.clone(),
                streams: streams.clone(),
                carries_tcp,
                half_close,
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
                                Ok(TunnelMessage::StreamData { id, data }) => {
                                    if !deliver_stream_data(&streams, &id, data)
                                        && let Ok(close) =
                                            (TunnelMessage::StreamClose { id }).serialize()
                                        && let Err(e) =
                                            ws_write.send(Message::Binary(close.into())).await
                                    {
                                        error!("Failed to send stream close: {}", e);
                                        break;
                                    }
                                }
                                Ok(TunnelMessage::StreamClose { id }) => {
                                    streams
                                        .lock()
                                        .unwrap_or_else(PoisonError::into_inner)
                                        .remove(&id);
                                }
                                Ok(TunnelMessage::StreamEnd { id }) => {
                                    if let Some(stream) = streams
                                        .lock()
                                        .unwrap_or_else(PoisonError::into_inner)
                                        .get(&id)
                                    {
                                        let _ = stream.data.send(StreamChunk::End);
                                    }
                                }
                                Ok(TunnelMessage::StreamCredit { id, bytes }) => {
                                    if let Some(stream) = streams
                                        .lock()
                                        .unwrap_or_else(PoisonError::into_inner)
                                        .get(&id)
                                    {
                                        stream.credit.add_permits(bytes.min(STREAM_WINDOW));
                                    }
                                }
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
                                            // Application-layer heartbeat (TunnelMessage::Pong), distinct from
                                            // WebSocket-level Message::Pong. Does NOT reset awaiting_pong.
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
                // Dual pong-miss detection: both this check (ping interval) and the
                // dedicated pong_timeout branch below can independently close the tunnel.
                // With production defaults (WS_PONG_TIMEOUT < WS_PING_INTERVAL), the
                // pong_timeout fires first; this check is a redundant safety net.
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
            if tunnel_lock
                .as_ref()
                .is_some_and(|conn| Arc::ptr_eq(&conn.pending_responses, &pending_responses))
            {
                *tunnel_lock = None;
            }
        }

        streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();

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
        &self, id: String, method: String, path: String, headers: Vec<(String, String)>,
        body: Vec<u8>, upgrade: bool,
    ) -> Result<(TunnelMessage, Option<TunnelStream>), String> {
        let (response_tx, response_rx) = oneshot::channel();

        let (pending_responses, request_tx, streams, incoming, half_close) = {
            let tunnel_lock = self.tunnel.read().await;
            let tunnel = tunnel_lock.as_ref().ok_or("No active tunnel connection")?;

            tunnel
                .pending_responses
                .write()
                .await
                .insert(id.clone(), response_tx);

            let incoming = upgrade.then(|| tunnel.add_stream(&id));

            tunnel
                .request_tx
                .send(TunnelMessage::HttpRequest {
                    id: id.clone(),
                    method,
                    path,
                    headers,
                    body,
                    upgrade,
                    flow_control: upgrade,
                })
                .map_err(|e| format!("Failed to send request: {}", e))?;

            (
                tunnel.pending_responses.clone(),
                tunnel.request_tx.clone(),
                tunnel.streams.clone(),
                incoming,
                tunnel.half_close,
            )
        };

        let response = match tokio::time::timeout(REQUEST_TIMEOUT, response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err("Response channel closed".to_string()),
            Err(_) => {
                let mut pending = pending_responses.write().await;
                pending.remove(&id);
                Err("Request timeout".to_string())
            }
        };

        let accepted = matches!(
            response,
            Ok(TunnelMessage::HttpResponse {
                status: 101,
                upgraded: true,
                flow_control: true,
                ..
            })
        );
        let stream = match incoming {
            Some((incoming, credit)) if accepted => Some(TunnelStream {
                id,
                incoming,
                credit,
                request_tx,
                streams,
                half_close,
            }),
            Some(_) => {
                streams
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&id);
                None
            }
            None => None,
        };

        response.map(|response| (response, stream))
    }

    pub async fn is_connected(&self) -> bool {
        self.tunnel.read().await.is_some()
    }

    /// Reports whether the connected app asked for raw TCP. The relay pipes
    /// connections on its HTTP port as streams only while this holds.
    pub async fn carries_tcp(&self) -> bool {
        self.tunnel
            .read()
            .await
            .as_ref()
            .is_some_and(|tunnel| tunnel.carries_tcp)
    }

    /// Asks the app to open a connection to its local port for one client
    /// connection, and returns the relay end of that stream.
    pub async fn open_stream(&self, id: String) -> Result<TunnelStream, String> {
        let tunnel_lock = self.tunnel.read().await;
        let tunnel = tunnel_lock.as_ref().ok_or("No active tunnel connection")?;
        let (incoming, credit) = tunnel.add_stream(&id);
        let stream = TunnelStream {
            id: id.clone(),
            incoming,
            credit,
            request_tx: tunnel.request_tx.clone(),
            streams: tunnel.streams.clone(),
            half_close: tunnel.half_close,
        };
        tunnel
            .request_tx
            .send(TunnelMessage::StreamOpen { id })
            .map_err(|e| format!("Failed to open stream: {}", e))?;
        Ok(stream)
    }
}

struct TunnelModeHandshake<'a> {
    carries_tcp: &'a mut bool,
    half_close: &'a mut bool,
}

impl Callback for TunnelModeHandshake<'_> {
    fn on_request(
        self, request: &Request, mut response: Response,
    ) -> Result<Response, ErrorResponse> {
        *self.carries_tcp = request.uri().path() == TCP_MODE_PATH;
        if *self.carries_tcp {
            response
                .headers_mut()
                .insert(TUNNEL_MODE_HEADER, HeaderValue::from_static("tcp"));
        }
        *self.half_close = request.headers().contains_key(HALF_CLOSE_HEADER);
        if *self.half_close {
            response
                .headers_mut()
                .insert(HALF_CLOSE_HEADER, HeaderValue::from_static("1"));
        }
        Ok(response)
    }
}

fn deliver_stream_data(streams: &StreamSenders, id: &str, data: Vec<u8>) -> bool {
    let mut open = streams.lock().unwrap_or_else(PoisonError::into_inner);
    let delivered = open
        .get(id)
        .is_some_and(|stream| stream.data.send(StreamChunk::Data(data)).is_ok());
    if !delivered {
        open.remove(id);
    }
    delivered
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

    #[tokio::test]
    async fn send_request_fails_fast_when_tunnel_drops_mid_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = Arc::new(WebSocketTunnelServer::new(0));
        let tunnel_clone = server.tunnel.clone();

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

        let (mut ws_client, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}", addr.port()))
                .await
                .unwrap();
        while !server.is_connected().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let sender = server.clone();
        let request = tokio::spawn(async move {
            sender
                .send_request(
                    "drop-test-1".to_string(),
                    "GET".to_string(),
                    "/".to_string(),
                    Vec::new(),
                    vec![],
                    false,
                )
                .await
        });

        let forwarded = tokio::time::timeout(Duration::from_secs(5), ws_client.next())
            .await
            .expect("Timed out waiting for the forwarded request")
            .expect("Stream ended")
            .expect("WebSocket error");
        assert!(
            matches!(forwarded, Message::Binary(_)),
            "Expected Binary, got {forwarded:?}"
        );

        drop(ws_client);

        let result = tokio::time::timeout(Duration::from_secs(5), request)
            .await
            .expect("send_request still waiting after the tunnel closed")
            .expect("send_request task panicked");

        let result = result.map(|(response, stream)| (response, stream.is_some()));
        match result {
            Ok((TunnelMessage::Error { id, message }, false)) => {
                assert_eq!(id.as_deref(), Some("drop-test-1"));
                assert!(
                    message.contains("tunnel closed"),
                    "Expected 'tunnel closed' error, got: {message}"
                );
            }
            other => panic!("Expected tunnel closed error, got: {other:?}"),
        }

        let _ = tokio::time::timeout(Duration::from_secs(5), server_handle)
            .await
            .expect("Server handler did not complete");
    }

    #[tokio::test]
    async fn old_tunnel_close_keeps_replacement_registered() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("ws://127.0.0.1:{}", addr.port());
        let tunnel: Arc<RwLock<Option<TunnelConnection>>> = Arc::new(RwLock::new(None));

        let tunnel_clone = tunnel.clone();
        let accept = tokio::spawn(async move {
            let mut handles = Vec::new();
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                handles.push(tokio::spawn(
                    WebSocketTunnelServer::handle_tunnel_connection_with_keepalive(
                        tunnel_clone.clone(),
                        stream,
                        Duration::from_secs(300),
                        Duration::from_secs(300),
                    ),
                ));
            }
            handles
        });

        let current_pending = || async {
            tunnel
                .read()
                .await
                .as_ref()
                .map(|conn| conn.pending_responses.clone())
        };

        let (client_a, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let pending_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(pending) = current_pending().await {
                    break pending;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("tunnel A was never registered");

        let (client_b, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let pending_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(pending) = current_pending().await
                    && !Arc::ptr_eq(&pending, &pending_a)
                {
                    break pending;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("tunnel B never replaced tunnel A");

        let mut handles = accept.await.unwrap();
        let handler_b = handles.pop().unwrap();
        let handler_a = handles.pop().unwrap();

        drop(client_a);
        tokio::time::timeout(Duration::from_secs(5), handler_a)
            .await
            .expect("handler A did not finish")
            .unwrap()
            .unwrap();

        let registered = current_pending()
            .await
            .expect("closing the old tunnel cleared the live replacement");
        assert!(Arc::ptr_eq(&registered, &pending_b));

        drop(client_b);
        tokio::time::timeout(Duration::from_secs(5), handler_b)
            .await
            .expect("handler B did not finish")
            .unwrap()
            .unwrap();
        assert!(current_pending().await.is_none());
    }
}
