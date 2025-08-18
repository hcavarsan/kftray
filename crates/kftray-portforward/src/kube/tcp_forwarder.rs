use std::sync::Arc;
use std::time::Duration;

use kftray_http_logs::HttpLogState;
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing::{
    debug,
    error,
};

use crate::kube::http_log_watcher::HttpLogStateWatcher;
use crate::Logger;

const BUFFER_SIZE: usize = 65536;
const TIMEOUT_DURATION: Duration = Duration::from_secs(600);

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

        if let Err(e) = stream.set_linger(None) {
            tracing::debug!("Failed to disable SO_LINGER: {}", e);
        }

        let sock_ref = SockRef::from(stream);

        if let Err(e) = sock_ref.set_recv_buffer_size(BUFFER_SIZE) {
            tracing::debug!("Failed to set receive buffer size: {}", e);
        }

        if let Err(e) = sock_ref.set_send_buffer_size(BUFFER_SIZE) {
            tracing::debug!("Failed to set send buffer size: {}", e);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn forward_streams(
        &mut self, client_stream: tokio::net::TcpStream,
        upstream_stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        client_address: std::net::SocketAddr, cancellation_token: CancellationToken,
        http_log_watcher: Arc<HttpLogStateWatcher>, http_log_state: Arc<HttpLogState>,
        local_port: u16,
    ) -> anyhow::Result<()> {
        Self::apply_socket_optimizations(&client_stream);

        let _log_subscriber = http_log_watcher.create_filtered_subscriber(self.config_id);
        let current_logging_enabled = http_log_watcher.get_http_logs(self.config_id).await;

        if current_logging_enabled && self.logger.is_none() {
            if let Err(e) = self.initialize_logger(&http_log_state, local_port).await {
                error!("Failed to initialize logger for {}: {}", client_address, e);
            }
        }

        self.forward_connection(
            Arc::new(Mutex::new(client_stream)),
            upstream_stream,
            cancellation_token,
            http_log_watcher,
            http_log_state,
            local_port,
        )
        .await
    }

    pub async fn initialize_logger(
        &mut self, http_log_state: &HttpLogState, local_port: u16,
    ) -> anyhow::Result<()> {
        if self.workload_type != "service" && self.workload_type != "pod" {
            return Ok(());
        }

        match http_log_state.get_http_logs(self.config_id).await {
            Ok(true) => {
                if self.logger.is_none() {
                    debug!(
                        "Initializing HTTP logger for config_id {} on port {}",
                        self.config_id, local_port
                    );
                    let logger =
                        kftray_http_logs::HttpLogger::for_config(self.config_id, local_port)
                            .await?;
                    self.logger = Some(logger);
                }
            }
            Ok(false) => {
                if self.logger.is_some() {
                    debug!(
                        "HTTP logging disabled for config_id {}, clearing logger",
                        self.config_id
                    );
                    self.logger = None;
                }
            }
            Err(e) => {
                error!("Failed to check HTTP logging state: {:?}", e);
            }
        }

        Ok(())
    }

    pub async fn forward_connection(
        &mut self, client_conn: Arc<Mutex<TcpStream>>,
        mut upstream_conn: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        cancellation_token: CancellationToken, http_log_watcher: Arc<HttpLogStateWatcher>,
        http_log_state: Arc<HttpLogState>, local_port: u16,
    ) -> anyhow::Result<()> {
        let log_subscriber = http_log_watcher.create_filtered_subscriber(self.config_id);
        let current_logging_enabled = http_log_watcher.get_http_logs(self.config_id).await;

        if current_logging_enabled && self.logger.is_none() {
            let _ = self.initialize_logger(&http_log_state, local_port).await;
        }
        if current_logging_enabled || self.logger.is_some() {
            let config_id = self.config_id;
            let shared_logger = Arc::new(Mutex::new(self.logger.take()));

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
                http_log_state.clone(),
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
                http_log_state,
                local_port,
            );

            let result = tokio::try_join!(client_to_upstream, upstream_to_client);

            self.logger = Arc::try_unwrap(shared_logger).unwrap().into_inner();

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
            let mut client_conn_guard = client_conn.lock().await;

            let config_id = self.config_id;
            let log_subscriber = http_log_watcher.create_filtered_subscriber(config_id);
            let copy_future =
                tokio::io::copy_bidirectional(&mut *client_conn_guard, &mut upstream_conn);
            let state_monitor = Self::monitor_logging_state_simple(
                config_id,
                log_subscriber,
                cancellation_token.clone(),
            );

            tokio::select! {
                result = copy_future => {
                    match result {
                        Ok(_) => debug!("Simple connection closed normally"),
                        Err(e) => {
                            error!("Simple connection closed with error: {}", e);
                            return Err(e.into());
                        }
                    }
                }
                _ = state_monitor => {
                    debug!("Connection interrupted for logging state change");
                    return Err(anyhow::anyhow!("Connection needs restart for HTTP logging"));
                }
            }
        }

        Ok(())
    }

    async fn monitor_logging_state_simple(
        config_id: i64,
        mut log_subscriber: tokio::sync::broadcast::Receiver<
            crate::kube::http_log_watcher::HttpLogStateEvent,
        >,
        cancellation_token: CancellationToken,
    ) {
        loop {
            tokio::select! {
                log_event = log_subscriber.recv() => {
                    if let Ok(event) = log_event {
                        debug!("Simple connection received log event: config_id={}, enabled={}", event.config_id, event.enabled);
                        if event.config_id == config_id && event.enabled {
                            debug!("HTTP logging enabled, terminating simple connection");
                            return;
                        }
                    } else {
                        debug!("Simple connection log subscriber error");
                    }
                }
                _ = cancellation_token.cancelled() => return,
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn forward_client_to_upstream<'a>(
        logger: Arc<Mutex<Option<crate::Logger>>>, config_id: i64,
        client_reader: &'a mut (impl AsyncReadExt + Unpin),
        upstream_writer: &'a mut (impl AsyncWriteExt + Unpin),
        request_id: Arc<Mutex<Option<String>>>, cancellation_token: CancellationToken,
        mut log_subscriber: tokio::sync::broadcast::Receiver<
            crate::kube::http_log_watcher::HttpLogStateEvent,
        >,
        http_log_state: Arc<HttpLogState>, local_port: u16,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut should_log = {
            let logger_guard = logger.lock().await;
            logger_guard.is_some()
        };
        let mut request_buffer = if should_log { Some(Vec::new()) } else { None };

        loop {
            tokio::select! {
                n = timeout(TIMEOUT_DURATION, client_reader.read(&mut buffer)) => {
                    let n = match n {
                        Ok(Ok(n)) => n,
                        Ok(Err(e)) => return Err(e.into()),
                        Err(_) => return Err(anyhow::anyhow!("Read timeout")),
                    };

                    if n == 0 {
                        break;
                    }

                    if should_log {
                        if let Some(ref mut req_buf) = request_buffer.as_mut() {
                            let logger_guard = logger.lock().await;
                            if let Some(ref log) = *logger_guard {
                            req_buf.extend_from_slice(&buffer[..n]);
                            let mut req_id_guard = request_id.lock().await;
                                let new_request_id = log.log_request(req_buf.clone().into()).await;
                                drop(logger_guard);
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
                    if let Ok(event) = log_event {
                        if event.config_id == config_id {
                            let new_should_log = event.enabled && {
                                let mut logger_guard = logger.lock().await;
                                if event.enabled && logger_guard.is_none() {
                                    match http_log_state.get_http_logs(config_id).await {
                                        Ok(true) => {
                                            if let Ok(new_logger) = kftray_http_logs::HttpLogger::for_config(config_id, local_port).await {
                                                *logger_guard = Some(new_logger);
                                                drop(logger_guard);
                                                true
                                            } else {
                                                drop(logger_guard);
                                                false
                                            }
                                        }
                                        _ => {
                                            drop(logger_guard);
                                            false
                                        }
                                    }
                                } else {
                                    let result = logger_guard.is_some();
                                    drop(logger_guard);
                                    result
                                }
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
        logger: Arc<Mutex<Option<crate::Logger>>>, config_id: i64,
        upstream_reader: &'a mut (impl AsyncReadExt + Unpin),
        client_writer: &'a mut (impl AsyncWriteExt + Unpin),
        request_id: Arc<Mutex<Option<String>>>, cancellation_token: CancellationToken,
        mut log_subscriber: tokio::sync::broadcast::Receiver<
            crate::kube::http_log_watcher::HttpLogStateEvent,
        >,
        http_log_state: Arc<HttpLogState>, local_port: u16,
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

        loop {
            tokio::select! {
                n = timeout(TIMEOUT_DURATION, upstream_reader.read(&mut buffer)) => {
                    let n = match n {
                        Ok(Ok(n)) => n,
                        Ok(Err(e)) => return Err(e.into()),
                        Err(_) => return Err(anyhow::anyhow!("Read timeout")),
                    };

                    if n == 0 {
                        if should_log {
                            if let Some(ref mut state) = response_state.as_mut() {
                                let logger_guard = logger.lock().await;
                                if let Some(ref log) = *logger_guard {
                                    if !state.buffer.is_empty() && !state.current_response_logged {
                                        let req_id_guard = request_id.lock().await;
                                        if let Some(req_id) = &*req_id_guard {
                                            log.log_response(state.buffer.clone().into(), req_id.clone()).await;
                                        }
                                    }
                                }
                            }
                        }
                        break;
                    }

                    if should_log {
                        if let Some(ref mut state) = response_state.as_mut() {
                            Self::handle_response_logging_static(&buffer[..n], state, &logger, &request_id).await;
                        }
                    }

                    if let Err(e) = client_writer.write_all(&buffer[..n]).await {
                        return Err(e.into());
                    }
                },
                log_event = log_subscriber.recv() => {
                    if let Ok(event) = log_event {
                        if event.config_id == config_id {
                            let new_should_log = event.enabled && {
                                let mut logger_guard = logger.lock().await;
                                if event.enabled && logger_guard.is_none() {
                                    match http_log_state.get_http_logs(config_id).await {
                                        Ok(true) => {
                                            if let Ok(new_logger) = kftray_http_logs::HttpLogger::for_config(config_id, local_port).await {
                                                *logger_guard = Some(new_logger);
                                                drop(logger_guard);
                                                true
                                            } else {
                                                drop(logger_guard);
                                                false
                                            }
                                        }
                                        _ => {
                                            drop(logger_guard);
                                            false
                                        }
                                    }
                                } else {
                                    let result = logger_guard.is_some();
                                    drop(logger_guard);
                                    result
                                }
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
                    }
                },
                _ = cancellation_token.cancelled() => break,
            }
        }

        let _ = client_writer.shutdown().await;

        Ok(())
    }

    async fn handle_response_logging_static(
        buffer: &[u8], state: &mut ResponseState, logger: &Arc<Mutex<Option<Logger>>>,
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
            let logger_guard = logger.lock().await;
            if let Some(ref log) = *logger_guard {
                let should_log = Self::should_log_response_static(state);
                if should_log && state.current_response_id.is_some() {
                    let response_id = state.current_response_id.as_ref().unwrap().clone();
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
        let http_log_state = kftray_http_logs::HttpLogState::new();
        let result = forwarder.initialize_logger(&http_log_state, 8080).await;
        assert!(result.is_ok());
        assert!(forwarder.logger.is_none());
    }

    #[tokio::test]
    async fn test_initialize_logger_state() {
        let mut forwarder = TcpForwarder::new(1, "pod".to_string());
        let http_log_state = kftray_http_logs::HttpLogState::new();

        let result = forwarder.initialize_logger(&http_log_state, 8080).await;
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
        let _http_log_state = HttpLogState::new();
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
        let http_log_state = Arc::new(HttpLogState::new());

        let logger = Arc::new(Mutex::new(None));
        let result = TcpForwarder::forward_client_to_upstream(
            logger,
            forwarder.config_id,
            &mut client_reader,
            &mut upstream_writer,
            request_id,
            cancellation_token,
            log_subscriber,
            http_log_state,
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
        let _http_log_state = HttpLogState::new();
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
        let http_log_state = Arc::new(HttpLogState::new());

        let logger = Arc::new(Mutex::new(None));
        let result = TcpForwarder::forward_upstream_to_client(
            logger,
            forwarder.config_id,
            &mut upstream_reader,
            &mut client_writer,
            request_id,
            cancellation_token,
            log_subscriber,
            http_log_state,
            8080,
        )
        .await;

        assert!(result.is_ok());
    }
}
