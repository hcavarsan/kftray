use std::sync::Arc;
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

// BufReader capacity for the bidirectional `copy_buf` loop. Larger
// buffers reduce syscall count at high RPS on small responses. 128KB
// fits within the 256KB SOCKET_BUF_SIZE so a single buffered read can
// drain a full kernel receive buffer in one syscall.
const BUFFER_SIZE: usize = 131_072;
const SOCKET_BUF_SIZE: usize = 256 * 1024;

/// Classify whether an I/O error represents a normal client-initiated
/// disconnect (e.g. wrk closing connections, browser navigating away, curl
/// hitting Ctrl-C). These are expected, not error conditions, and should
/// log at debug level instead of polluting error logs at high RPS.
fn is_client_disconnect(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::NotConnected
    )
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

        if let Err(e) = sock_ref.set_recv_buffer_size(SOCKET_BUF_SIZE) {
            tracing::debug!("Failed to set receive buffer size: {}", e);
        }

        if let Err(e) = sock_ref.set_send_buffer_size(SOCKET_BUF_SIZE) {
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

        let http_logs_enabled =
            match kftray_commons::utils::http_logs_config::get_http_logs_config(self.config_id)
                .await
            {
                Ok(config) => config.enabled,
                Err(_) => false,
            };

        if http_logs_enabled {
            if self.logger.is_none() {
                debug!(
                    "Initializing HTTP logger for config_id {} on port {}",
                    self.config_id, local_port
                );
                let logger =
                    kftray_http_logs::HttpLogger::for_config(self.config_id, local_port).await?;
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

    // Forward client <-> upstream bytes until EITHER direction finishes.
    //
    // We deliberately do NOT wait for both directions to EOF (as `try_join!` or
    // `copy_bidirectional` would). Upstream Kubernetes WebSocket port-forwards
    // use HTTP keep-alive on the pod side and never reciprocate the v5 close
    // signal we send on `poll_shutdown`. Waiting for upstream EOF would hang the
    // task forever, leaking the local TcpStream (CLOSE_WAIT) and the
    // pre-allocated kube-portforward channel pair until the session is exhausted
    // (~64 requests).
    //
    // When the winning future resolves, dropping the loser cancels its in-flight
    // I/O. The owning `client_conn` and `upstream_conn` then drop at end of
    // scope, triggering `kube_portforward::ReleaseGuard::Drop` which sends a
    // final v5 0xFF to the apiserver (graceful pod-side teardown) and frees the
    // channel pair back to the session.
    pub async fn forward_connection(
        &mut self, client_conn: Arc<Mutex<TcpStream>>,
        upstream_conn: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        cancellation_token: CancellationToken, http_log_watcher: Arc<HttpLogStateWatcher>,
        local_port: u16,
    ) -> anyhow::Result<()> {
        let log_subscriber = http_log_watcher.create_filtered_subscriber(self.config_id);
        let current_logging_enabled = http_log_watcher.get_http_logs(self.config_id).await;

        if current_logging_enabled
            && self.logger.is_none()
            && let Err(e) = self.initialize_logger(local_port).await
        {
            tracing::warn!("Logger init failed for config {}: {e}", self.config_id);
        }
        if current_logging_enabled || self.logger.is_some() {
            let config_id = self.config_id;
            let shared_logger: Arc<Mutex<Option<Arc<crate::Logger>>>> =
                Arc::new(Mutex::new(self.logger.take().map(Arc::new)));

            let request_id = Arc::new(Mutex::new(None));
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
            );

            let result: anyhow::Result<()> = tokio::select! {
                biased;
                res = client_to_upstream => {
                    match res {
                        Ok(()) => {
                            tracing::debug!("HTTP-aware: client->upstream finished (client EOF)");
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
                res = upstream_to_client => {
                    match res {
                        Ok(()) => {
                            tracing::debug!("HTTP-aware: upstream->client finished (upstream EOF)");
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
            };

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
        } else {
            let t_start = std::time::Instant::now();
            let mut client_conn_guard = client_conn.lock().await;
            let (client_reader, mut client_writer) = tokio::io::split(&mut *client_conn_guard);
            let (upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

            let mut client_reader = tokio::io::BufReader::with_capacity(65_536, client_reader);
            let mut upstream_reader = tokio::io::BufReader::with_capacity(65_536, upstream_reader);

            // Race client→upstream vs upstream→client. When either direction
            // finishes, the other is dropped. This works correctly with SPDY
            // because poll_shutdown() sends DATA+FIN and sets a graceful flag
            // on the StreamGuard — subsequent drop skips RST_STREAM, allowing
            // the remote to finish its response naturally.
            let c2u = async {
                let res = tokio::io::copy_buf(&mut client_reader, &mut upstream_writer).await;
                let _ = upstream_writer.shutdown().await;
                res
            };

            let u2c = async {
                let res = tokio::io::copy_buf(&mut upstream_reader, &mut client_writer).await;
                let _ = client_writer.shutdown().await;
                res
            };

            tokio::select! {
                biased;
                res = c2u => match res {
                    Ok(n) => debug!("simple: c->u finished {}B in {}ms", n, t_start.elapsed().as_millis()),
                    Err(e) => {
                        if is_client_disconnect(&e) {
                            debug!("simple c->u closed by client: {}", e);
                        } else {
                            error!("simple c->u error: {}", e);
                        }
                        return Err(e.into());
                    }
                },
                res = u2c => match res {
                    Ok(n) => debug!("simple: u->c finished {}B in {}ms", n, t_start.elapsed().as_millis()),
                    Err(e) => {
                        if is_client_disconnect(&e) {
                            debug!("simple u->c closed by client: {}", e);
                        } else {
                            error!("simple u->c error: {}", e);
                        }
                        return Err(e.into());
                    }
                },
                _ = cancellation_token.cancelled() => debug!("connection cancelled"),
            }
        }

        Ok(())
    }

    pub async fn forward_tls_streams(
        &self, client: tokio_rustls::server::TlsStream<TcpStream>,
        upstream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<()> {
        Self::apply_socket_optimizations(client.get_ref().0);

        let (client_r, mut client_w) = tokio::io::split(client);
        let (upstream_r, mut upstream_w) = tokio::io::split(upstream);

        let mut client_r = tokio::io::BufReader::with_capacity(BUFFER_SIZE, client_r);
        let mut upstream_r = tokio::io::BufReader::with_capacity(BUFFER_SIZE, upstream_r);

        let c2u = async {
            let r = tokio::io::copy_buf(&mut client_r, &mut upstream_w).await;
            let _ = upstream_w.shutdown().await;
            r
        };
        let u2c = async {
            let r = tokio::io::copy_buf(&mut upstream_r, &mut client_w).await;
            let _ = client_w.shutdown().await;
            r
        };

        tokio::select! {
            biased;
            res = c2u => match res {
                Ok(n) => debug!("TLS client->upstream: {} bytes", n),
                Err(e) => {
                    if is_client_disconnect(&e) {
                        debug!("TLS c->u closed by client: {}", e);
                    } else {
                        error!("TLS c->u error: {}", e);
                    }
                    return Err(e.into());
                }
            },
            res = u2c => match res {
                Ok(n) => debug!("TLS upstream->client: {} bytes", n),
                Err(e) => {
                    if is_client_disconnect(&e) {
                        debug!("TLS u->c closed by client: {}", e);
                    } else {
                        error!("TLS u->c error: {}", e);
                    }
                    return Err(e.into());
                }
            },
            _ = cancellation_token.cancelled() => {
                debug!("TLS connection canceled");
                return Err(anyhow::anyhow!("TLS connection canceled"));
            }
        }
        Ok(())
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
        local_port: u16,
    ) -> anyhow::Result<()> {
        // Heap-allocated to avoid blowing the async-task stack — this buffer
        // lives inside an async state machine and in debug builds the state
        // machine is not size-optimized, so a 128KB stack array overflows.
        let mut buffer = vec![0u8; BUFFER_SIZE];
        let mut should_log = {
            let logger_guard = logger.lock().await;
            logger_guard.is_some()
        };
        let mut request_buffer = if should_log { Some(Vec::new()) } else { None };

        loop {
            tokio::select! {
                result = client_reader.read(&mut buffer) => {
                    let n = match result {
                        Ok(n) => n,
                        Err(e) => return Err(e.into()),
                    };

                    if n == 0 {
                        break;
                    }

                    if should_log {
                        if let Some(ref mut req_buf) = request_buffer.as_mut() {
                            req_buf.extend_from_slice(&buffer[..n]);
                            let maybe_logger = {
                                let guard = logger.lock().await;
                                guard.clone()
                            };
                            if let Some(log) = maybe_logger {
                                let new_request_id = log.log_request(req_buf.clone().into()).await;
                                let mut req_id_guard = request_id.lock().await;
                                *req_id_guard = Some(new_request_id);

                                if let Err(e) = upstream_writer.write_all(req_buf).await {
                                    return Err(e.into());
                                }
                                req_buf.clear();
                            }
                        }
                    } else if let Err(e) = upstream_writer.write_all(&buffer[..n]).await {
                        return Err(e.into());
                    }
                },
                log_event = log_subscriber.recv() => {
                    if let Ok(event) = log_event
                        && event.config_id == config_id {
                            let needs_logger = event.enabled && {
                                let guard = logger.lock().await;
                                guard.is_none()
                            };

                            let new_should_log = if needs_logger {
                                let http_logs_enabled = match kftray_commons::utils::http_logs_config::get_http_logs_config(config_id).await {
                                    Ok(config) => config.enabled,
                                    Err(_) => false,
                                };

                                if http_logs_enabled {
                                    match kftray_http_logs::HttpLogger::for_config(config_id, local_port).await {
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
                                    request_buffer = Some(Vec::new());
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

    #[allow(clippy::too_many_arguments)]
    async fn forward_upstream_to_client<'a>(
        logger: Arc<Mutex<Option<Arc<crate::Logger>>>>, config_id: i64,
        upstream_reader: &'a mut (impl AsyncReadExt + Unpin),
        client_writer: &'a mut (impl AsyncWriteExt + Unpin),
        request_id: Arc<Mutex<Option<String>>>, cancellation_token: CancellationToken,
        mut log_subscriber: tokio::sync::broadcast::Receiver<
            crate::kube::http_log_watcher::HttpLogStateEvent,
        >,
        local_port: u16,
    ) -> anyhow::Result<()> {
        // Heap-allocated; see `forward_client_to_upstream` for rationale.
        let mut buffer = vec![0u8; BUFFER_SIZE];
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

        loop {
            tokio::select! {
                result = upstream_reader.read(&mut buffer) => {
                    let n = match result {
                        Ok(n) => n,
                        Err(e) => return Err(e.into()),
                    };

                    if n == 0 {
                        if should_log
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

                    if should_log
                        && let Some(ref mut state) = response_state.as_mut() {
                            Self::handle_response_logging_static(&buffer[..n], state, &logger, &request_id).await;
                        }

                    if let Err(e) = client_writer.write_all(&buffer[..n]).await {
                        return Err(e.into());
                    }
                },
                log_event = log_subscriber.recv() => {
                    if let Ok(event) = log_event
                        && event.config_id == config_id {
                            let needs_logger = event.enabled && {
                                let guard = logger.lock().await;
                                guard.is_none()
                            };

                            let new_should_log = if needs_logger {
                                let http_logs_enabled = match kftray_commons::utils::http_logs_config::get_http_logs_config(config_id).await {
                                    Ok(config) => config.enabled,
                                    Err(_) => false,
                                };

                                if http_logs_enabled {
                                    match kftray_http_logs::HttpLogger::for_config(config_id, local_port).await {
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
                _ = cancellation_token.cancelled() => break,
            }
        }

        let _ = client_writer.shutdown().await;

        Ok(())
    }

    async fn handle_response_logging_static(
        buffer: &[u8], state: &mut ResponseState, logger: &Arc<Mutex<Option<Arc<Logger>>>>,
        request_id: &Arc<Mutex<Option<String>>>,
    ) {
        let req_id_guard = request_id.lock().await;
        let current_req_id = req_id_guard.clone();
        drop(req_id_guard);

        let is_new_response = match (&state.current_response_id, &current_req_id) {
            (Some(current_id), Some(req_id)) => current_id != req_id,
            (None, Some(_)) => true,
            _ => false,
        };

        if is_new_response {
            state.reset_for_new_response(current_req_id);
        }

        if state.buffer.is_empty() && !buffer.is_empty() {
            if state.first_chunk_time.is_none() {
                state.first_chunk_time = Some(tokio::time::Instant::now());
            }
            state.is_chunked = kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::detect_chunked_encoding(buffer);
        }

        kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::process_chunk(
            buffer,
            state.is_chunked,
            &mut state.found_end_marker,
            &mut state.total_chunks_received,
        );

        state.buffer.extend_from_slice(buffer);

        if !state.current_response_logged {
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
                let now = tokio::time::Instant::now();
                if now >= force_time {
                    let needs_more_time =
                        state.buffer.windows(18).any(|w| {
                            w == b"Content-Encoding: gzip" || w == b"content-encoding: gzip"
                        }) && state.total_chunks_received < 20
                            && now.saturating_duration_since(force_time)
                                < tokio::time::Duration::from_millis(10);
                    return !needs_more_time;
                }
            } else if let Some(first_time) = state.first_chunk_time {
                return first_time.elapsed().as_secs() > 5;
            }
        } else {
            return kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::check_content_length_match(&state.buffer)
                || kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::is_websocket_upgrade(&state.buffer);
        }
        false
    }

    fn can_clear_response_buffer_static(state: &ResponseState) -> bool {
        if state.is_chunked {
            state.found_end_marker
        } else {
            kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::check_content_length_match(&state.buffer)
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

    fn reset_for_next_response(&mut self) {
        self.buffer.clear();
        self.is_chunked = false;
        self.found_end_marker = false;
        self.total_chunks_received = 0;
        self.first_chunk_time = None;
        self.force_log_time = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let result = TcpForwarder::forward_client_to_upstream(
            logger,
            forwarder.config_id,
            &mut client_reader,
            &mut upstream_writer,
            request_id,
            cancellation_token,
            log_subscriber,
            8080,
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
        let result = TcpForwarder::forward_upstream_to_client(
            logger,
            forwarder.config_id,
            &mut upstream_reader,
            &mut client_writer,
            request_id,
            cancellation_token,
            log_subscriber,
            8080,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn client_eof_releases_resources_promptly() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let accept_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            stream
        });

        let mut client_side = tokio::net::TcpStream::connect(addr).await.unwrap();
        let server_side = accept_task.await.unwrap();

        let (upstream_local, mut upstream_remote) = tokio::io::duplex(1024);

        let mut forwarder = TcpForwarder::new(1, "pod".to_string());
        let cancellation_token = CancellationToken::new();
        let watcher = Arc::new(HttpLogStateWatcher::new());

        let forward_handle = tokio::spawn(async move {
            forwarder
                .forward_connection(
                    Arc::new(Mutex::new(server_side)),
                    upstream_local,
                    cancellation_token,
                    watcher,
                    8080,
                )
                .await
        });

        client_side
            .write_all(b"GET / HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        client_side.flush().await.unwrap();

        let mut buf = [0u8; 1024];
        let _ = upstream_remote.read(&mut buf).await.unwrap();

        drop(client_side);

        let result = tokio::time::timeout(Duration::from_secs(1), forward_handle).await;
        assert!(
            result.is_ok(),
            "forward_connection must return promptly after client EOF, did not finish within 1s"
        );
    }
}
