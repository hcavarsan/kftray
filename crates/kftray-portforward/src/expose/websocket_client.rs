use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Arc;

use bytes::Bytes;
use futures::{
    SinkExt,
    StreamExt,
};
use http_body_util::{
    BodyExt,
    Full,
};
use hyper::upgrade::Upgraded;
use hyper::{
    Method,
    Request,
    StatusCode,
    Uri,
};
use hyper_util::client::legacy::Client as LegacyClient;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{
    TokioExecutor,
    TokioIo,
};
use kftray_commons::models::http_logs_config_model::HttpLogsConfig;
use kftray_commons::models::tunnel_protocol::{
    HALF_CLOSE_HEADER,
    STREAM_WINDOW,
    TCP_MODE_PATH,
    TUNNEL_MODE_HEADER,
    TunnelMessage,
};
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::http_logs_config::get_http_logs_config_with_mode;
use kftray_http_logs::HttpLogger;
use log::{
    debug,
    error,
    info,
    warn,
};
use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    AsyncWrite,
    AsyncWriteExt,
};
use tokio::net::TcpStream;
use tokio::sync::{
    Mutex,
    RwLock,
    Semaphore,
    mpsc,
    oneshot,
};
use tokio::task::JoinSet;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Error as WsError,
        Message,
        client::IntoClientRequest,
        http::HeaderValue,
    },
};

struct StreamRoute {
    data: mpsc::UnboundedSender<StreamChunk>,
    credit: Arc<Semaphore>,
}

enum StreamChunk {
    Data(Vec<u8>),
    End,
}

type StreamRoutes = HashMap<String, StreamRoute>;

fn add_stream(
    streams: &mut StreamRoutes, id: String,
) -> (mpsc::UnboundedReceiver<StreamChunk>, Arc<Semaphore>) {
    let (data, data_rx) = mpsc::unbounded_channel();
    let credit = Arc::new(Semaphore::new(STREAM_WINDOW));
    streams.insert(
        id,
        StreamRoute {
            data,
            credit: credit.clone(),
        },
    );
    (data_rx, credit)
}

struct ExposeHttpLogs {
    config_id: i64,
    mode: DatabaseMode,
    logger: Mutex<Option<Arc<HttpLogger>>>,
    settings: RwLock<Option<HttpLogsConfig>>,
}

impl ExposeHttpLogs {
    async fn sync(&self) {
        match get_http_logs_config_with_mode(self.config_id, self.mode).await {
            Ok(settings) => {
                let policy_changed = self.settings.read().await.as_ref().is_some_and(|old| {
                    old.max_file_size != settings.max_file_size
                        || old.retention_days != settings.retention_days
                        || old.auto_cleanup != settings.auto_cleanup
                });
                if policy_changed {
                    let logger = self.logger.lock().await.clone();
                    if let Some(logger) = logger
                        && let Err(error) = logger
                            .update_rotation_policy(
                                settings.max_file_size,
                                settings.retention_days,
                                settings.auto_cleanup,
                            )
                            .await
                    {
                        warn!(
                            "HTTP log policy update failed for config {}: {}",
                            self.config_id, error
                        );
                    }
                }
                *self.settings.write().await = Some(settings);
            }
            Err(error) => debug!(
                "HTTP log settings read failed for config {}: {}",
                self.config_id, error
            ),
        }
    }
}

#[derive(Clone)]
pub struct WebSocketTunnelClient {
    websocket_port: u16,
    local_service_address: String,
    local_service_port: u16,
    carries_tcp: bool,
    http_logs: Option<Arc<ExposeHttpLogs>>,
}

impl WebSocketTunnelClient {
    pub fn new(
        websocket_port: u16, local_service_address: String, local_service_port: u16,
    ) -> Self {
        Self {
            websocket_port,
            local_service_address,
            local_service_port,
            carries_tcp: false,
            http_logs: None,
        }
    }

    /// Asks the relay to pipe raw TCP to the local port instead of HTTP.
    pub fn carrying_tcp(mut self, carries_tcp: bool) -> Self {
        self.carries_tcp = carries_tcp;
        self
    }

    pub fn with_http_logs(mut self, config_id: i64, mode: DatabaseMode) -> Self {
        self.http_logs = Some(Arc::new(ExposeHttpLogs {
            config_id,
            mode,
            logger: Mutex::new(None),
            settings: RwLock::new(None),
        }));
        self
    }

    async fn http_logger(&self) -> Option<Arc<HttpLogger>> {
        if self.carries_tcp {
            return None;
        }
        let logs = self.http_logs.as_ref()?;
        let settings = logs.settings.read().await;
        let config = settings.as_ref()?;
        if !config.enabled {
            return None;
        }
        let mut logger = logs.logger.lock().await;
        if logger.is_none() {
            match HttpLogger::for_config(config, self.local_service_port).await {
                Ok(created) => *logger = Some(Arc::new(created)),
                Err(error) => warn!(
                    "HTTP log creation failed for config {}: {}",
                    logs.config_id, error
                ),
            }
        }
        logger.clone()
    }

