use std::sync::Arc;
use std::time::Duration;

use kftray_http_logs::HttpLogState;
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::time::timeout;
use tracing::{
    debug,
    error,
};

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

    pub async fn update_logger_state(
        &mut self, http_log_state: &HttpLogState, local_port: u16,
    ) -> anyhow::Result<()> {
        self.initialize_logger(http_log_state, local_port).await
    }

    pub async fn forward_connection(
        &self, client_conn: Arc<Mutex<TcpStream>>,
        upstream_conn: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        cancel_notifier: Arc<Notify>, logging_enabled: bool,
    ) -> anyhow::Result<()> {
        if !logging_enabled || self.logger.is_none() {
            let mut client_conn_guard = client_conn.lock().await;
            let client_stream = &mut *client_conn_guard;
            let mut pinned_upstream = Box::pin(upstream_conn);

            tokio::select! {
                result = tokio::io::copy_bidirectional(client_stream, &mut pinned_upstream) => {
                    match result {
                        Ok(_) => {},
                        Err(e) if matches!(e.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof) => {},
                        Err(_) => {},
                    }
                }
                _ = cancel_notifier.notified() => {}
            }
            return Ok(());
        }

        let request_id = Arc::new(Mutex::new(None));

        let mut client_conn_guard = client_conn.lock().await;
        let (mut client_reader, mut client_writer) = tokio::io::split(&mut *client_conn_guard);

        let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

        let client_to_upstream = self.handle_client_to_upstream(
            &mut client_reader,
            &mut upstream_writer,
            self.logger.clone(),
            Arc::clone(&request_id),
            cancel_notifier.clone(),
        );

        let upstream_to_client = self.handle_upstream_to_client(
            &mut upstream_reader,
            &mut client_writer,
            self.logger.clone(),
            Arc::clone(&request_id),
            cancel_notifier.clone(),
        );

        match tokio::try_join!(client_to_upstream, upstream_to_client) {
            Ok(_) => {
                debug!("Connection closed normally");
            }
            Err(e) => {
                error!(
                    error = e.as_ref() as &dyn std::error::Error,
                    "Connection closed with error"
                );
                return Err(e);
            }
        }

        Ok(())
    }

    async fn handle_client_to_upstream<'a>(
        &'a self, client_reader: &'a mut (impl AsyncReadExt + Unpin),
        upstream_writer: &'a mut (impl AsyncWriteExt + Unpin), logger: Option<Logger>,
        request_id: Arc<Mutex<Option<String>>>, cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let should_log = logger.is_some();
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
                        if let (Some(ref mut req_buf), Some(ref logger)) = (request_buffer.as_mut(), &logger) {
                            req_buf.extend_from_slice(&buffer[..n]);
                            let mut req_id_guard = request_id.lock().await;
                            let new_request_id = logger.log_request(req_buf.clone().into()).await;
                            *req_id_guard = Some(new_request_id);

                            if let Err(e) = upstream_writer.write_all(req_buf).await {
                                return Err(e.into());
                            }
                            req_buf.clear();
                        }
                    } else if let Err(e) = upstream_writer.write_all(&buffer[..n]).await {
                        return Err(e.into());
                    }
                },
                _ = cancel_notifier.notified() => break,
            }
        }

        let _ = upstream_writer.shutdown().await;

        Ok(())
    }

    async fn handle_upstream_to_client<'a>(
        &'a self, upstream_reader: &'a mut (impl AsyncReadExt + Unpin),
        client_writer: &'a mut (impl AsyncWriteExt + Unpin), logger: Option<Logger>,
        request_id: Arc<Mutex<Option<String>>>, cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let should_log = logger.is_some();

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
                            if let (Some(ref mut state), Some(ref logger)) = (response_state.as_mut(), &logger) {
                                if !state.buffer.is_empty() && !state.current_response_logged {
                                    let req_id_guard = request_id.lock().await;
                                    if let Some(req_id) = &*req_id_guard {
                                        logger.log_response(state.buffer.clone().into(), req_id.clone()).await;
                                    }
                                }
                            }
                        }
                        break;
                    }

                    if should_log {
                        if let Some(ref mut state) = response_state.as_mut() {
                            self.handle_response_logging(&buffer[..n], state, &logger, &request_id).await;
                        }
                    }


                    if let Err(e) = client_writer.write_all(&buffer[..n]).await {
                        return Err(e.into());
                    }
                },
                _ = cancel_notifier.notified() => break,
            }
        }

        let _ = client_writer.shutdown().await;

        Ok(())
    }

    async fn handle_response_logging(
        &self, buffer: &[u8], state: &mut ResponseState, logger: &Option<Logger>,
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
            if let Some(logger) = logger {
                let should_log = self.should_log_response(state);
                if should_log && state.current_response_id.is_some() {
                    let response_id = state.current_response_id.as_ref().unwrap().clone();
                    state.current_response_logged = true;
                    logger
                        .log_response(state.buffer.clone().into(), response_id)
                        .await;

                    if self.can_clear_response_buffer(state) {
                        state.reset_for_next_response();
                    }
                }
            }
        }
    }

    fn should_log_response(&self, state: &mut ResponseState) -> bool {
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

    fn can_clear_response_buffer(&self, state: &ResponseState) -> bool {
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
    async fn test_update_logger_state() {
        let mut forwarder = TcpForwarder::new(1, "pod".to_string());
        let http_log_state = kftray_http_logs::HttpLogState::new();

        let result = forwarder.update_logger_state(&http_log_state, 8080).await;
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
        let cancel_notifier = Arc::new(Notify::new());
        let cancel_clone = Arc::clone(&cancel_notifier);

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
            cancel_clone.notify_one();
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
        let cancel_notifier = Arc::new(Notify::new());
        let cancel_clone = cancel_notifier.clone();

        tokio::spawn(async move {
            client_write.write_all(b"test data").await.unwrap();
            client_write.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_clone.notify_one();
        });

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let _ = upstream_read.read(&mut buf).await;
        });

        let mut client_reader = client_read;
        let mut upstream_writer = upstream_write;

        let result = forwarder
            .handle_client_to_upstream(
                &mut client_reader,
                &mut upstream_writer,
                None,
                request_id,
                cancel_notifier,
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
        let cancel_notifier = Arc::new(Notify::new());
        let cancel_clone = cancel_notifier.clone();

        tokio::spawn(async move {
            upstream_write
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\ntest data")
                .await
                .unwrap();
            upstream_write.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_clone.notify_one();
        });

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let _ = client_read.read(&mut buf).await;
        });

        let mut upstream_reader = upstream_read;
        let mut client_writer = client_write;

        let result = forwarder
            .handle_upstream_to_client(
                &mut upstream_reader,
                &mut client_writer,
                None,
                request_id,
                cancel_notifier,
            )
            .await;

        assert!(result.is_ok());
    }
}
