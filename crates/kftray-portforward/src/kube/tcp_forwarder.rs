use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::time::Duration;

use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{
    debug,
    error,
};

use crate::Logger;
use crate::kube::http_log_watcher::HttpLogStateWatcher;

const BUFFER_SIZE: usize = 65536;
const MAX_LOGGED_REQUEST_SIZE: usize = 10 * 1024 * 1024;
const MAX_CHUNK_LINE_SIZE: usize = 4096;

#[derive(Default)]
struct RequestLogBuffer {
    buffer: Vec<u8>,
    body: RequestBody,
    logged_at_head: bool,
}

#[derive(Default)]
enum RequestBody {
    #[default]
    Headers,
    Length(usize),
    ChunkSize(Vec<u8>),
    ChunkData(usize),
    Trailers(Vec<u8>),
}

impl RequestLogBuffer {
    fn observe(&mut self, mut bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut complete = Vec::new();
        while !bytes.is_empty() {
            let consumed = match &mut self.body {
                RequestBody::Headers => {
                    let start = self.buffer.len();
                    self.buffer.extend_from_slice(bytes);
                    match Self::parse_head(&self.buffer) {
                        Ok(Some((head_len, body, expect_continue))) => {
                            self.buffer.truncate(head_len);
                            self.body = body;
                            if expect_continue && !matches!(self.body, RequestBody::Length(0)) {
                                complete.push(std::mem::take(&mut self.buffer));
                                self.logged_at_head = true;
                            }
                            head_len - start
                        }
                        Ok(None) if self.buffer.len() <= BUFFER_SIZE => bytes.len(),
                        _ => {
                            self.reset();
                            return complete;
                        }
                    }
                }
                RequestBody::Length(remaining) | RequestBody::ChunkData(remaining) => {
                    let consumed = (*remaining).min(bytes.len());
                    *remaining -= consumed;
                    Self::store(&mut self.buffer, self.logged_at_head, &bytes[..consumed]);
                    consumed
                }
                RequestBody::ChunkSize(line) | RequestBody::Trailers(line) => {
                    let consumed = bytes
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(bytes.len(), |end| end + 1);
                    line.extend_from_slice(&bytes[..consumed]);
                    if line.len() > MAX_CHUNK_LINE_SIZE {
                        self.reset();
                        return complete;
                    }
                    Self::store(&mut self.buffer, self.logged_at_head, &bytes[..consumed]);
                    consumed
                }
            };
            bytes = &bytes[consumed..];

            let next = match &mut self.body {
                RequestBody::Headers => None,
                RequestBody::Length(0) => Some(RequestBody::Headers),
                RequestBody::ChunkData(0) => Some(RequestBody::ChunkSize(Vec::new())),
                RequestBody::ChunkSize(line) if line.ends_with(b"\n") => {
                    match httparse::parse_chunk_size(line) {
                        Ok(httparse::Status::Complete((_, 0))) => {
                            Some(RequestBody::Trailers(Vec::new()))
                        }
                        Ok(httparse::Status::Complete((_, size))) => Some(RequestBody::ChunkData(
                            usize::try_from(size)
                                .unwrap_or(usize::MAX)
                                .saturating_add(2),
                        )),
                        _ => {
                            self.reset();
                            return complete;
                        }
                    }
                }
                RequestBody::Trailers(line) if matches!(line.as_slice(), b"\r\n" | b"\n") => {
                    Some(RequestBody::Headers)
                }
                RequestBody::Trailers(line) if line.ends_with(b"\n") => {
                    line.clear();
                    None
                }
                _ => None,
            };
            if let Some(next) = next {
                if matches!(next, RequestBody::Headers) {
                    let request = std::mem::take(&mut self.buffer);
                    if !std::mem::take(&mut self.logged_at_head) {
                        complete.push(request);
                    }
                }
                self.body = next;
            }
        }
        complete
    }

    fn take_started(&mut self) -> Option<Vec<u8>> {
        if matches!(self.body, RequestBody::Headers) || self.logged_at_head {
            return None;
        }
        let started = std::mem::take(&mut self.buffer);
        self.reset();
        Some(started)
    }

    fn store(buffer: &mut Vec<u8>, logged_at_head: bool, bytes: &[u8]) {
        if logged_at_head {
            return;
        }
        let room = MAX_LOGGED_REQUEST_SIZE.saturating_sub(buffer.len());
        buffer.extend_from_slice(&bytes[..bytes.len().min(room)]);
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.body = RequestBody::Headers;
        self.logged_at_head = false;
    }

    fn parse_head(buffer: &[u8]) -> Result<Option<(usize, RequestBody, bool)>, httparse::Error> {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut request = httparse::Request::new(&mut headers);
        let head_len = match request.parse(buffer)? {
            httparse::Status::Complete(head_len) => head_len,
            httparse::Status::Partial => return Ok(None),
        };
        let header_value = |name: &str| {
            request
                .headers
                .iter()
                .rev()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .and_then(|h| std::str::from_utf8(h.value).ok())
        };
        let chunked = header_value("transfer-encoding").is_some_and(|v| {
            v.rsplit(',')
                .next()
                .is_some_and(|coding| coding.trim().eq_ignore_ascii_case("chunked"))
        });
        let body = if chunked {
            RequestBody::ChunkSize(Vec::new())
        } else {
            let length = header_value("content-length")
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            RequestBody::Length(length)
        };
        let expect_continue =
            header_value("expect").is_some_and(|v| v.trim().eq_ignore_ascii_case("100-continue"));
        Ok(Some((head_len, body, expect_continue)))
    }
}

#[derive(Default)]
struct UpgradeDetector {
    headers: Vec<u8>,
    body_remaining: usize,
    opaque: bool,
}

impl UpgradeDetector {
    fn observe(&mut self, mut bytes: &[u8]) -> bool {
        while !bytes.is_empty() && !self.opaque {
            if self.body_remaining > 0 {
                let consumed = self.body_remaining.min(bytes.len());
                self.body_remaining -= consumed;
                bytes = &bytes[consumed..];
                continue;
            }
            // Bound header storage and never classify a response body as a
            // handshake.
            self.headers.push(bytes[0]);
            bytes = &bytes[1..];
            let prefix_len = self.headers.len().min(5);
            if self.headers[..prefix_len] != b"HTTP/"[..prefix_len] {
                self.opaque = true;
                self.headers.clear();
                return false;
            }
            if self.headers.ends_with(b"\r\n\r\n") {
                if kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::is_websocket_upgrade(&self.headers) {
                    self.headers.clear();
                    return true;
                }
                let mut headers = [httparse::EMPTY_HEADER; 64];
                let mut response = httparse::Response::new(&mut headers);
                if matches!(
                    response.parse(&self.headers),
                    Ok(httparse::Status::Complete(_))
                ) {
                    let no_body = matches!(response.code, Some(100..=199) | Some(204) | Some(304));
                    let length = response
                        .headers
                        .iter()
                        .find(|h| h.name.eq_ignore_ascii_case("content-length"))
                        .and_then(|h| std::str::from_utf8(h.value).ok())
                        .and_then(|v| v.trim().parse::<usize>().ok());
                    let transfer_encoded = response
                        .headers
                        .iter()
                        .any(|h| h.name.eq_ignore_ascii_case("transfer-encoding"));
                    // Unknown/streamed framing stays opaque; it must not create
                    // a false upgrade.
                    self.opaque = response.code == Some(101)
                        || transfer_encoded
                        || (!no_body && length.is_none());
                    self.body_remaining = if no_body { 0 } else { length.unwrap_or(0) };
                } else {
                    self.opaque = true;
                }
                self.headers.clear();
            } else if self.headers.len() == BUFFER_SIZE {
                self.opaque = true;
                self.headers.clear();
            }
        }
        false
    }
}

#[derive(Clone)]
pub struct TcpForwarder {
    config_id: i64,
    workload_type: String,
    logger: Option<Logger>,
}

impl TcpForwarder {
    pub fn new(config_id: i64, workload_type: String) -> Self {
        Self {
            config_id,
            workload_type,
            logger: None,
        }
    }

    pub fn is_logging_enabled(&self) -> bool {
        self.logger.is_some()
    }