    pub async fn start(&self, ready: oneshot::Sender<Result<(), String>>) -> Result<(), String> {
        let _log_sync = if let Some(logs) = &self.http_logs
            && !self.carries_tcp
        {
            logs.sync().await;
            let logs = logs.clone();
            Some(tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
                async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
                    interval.tick().await;
                    loop {
                        interval.tick().await;
                        logs.sync().await;
                    }
                },
            )))
        } else {
            None
        };
        self.reconnect_loop(ready, 100).await
    }

    /// The retry loop behind [`Self::start`], with the attempt budget as a
    /// parameter so a test can exhaust it without waiting on the real one.
    async fn reconnect_loop(
        &self, ready: oneshot::Sender<Result<(), String>>, max_retries: u32,
    ) -> Result<(), String> {
        let mut ready = Some(ready);
        let ws_url = format!(
            "ws://127.0.0.1:{}{}",
            self.websocket_port,
            if self.carries_tcp { TCP_MODE_PATH } else { "" }
        );
        let mut retry_count = 0;

        loop {
            info!(
                "Connecting WebSocket client to {} (attempt {}/{})",
                ws_url,
                retry_count + 1,
                max_retries
            );

            let mut connected = false;
            match self
                .connect_and_run(&ws_url, &mut ready, &mut connected)
                .await
            {
                Ok(_) => {
                    info!("WebSocket tunnel disconnected gracefully");
                }
                Err(e) => {
                    error!("WebSocket tunnel error: {}", e);
                }
            }

            if connected {
                retry_count = 0;
            }
            retry_count += 1;
            if retry_count >= max_retries {
                let message = format!("Max reconnection attempts ({}) reached", max_retries);
                // Every attempt so far was retryable, so `ready` is still
                // pending: without this, the caller only learns startup
                // ended through the channel closing, losing why.
                if let Some(sender) = ready.take() {
                    let _ = sender.send(Err(message.clone()));
                }
                return Err(message);
            }

            let backoff_secs = std::cmp::min(2_u64.pow(retry_count.min(4)), 30);
            warn!(
                "WebSocket disconnected. Reconnecting in {} seconds...",
                backoff_secs
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
        }
    }

    async fn connect_and_run(
        &self, ws_url: &str, ready: &mut Option<oneshot::Sender<Result<(), String>>>,
        connected: &mut bool,
    ) -> Result<(), String> {
        let mut request = ws_url
            .into_client_request()
            .map_err(|e| format!("Invalid tunnel URL {ws_url}: {e}"))?;
        request
            .headers_mut()
            .insert(HALF_CLOSE_HEADER, HeaderValue::from_static("1"));
        let (ws_stream, response) = match connect_async(request).await {
            Ok(connection) => connection,
            Err(error) => {
                let message = format!("Failed to connect to WebSocket: {error}");
                // A rejection the relay may recover from keeps the readiness
                // sender pending, so the reconnect loop can retry within the
                // startup timeout instead of tearing the exposure down.
                let is_permanent = match &error {
                    WsError::Http(response) => !matches!(
                        response.status().as_u16(),
                        408 | 425 | 429 | 500 | 502 | 503 | 504
                    ),
                    _ => false,
                };
                if is_permanent
                    && let Some(ready) = ready.take()
                    && ready.send(Err(message.clone())).is_err()
                {
                    return Err(format!("Expose startup was cancelled: {message}"));
                }
                return Err(message);
            }
        };
        if self.carries_tcp
            && response
                .headers()
                .get(TUNNEL_MODE_HEADER)
                .is_none_or(|mode| mode != "tcp")
        {
            let message = "The kftray-server relay in the cluster cannot carry TCP. Start the \
                           expose again so the relay pod pulls the latest image"
                .to_owned();
            if let Some(ready) = ready.take() {
                let _ = ready.send(Err(message.clone()));
            }
            return Err(message);
        }
        let half_close = response.headers().contains_key(HALF_CLOSE_HEADER);
        if let Some(ready) = ready.take() {
            ready
                .send(Ok(()))
                .map_err(|_| "Expose startup was cancelled".to_owned())?;
        }

        *connected = true;
        info!("WebSocket tunnel connected");

        let (mut ws_write, mut ws_read) = ws_stream.split();

        let mut http_connector = HttpConnector::new();
        http_connector.set_nodelay(true);
        http_connector.set_keepalive(Some(std::time::Duration::from_secs(30)));
        let http_client = LegacyClient::builder(TokioExecutor::new()).build(http_connector);

        let target = Arc::new(self.clone());
        let mut requests = JoinSet::new();
        let mut streams = StreamRoutes::new();
        let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<TunnelMessage>();

        loop {
            let msg = tokio::select! {
                Some(joined) = requests.join_next(), if !requests.is_empty() => {
                    let (response_msg, upgraded) = match joined {
                        Ok(reply) => reply,
                        Err(e) => {
                            error!("Request task failed: {}", e);
                            continue;
                        }
                    };
                    let stream = match (&response_msg, upgraded) {
                        (TunnelMessage::HttpResponse { id, .. }, Some(upgraded)) => {
                            Some((id.clone(), upgraded))
                        }
                        _ => None,
                    };
                    let _ = outgoing_tx.send(response_msg);
                    if let Some((id, upgraded)) = stream {
                        let (to_local_rx, credit) = add_stream(&mut streams, id.clone());
                        tokio::spawn(pipe_stream(
                            id,
                            TokioIo::new(upgraded),
                            to_local_rx,
                            credit,
                            outgoing_tx.clone(),
                            half_close,
                        ));
                    }
                    continue;
                }
                Some(outgoing) = outgoing_rx.recv() => {
                    if let TunnelMessage::StreamClose { id } = &outgoing {
                        streams.remove(id);
                    }
                    match outgoing.serialize() {
                        Ok(data) => {
                            if let Err(e) = ws_write.send(Message::Binary(data.into())).await {
                                error!("Failed to send response through WebSocket: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to serialize response: {}", e);
                        }
                    }
                    continue;
                }
                msg = ws_read.next() => match msg {
                    Some(msg) => msg,
                    None => break,
                },
            };

            match msg {
                Ok(Message::Binary(data)) => match TunnelMessage::deserialize(&data) {
                    Ok(TunnelMessage::HttpRequest {
                        id,
                        method,
                        path,
                        headers,
                        body,
                        upgrade,
                        flow_control,
                    }) => {
                        debug!("Received HTTP request: {} {}", method, path);

                        let target = Arc::clone(&target);
                        let http_client = http_client.clone();
                        requests.spawn(async move {
                            target
                                .forward_to_local_service(
                                    &http_client,
                                    id,
                                    method,
                                    path,
                                    headers,
                                    body,
                                    upgrade && flow_control,
                                )
                                .await
                        });
                    }
                    Ok(TunnelMessage::StreamOpen { id }) => {
                        let (to_local_rx, credit) = add_stream(&mut streams, id.clone());
                        let address = (self.local_service_address.clone(), self.local_service_port);
                        let outgoing = outgoing_tx.clone();
                        tokio::spawn(async move {
                            match TcpStream::connect(&address).await {
                                Ok(local) => {
                                    let _ = local.set_nodelay(true);
                                    pipe_stream(
                                        id,
                                        local,
                                        to_local_rx,
                                        credit,
                                        outgoing,
                                        half_close,
                                    )
                                    .await;
                                }
                                Err(e) => {
                                    warn!(
                                        "Failed to connect to local service at {}:{}: {}",
                                        address.0, address.1, e
                                    );
                                    let _ = outgoing.send(TunnelMessage::StreamClose { id });
                                }
                            }
                        });
                    }
                    Ok(TunnelMessage::StreamData { id, data }) => {
                        if let Some(stream) = streams.get(&id)
                            && stream.data.send(StreamChunk::Data(data)).is_err()
                        {
                            streams.remove(&id);
                        }
                    }
                    Ok(TunnelMessage::StreamEnd { id }) => {
                        if let Some(stream) = streams.get(&id) {
                            let _ = stream.data.send(StreamChunk::End);
                        }
                    }
                    Ok(TunnelMessage::StreamCredit { id, bytes }) => {
                        if let Some(stream) = streams.get(&id) {
                            stream.credit.add_permits(bytes.min(STREAM_WINDOW));
                        }
                    }
                    Ok(TunnelMessage::StreamClose { id }) => {
                        streams.remove(&id);
                    }
                    Ok(TunnelMessage::Ping) => {
                        debug!("Received ping, sending pong");
                        let pong = TunnelMessage::Pong;
                        if let Ok(pong_data) = pong.serialize() {
                            let _ = ws_write.send(Message::Binary(pong_data.into())).await;
                        }
                    }
                    Ok(_) => {
                        warn!("Received unexpected message type from pod");
                    }
                    Err(e) => {
                        error!("Failed to deserialize tunnel message: {}", e);
                    }
                },
                Ok(Message::Ping(_)) => {
                    debug!("Received WebSocket ping");
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by pod");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn forward_to_local_service(
        &self, http_client: &LegacyClient<HttpConnector, Full<Bytes>>, request_id: String,
        method: String, path: String, headers: Vec<(String, String)>, body: Vec<u8>, upgrade: bool,
    ) -> (TunnelMessage, Option<Upgraded>) {
        let uri_str = format!(
            "http://{}:{}{}",
            self.local_service_address, self.local_service_port, path
        );
        debug!(
            "Forwarding {} {} to local service at {}",
            method, path, uri_str
        );
        let uri: Uri = match uri_str.parse() {
            Ok(u) => u,
            Err(e) => {
                error!("Invalid URI {}: {}", uri_str, e);
                let message = format!("Invalid URI: {}", e);
                return (
                    TunnelMessage::Error {
                        id: Some(request_id),
                        message,
                    },
                    None,
                );
            }
        };

        let http_method = match Method::from_bytes(method.as_bytes()) {
            Ok(m) => m,
            Err(_) => {
                error!("Unsupported HTTP method: {}", method);
                let message = format!("Unsupported method: {}", method);
                return (
                    TunnelMessage::Error {
                        id: Some(request_id),
                        message,
                    },
                    None,
                );
            }
        };

        let mut req_builder = Request::builder().method(http_method).uri(uri);

        for (key, value) in headers {
            if let Ok(header_name) = hyper::header::HeaderName::from_bytes(key.as_bytes())
                && let Ok(header_value) = hyper::header::HeaderValue::from_str(&value)
            {
                req_builder = req_builder.header(header_name, header_value);
            }
        }

        let request_log = match (self.http_logger().await, req_builder.headers_ref()) {
            (Some(logger), Some(headers)) => {
                let message =
                    logged_http_message(format_args!("{method} {path} HTTP/1.1"), headers, &body);
                let id = logger.log_request(message).await;
                Some((logger, id))
            }
            _ => None,
        };

        let request = match req_builder.body(Full::new(Bytes::from(body))) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to build HTTP request: {}", e);
                let message = format!("Failed to build request: {}", e);
                return (
                    TunnelMessage::Error {
                        id: Some(request_id),
                        message,
                    },
                    None,
                );
            }
        };

        match http_client.request(request).await {
            Ok(response) => {
                let status = response.status().as_u16();

                let response_headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.to_string(),
                            String::from_utf8_lossy(v.as_bytes()).into_owned(),
                        )
                    })
                    .collect();

                if upgrade && response.status() == StatusCode::SWITCHING_PROTOCOLS {
                    return match hyper::upgrade::on(response).await {
                        Ok(upgraded) => (
                            TunnelMessage::HttpResponse {
                                id: request_id,
                                status,
                                headers: response_headers,
                                body: Vec::new(),
                                upgraded: true,
                                flow_control: true,
                            },
                            Some(upgraded),
                        ),
                        Err(e) => {
                            error!("Failed to upgrade the local connection: {}", e);
                            let message = format!("Failed to upgrade the local connection: {}", e);
                            (
                                TunnelMessage::Error {
                                    id: Some(request_id),
                                    message,
                                },
                                None,
                            )
                        }
                    };
                }

                let (parts, response_body) = response.into_parts();
                let reply = match response_body.collect().await {
                    Ok(collected) => {
                        let body_bytes = collected.to_bytes();
                        if let Some((logger, id)) = request_log {
                            let message = logged_http_message(
                                format_args!("HTTP/1.1 {}", parts.status),
                                &parts.headers,
                                &body_bytes,
                            );
                            logger.log_response(message, id).await;
                        }
                        debug!("Forwarded request successfully, status: {}", status);
                        TunnelMessage::HttpResponse {
                            id: request_id,
                            status,
                            headers: response_headers,
                            body: body_bytes.to_vec(),
                            upgraded: false,
                            flow_control: false,
                        }
                    }
                    Err(e) => {
                        error!("Failed to read response body: {}", e);
                        TunnelMessage::Error {
                            id: Some(request_id),
                            message: format!("Failed to read response body: {}", e),
                        }
                    }
                };
                (reply, None)
            }
            Err(e) => {
                error!(
                    "Failed to forward request to local service at {}:{}: {:?}",
                    self.local_service_port, uri_str, e
                );
                let message = format!(
                    "Failed to connect to local service at localhost:{}: {}",
                    self.local_service_port, e
                );
                (
                    TunnelMessage::Error {
                        id: Some(request_id),
                        message,
                    },
                    None,
                )
            }
        }
    }
}

fn logged_http_message(
    start_line: std::fmt::Arguments<'_>, headers: &hyper::HeaderMap, body: &[u8],
) -> Bytes {
    let mut buffer = Vec::with_capacity(body.len() + 512);
    write!(buffer, "{start_line}\r\n").unwrap();
    for (name, value) in headers {
        if name != hyper::header::TRANSFER_ENCODING {
            write!(
                buffer,
                "{name}: {}\r\n",
                String::from_utf8_lossy(value.as_bytes())
            )
            .unwrap();
        }
    }
    buffer.extend_from_slice(b"\r\n");
    buffer.extend_from_slice(body);
    Bytes::from(buffer)
}

async fn pipe_stream<T: AsyncRead + AsyncWrite>(
    id: String, local: T, mut from_relay: mpsc::UnboundedReceiver<StreamChunk>,
    credit: Arc<Semaphore>, outgoing: mpsc::UnboundedSender<TunnelMessage>, half_close: bool,
) {
    let (mut reader, mut writer) = tokio::io::split(local);
    let mut buf = vec![0u8; 16 * 1024];
    let mut granted = 0;
    let mut reading = true;
    let mut writing = true;
    let mut to_local = Vec::new();
    let mut written = 0;

    let local_closed = loop {
        if !reading && !writing {
            break true;
        }
        tokio::select! {
            taken = take_credit(&credit, buf.len()), if reading && granted == 0 => granted = taken,
            read = reader.read(&mut buf[..granted]), if reading && granted > 0 => match read {
                Ok(0) if half_close => {
                    reading = false;
                    if outgoing.send(TunnelMessage::StreamEnd { id: id.clone() }).is_err() {
                        break false;
                    }
                }
                Ok(0) | Err(_) => break true,
                Ok(n) => {
                    granted -= n;
                    let data = TunnelMessage::StreamData {
                        id: id.clone(),
                        data: buf[..n].to_vec(),
                    };
                    if outgoing.send(data).is_err() {
                        break false;
                    }
                }
            },
            sent = writer.write(&to_local[written..]), if !to_local.is_empty() => match sent {
                Ok(0) | Err(_) => break true,
                Ok(n) => {
                    written += n;
                    if written == to_local.len() {
                        let consumed = TunnelMessage::StreamCredit {
                            id: id.clone(),
                            bytes: to_local.len(),
                        };
                        to_local.clear();
                        written = 0;
                        if outgoing.send(consumed).is_err() {
                            break false;
                        }
                    }
                }
            },
            chunk = from_relay.recv(), if to_local.is_empty() => match chunk {
                Some(StreamChunk::Data(data)) => to_local = data,
                Some(StreamChunk::End) => {
                    writing = false;
                    let _ = writer.shutdown().await;
                }
                None => break false,
            },
        }
    };

    if writing {
        let _ = writer.shutdown().await;
    }
    if local_closed {
        let _ = outgoing.send(TunnelMessage::StreamClose { id });
    }
}