    pub fn apply_socket_optimizations(stream: &tokio::net::TcpStream) {
        use socket2::SockRef;

        if let Err(e) = stream.set_nodelay(true) {
            tracing::debug!("Failed to set TCP_NODELAY: {}", e);
        }

        let sock_ref = SockRef::from(stream);

        if let Err(e) = sock_ref.set_recv_buffer_size(BUFFER_SIZE) {
            tracing::debug!("Failed to set receive buffer size: {}", e);
        }

        if let Err(e) = sock_ref.set_send_buffer_size(BUFFER_SIZE) {
            tracing::debug!("Failed to set send buffer size: {}", e);
        }

        // Enable TCP keep-alive for early detection of broken connections
        let keepalive = socket2::TcpKeepalive::new()
            .with_time(Duration::from_secs(60))
            .with_interval(Duration::from_secs(10));

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let keepalive = keepalive.with_retries(3);

        if let Err(e) = sock_ref.set_tcp_keepalive(&keepalive) {
            tracing::debug!("Failed to set TCP keepalive: {}", e);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn forward_streams(
        &mut self, client_stream: tokio::net::TcpStream,
        upstream_stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        client_address: std::net::SocketAddr, cancellation_token: CancellationToken,
        http_log_watcher: Arc<HttpLogStateWatcher>, local_port: u16,
    ) -> anyhow::Result<()> {
        Self::apply_socket_optimizations(&client_stream);

        let _log_subscriber = http_log_watcher.create_filtered_subscriber(self.config_id);
        let current_logging_enabled = http_log_watcher.get_http_logs(self.config_id).await;

        if current_logging_enabled
            && self.logger.is_none()
            && let Err(e) = self.initialize_logger(local_port).await
        {
            error!("Failed to initialize logger for {}: {}", client_address, e);
        }

        self.forward_connection(
            Arc::new(Mutex::new(client_stream)),
            upstream_stream,
            cancellation_token,
            http_log_watcher,
            local_port,
        )
        .await
    }

    pub async fn initialize_logger(&mut self, local_port: u16) -> anyhow::Result<()> {
        if self.workload_type != "service" && self.workload_type != "pod" {
            return Ok(());
        }

        let http_logs_config =
            kftray_commons::utils::http_logs_config::get_http_logs_config(self.config_id)
                .await
                .ok()
                .filter(|config| config.enabled);

        if let Some(http_logs_config) = http_logs_config {
            if self.logger.is_none() {
                debug!(
                    "Initializing HTTP logger for config_id {} on port {}",
                    self.config_id, local_port
                );
                let logger =
                    kftray_http_logs::HttpLogger::for_config(&http_logs_config, local_port).await?;
                self.logger = Some(logger);
            }
        } else if self.logger.is_some() {
            debug!(
                "HTTP logging disabled for config_id {}, clearing logger",
                self.config_id
            );
            self.logger = None;
        }

        Ok(())
    }

    pub async fn forward_connection(
        &mut self, client_conn: Arc<Mutex<TcpStream>>,
        upstream_conn: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        cancellation_token: CancellationToken, http_log_watcher: Arc<HttpLogStateWatcher>,
        local_port: u16,
    ) -> anyhow::Result<()> {
        let log_subscriber = http_log_watcher.create_filtered_subscriber(self.config_id);
        let current_logging_enabled = http_log_watcher.get_http_logs(self.config_id).await;

        if current_logging_enabled && self.logger.is_none() {
            let _ = self.initialize_logger(local_port).await;
        }
        let config_id = self.config_id;
        let shared_logger: Arc<Mutex<Option<Arc<crate::Logger>>>> =
            Arc::new(Mutex::new(self.logger.take().map(Arc::new)));

        let request_id = Arc::new(Mutex::new(None));
        let websocket_tunnel_mode = Arc::new(AtomicBool::new(false));
        let mut client_conn_guard = client_conn.lock().await;
        let (mut client_reader, mut client_writer) = tokio::io::split(&mut *client_conn_guard);
        let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

        let client_to_upstream = Self::forward_client_to_upstream(
            Arc::clone(&shared_logger),
            config_id,
            &mut client_reader,
            &mut upstream_writer,
            Arc::clone(&request_id),
            cancellation_token.clone(),
            log_subscriber.resubscribe(),
            local_port,
            Arc::clone(&websocket_tunnel_mode),
        );

        let upstream_to_client = Self::forward_upstream_to_client(
            Arc::clone(&shared_logger),
            config_id,
            &mut upstream_reader,
            &mut client_writer,
            Arc::clone(&request_id),
            cancellation_token.clone(),
            log_subscriber,
            local_port,
            Arc::clone(&websocket_tunnel_mode),
        );

        let result = tokio::try_join!(client_to_upstream, upstream_to_client);

        self.logger = Arc::try_unwrap(shared_logger)
            .ok()
            .and_then(|mutex| mutex.into_inner())
            .and_then(|arc_logger| Arc::try_unwrap(arc_logger).ok());

        match result {
            Ok(_) => {
                debug!("HTTP-aware connection closed normally");
            }
            Err(e) => {
                error!("HTTP-aware connection closed with error: {}", e);
                return Err(e);
            }
        }

        Ok(())
    }

    pub async fn forward_tls_streams(
        &self, mut client: tokio_rustls::server::TlsStream<TcpStream>,
        mut upstream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<()> {
        // Apply socket optimizations to the underlying TCP stream
        Self::apply_socket_optimizations(client.get_ref().0);

        let copy_fut = tokio::io::copy_bidirectional(&mut client, &mut upstream);
        tokio::select! {
            res = copy_fut => match res {
                Ok((to_upstream, to_client)) => {
                    debug!(
                        "TLS connection closed: sent {} bytes, received {} bytes",
                        to_upstream, to_client
                    );
                    Ok(())
                }
                Err(e) => {
                    error!("TLS connection closed with error: {}", e);
                    Err(e.into())
                }
            },
            _ = cancellation_token.cancelled() => {
                debug!("TLS connection canceled");
                Err(anyhow::anyhow!("TLS connection canceled"))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn forward_client_to_upstream<'a>(
        logger: Arc<Mutex<Option<Arc<crate::Logger>>>>, config_id: i64,
        client_reader: &'a mut (impl AsyncReadExt + Unpin),
        upstream_writer: &'a mut (impl AsyncWriteExt + Unpin),
        request_id: Arc<Mutex<Option<String>>>, cancellation_token: CancellationToken,
        mut log_subscriber: tokio::sync::broadcast::Receiver<
            crate::kube::http_log_watcher::HttpLogStateEvent,
        >,
        local_port: u16, websocket_tunnel_mode: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut should_log = {
            let logger_guard = logger.lock().await;
            logger_guard.is_some()
        };
        let mut request_buffer = should_log.then(RequestLogBuffer::default);

        loop {
            tokio::select! {
                result = client_reader.read(&mut buffer) => {
                    let n = match result {
                        Ok(n) => n,
                        Err(e) => return Err(e.into()),
                    };

                    let logging_requests = should_log && !websocket_tunnel_mode.load(Ordering::SeqCst);

                    if n == 0 {
                        if logging_requests
                            && let Some(started) = request_buffer.as_mut().and_then(RequestLogBuffer::take_started) {
                                Self::log_requests(vec![started], &logger, &request_id).await;
                            }
                        break;
                    }

                    if logging_requests && let Some(req_buf) = request_buffer.as_mut() {
                        Self::log_requests(req_buf.observe(&buffer[..n]), &logger, &request_id).await;
                    }

                    tokio::select! {
                        result = upstream_writer.write_all(&buffer[..n]) => result?,
                        _ = cancellation_token.cancelled() => break,
                    }
                },
                log_event = log_subscriber.recv() => {
                    if websocket_tunnel_mode.load(Ordering::SeqCst) {
                        continue;
                    }
                    if let Ok(event) = log_event
                        && event.config_id == config_id {
                            let needs_logger = event.enabled && {
                                let guard = logger.lock().await;
                                guard.is_none()
                            };

                            let new_should_log = if needs_logger {
                                let http_logs_config = kftray_commons::utils::http_logs_config::get_http_logs_config(config_id).await.ok().filter(|config| config.enabled);

                                if let Some(http_logs_config) = http_logs_config {
                                    match kftray_http_logs::HttpLogger::for_config(&http_logs_config, local_port).await {
                                        Ok(new_logger) => {
                                            let mut guard = logger.lock().await;
                                            if guard.is_none() {
                                                *guard = Some(Arc::new(new_logger));
                                            }
                                            true
                                        }
                                        _ => false,
                                    }
                                } else {
                                    false
                                }
                            } else if event.enabled {
                                let guard = logger.lock().await;
                                guard.is_some()
                            } else {
                                false
                            };

                            if new_should_log != should_log {
                                should_log = new_should_log;
                                if should_log {
                                    request_buffer = Some(RequestLogBuffer::default());
                                    debug!("Enabled HTTP logging for client-to-upstream");
                                } else {
                                    request_buffer = None;
                                    debug!("Disabled HTTP logging for client-to-upstream");
                                }
                            }
                    }
                },
                _ = cancellation_token.cancelled() => break,
            }
        }

        let _ = upstream_writer.shutdown().await;

        Ok(())
    }

    async fn log_requests(
        requests: Vec<Vec<u8>>, logger: &Arc<Mutex<Option<Arc<Logger>>>>,
        request_id: &Arc<Mutex<Option<String>>>,
    ) {
        if requests.is_empty() {
            return;
        }
        let maybe_logger = logger.lock().await.clone();
        if let Some(log) = maybe_logger {
            for request in requests {
                let new_request_id = log.log_request(request.into()).await;
                *request_id.lock().await = Some(new_request_id);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn forward_upstream_to_client<'a>(
        logger: Arc<Mutex<Option<Arc<crate::Logger>>>>, config_id: i64,
        upstream_reader: &'a mut (impl AsyncReadExt + Unpin),
        client_writer: &'a mut (impl AsyncWriteExt + Unpin),
        request_id: Arc<Mutex<Option<String>>>, cancellation_token: CancellationToken,
        mut log_subscriber: tokio::sync::broadcast::Receiver<
            crate::kube::http_log_watcher::HttpLogStateEvent,
        >,
        local_port: u16, websocket_tunnel_mode: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut should_log = {
            let logger_guard = logger.lock().await;
            logger_guard.is_some()
        };

        let mut response_state = if should_log {
            Some(ResponseState {
                buffer: Vec::new(),
                is_chunked: false,
                found_end_marker: false,
                total_chunks_received: 0,
                current_response_id: None,
                current_response_logged: false,
                first_chunk_time: None,
                force_log_time: None,
            })
        } else {
            None
        };
        let mut upgrade_detector = UpgradeDetector::default();

        loop {
            let log_deadline = response_state
                .as_ref()
                .and_then(ResponseState::pending_log_deadline);
            tokio::select! {
                result = upstream_reader.read(&mut buffer) => {
                    let n = match result {
                        Ok(n) => n,
                        Err(e) => return Err(e.into()),
                    };

                    if n == 0 {
                        if should_log
                            && !websocket_tunnel_mode.load(Ordering::SeqCst)
                            && let Some(ref mut state) = response_state.as_mut() {
                                let maybe_logger = {
                                    let guard = logger.lock().await;
                                    guard.clone()
                                };
                                if let Some(log) = maybe_logger
                                    && !state.buffer.is_empty() && !state.current_response_logged {
                                        let req_id_guard = request_id.lock().await;
                                        if let Some(req_id) = &*req_id_guard {
                                            log.log_response(state.buffer.clone().into(), req_id.clone()).await;
                                        }
                                    }
                        }
                        break;
                    }

                    if websocket_tunnel_mode.load(Ordering::SeqCst) {
                        tokio::select! {
                            result = client_writer.write_all(&buffer[..n]) => result?,
                            _ = cancellation_token.cancelled() => break,
                        }
                        continue;
                    }

                    let detected_upgrade = upgrade_detector.observe(&buffer[..n]);
                    let logged_upgrade = if should_log
                        && let Some(ref mut state) = response_state.as_mut() {
                            Self::handle_response_logging_static(&buffer[..n], state, &logger, &request_id).await
                        } else {
                            false
                        };

                    if logged_upgrade || detected_upgrade {
                        websocket_tunnel_mode.store(true, Ordering::SeqCst);
                        response_state = None;
                        should_log = false;
                    }

                    tokio::select! {
                        result = client_writer.write_all(&buffer[..n]) => result?,
                        _ = cancellation_token.cancelled() => break,
                    }
                },
                log_event = log_subscriber.recv() => {
                    if websocket_tunnel_mode.load(Ordering::SeqCst) {
                        continue;
                    }
                    if let Ok(event) = log_event
                        && event.config_id == config_id {
                            let needs_logger = event.enabled && {
                                let guard = logger.lock().await;
                                guard.is_none()
                            };

                            let new_should_log = if needs_logger {
                                let http_logs_config = kftray_commons::utils::http_logs_config::get_http_logs_config(config_id).await.ok().filter(|config| config.enabled);

                                if let Some(http_logs_config) = http_logs_config {
                                    match kftray_http_logs::HttpLogger::for_config(&http_logs_config, local_port).await {
                                        Ok(new_logger) => {
                                            let mut guard = logger.lock().await;
                                            if guard.is_none() {
                                                *guard = Some(Arc::new(new_logger));
                                            }
                                            true
                                        }
                                        _ => false,
                                    }
                                } else {
                                    false
                                }
                            } else if event.enabled {
                                let guard = logger.lock().await;
                                guard.is_some()
                            } else {
                                false
                            };

                            if new_should_log != should_log {
                                should_log = new_should_log;
                                if should_log {
                                    response_state = Some(ResponseState {
                                        buffer: Vec::new(),
                                        is_chunked: false,
                                        found_end_marker: false,
                                        total_chunks_received: 0,
                                        current_response_id: None,
                                        current_response_logged: false,
                                        first_chunk_time: None,
                                        force_log_time: None,
                                    });
                                    debug!("Enabled HTTP logging for upstream-to-client");
                                } else {
                                    response_state = None;
                                    debug!("Disabled HTTP logging for upstream-to-client");
                                }
                            }
                    }
                },
                _ = tokio::time::sleep_until(log_deadline.unwrap_or_else(tokio::time::Instant::now)), if log_deadline.is_some() => {
                    if let Some(state) = response_state.as_mut() {
                        Self::log_response_if_ready(state, &logger).await;
                    }
                },
                _ = cancellation_token.cancelled() => break,
            }
        }

        let _ = client_writer.shutdown().await;

        Ok(())
    }

    async fn handle_response_logging_static(
        buffer: &[u8], state: &mut ResponseState, logger: &Arc<Mutex<Option<Arc<Logger>>>>,
        request_id: &Arc<Mutex<Option<String>>>,
    ) -> bool {
        let req_id_guard = request_id.lock().await;
        let current_req_id = req_id_guard.clone();
        drop(req_id_guard);

        let is_new_response = match (&state.current_response_id, &current_req_id) {
            (Some(current_id), Some(req_id)) => current_id != req_id,
            (None, Some(_)) => true,
            _ => false,
        };

        if is_new_response {
            if !state.current_response_logged
                && !state.buffer.is_empty()
                && let Some(previous_id) = state.current_response_id.take()
            {
                let maybe_logger = {
                    let guard = logger.lock().await;
                    guard.clone()
                };
                if let Some(log) = maybe_logger {
                    log.log_response(std::mem::take(&mut state.buffer).into(), previous_id)
                        .await;
                }
            }
            state.reset_for_new_response(current_req_id);
        }

        let scanned_len = state.buffer.len();
        state.buffer.extend_from_slice(buffer);
        let dropped = state.drop_interim_responses();
        let scanned_len = scanned_len.saturating_sub(dropped);

        if dropped > 0 {
            state.first_chunk_time = None;
        }
        if state.first_chunk_time.is_none() && !state.buffer.is_empty() {
            state.first_chunk_time = Some(tokio::time::Instant::now());
        }

        if !state.is_chunked && !state.found_end_marker {
            state.is_chunked = kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::detect_chunked_encoding(&state.buffer);
        }

        kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::process_chunk(
            &state.buffer,
            scanned_len,
            state.is_chunked,
            &mut state.found_end_marker,
            &mut state.total_chunks_received,
        );
        let is_websocket_upgrade =
            kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::is_websocket_upgrade(
                &state.buffer,
            );
        if is_websocket_upgrade {
            // A read may contain both the handshake and the first WebSocket
            // frame. Log only the complete HTTP headers; forwarding
            // still uses the original read.
            if let Some(headers_end) = state.buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                state.buffer.truncate(headers_end + 4);
            }
        }

        Self::log_response_if_ready(state, logger).await;

        // The caller switches to raw forwarding only after the handshake is
        // logged.
        is_websocket_upgrade
    }

    async fn log_response_if_ready(
        state: &mut ResponseState, logger: &Arc<Mutex<Option<Arc<Logger>>>>,
    ) {
        if state.current_response_logged {
            return;
        }
        let maybe_logger = {
            let guard = logger.lock().await;
            guard.clone()
        };
        if let Some(log) = maybe_logger {
            let should_log = Self::should_log_response_static(state);
            if should_log && let Some(response_id) = state.current_response_id.clone() {
                state.current_response_logged = true;
                log.log_response(state.buffer.clone().into(), response_id)
                    .await;

                if Self::can_clear_response_buffer_static(state) {
                    state.reset_for_next_response();
                }
            }
        }
    }

    fn should_log_response_static(state: &mut ResponseState) -> bool {
        if state.is_chunked {
            if state.found_end_marker && state.force_log_time.is_none() {
                let delay = if !state.buffer.is_empty()
                    && state.buffer.len() < 40_000
                    && state.total_chunks_received < 20
                {
                    tokio::time::Instant::now() + tokio::time::Duration::from_millis(10)
                } else {
                    tokio::time::Instant::now()
                };
                state.force_log_time = Some(delay);
                return delay <= tokio::time::Instant::now();
            }
            if let Some(force_time) = state.force_log_time {
                return tokio::time::Instant::now() >= force_time;
            } else if let Some(first_time) = state.first_chunk_time {
                return first_time.elapsed().as_secs() > 5;
            }
        } else {
            return kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::check_content_length_match(&state.buffer)
                || kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::has_bodyless_status(&state.buffer)
                || kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::is_websocket_upgrade(&state.buffer);
        }
        false
    }

    fn can_clear_response_buffer_static(state: &ResponseState) -> bool {
        if state.is_chunked {
            state.found_end_marker
        } else {
            kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::check_content_length_match(&state.buffer)
                || kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::has_bodyless_status(&state.buffer)
        }
    }
}

struct ResponseState {
    buffer: Vec<u8>,
    is_chunked: bool,
    found_end_marker: bool,
    total_chunks_received: usize,
    current_response_id: Option<String>,
    current_response_logged: bool,
    first_chunk_time: Option<tokio::time::Instant>,
    force_log_time: Option<tokio::time::Instant>,
}

impl ResponseState {
    fn reset_for_new_response(&mut self, request_id: Option<String>) {
        self.buffer.clear();
        self.is_chunked = false;
        self.found_end_marker = false;
        self.total_chunks_received = 0;
        self.current_response_logged = false;
        self.current_response_id = request_id;
        self.first_chunk_time = Some(tokio::time::Instant::now());
        self.force_log_time = None;
    }

    fn pending_log_deadline(&self) -> Option<tokio::time::Instant> {
        if self.current_response_logged || self.current_response_id.is_none() {
            None
        } else {
            self.force_log_time
        }
    }

    fn reset_for_next_response(&mut self) {
        self.buffer.clear();
        self.is_chunked = false;
        self.found_end_marker = false;
        self.total_chunks_received = 0;
        self.first_chunk_time = None;
        self.force_log_time = None;
    }

    fn drop_interim_responses(&mut self) -> usize {
        let mut dropped = 0;
        loop {
            let mut headers = [httparse::EMPTY_HEADER; 64];
            let mut response = httparse::Response::new(&mut headers);
            let head_len = match response.parse(&self.buffer) {
                Ok(httparse::Status::Complete(head_len))
                    if matches!(response.code, Some(100 | 102..=199)) =>
                {
                    head_len
                }
                _ => return dropped,
            };
            self.buffer.drain(..head_len);
            dropped += head_len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEBSOCKET_HANDSHAKE: &[u8] = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: test-accept\r\n\r\n";

    async fn check_websocket_response_forwarding(
        response: &[u8], split_at: usize, logging_enabled: bool, expect_tunnel: bool,
    ) {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("websocket.log");
        let http_logger = Arc::new(
            Logger::new(
                kftray_http_logs::LogConfig::new(temp_dir.path().to_path_buf()),
                log_path.clone(),
            )
            .await
            .unwrap(),
        );
        let req_id = http_logger
            .log_request(bytes::Bytes::from_static(
                b"GET /websocket HTTP/1.1\r\nHost: example.test\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            ))
            .await;
        let logger = Arc::new(Mutex::new(logging_enabled.then(|| http_logger.clone())));
        let request_id = Arc::new(Mutex::new(Some(req_id)));
        let tunnel = Arc::new(AtomicBool::new(false));
        let forward_tunnel = tunnel.clone();
        let (mut upstream_reader, mut upstream_writer) = tokio::io::duplex(4096);
        let (mut client_writer, mut client_reader) = tokio::io::duplex(4096);
        let (events, subscriber) = tokio::sync::broadcast::channel(8);
        let forward = tokio::spawn(async move {
            TcpForwarder::forward_upstream_to_client(
                logger,
                1,
                &mut upstream_reader,
                &mut client_writer,
                request_id,
                CancellationToken::new(),
                subscriber,
                8080,
                forward_tunnel,
            )
            .await
            .unwrap();
        });

        tokio::time::timeout(Duration::from_secs(5), async {
            upstream_writer
                .write_all(&response[..split_at])
                .await
                .unwrap();
            let mut first = vec![0; split_at];
            client_reader.read_exact(&mut first).await.unwrap();
            assert_eq!(first, response[..split_at]);
            assert!(
                !tunnel.load(Ordering::SeqCst),
                "incomplete headers must not switch modes"
            );

            let frame = b"\x81\x0dframe-payload";
            let mut last = response[split_at..].to_vec();
            if expect_tunnel || !logging_enabled {
                last.extend_from_slice(frame);
            }
            upstream_writer.write_all(&last).await.unwrap();
            let mut received = vec![0; last.len()];
            client_reader.read_exact(&mut received).await.unwrap();
            assert_eq!(received, last, "coalesced frame bytes must be preserved");
            assert_eq!(tunnel.load(Ordering::SeqCst), expect_tunnel);

            if expect_tunnel || !logging_enabled {
                events
                    .send(crate::kube::http_log_watcher::HttpLogStateEvent::new(
                        1, true,
                    ))
                    .unwrap();
                tokio::task::yield_now().await;
                upstream_writer.write_all(frame).await.unwrap();
                let mut received = vec![0; frame.len()];
                client_reader.read_exact(&mut received).await.unwrap();
                assert_eq!(received, frame, "later frame bytes must be preserved");
            }
            upstream_writer.shutdown().await.unwrap();
            forward.await.unwrap();
        })
        .await
        .expect("forwarding should not stall");

        let status = std::str::from_utf8(response)
            .unwrap()
            .lines()
            .next()
            .unwrap();
        if logging_enabled {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    http_logger.flush().await.unwrap();
                    let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
                    if contents.contains(status) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("the HTTP response must be logged before leaving HTTP mode");
        }
        http_logger.shutdown().await;
        let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
        assert_eq!(
            contents.matches(status).count(),
            usize::from(logging_enabled)
        );
        assert!(
            !contents.contains("frame-payload"),
            "WebSocket frames are not HTTP log entries"
        );
    }

    async fn assert_chunked_logging_waits_for_completion(fragments: &[&[u8]]) {
        let mut state = ResponseState {
            buffer: Vec::new(),
            is_chunked: false,
            found_end_marker: false,
            total_chunks_received: 0,
            current_response_id: None,
            current_response_logged: false,
            first_chunk_time: None,
            force_log_time: None,
        };
        let logger = Arc::new(Mutex::new(None));
        let request_id = Arc::new(Mutex::new(Some("chunked-request".to_string())));

        for fragment in &fragments[..fragments.len() - 1] {
            TcpForwarder::handle_response_logging_static(
                fragment,
                &mut state,
                &logger,
                &request_id,
            )
            .await;
            tokio::time::advance(Duration::from_millis(20)).await;
            assert!(
                !state.found_end_marker,
                "incomplete response must stay open"
            );
            assert!(!TcpForwarder::should_log_response_static(&mut state));
        }

        TcpForwarder::handle_response_logging_static(
            fragments.last().unwrap(),
            &mut state,
            &logger,
            &request_id,
        )
        .await;
        assert!(
            state.is_chunked,
            "split headers must retain chunked framing"
        );
        assert!(
            state.found_end_marker,
            "split end marker must be recognized"
        );
        assert!(!TcpForwarder::should_log_response_static(&mut state));
        tokio::time::advance(Duration::from_millis(20)).await;
        assert!(TcpForwarder::should_log_response_static(&mut state));
    }

    #[tokio::test(start_paused = true)]
    async fn chunked_logging_waits_for_body_after_split_headers() {
        assert_chunked_logging_waits_for_completion(&[
            b"HTTP/",
            b"1.1 200 OK\r\nTransfer-Encoding: chunked\r\nExpires: 0\r\n",
            b"\r\n",
            b"a\r\nfirst-part\r\n",
            b"9\r\nlast-part\r\n0\r\n\r\n",
        ])
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn chunked_logging_recognizes_split_marker_after_http_body_fragment() {
        assert_chunked_logging_waits_for_completion(&[
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nExpires: 0\r\n\r\ne\r\n",
            b"HTTP/last-part\r\n0\r",
            b"\n\r\n",
        ])
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn chunked_response_after_a_slow_continue_waits_for_its_last_chunk() {
        let mut state = ResponseState {
            buffer: Vec::new(),
            is_chunked: false,
            found_end_marker: false,
            total_chunks_received: 0,
            current_response_id: None,
            current_response_logged: false,
            first_chunk_time: None,
            force_log_time: None,
        };
        let logger = Arc::new(Mutex::new(None));
        let request_id = Arc::new(Mutex::new(Some("upload".to_string())));

        TcpForwarder::handle_response_logging_static(
            b"HTTP/1.1 100 Continue\r\n\r\n",
            &mut state,
            &logger,
            &request_id,
        )
        .await;
        tokio::time::advance(Duration::from_secs(6)).await;
        TcpForwarder::handle_response_logging_static(
            b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nfirst\r\n",
            &mut state,
            &logger,
            &request_id,
        )
        .await;

        assert!(
            !TcpForwarder::should_log_response_static(&mut state),
            "final response logged before its last chunk"
        );
    }

    #[tokio::test]
    async fn chunked_response_split_across_reads_is_logged_complete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("chunked.log");
        let http_logger = Arc::new(
            Logger::new(
                kftray_http_logs::LogConfig::new(temp_dir.path().to_path_buf()),
                log_path.clone(),
            )
            .await
            .unwrap(),
        );
        let req_id = http_logger
            .log_request(bytes::Bytes::from_static(
                b"GET /chunked HTTP/1.1\r\nHost: example.test\r\n\r\n",
            ))
            .await;
        let logger = Arc::new(Mutex::new(Some(http_logger.clone())));
        let request_id = Arc::new(Mutex::new(Some(req_id)));
        let tunnel = Arc::new(AtomicBool::new(false));
        let (mut upstream_reader, mut upstream_writer) = tokio::io::duplex(4096);
        let (mut client_writer, mut client_reader) = tokio::io::duplex(4096);
        let (_events, subscriber) = tokio::sync::broadcast::channel(8);
        let forward = tokio::spawn(async move {
            TcpForwarder::forward_upstream_to_client(
                logger,
                1,
                &mut upstream_reader,
                &mut client_writer,
                request_id,
                CancellationToken::new(),
                subscriber,
                8080,
                tunnel,
            )
            .await
            .unwrap();
        });

        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nCache-Control: public, max-age=3600\r\nExpires: 0\r\n\r\na\r\nfirst-part\r\n9\r\nlast-part\r\n0\r\n\r\n";
        let mut prev = 0usize;
        for split in [23usize, 95, 102, 118] {
            let part = &response[prev..split];
            upstream_writer.write_all(part).await.unwrap();
            let mut received = vec![0; part.len()];
            client_reader.read_exact(&mut received).await.unwrap();
            assert_eq!(received, part);
            prev = split;
            http_logger.flush().await.unwrap();
            let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
            assert!(
                !contents.contains("first-part"),
                "incomplete chunked response must not be logged yet"
            );
        }

        let tail = &response[prev..];
        upstream_writer.write_all(tail).await.unwrap();
        let mut received = vec![0; tail.len()];
        client_reader.read_exact(&mut received).await.unwrap();
        assert_eq!(received, tail);

        upstream_writer.shutdown().await.unwrap();
        forward.await.unwrap();

        let contents = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                http_logger.flush().await.unwrap();
                let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
                if contents.contains("last-part") {
                    break contents;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("complete chunked response must be logged");
        assert!(
            contents.contains("first-part") && contents.contains("last-part"),
            "logged body must cover every chunk: {contents}"
        );
        http_logger.shutdown().await;
    }

    #[tokio::test]
    async fn chunked_response_on_an_idle_keep_alive_connection_is_logged() {
        let (_temp_dir, log_path, http_logger) = temp_http_logger("idle-chunked.log").await;
        let req_id = http_logger
            .log_request(bytes::Bytes::from_static(
                b"GET /idle HTTP/1.1\r\nHost: example.test\r\n\r\n",
            ))
            .await;
        let logger = Arc::new(Mutex::new(Some(http_logger.clone())));
        let request_id = Arc::new(Mutex::new(Some(req_id)));
        let tunnel = Arc::new(AtomicBool::new(false));
        let (mut upstream_reader, mut upstream_writer) = tokio::io::duplex(4096);
        let (mut client_writer, mut client_reader) = tokio::io::duplex(4096);
        let (_events, subscriber) = tokio::sync::broadcast::channel(8);
        let forward = tokio::spawn(async move {
            TcpForwarder::forward_upstream_to_client(
                logger,
                1,
                &mut upstream_reader,
                &mut client_writer,
                request_id,
                CancellationToken::new(),
                subscriber,
                8080,
                tunnel,
            )
            .await
            .unwrap();
        });

        pass_through(
            &mut upstream_writer,
            &mut client_reader,
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        )
        .await;

        let contents = read_log_until(&http_logger, &log_path, "# Response at").await;
        assert!(
            contents.contains("hello"),
            "logged response must carry its body: {contents}"
        );

        upstream_writer.shutdown().await.unwrap();
        forward.await.unwrap();
        http_logger.shutdown().await;
    }

    #[tokio::test]
    async fn content_length_response_split_before_last_bytes_is_logged_complete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("content-length.log");
        let http_logger = Arc::new(
            Logger::new(
                kftray_http_logs::LogConfig::new(temp_dir.path().to_path_buf()),
                log_path.clone(),
            )
            .await
            .unwrap(),
        );
        let req_id = http_logger
            .log_request(bytes::Bytes::from_static(
                b"GET /content-length HTTP/1.1\r\nHost: example.test\r\n\r\n",
            ))
            .await;
        let logger = Arc::new(Mutex::new(Some(http_logger.clone())));
        let request_id = Arc::new(Mutex::new(Some(req_id)));
        let tunnel = Arc::new(AtomicBool::new(false));
        let (mut upstream_reader, mut upstream_writer) = tokio::io::duplex(4096);
        let (mut client_writer, mut client_reader) = tokio::io::duplex(4096);
        let (_events, subscriber) = tokio::sync::broadcast::channel(8);
        let forward = tokio::spawn(async move {
            TcpForwarder::forward_upstream_to_client(
                logger,
                1,
                &mut upstream_reader,
                &mut client_writer,
                request_id,
                CancellationToken::new(),
                subscriber,
                8080,
                tunnel,
            )
            .await
            .unwrap();
        });

        for part in [
            &b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 13\r\n\r\ncomplete-"
                [..],
            &b"TAIL"[..],
        ] {
            upstream_writer.write_all(part).await.unwrap();
            let mut received = vec![0; part.len()];
            client_reader.read_exact(&mut received).await.unwrap();
            assert_eq!(received, part);
        }
        upstream_writer.shutdown().await.unwrap();
        forward.await.unwrap();

        let contents = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                http_logger.flush().await.unwrap();
                let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
                if contents.contains("complete-") {
                    break contents;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("content-length response must be logged");
        assert!(
            contents.contains("complete-TAIL"),
            "logged body lost its last bytes: {contents}"
        );
        http_logger.shutdown().await;
    }

    async fn send_client_reads(
        logger: Arc<Mutex<Option<Arc<Logger>>>>, request_id: Arc<Mutex<Option<String>>>,
        reads: &[&[u8]],
    ) {
        let (mut client_reader, mut client_writer) = tokio::io::duplex(4096);
        let (mut upstream_writer, mut upstream_reader) = tokio::io::duplex(4096);
        let (_events, subscriber) = tokio::sync::broadcast::channel(8);
        let forward = tokio::spawn(async move {
            TcpForwarder::forward_client_to_upstream(
                logger,
                1,
                &mut client_reader,
                &mut upstream_writer,
                request_id,
                CancellationToken::new(),
                subscriber,
                8080,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        });
        for read in reads {
            client_writer.write_all(read).await.unwrap();
            let mut received = vec![0; read.len()];
            upstream_reader.read_exact(&mut received).await.unwrap();
            assert_eq!(&received, read);
        }
        drop(client_writer);
        forward.await.unwrap();
    }

    async fn read_log_until(
        http_logger: &Logger, log_path: &std::path::Path, needle: &str,
    ) -> String {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                http_logger.flush().await.unwrap();
                let contents = tokio::fs::read_to_string(log_path).await.unwrap();
                if contents.contains(needle) {
                    break contents;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{needle} never reached the log"))
    }

    fn trace_ids_after(contents: &str, marker: &str) -> Vec<String> {
        let lines: Vec<&str> = contents.lines().collect();
        lines
            .windows(2)
            .filter(|pair| pair[1].starts_with(marker))
            .filter_map(|pair| pair[0].strip_prefix("# Trace ID: "))
            .map(str::to_string)
            .collect()
    }

    #[tokio::test]
    async fn request_body_in_a_later_read_is_logged_with_its_response() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("split-request.log");
        let http_logger = Arc::new(
            Logger::new(
                kftray_http_logs::LogConfig::new(temp_dir.path().to_path_buf()),
                log_path.clone(),
            )
            .await
            .unwrap(),
        );
        let logger = Arc::new(Mutex::new(Some(http_logger.clone())));
        let request_id = Arc::new(Mutex::new(None));

        send_client_reads(
            Arc::clone(&logger),
            Arc::clone(&request_id),
            &[
                b"POST /upload HTTP/1.1\r\nHost: example.test\r\nContent-Type: application/json\r\nContent-Length: 17\r\n\r\n",
                b"{\"name\":\"widget\"}",
            ],
        )
        .await;

        let (mut upstream_reader, mut upstream_writer) = tokio::io::duplex(4096);
        let (mut client_writer, mut client_reader) = tokio::io::duplex(4096);
        let (_events, subscriber) = tokio::sync::broadcast::channel(8);
        let forward = tokio::spawn(async move {
            TcpForwarder::forward_upstream_to_client(
                logger,
                1,
                &mut upstream_reader,
                &mut client_writer,
                request_id,
                CancellationToken::new(),
                subscriber,
                8080,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        });
        let response = b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nok";
        upstream_writer.write_all(response).await.unwrap();
        let mut received = vec![0; response.len()];
        client_reader.read_exact(&mut received).await.unwrap();
        upstream_writer.shutdown().await.unwrap();
        forward.await.unwrap();

        let contents = read_log_until(&http_logger, &log_path, "201 Created").await;
        let requests = trace_ids_after(&contents, "# Request at");
        let responses = trace_ids_after(&contents, "# Response at");
        assert_eq!(requests.len(), 1, "one request entry expected: {contents}");
        assert_eq!(
            requests, responses,
            "response not paired with request: {contents}"
        );
        assert!(
            contents.contains("widget"),
            "request body missing: {contents}"
        );
        assert!(
            !contents.contains("<empty body>"),
            "body logged as empty: {contents}"
        );
        http_logger.shutdown().await;
    }

    #[tokio::test]
    async fn chunked_request_split_across_reads_and_pipelined_request_are_logged_apart() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("chunked-request.log");
        let http_logger = Arc::new(
            Logger::new(
                kftray_http_logs::LogConfig::new(temp_dir.path().to_path_buf()),
                log_path.clone(),
            )
            .await
            .unwrap(),
        );
        let logger = Arc::new(Mutex::new(Some(http_logger.clone())));
        let request_id = Arc::new(Mutex::new(None));

        send_client_reads(
            logger,
            Arc::clone(&request_id),
            &[
                b"POST /chunks HTTP/1.1\r\nHost: example.test\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nfirst-",
                b"\r\n",
                b"4\r\nlast\r\n0\r\n\r\nGET /next HTTP/1.1\r\nHost: example.test\r\n\r\n",
            ],
        )
        .await;

        let contents = read_log_until(&http_logger, &log_path, "GET /next").await;
        let requests = trace_ids_after(&contents, "# Request at");
        assert_eq!(
            requests.len(),
            2,
            "two request entries expected: {contents}"
        );
        assert!(
            contents.contains("first-") && contents.contains("last"),
            "chunked body incomplete: {contents}"
        );
        assert_eq!(request_id.lock().await.as_ref(), requests.last());
        http_logger.shutdown().await;
    }

    #[tokio::test]
    async fn request_cut_short_by_the_client_is_still_logged() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("cut-request.log");
        let http_logger = Arc::new(
            Logger::new(
                kftray_http_logs::LogConfig::new(temp_dir.path().to_path_buf()),
                log_path.clone(),
            )
            .await
            .unwrap(),
        );
        let logger = Arc::new(Mutex::new(Some(http_logger.clone())));

        send_client_reads(
            logger,
            Arc::new(Mutex::new(None)),
            &[b"PUT /aborted HTTP/1.1\r\nHost: example.test\r\nContent-Length: 100\r\n\r\npartial"],
        )
        .await;

        let contents = read_log_until(&http_logger, &log_path, "PUT /aborted").await;
        assert_eq!(trace_ids_after(&contents, "# Request at").len(), 1);
        http_logger.shutdown().await;
    }

    fn spawn_forwarders(
        logger: Arc<Mutex<Option<Arc<Logger>>>>,
    ) -> (
        tokio::io::DuplexStream,
        tokio::io::DuplexStream,
        tokio::task::JoinHandle<()>,
    ) {
        let (client, client_side) = tokio::io::duplex(4096);
        let (upstream, upstream_side) = tokio::io::duplex(4096);
        let forwarders = tokio::spawn(async move {
            let (_events, subscriber) = tokio::sync::broadcast::channel(8);
            let request_id = Arc::new(Mutex::new(None));
            let tunnel = Arc::new(AtomicBool::new(false));
            let (mut client_reader, mut client_writer) = tokio::io::split(client_side);
            let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_side);
            let (to_upstream, to_client) = tokio::join!(
                TcpForwarder::forward_client_to_upstream(
                    Arc::clone(&logger),
                    1,
                    &mut client_reader,
                    &mut upstream_writer,
                    Arc::clone(&request_id),
                    CancellationToken::new(),
                    subscriber.resubscribe(),
                    8080,
                    Arc::clone(&tunnel),
                ),
                TcpForwarder::forward_upstream_to_client(
                    logger,
                    1,
                    &mut upstream_reader,
                    &mut client_writer,
                    request_id,
                    CancellationToken::new(),
                    subscriber,
                    8080,
                    tunnel,
                ),
            );
            to_upstream.unwrap();
            to_client.unwrap();
        });
        (client, upstream, forwarders)
    }

    async fn pass_through(
        from: &mut tokio::io::DuplexStream, to: &mut tokio::io::DuplexStream, bytes: &[u8],
    ) {
        from.write_all(bytes).await.unwrap();
        let mut received = vec![0; bytes.len()];
        to.read_exact(&mut received).await.unwrap();
        assert_eq!(received, bytes);
    }

    async fn temp_http_logger(name: &str) -> (tempfile::TempDir, std::path::PathBuf, Arc<Logger>) {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join(name);
        let http_logger = Arc::new(
            Logger::new(
                kftray_http_logs::LogConfig::new(temp_dir.path().to_path_buf()),
                log_path.clone(),
            )
            .await
            .unwrap(),
        );
        (temp_dir, log_path, http_logger)
    }

    #[tokio::test]
    async fn early_final_response_to_expect_continue_request_shares_its_trace_id() {
        let (_temp_dir, log_path, http_logger) = temp_http_logger("early-response.log").await;
        let (mut client, mut upstream, forwarders) =
            spawn_forwarders(Arc::new(Mutex::new(Some(http_logger.clone()))));

        pass_through(
            &mut client,
            &mut upstream,
            b"GET /first HTTP/1.1\r\nHost: example.test\r\n\r\n",
        )
        .await;
        pass_through(
            &mut upstream,
            &mut client,
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst",
        )
        .await;
        pass_through(
            &mut client,
            &mut upstream,
            b"POST /upload HTTP/1.1\r\nHost: example.test\r\nExpect: 100-continue\r\nContent-Length: 1000\r\n\r\n",
        )
        .await;
        pass_through(
            &mut upstream,
            &mut client,
            b"HTTP/1.1 417 Expectation Failed\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
        drop(client);
        drop(upstream);
        forwarders.await.unwrap();

        let contents = read_log_until(&http_logger, &log_path, "417 Expectation Failed").await;
        let requests = trace_ids_after(&contents, "# Request at");
        let responses = trace_ids_after(&contents, "# Response at");
        assert_eq!(
            requests.len(),
            2,
            "two request entries expected: {contents}"
        );
        assert_eq!(
            requests, responses,
            "responses not paired with requests: {contents}"
        );
        http_logger.shutdown().await;
    }

    #[tokio::test]
    async fn expect_continue_request_logs_the_final_response_not_the_interim_one() {
        let (_temp_dir, log_path, http_logger) = temp_http_logger("continue.log").await;
        let (mut client, mut upstream, forwarders) =
            spawn_forwarders(Arc::new(Mutex::new(Some(http_logger.clone()))));

        pass_through(
            &mut client,
            &mut upstream,
            b"POST /upload HTTP/1.1\r\nHost: example.test\r\nExpect: 100-continue\r\nContent-Length: 17\r\n\r\n",
        )
        .await;
        pass_through(&mut upstream, &mut client, b"HTTP/1.1 100 Continue\r\n\r\n").await;
        pass_through(&mut client, &mut upstream, b"{\"name\":\"widget\"}").await;
        pass_through(
            &mut upstream,
            &mut client,
            b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nok",
        )
        .await;

        let contents = read_log_until(&http_logger, &log_path, "201 Created").await;
        drop(client);
        drop(upstream);
        forwarders.await.unwrap();

        let requests = trace_ids_after(&contents, "# Request at");
        let responses = trace_ids_after(&contents, "# Response at");
        assert_eq!(requests.len(), 1, "one request entry expected: {contents}");
        assert_eq!(
            requests, responses,
            "response not paired with request: {contents}"
        );
        assert!(
            !contents.contains("100 Continue"),
            "interim response logged: {contents}"
        );
        http_logger.shutdown().await;
    }

    async fn assert_reused_connection_logs_first_response(
        first_request: &'static [u8], first_response: &'static [u8], first_status: &str,
        logged_before_next_request: bool,
    ) {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("reused.log");
        let http_logger = Arc::new(
            Logger::new(
                kftray_http_logs::LogConfig::new(temp_dir.path().to_path_buf()),
                log_path.clone(),
            )
            .await
            .unwrap(),
        );
        let first_id = http_logger
            .log_request(bytes::Bytes::from_static(first_request))
            .await;
        let logger = Arc::new(Mutex::new(Some(http_logger.clone())));
        let request_id = Arc::new(Mutex::new(Some(first_id)));
        let tunnel = Arc::new(AtomicBool::new(false));
        let (mut upstream_reader, mut upstream_writer) = tokio::io::duplex(4096);
        let (mut client_writer, mut client_reader) = tokio::io::duplex(4096);
        let (_events, subscriber) = tokio::sync::broadcast::channel(8);
        let forward_request_id = Arc::clone(&request_id);
        let forward = tokio::spawn(async move {
            TcpForwarder::forward_upstream_to_client(
                logger,
                1,
                &mut upstream_reader,
                &mut client_writer,
                forward_request_id,
                CancellationToken::new(),
                subscriber,
                8080,
                tunnel,
            )
            .await
            .unwrap();
        });

        upstream_writer.write_all(first_response).await.unwrap();
        let mut received = vec![0; first_response.len()];
        client_reader.read_exact(&mut received).await.unwrap();
        assert_eq!(received, first_response);

        if logged_before_next_request {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    http_logger.flush().await.unwrap();
                    let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
                    if contents.lines().any(|line| line == first_status) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("bodyless response must be logged once its headers arrive");
        }

        let second_id = http_logger
            .log_request(bytes::Bytes::from_static(
                b"GET /second HTTP/1.1\r\nHost: example.test\r\n\r\n",
            ))
            .await;
        *request_id.lock().await = Some(second_id);

        let second_response = b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecond";
        upstream_writer.write_all(second_response).await.unwrap();
        let mut received = vec![0; second_response.len()];
        client_reader.read_exact(&mut received).await.unwrap();
        assert_eq!(received, second_response);
        upstream_writer.shutdown().await.unwrap();
        forward.await.unwrap();

        let contents = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                http_logger.flush().await.unwrap();
                let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
                if contents.lines().any(|line| line == "second") {
                    break contents;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("second response must be logged");
        let status_lines: Vec<&str> = contents
            .lines()
            .filter(|line| line.starts_with("HTTP/1.1 "))
            .collect();
        assert_eq!(
            status_lines.len(),
            2,
            "both responses must be logged: {contents}"
        );
        assert!(
            status_lines.contains(&first_status),
            "first response missing from the log: {contents}"
        );
        http_logger.shutdown().await;
    }

    #[tokio::test]
    async fn not_modified_response_on_reused_connection_is_logged() {
        assert_reused_connection_logs_first_response(
            b"GET /first HTTP/1.1\r\nHost: example.test\r\n\r\n",
            b"HTTP/1.1 304 Not Modified\r\nETag: \"abc\"\r\n\r\n",
            "HTTP/1.1 304 Not Modified",
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn no_content_response_on_reused_connection_is_logged() {
        assert_reused_connection_logs_first_response(
            b"GET /first HTTP/1.1\r\nHost: example.test\r\n\r\n",
            b"HTTP/1.1 204 No Content\r\n\r\n",
            "HTTP/1.1 204 No Content",
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn head_response_on_reused_connection_is_logged() {
        assert_reused_connection_logs_first_response(
            b"HEAD /first HTTP/1.1\r\nHost: example.test\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 1234\r\n\r\n",
            "HTTP/1.1 200 OK",
            false,
        )
        .await;
    }

    #[tokio::test]
    async fn test_websocket_handshake_logged_before_tunnel() {
        check_websocket_response_forwarding(WEBSOCKET_HANDSHAKE, 30, true, true).await;
    }

    #[tokio::test]
    async fn test_websocket_rejection_stays_in_http_mode() {
        check_websocket_response_forwarding(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n",
            15,
            true,
            false,
        )
        .await;
    }

    #[tokio::test]
    async fn test_websocket_without_logging_preserves_bytes() {
        check_websocket_response_forwarding(WEBSOCKET_HANDSHAKE, 30, false, true).await;
    }

    #[test]
    fn test_upgrade_detector_is_bounded() {
        let mut detector = UpgradeDetector::default();
        assert!(!detector.observe(b"H"));
        assert!(detector.observe(&WEBSOCKET_HANDSHAKE[1..]));
        assert!(detector.headers.is_empty());
        let mut oversized = b"HTTP/1.1 101 Switching Protocols\r\nX-Long: ".to_vec();
        oversized.resize(BUFFER_SIZE * 2, b'x');
        assert!(!detector.observe(&oversized));
        assert!(detector.headers.is_empty());
        assert!(!detector.observe(&vec![0x81; BUFFER_SIZE * 2]));
        assert!(detector.headers.is_empty());
    }

    #[test]
    fn test_upgrade_detector_does_not_classify_body_bytes() {
        let mut detector = UpgradeDetector::default();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            WEBSOCKET_HANDSHAKE.len()
        );
        assert!(!detector.observe(response.as_bytes()));
        assert!(!detector.observe(WEBSOCKET_HANDSHAKE));
        assert!(detector.observe(WEBSOCKET_HANDSHAKE));
    }

    #[tokio::test]
    async fn test_blocked_writes_are_cancellable_inside_and_outside_a_tunnel() {
        for (upstream_to_client, tunnel_mode) in
            [(false, true), (true, true), (false, false), (true, false)]
        {
            let (mut reader, mut source) = tokio::io::duplex(1024);
            let (mut writer, _unread_peer) = tokio::io::duplex(1);
            source.write_all(&[0x81; 64]).await.unwrap();
            let token = CancellationToken::new();
            let cancel = token.clone();
            let (_events, receiver) = tokio::sync::broadcast::channel(1);
            let mut forward = tokio::spawn(async move {
                let logger = Arc::new(Mutex::new(None));
                let request_id = Arc::new(Mutex::new(None));
                let tunnel = Arc::new(AtomicBool::new(tunnel_mode));
                if upstream_to_client {
                    TcpForwarder::forward_upstream_to_client(
                        logger,
                        1,
                        &mut reader,
                        &mut writer,
                        request_id,
                        token,
                        receiver,
                        8080,
                        tunnel,
                    )
                    .await
                } else {
                    TcpForwarder::forward_client_to_upstream(
                        logger,
                        1,
                        &mut reader,
                        &mut writer,
                        request_id,
                        token,
                        receiver,
                        8080,
                        tunnel,
                    )
                    .await
                }
            });
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert!(!forward.is_finished());
            cancel.cancel();
            let result = tokio::time::timeout(Duration::from_secs(1), &mut forward).await;
            if result.is_err() {
                forward.abort();
            }
            result
                .expect("cancellation must settle a blocked raw write")
                .unwrap()
                .unwrap();
        }
    }

    #[tokio::test]
    async fn test_new() {
        let forwarder = TcpForwarder::new(1, "pod".to_string());
        assert_eq!(forwarder.config_id, 1);
        assert_eq!(forwarder.workload_type, "pod");
        assert!(forwarder.logger.is_none());
    }

    #[tokio::test]
    async fn test_initialize_logger_non_service_workload() {
        let mut forwarder = TcpForwarder::new(1, "deployment".to_string());
        let result = forwarder.initialize_logger(8080).await;
        assert!(result.is_ok());
        assert!(forwarder.logger.is_none());
    }

    #[tokio::test]
    async fn test_initialize_logger_state() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mut forwarder = TcpForwarder::new(1, "pod".to_string());

        let result = forwarder.initialize_logger(8080).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_forward_connection_basic() {
        let (mut client_to_upstream, mut client_reader) = tokio::io::duplex(1024);
        let (mut upstream_to_client, mut upstream_reader) = tokio::io::duplex(1024);

        let test_request = b"GET /test HTTP/1.1\r\n\r\n";
        let test_response = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nHello World";

        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();

        let _forwarder = TcpForwarder::new(1, "pod".to_string());
        let cancellation_token = CancellationToken::new();
        let cancel_clone = cancellation_token.clone();

        let upstream_task = tokio::spawn(async move {
            let mut buf = vec![0u8; 1024];
            let n = client_reader
                .read(&mut buf)
                .await
                .expect("Read should succeed");
            buf.truncate(n);

            tx.send(buf).expect("Channel send should succeed");

            upstream_to_client
                .write_all(test_response)
                .await
                .expect("Write should succeed");

            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_clone.cancel();
        });

        let client_task = tokio::spawn(async move {
            client_to_upstream
                .write_all(test_request)
                .await
                .expect("Write should succeed");
            client_to_upstream
                .flush()
                .await
                .expect("Flush should succeed");

            let mut response_buf = vec![0u8; 1024];
            let n = upstream_reader
                .read(&mut response_buf)
                .await
                .expect("Read should succeed");
            response_buf.truncate(n);

            assert_eq!(
                response_buf, test_response,
                "Response should match expected"
            );
        });

        let received_request = tokio::time::timeout(Duration::from_millis(300), rx)
            .await
            .expect("Should complete in time")
            .expect("Channel should return data");

        assert_eq!(
            received_request, test_request,
            "Request should match expected"
        );

        let _ = tokio::join!(upstream_task, client_task);
    }

    #[tokio::test]
    async fn test_handle_client_to_upstream_early_cancel() {
        let (client_read, mut client_write) = tokio::io::duplex(1024);
        let (upstream_write, mut upstream_read) = tokio::io::duplex(1024);

        let forwarder = TcpForwarder::new(1, "pod".to_string());
        let request_id = Arc::new(Mutex::new(None));
        let cancellation_token = CancellationToken::new();
        let cancel_clone = cancellation_token.clone();

        tokio::spawn(async move {
            client_write.write_all(b"test data").await.unwrap();
            client_write.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_clone.cancel();
        });

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let _ = upstream_read.read(&mut buf).await;
        });

        let mut client_reader = client_read;
        let mut upstream_writer = upstream_write;

        let (_, log_subscriber) = tokio::sync::broadcast::channel(1);

        let logger = Arc::new(Mutex::new(None));
        let websocket_tunnel_mode = Arc::new(AtomicBool::new(false));
        let result = TcpForwarder::forward_client_to_upstream(
            logger,
            forwarder.config_id,
            &mut client_reader,
            &mut upstream_writer,
            request_id,
            cancellation_token,
            log_subscriber,
            8080,
            websocket_tunnel_mode,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_upstream_to_client_early_cancel() {
        let (upstream_read, mut upstream_write) = tokio::io::duplex(1024);
        let (client_write, mut client_read) = tokio::io::duplex(1024);

        let forwarder = TcpForwarder::new(1, "pod".to_string());
        let request_id = Arc::new(Mutex::new(Some("req123".to_string())));
        let cancellation_token = CancellationToken::new();
        let cancel_clone = cancellation_token.clone();

        tokio::spawn(async move {
            upstream_write
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\ntest data")
                .await
                .unwrap();
            upstream_write.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_clone.cancel();
        });

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let _ = client_read.read(&mut buf).await;
        });

        let mut upstream_reader = upstream_read;
        let mut client_writer = client_write;

        let (_, log_subscriber) = tokio::sync::broadcast::channel(1);

        let logger = Arc::new(Mutex::new(None));
        let websocket_tunnel_mode = Arc::new(AtomicBool::new(false));
        let result = TcpForwarder::forward_upstream_to_client(
            logger,
            forwarder.config_id,
            &mut upstream_reader,
            &mut client_writer,
            request_id,
            cancellation_token,
            log_subscriber,
            8080,
            websocket_tunnel_mode,
        )
        .await;

        assert!(result.is_ok());
    }

    fn log_dir_contains(dir: &std::path::Path, needle: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                log_dir_contains(&path, needle)
            } else {
                std::fs::read_to_string(&path).is_ok_and(|text| text.contains(needle))
            }
        })
    }

    async fn open_forwarded_connection(
        config_id: i64, watcher: Arc<HttpLogStateWatcher>, cancel: CancellationToken,
    ) -> (TcpStream, tokio::task::JoinHandle<anyhow::Result<()>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, peer) = listener.accept().await.unwrap();
        let (upstream, mut origin) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            while let Ok(n) = origin.read(&mut buf).await {
                if n == 0
                    || origin
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: text/plain\r\n\r\nok",
                        )
                        .await
                        .is_err()
                {
                    break;
                }
            }
        });
        let forward = tokio::spawn(async move {
            TcpForwarder::new(config_id, "service".to_string())
                .forward_streams(server, upstream, peer, cancel, watcher, 18080)
                .await
        });
        (client, forward)
    }

    async fn get_through(client: &mut TcpStream, path: &str) {
        client
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: example.test\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut response = Vec::new();
        let mut buf = [0u8; 4096];
        while !response.ends_with(b"ok") {
            let n = tokio::time::timeout(Duration::from_secs(5), client.read(&mut buf))
                .await
                .expect("the response should arrive")
                .unwrap();
            assert!(n > 0, "the connection closed before the response arrived");
            response.extend_from_slice(&buf[..n]);
        }
    }

    #[tokio::test]
    async fn http_logs_enabled_later_log_requests_on_a_connection_opened_with_logs_off() {
        let _db = kftray_commons::test_utils::test_db().await;
        let config_dir = tempfile::tempdir().unwrap();
        let _env = kftray_commons::test_utils::EnvVarGuard::set(
            "KFTRAY_CONFIG",
            config_dir.path().to_str().unwrap(),
        );
        kftray_commons::utils::db::init().await.unwrap();
        kftray_commons::utils::migration::migrate_configs(None)
            .await
            .unwrap();
        let config_id = kftray_commons::utils::config::insert_config(
            kftray_commons::models::config_model::Config {
                service: Some("web".into()),
                namespace: "default".into(),
                workload_type: Some("service".into()),
                protocol: "tcp".into(),
                remote_port: Some(80),
                local_port: Some(18080),
                context: Some("ctx".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut logs_config =
            kftray_commons::models::http_logs_config_model::HttpLogsConfig::new(config_id);
        logs_config.enabled = false;
        kftray_commons::utils::http_logs_config::update_http_logs_config(&logs_config)
            .await
            .unwrap();
        let watcher = Arc::new(HttpLogStateWatcher::new());
        watcher.set_http_logs(config_id, false).await.unwrap();
        let cancel = CancellationToken::new();

        let (mut kept, kept_forward) =
            open_forwarded_connection(config_id, watcher.clone(), cancel.clone()).await;
        get_through(&mut kept, "/before-enable").await;

        logs_config.enabled = true;
        kftray_commons::utils::http_logs_config::update_http_logs_config(&logs_config)
            .await
            .unwrap();
        watcher.set_http_logs(config_id, true).await.unwrap();

        let logged_after_enable = tokio::time::timeout(Duration::from_secs(10), async {
            for attempt in 0.. {
                get_through(&mut kept, &format!("/after-enable-{attempt}")).await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                if log_dir_contains(config_dir.path(), "/after-enable-") {
                    return;
                }
            }
        })
        .await
        .is_ok();

        drop(kept);
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(5), kept_forward)
            .await
            .expect("the forwarder should stop once the connection closes")
            .unwrap()
            .unwrap();

        assert!(
            logged_after_enable,
            "a request on a connection opened with logs off was not logged after logs were enabled"
        );
        assert!(
            !log_dir_contains(config_dir.path(), "/before-enable"),
            "a request sent while logs were off was logged"
        );
    }
}