/// Waits for credit on a stream and takes up to `max` bytes of it. The
/// stream ends through its data channel, so a closed semaphore never
/// resolves and cannot win over data still queued for the local app.
async fn take_credit(credit: &Semaphore, max: usize) -> usize {
    match credit.acquire().await {
        Ok(first) => {
            first.forget();
            1 + credit.forget_permits(max - 1)
        }
        Err(_) => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use super::WebSocketTunnelClient;

    #[tokio::test]
    async fn startup_waits_for_the_websocket_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            8080,
        );
        let (ready_tx, mut ready_rx) = oneshot::channel();
        let task = tokio::spawn(async move { client.start(ready_tx).await });
        let (socket, _) = listener.accept().await.unwrap();
        assert!(matches!(
            ready_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let _peer = tokio_tungstenite::accept_async(socket).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), ready_rx)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn rejected_handshake_fails_startup() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            8080,
        );
        let (ready_tx, ready_rx) = oneshot::channel();
        let task = tokio::spawn(async move { client.start(ready_tx).await });
        let (mut socket, _) = listener.accept().await.unwrap();
        socket
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), ready_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn transient_connection_failure_retries_before_reporting_readiness() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            8080,
        );
        let (ready_tx, ready_rx) = oneshot::channel();
        let task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            client.start(ready_tx).await
        }));
        let (first, _) = listener.accept().await.unwrap();
        drop(first);
        let peer = async {
            let (socket, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(socket).await.unwrap()
        };
        let (ready, _peer) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(ready_rx, peer)
        })
        .await
        .unwrap();
        ready.unwrap().unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn exhausting_every_retry_reports_the_failure_through_ready() {
        // A port nothing listens on: every attempt is refused immediately,
        // which classifies as retryable, so the loop runs out its attempt
        // budget instead of ever reaching a permanent failure that sends
        // early.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let client = WebSocketTunnelClient::new(port, "127.0.0.1".to_owned(), 8080);
        let (ready_tx, ready_rx) = oneshot::channel();
        let result =
            tokio::time::timeout(Duration::from_secs(10), client.reconnect_loop(ready_tx, 2))
                .await
                .expect("a 2-attempt budget must not need more than a few seconds");
        let error = result.expect_err("every attempt was refused");
        assert!(error.contains("Max reconnection attempts"), "{error}");
        let ready_result = ready_rx
            .await
            .expect("ready must be resolved with the failure, not dropped silently");
        assert_eq!(ready_result, Err(error));
    }

    #[tokio::test]
    async fn a_tunnel_that_connected_does_not_spend_the_retry_budget() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = WebSocketTunnelClient::new(port, "127.0.0.1".to_owned(), 8080);
        let (ready_tx, ready_rx) = oneshot::channel();
        let task = tokio::spawn(async move { client.reconnect_loop(ready_tx, 2).await });

        let mut ready_rx = Some(ready_rx);
        for connection in 1..=3 {
            let (socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap_or_else(|_| panic!("the client gave up before connection {connection}"))
                .unwrap();
            let peer = tokio_tungstenite::accept_async(socket).await.unwrap();
            if let Some(ready) = ready_rx.take() {
                ready.await.unwrap().unwrap();
            }
            drop(peer);
        }
        assert!(!task.is_finished(), "{:?}", task.await);
        task.abort();
    }

    #[tokio::test]
    async fn forwards_the_request_without_a_disposable_tcp_probe() {
        use http_body_util::Full;
        use hyper::body::Bytes;
        use hyper_util::client::legacy::Client;
        use hyper_util::rt::TokioExecutor;
        use tokio::io::AsyncReadExt;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            9999,
            "127.0.0.1".to_owned(),
            listener.local_addr().unwrap().port(),
        );
        let http_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            assert!(request.starts_with(b"GET /request HTTP/1.1\r\n"));
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nresponse",
                )
                .await
                .unwrap();
        });
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            client.forward_to_local_service(
                &http_client,
                "request".to_owned(),
                "GET".to_owned(),
                "/request".to_owned(),
                Vec::new(),
                Vec::new(),
                false,
            ),
        )
        .await
        .unwrap()
        .0;
        match response {
            super::TunnelMessage::HttpResponse { status, body, .. } => {
                assert_eq!(status, 200);
                assert_eq!(body, b"response");
            }
            other => panic!("expected HTTP response, got {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn forwards_a_webdav_method_to_the_local_app() {
        use http_body_util::Full;
        use hyper::body::Bytes;
        use hyper_util::client::legacy::Client;
        use hyper_util::rt::TokioExecutor;
        use tokio::io::AsyncReadExt;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            9999,
            "127.0.0.1".to_owned(),
            listener.local_addr().unwrap().port(),
        );
        let http_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            assert!(request.starts_with(b"PROPFIND /files/ HTTP/1.1\r\n"));
            socket
                .write_all(b"HTTP/1.1 207 Multi-Status\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            client.forward_to_local_service(
                &http_client,
                "dav".to_owned(),
                "PROPFIND".to_owned(),
                "/files/".to_owned(),
                vec![("Depth".to_owned(), "1".to_owned())],
                Vec::new(),
                false,
            ),
        )
        .await
        .unwrap()
        .0;
        match response {
            super::TunnelMessage::HttpResponse { status, .. } => assert_eq!(status, 207),
            other => panic!("expected HTTP response, got {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn repeated_headers_keep_every_value() {
        use http_body_util::Full;
        use hyper::body::Bytes;
        use hyper_util::client::legacy::Client;
        use hyper_util::rt::TokioExecutor;
        use tokio::io::AsyncReadExt;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            9999,
            "127.0.0.1".to_owned(),
            listener.local_addr().unwrap().port(),
        );
        let http_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nSet-Cookie: session=abc; Path=/\r\nSet-Cookie: csrf=xyz; \
                      Path=/\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                )
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            client.forward_to_local_service(
                &http_client,
                "request".to_owned(),
                "GET".to_owned(),
                "/login".to_owned(),
                vec![
                    ("x-tag".to_owned(), "one".to_owned()),
                    ("x-tag".to_owned(), "two".to_owned()),
                ],
                Vec::new(),
                false,
            ),
        )
        .await
        .unwrap()
        .0;
        let request = server.await.unwrap();
        assert!(
            request.contains("x-tag: one\r\n") && request.contains("x-tag: two\r\n"),
            "{request}"
        );
        let wire = response.serialize().unwrap();
        match super::TunnelMessage::deserialize(&wire).unwrap() {
            super::TunnelMessage::HttpResponse { headers, .. } => {
                let cookies: Vec<&str> = headers
                    .iter()
                    .filter(|(name, _)| name == "set-cookie")
                    .map(|(_, value)| value.as_str())
                    .collect();
                assert_eq!(cookies, ["session=abc; Path=/", "csrf=xyz; Path=/"]);
            }
            other => panic!("expected HTTP response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_ascii_response_header_crosses_the_tunnel() {
        use http_body_util::Full;
        use hyper::body::Bytes;
        use hyper_util::client::legacy::Client;
        use hyper_util::rt::TokioExecutor;
        use tokio::io::AsyncReadExt;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            9999,
            "127.0.0.1".to_owned(),
            listener.local_addr().unwrap().port(),
        );
        let http_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(
                    "HTTP/1.1 200 OK\r\nContent-Disposition: attachment; filename=\"caf\u{e9}.txt\"\
                     \r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                        .as_bytes(),
                )
                .await
                .unwrap();
        });
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            client.forward_to_local_service(
                &http_client,
                "request".to_owned(),
                "GET".to_owned(),
                "/file".to_owned(),
                Vec::new(),
                Vec::new(),
                false,
            ),
        )
        .await
        .unwrap()
        .0;
        server.await.unwrap();
        let wire = response.serialize().unwrap();
        match super::TunnelMessage::deserialize(&wire).unwrap() {
            super::TunnelMessage::HttpResponse { headers, .. } => {
                let disposition = headers
                    .iter()
                    .find(|(name, _)| name == "content-disposition")
                    .map(|(_, value)| value.as_str());
                assert_eq!(
                    disposition,
                    Some("attachment; filename=\"caf\u{e9}.txt\""),
                    "{headers:?}"
                );
            }
            other => panic!("expected HTTP response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn open_response_does_not_hold_up_other_requests() {
        use futures::{
            SinkExt,
            StreamExt,
        };
        use tokio::io::AsyncReadExt;
        use tokio_tungstenite::tungstenite::Message;

        let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_port = app.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = app.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap();
                    if buf[..n].starts_with(b"GET /events") {
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: 1\n\n",
                            )
                            .await
                            .unwrap();
                        std::future::pending::<()>().await;
                    } else {
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                            )
                            .await
                            .unwrap();
                    }
                });
            }
        });

        let request = |id: &str, path: &str| {
            super::TunnelMessage::HttpRequest {
                id: id.to_owned(),
                method: "GET".to_owned(),
                path: path.to_owned(),
                headers: Vec::new(),
                body: Vec::new(),
                upgrade: false,
                flow_control: false,
            }
            .serialize()
            .unwrap()
        };

        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            ws_listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            app_port,
        );
        let (ready_tx, ready_rx) = oneshot::channel();
        let _task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            client.start(ready_tx).await
        }));
        let (socket, _) = ws_listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
        ready_rx.await.unwrap().unwrap();

        ws.send(Message::Binary(request("sse", "/events").into()))
            .await
            .unwrap();
        ws.send(Message::Binary(request("fast", "/health").into()))
            .await
            .unwrap();
        ws.send(Message::Binary(
            super::TunnelMessage::Ping.serialize().unwrap().into(),
        ))
        .await
        .unwrap();

        let mut replies = Vec::new();
        while replies.len() < 2 {
            let reply = tokio::time::timeout(Duration::from_secs(3), ws.next())
                .await
                .expect("no reply to /health or ping within 3s while /events is open")
                .unwrap()
                .unwrap();
            replies.push(super::TunnelMessage::deserialize(&reply.into_data()).unwrap());
        }
        assert!(replies.iter().any(|reply| matches!(
            reply,
            super::TunnelMessage::HttpResponse { id, status: 200, body, .. }
                if id == "fast" && body == b"ok"
        )));
        assert!(
            replies
                .iter()
                .any(|reply| matches!(reply, super::TunnelMessage::Pong))
        );
    }

    #[tokio::test]
    async fn upgrade_request_streams_bytes_to_the_local_app_and_back() {
        use futures::{
            SinkExt,
            StreamExt,
        };
        use tokio::io::AsyncReadExt;
        use tokio_tungstenite::tungstenite::Message;

        use super::TunnelMessage;

        let _db = kftray_commons::test_utils::test_db().await;
        let dir = tempfile::tempdir().unwrap();
        let _env = kftray_commons::test_utils::EnvVarGuard::set(
            "KFTRAY_CONFIG",
            dir.path().to_str().unwrap(),
        );
        let mode = super::DatabaseMode::Memory;
        let config_id = create_logged_config(mode).await;
        set_logging(config_id, mode, true).await;

        let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_port = app.local_addr().unwrap().port();
        let app_task = tokio::spawn(async move {
            let (mut socket, _) = app.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                      Connection: Upgrade\r\n\r\n",
                )
                .await
                .unwrap();
            let mut ping = [0u8; 4];
            socket.read_exact(&mut ping).await.unwrap();
            socket.write_all(b"pong").await.unwrap();
            String::from_utf8(head).unwrap().to_ascii_lowercase()
        });

        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            ws_listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            app_port,
        )
        .with_http_logs(config_id, mode);
        let (ready_tx, ready_rx) = oneshot::channel();
        let _task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            client.start(ready_tx).await
        }));
        let (socket, _) = ws_listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
        ready_rx.await.unwrap().unwrap();

        let request = TunnelMessage::HttpRequest {
            id: "ws".to_owned(),
            method: "GET".to_owned(),
            path: "/socket".to_owned(),
            headers: vec![
                ("connection".to_owned(), "upgrade".to_owned()),
                ("upgrade".to_owned(), "websocket".to_owned()),
            ],
            body: Vec::new(),
            upgrade: true,
            flow_control: true,
        };
        ws.send(Message::Binary(request.serialize().unwrap().into()))
            .await
            .unwrap();

        let mut replies = Vec::new();
        loop {
            let reply = tokio::time::timeout(Duration::from_secs(3), ws.next())
                .await
                .expect("no tunnel message within 3s")
                .unwrap()
                .unwrap();
            let reply = TunnelMessage::deserialize(&reply.into_data()).unwrap();
            if let TunnelMessage::HttpResponse {
                status: 101,
                upgraded: true,
                ..
            } = reply
            {
                let data = TunnelMessage::StreamData {
                    id: "ws".to_owned(),
                    data: b"ping".to_vec(),
                };
                ws.send(Message::Binary(data.serialize().unwrap().into()))
                    .await
                    .unwrap();
            }
            let closed = matches!(&reply, TunnelMessage::StreamClose { id } if id == "ws");
            replies.push(reply);
            if closed {
                break;
            }
        }

        let head = app_task.await.unwrap();
        assert!(head.contains("upgrade: websocket"), "{head}");
        let received: Vec<u8> = replies
            .iter()
            .filter_map(|reply| match reply {
                TunnelMessage::StreamData { id, data } if id == "ws" => Some(data.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(received, b"pong", "{replies:?}");
        assert!(
            replies.iter().any(|reply| matches!(
                reply,
                TunnelMessage::StreamCredit { id, bytes: 4 } if id == "ws"
            )),
            "{replies:?}"
        );
        assert!(
            matches!(
                replies.first(),
                Some(TunnelMessage::HttpResponse {
                    status: 101,
                    upgraded: true,
                    flow_control: true,
                    ..
                })
            ),
            "{replies:?}"
        );
        let path = dir
            .path()
            .join("http_logs")
            .join(format!("{config_id}_{app_port}.http"));
        let contents = wait_for_log(&path, "GET /socket HTTP/1.1").await;
        assert_eq!(contents.matches("# Request at:").count(), 1, "{contents}");
        assert!(!contents.contains("# Response at:"), "{contents}");
        assert!(!contents.contains("ping"), "{contents}");
        assert!(!contents.contains("pong"), "{contents}");
    }

    type Relay = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

    async fn connect_relay(
        app_port: u16,
    ) -> (
        Relay,
        tokio_util::task::AbortOnDropHandle<Result<(), String>>,
    ) {
        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            ws_listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            app_port,
        );
        let (ready_tx, ready_rx) = oneshot::channel();
        let task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            client.start(ready_tx).await
        }));
        let (socket, _) = ws_listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(socket).await.unwrap();
        ready_rx.await.unwrap().unwrap();
        (ws, task)
    }

    async fn send(relay: &mut Relay, message: super::TunnelMessage) {
        use futures::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        relay
            .send(Message::Binary(message.serialize().unwrap().into()))
            .await
            .unwrap();
    }

    async fn recv(relay: &mut Relay) -> super::TunnelMessage {
        use futures::StreamExt;

        let message = tokio::time::timeout(Duration::from_secs(5), relay.next())
            .await
            .expect("no tunnel message within 5s")
            .unwrap()
            .unwrap();
        super::TunnelMessage::deserialize(&message.into_data()).unwrap()
    }

    async fn assert_quiet(relay: &mut Relay) {
        use futures::StreamExt;

        if let Ok(message) = tokio::time::timeout(Duration::from_millis(300), relay.next()).await {
            panic!("expected no tunnel message, got {message:?}");
        }
    }

    async fn stream_bytes(relay: &mut Relay, id: &str, total: usize) -> usize {
        let mut received = 0;
        while received < total {
            match recv(relay).await {
                super::TunnelMessage::StreamData { id: got, data } if got == id => {
                    received += data.len();
                }
                other => panic!("expected stream data for {id}, got {other:?}"),
            }
        }
        received
    }

    fn upgrade_request(id: &str, flow_control: bool) -> super::TunnelMessage {
        super::TunnelMessage::HttpRequest {
            id: id.to_owned(),
            method: "GET".to_owned(),
            path: format!("/{id}"),
            headers: vec![
                ("connection".to_owned(), "upgrade".to_owned()),
                ("upgrade".to_owned(), "websocket".to_owned()),
            ],
            body: Vec::new(),
            upgrade: true,
            flow_control,
        }
    }

    async fn open_stream(relay: &mut Relay, id: &str) {
        send(relay, upgrade_request(id, true)).await;
        match recv(relay).await {
            super::TunnelMessage::HttpResponse {
                id: got,
                status: 101,
                upgraded: true,
                flow_control: true,
                ..
            } => assert_eq!(got, id),
            other => panic!("expected an upgraded response for {id}, got {other:?}"),
        }
    }

    async fn flooding_app() -> (u16, tokio::sync::mpsc::UnboundedReceiver<(String, Vec<u8>)>) {
        use tokio::io::AsyncReadExt;

        let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = app.local_addr().unwrap().port();
        let (closed_tx, closed_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = app.accept().await.unwrap();
                let closed_tx = closed_tx.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        head.push(socket.read_u8().await.unwrap());
                    }
                    let head = String::from_utf8(head).unwrap();
                    let path = head.split(' ').nth(1).unwrap().to_owned();
                    if path == "/health" {
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                            )
                            .await
                            .unwrap();
                        return;
                    }
                    socket
                        .write_all(
                            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                              Connection: Upgrade\r\n\r\n",
                        )
                        .await
                        .unwrap();
                    let (mut read, mut write) = socket.into_split();
                    let flood = tokio::spawn(async move {
                        let _ = write.write_all(&vec![7u8; 4 * super::STREAM_WINDOW]).await;
                        std::future::pending::<()>().await;
                    });
                    let mut received = Vec::new();
                    let _ = read.read_to_end(&mut received).await;
                    flood.abort();
                    let _ = closed_tx.send((path, received));
                });
            }
        });
        (port, closed_rx)
    }

    #[tokio::test]
    async fn upgrade_request_without_flow_control_is_not_upgraded() {
        let (app_port, _closed) = flooding_app().await;
        let (mut relay, _task) = connect_relay(app_port).await;

        send(&mut relay, upgrade_request("old", false)).await;
        match recv(&mut relay).await {
            super::TunnelMessage::HttpResponse {
                id,
                upgraded: false,
                flow_control: false,
                ..
            } => assert_eq!(id, "old"),
            other => panic!("expected a plain response, got {other:?}"),
        }
        assert_quiet(&mut relay).await;
    }

    #[tokio::test]
    async fn stalled_stream_stops_at_the_window_and_other_traffic_keeps_moving() {
        let (app_port, mut closed) = flooding_app().await;
        let (mut relay, _task) = connect_relay(app_port).await;
        let window = super::STREAM_WINDOW;

        open_stream(&mut relay, "a").await;
        assert_eq!(stream_bytes(&mut relay, "a", window).await, window);
        assert_quiet(&mut relay).await;

        open_stream(&mut relay, "b").await;
        assert_eq!(stream_bytes(&mut relay, "b", window).await, window);
        assert_quiet(&mut relay).await;

        send(
            &mut relay,
            super::TunnelMessage::HttpRequest {
                id: "health".to_owned(),
                method: "GET".to_owned(),
                path: "/health".to_owned(),
                headers: Vec::new(),
                body: Vec::new(),
                upgrade: false,
                flow_control: false,
            },
        )
        .await;
        match recv(&mut relay).await {
            super::TunnelMessage::HttpResponse {
                id, status, body, ..
            } => {
                assert_eq!(
                    (id.as_str(), status, body.as_slice()),
                    ("health", 200, &b"ok"[..])
                );
            }
            other => panic!("expected the health response, got {other:?}"),
        }

        send(
            &mut relay,
            super::TunnelMessage::StreamCredit {
                id: "a".to_owned(),
                bytes: 1000,
            },
        )
        .await;
        assert_eq!(stream_bytes(&mut relay, "a", 1000).await, 1000);
        assert_quiet(&mut relay).await;

        send(
            &mut relay,
            super::TunnelMessage::StreamData {
                id: "a".to_owned(),
                data: b"bye".to_vec(),
            },
        )
        .await;
        send(
            &mut relay,
            super::TunnelMessage::StreamClose { id: "a".to_owned() },
        )
        .await;
        let first = tokio::time::timeout(Duration::from_secs(5), closed.recv()).await;
        assert_eq!(first.unwrap(), Some(("/a".to_owned(), b"bye".to_vec())));

        drop(relay);
        let second = tokio::time::timeout(Duration::from_secs(5), closed.recv()).await;
        assert_eq!(second.unwrap(), Some(("/b".to_owned(), Vec::new())));
    }

    #[allow(clippy::result_large_err)]
    async fn connect_tcp_relay(
        app_port: u16, relay_supports_tcp: bool, relay_half_close: bool,
    ) -> (
        Relay,
        oneshot::Receiver<Result<(), String>>,
        tokio_util::task::AbortOnDropHandle<Result<(), String>>,
    ) {
        use tokio_tungstenite::tungstenite::handshake::server::{
            Request,
            Response,
        };

        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            ws_listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            app_port,
        )
        .carrying_tcp(true);
        let (ready_tx, ready_rx) = oneshot::channel();
        let task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            client.start(ready_tx).await
        }));
        let (socket, _) = ws_listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_hdr_async(
            socket,
            |request: &Request, mut response: Response| {
                assert_eq!(request.uri().path(), super::TCP_MODE_PATH);
                assert!(request.headers().contains_key(super::HALF_CLOSE_HEADER));
                if relay_supports_tcp {
                    response
                        .headers_mut()
                        .insert(super::TUNNEL_MODE_HEADER, "tcp".parse().unwrap());
                }
                if relay_half_close {
                    response
                        .headers_mut()
                        .insert(super::HALF_CLOSE_HEADER, "1".parse().unwrap());
                }
                Ok(response)
            },
        )
        .await
        .unwrap();
        (ws, ready_rx, task)
    }

    #[tokio::test]
    async fn tcp_stream_carries_bytes_to_the_local_port_and_back() {
        use tokio::io::AsyncReadExt;

        let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_port = app.local_addr().unwrap().port();
        let app_task = tokio::spawn(async move {
            let (mut socket, _) = app.accept().await.unwrap();
            let mut request = [0u8; 8];
            socket.read_exact(&mut request).await.unwrap();
            socket.write_all(b"N").await.unwrap();
            let mut rest = Vec::new();
            socket.read_to_end(&mut rest).await.unwrap();
            (request, rest)
        });
        let (mut relay, ready, _task) = connect_tcp_relay(app_port, true, false).await;
        ready.await.unwrap().unwrap();

        let ssl_request = vec![0, 0, 0, 8, 4, 210, 22, 47];
        send(
            &mut relay,
            super::TunnelMessage::StreamOpen {
                id: "db".to_owned(),
            },
        )
        .await;
        send(
            &mut relay,
            super::TunnelMessage::StreamData {
                id: "db".to_owned(),
                data: ssl_request.clone(),
            },
        )
        .await;

        let mut credited = 0;
        let mut reply = Vec::new();
        while credited < ssl_request.len() || reply.is_empty() {
            match recv(&mut relay).await {
                super::TunnelMessage::StreamCredit { id, bytes } if id == "db" => {
                    credited += bytes;
                }
                super::TunnelMessage::StreamData { id, data } if id == "db" => {
                    reply.extend(data);
                }
                other => panic!("expected stream traffic for db, got {other:?}"),
            }
        }
        assert_eq!(reply, b"N");

        send(
            &mut relay,
            super::TunnelMessage::StreamClose {
                id: "db".to_owned(),
            },
        )
        .await;
        let (request, rest) = tokio::time::timeout(Duration::from_secs(5), app_task)
            .await
            .expect("the local connection stayed open after StreamClose")
            .unwrap();
        assert_eq!(request.to_vec(), ssl_request);
        assert!(rest.is_empty());
    }

    #[tokio::test]
    async fn tcp_stream_to_a_closed_local_port_is_closed() {
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_port = closed.local_addr().unwrap().port();
        drop(closed);
        let (mut relay, ready, _task) = connect_tcp_relay(app_port, true, false).await;
        ready.await.unwrap().unwrap();

        send(
            &mut relay,
            super::TunnelMessage::StreamOpen {
                id: "db".to_owned(),
            },
        )
        .await;
        assert!(matches!(
            recv(&mut relay).await,
            super::TunnelMessage::StreamClose { id } if id == "db"
        ));
    }

    #[tokio::test]
    async fn tcp_expose_on_a_relay_without_tcp_support_fails_startup() {
        let (_relay, ready, _task) = connect_tcp_relay(5432, false, false).await;
        let error = tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.contains("cannot carry TCP"), "{error}");
    }

    #[tokio::test]
    async fn tcp_stream_end_lets_the_local_port_reply_after_eof() {
        use tokio::io::AsyncReadExt;

        let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_port = app.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = app.accept().await.unwrap();
            let mut request = Vec::new();
            socket.read_to_end(&mut request).await.unwrap();
            socket.write_all(b"pong").await.unwrap();
        });
        let (mut relay, ready, _task) = connect_tcp_relay(app_port, true, true).await;
        ready.await.unwrap().unwrap();

        let id = || "db".to_owned();
        send(&mut relay, super::TunnelMessage::StreamOpen { id: id() }).await;
        send(
            &mut relay,
            super::TunnelMessage::StreamData {
                id: id(),
                data: b"ping".to_vec(),
            },
        )
        .await;
        send(&mut relay, super::TunnelMessage::StreamEnd { id: id() }).await;

        let mut reply = Vec::new();
        let mut ended = false;
        loop {
            match recv(&mut relay).await {
                super::TunnelMessage::StreamCredit { bytes: 4, .. } => {}
                super::TunnelMessage::StreamData { data, .. } => reply.extend(data),
                super::TunnelMessage::StreamEnd { .. } => ended = true,
                super::TunnelMessage::StreamClose { id: got } if got == id() => break,
                other => panic!("expected stream traffic for db, got {other:?}"),
            }
        }
        assert_eq!(reply, b"pong");
        assert!(
            ended,
            "the app closed the stream without ending its side first"
        );
    }

    #[tokio::test]
    async fn tcp_stream_moves_both_ways_when_the_local_port_writes_before_it_reads() {
        use std::sync::Arc;

        use futures::{
            SinkExt,
            StreamExt,
        };
        use tokio::io::AsyncReadExt;
        use tokio::sync::Semaphore;
        use tokio_tungstenite::tungstenite::Message;

        use super::{
            STREAM_WINDOW,
            TunnelMessage,
        };

        const TOTAL: usize = 8 * 1024 * 1024;
        const CHUNK: usize = 16 * 1024;
        let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_port = app.local_addr().unwrap().port();
        let local_port = tokio::spawn(async move {
            let (mut socket, _) = app.accept().await.unwrap();
            for _ in 0..TOTAL / CHUNK {
                socket.write_all(&[1u8; CHUNK]).await.unwrap();
            }
            let mut buf = vec![0u8; CHUNK];
            let mut got = 0;
            while got < TOTAL {
                got += socket.read(&mut buf).await.unwrap();
            }
        });
        let (relay, ready, _task) = connect_tcp_relay(app_port, true, true).await;
        ready.await.unwrap().unwrap();
        let (mut sink, mut stream) = relay.split();
        let (out, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<TunnelMessage>();
        tokio::spawn(async move {
            while let Some(message) = out_rx.recv().await {
                let bytes = message.serialize().unwrap();
                sink.send(Message::Binary(bytes.into())).await.unwrap();
            }
        });
        let id = || "db".to_owned();
        out.send(TunnelMessage::StreamOpen { id: id() }).unwrap();
        let window = Arc::new(Semaphore::new(STREAM_WINDOW));
        let (credit, reply) = (window.clone(), out.clone());
        tokio::spawn(async move {
            while let Some(Ok(message)) = stream.next().await {
                match TunnelMessage::deserialize(&message.into_data()).unwrap() {
                    TunnelMessage::StreamData { data, .. } => {
                        let bytes = data.len();
                        reply
                            .send(TunnelMessage::StreamCredit { id: id(), bytes })
                            .unwrap();
                    }
                    TunnelMessage::StreamCredit { bytes, .. } => credit.add_permits(bytes),
                    _ => {}
                }
            }
        });
        let relay_writer = tokio::spawn(async move {
            for _ in 0..TOTAL / CHUNK {
                window.acquire_many(CHUNK as u32).await.unwrap().forget();
                let data = vec![2u8; CHUNK];
                out.send(TunnelMessage::StreamData { id: id(), data })
                    .unwrap();
            }
        });

        tokio::time::timeout(Duration::from_secs(60), async {
            relay_writer.await.unwrap();
            local_port.await.unwrap();
        })
        .await
        .expect("8 MiB each way through the tunnel did not finish in 60 s");
    }
    async fn wait_for_log(path: &std::path::Path, expected: &str) -> String {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let contents = tokio::fs::read_to_string(path).await.unwrap_or_default();
                if contents.contains(expected) {
                    return contents;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the completed HTTP exchange did not reach the log")
    }

    async fn create_logged_config(mode: super::DatabaseMode) -> i64 {
        kftray_commons::utils::config::insert_config_with_mode(
            kftray_commons::models::config_model::Config {
                alias: Some("logged-expose".into()),
                workload_type: Some("expose".into()),
                protocol: "tcp".into(),
                local_port: Some(8080),
                ..Default::default()
            },
            mode,
        )
        .await
        .unwrap()
    }

    async fn set_logging(config_id: i64, mode: super::DatabaseMode, enabled: bool) {
        let mut config =
            kftray_commons::models::http_logs_config_model::HttpLogsConfig::new(config_id);
        config.enabled = enabled;
        kftray_commons::utils::http_logs_config::update_http_logs_config_with_mode(&config, mode)
            .await
            .unwrap();
    }

    async fn wait_for_log_setting(logs: &super::ExposeHttpLogs, enabled: bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if logs
                    .settings
                    .read()
                    .await
                    .as_ref()
                    .is_some_and(|config| config.enabled == enabled)
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the logging setting did not refresh");
    }

    #[tokio::test]
    async fn expose_http_logs_follow_toggles_and_pair_concurrent_responses() {
        use tokio::io::AsyncReadExt;

        use super::{
            DatabaseMode,
            TunnelMessage,
        };

        let _db = kftray_commons::test_utils::test_db().await;
        let dir = tempfile::tempdir().unwrap();
        let _env = kftray_commons::test_utils::EnvVarGuard::set(
            "KFTRAY_CONFIG",
            dir.path().to_str().unwrap(),
        );
        kftray_commons::utils::db::init().await.unwrap();
        kftray_commons::utils::migration::migrate_configs(None)
            .await
            .unwrap();

        for mode in [DatabaseMode::File, DatabaseMode::Memory] {
            let config_id = create_logged_config(mode).await;
            set_logging(config_id, mode, false).await;
            let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let app_port = app.local_addr().unwrap().port();
            let (started, slow_started) = oneshot::channel();
            let (release, released) = oneshot::channel();
            let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                let mut started = Some(started);
                let mut released = Some(released);
                let mut replies = tokio::task::JoinSet::new();
                for _ in 0..6 {
                    let (mut socket, _) = app.accept().await.unwrap();
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        head.push(socket.read_u8().await.unwrap());
                    }
                    if head.starts_with(b"POST /slow?q=one ") {
                        let mut body = [0; 6];
                        socket.read_exact(&mut body).await.unwrap();
                        assert_eq!(&body, b"sample");
                        started.take().unwrap().send(()).unwrap();
                        let released = released.take().unwrap();
                        replies.spawn(async move {
                            released.await.unwrap();
                            socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 5\r\nConnection: close\r\n\r\nlater").await.unwrap();
                        });
                    } else {
                        replies.spawn(async move {
                            socket.write_all(b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n").await.unwrap();
                        });
                    }
                }
                while let Some(reply) = replies.join_next().await {
                    reply.unwrap();
                }
            }));
            let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = WebSocketTunnelClient::new(
                ws_listener.local_addr().unwrap().port(),
                "127.0.0.1".into(),
                app_port,
            )
            .with_http_logs(config_id, mode);
            let log_state = client.http_logs.as_ref().unwrap().clone();
            let (ready_tx, ready_rx) = oneshot::channel();
            let _client = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                client.start(ready_tx).await
            }));
            let (socket, _) = ws_listener.accept().await.unwrap();
            let mut relay = tokio_tungstenite::accept_async(socket).await.unwrap();
            ready_rx.await.unwrap().unwrap();
            let request = |id: &str| TunnelMessage::HttpRequest {
                id: id.into(),
                method: "GET".into(),
                path: format!("/{id}"),
                headers: Vec::new(),
                body: Vec::new(),
                upgrade: false,
                flow_control: false,
            };
            send(&mut relay, request("hidden-before")).await;
            assert!(matches!(
                recv(&mut relay).await,
                TunnelMessage::HttpResponse { status: 201, .. }
            ));
            set_logging(config_id, mode, true).await;
            wait_for_log_setting(&log_state, true).await;
            send(
                &mut relay,
                TunnelMessage::HttpRequest {
                    id: "slow".into(),
                    method: "POST".into(),
                    path: "/slow?q=one".into(),
                    headers: vec![
                        ("content-length".into(), "6".into()),
                        ("x-repeat".into(), "first".into()),
                        ("x-repeat".into(), "second".into()),
                    ],
                    body: b"sample".to_vec(),
                    upgrade: false,
                    flow_control: false,
                },
            )
            .await;
            tokio::time::timeout(Duration::from_secs(5), slow_started)
                .await
                .unwrap()
                .unwrap();
            send(&mut relay, request("fast")).await;
            assert!(
                matches!(recv(&mut relay).await, TunnelMessage::HttpResponse { id, status: 201, body, .. } if id == "fast" && body == b"hello")
            );
            set_logging(config_id, mode, false).await;
            wait_for_log_setting(&log_state, false).await;
            send(&mut relay, request("hidden-between")).await;
            assert!(matches!(
                recv(&mut relay).await,
                TunnelMessage::HttpResponse { status: 201, .. }
            ));
            set_logging(config_id, mode, true).await;
            wait_for_log_setting(&log_state, true).await;
            send(&mut relay, request("resumed")).await;
            assert!(
                matches!(recv(&mut relay).await, TunnelMessage::HttpResponse { id, status: 201, .. } if id == "resumed")
            );
            set_logging(config_id, mode, false).await;
            wait_for_log_setting(&log_state, false).await;
            release.send(()).unwrap();
            assert!(
                matches!(recv(&mut relay).await, TunnelMessage::HttpResponse { id, status: 503, body, .. } if id == "slow" && body == b"later")
            );
            send(&mut relay, request("hidden-after")).await;
            assert!(matches!(
                recv(&mut relay).await,
                TunnelMessage::HttpResponse { status: 201, .. }
            ));
            server.await.unwrap();
            let path = dir
                .path()
                .join("http_logs")
                .join(format!("{config_id}_{app_port}.http"));
            let contents = wait_for_log(&path, "later").await;
            assert_eq!(contents.matches("# Request at:").count(), 3, "{contents}");
            assert_eq!(contents.matches("# Response at:").count(), 3, "{contents}");
            assert!(contents.contains("POST /slow?q=one HTTP/1.1"), "{contents}");
            assert!(contents.contains("sample"), "{contents}");
            for header in [
                "x-repeat: first",
                "x-repeat: second",
                "set-cookie: a=1",
                "set-cookie: b=2",
            ] {
                assert!(contents.contains(header), "{contents}");
            }
            assert!(!contents.contains("hidden-"), "{contents}");
            assert!(!contents.contains("chunked"), "{contents}");
            let entries: Vec<_> = contents.split("# Trace ID: ").skip(1).collect();
            let mut request_ids = Vec::new();
            for (path, status) in [
                ("POST /slow?q=one", "503 Service Unavailable"),
                ("GET /fast", "201 Created"),
                ("GET /resumed", "201 Created"),
            ] {
                let entry = entries.iter().find(|entry| entry.contains(path)).unwrap();
                let id = entry.lines().next().unwrap();
                let response = entries
                    .iter()
                    .find(|entry| entry.starts_with(id) && entry.contains("# Response at:"))
                    .unwrap();
                assert!(response.contains(status), "{contents}");
                request_ids.push(id);
            }
            assert_ne!(request_ids[0], request_ids[1]);
        }
    }

    #[tokio::test]
    async fn expose_requests_continue_when_the_settings_pool_is_busy() {
        use tokio::io::AsyncReadExt;

        use super::{
            DatabaseMode,
            TunnelMessage,
        };

        let _db = kftray_commons::test_utils::test_db().await;
        let dir = tempfile::tempdir().unwrap();
        let _env = kftray_commons::test_utils::EnvVarGuard::set(
            "KFTRAY_CONFIG",
            dir.path().to_str().unwrap(),
        );
        let mode = DatabaseMode::Memory;
        let config_id = create_logged_config(mode).await;
        for enabled in [false, true] {
            set_logging(config_id, mode, enabled).await;
            let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let app_port = app.local_addr().unwrap().port();
            let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                let (mut socket, _) = app.accept().await.unwrap();
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(socket.read_u8().await.unwrap());
                }
                socket
                    .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
            }));
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = WebSocketTunnelClient::new(
                listener.local_addr().unwrap().port(),
                "127.0.0.1".into(),
                app_port,
            )
            .with_http_logs(config_id, mode);
            let (ready_tx, ready_rx) = oneshot::channel();
            let client = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                client.start(ready_tx).await
            }));
            let (socket, _) = listener.accept().await.unwrap();
            let mut relay = tokio_tungstenite::accept_async(socket).await.unwrap();
            ready_rx.await.unwrap().unwrap();
            let context = kftray_commons::utils::db_mode::DatabaseManager::get_context(mode)
                .await
                .unwrap();
            let mut held = Vec::new();
            for _ in 0..context.pool.options().get_max_connections() {
                held.push(context.pool.acquire().await.unwrap());
            }
            send(
                &mut relay,
                TunnelMessage::HttpRequest {
                    id: "unblocked".into(),
                    method: "GET".into(),
                    path: "/unblocked".into(),
                    headers: Vec::new(),
                    body: Vec::new(),
                    upgrade: false,
                    flow_control: false,
                },
            )
            .await;
            let reply = tokio::time::timeout(Duration::from_secs(1), recv(&mut relay)).await;
            drop(held);
            assert!(matches!(
                reply.unwrap(),
                TunnelMessage::HttpResponse { status: 204, .. }
            ));
            server.await.unwrap();
            client.abort();
            let _ = client.await;
        }
    }

    #[tokio::test]
    async fn expose_logging_applies_saved_rotation_and_cleanup_settings() {
        use tokio::io::AsyncReadExt;

        use super::{
            DatabaseMode,
            TunnelMessage,
        };

        let _db = kftray_commons::test_utils::test_db().await;
        let dir = tempfile::tempdir().unwrap();
        let _env = kftray_commons::test_utils::EnvVarGuard::set(
            "KFTRAY_CONFIG",
            dir.path().to_str().unwrap(),
        );
        let mode = DatabaseMode::Memory;
        let config_id = create_logged_config(mode).await;
        let mut settings =
            kftray_commons::models::http_logs_config_model::HttpLogsConfig::new(config_id);
        settings.enabled = true;
        settings.auto_cleanup = false;
        kftray_commons::utils::http_logs_config::update_http_logs_config_with_mode(&settings, mode)
            .await
            .unwrap();
        let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = app.local_addr().unwrap().port();
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            for size in [2048, 0, 0] {
                let (mut socket, _) = app.accept().await.unwrap();
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(socket.read_u8().await.unwrap());
                }
                socket.read_exact(&mut vec![0; size]).await.unwrap();
                socket
                    .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
            }
        }));
        let client = WebSocketTunnelClient::new(9999, "127.0.0.1".into(), port)
            .with_http_logs(config_id, mode);
        let logs = client.http_logs.as_ref().unwrap();
        logs.sync().await;
        let http =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build_http::<http_body_util::Full<bytes::Bytes>>();
        let path = dir
            .path()
            .join("http_logs")
            .join(format!("{config_id}_{port}.http"));
        for (index, name) in ["before", "after", "last"].into_iter().enumerate() {
            let stale = dir
                .path()
                .join("http_logs")
                .join(format!("{config_id}_{port}_20000101_00000{index}.http"));
            if index > 0 {
                tokio::fs::write(&stale, "expired rotation").await.unwrap();
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&stale)
                    .unwrap();
                file.set_times(
                    std::fs::FileTimes::new().set_modified(
                        std::time::SystemTime::now() - Duration::from_secs(2 * 86400),
                    ),
                )
                .unwrap();
                settings.max_file_size = if index == 1 { 1024 } else { 256 };
                settings.retention_days = if index == 1 { 1 } else { 0 };
                settings.auto_cleanup = index == 1;
                kftray_commons::utils::http_logs_config::update_http_logs_config_with_mode(
                    &settings, mode,
                )
                .await
                .unwrap();
                logs.sync().await;
            }
            let body = if index == 0 {
                vec![b'x'; 2048]
            } else {
                Vec::new()
            };
            let (reply, _) = client
                .forward_to_local_service(
                    &http,
                    name.into(),
                    "POST".into(),
                    format!("/{name}"),
                    vec![("content-length".into(), body.len().to_string())],
                    body,
                    false,
                )
                .await;
            assert!(matches!(
                reply,
                TunnelMessage::HttpResponse { status: 204, .. }
            ));
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let mut files = tokio::fs::read_dir(path.parent().unwrap()).await.unwrap();
                    let mut found = false;
                    while let Some(file) = files.next_entry().await.unwrap() {
                        let contents = tokio::fs::read_to_string(file.path())
                            .await
                            .unwrap_or_default();
                        found |= contents.contains(&format!("POST /{name} HTTP/1.1"));
                    }
                    if found {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("missing log entry for {name}"));
            if index == 1 {
                assert!(
                    !stale.exists(),
                    "the new retention policy did not remove an expired log"
                );
                let mut rotated = Vec::new();
                let mut files = tokio::fs::read_dir(path.parent().unwrap()).await.unwrap();
                while let Some(file) = files.next_entry().await.unwrap() {
                    if file.path() != path {
                        rotated.push(tokio::fs::read_to_string(file.path()).await.unwrap());
                    }
                }
                assert!(
                    rotated
                        .iter()
                        .any(|contents| contents.contains("POST /before HTTP/1.1")),
                    "the new file limit did not rotate the existing log"
                );
            } else if index == 2 {
                assert!(
                    stale.exists(),
                    "disabled automatic cleanup removed an expired log"
                );
            }
        }
        server.await.unwrap();
    }
}
