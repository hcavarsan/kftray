use std::sync::Arc;

use kftray_http_logs::HttpLogger;
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::{
    BUFFER_SIZE,
    TcpForwarder,
};

pub(super) struct ResponseState {
    pub(super) buffer: Vec<u8>,
    pub(super) is_chunked: bool,
    pub(super) found_end_marker: bool,
    pub(super) total_chunks_received: usize,
    pub(super) current_response_id: Option<String>,
    pub(super) current_response_logged: bool,
    pub(super) first_chunk_time: Option<tokio::time::Instant>,
    pub(super) force_log_time: Option<tokio::time::Instant>,
}

impl ResponseState {
    pub(super) fn new() -> Self {
        Self {
            buffer: Vec::new(),
            is_chunked: false,
            found_end_marker: false,
            total_chunks_received: 0,
            current_response_id: None,
            current_response_logged: false,
            first_chunk_time: None,
            force_log_time: None,
        }
    }

    pub(super) fn reset_for_new_response(&mut self, request_id: Option<String>) {
        self.buffer.clear();
        self.is_chunked = false;
        self.found_end_marker = false;
        self.total_chunks_received = 0;
        self.current_response_logged = false;
        self.current_response_id = request_id;
        self.first_chunk_time = Some(tokio::time::Instant::now());
        self.force_log_time = None;
    }

    pub(super) fn reset_for_next_response(&mut self) {
        self.buffer.clear();
        self.is_chunked = false;
        self.found_end_marker = false;
        self.total_chunks_received = 0;
        self.first_chunk_time = None;
        self.force_log_time = None;
    }
}

impl TcpForwarder {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn forward_client_to_upstream<'a>(
        logger: Arc<Mutex<Option<Arc<HttpLogger>>>>, config_id: i64,
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
    pub(crate) async fn forward_upstream_to_client<'a>(
        logger: Arc<Mutex<Option<Arc<HttpLogger>>>>, config_id: i64,
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
            Some(ResponseState::new())
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
                                    response_state = Some(ResponseState::new());
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

    pub(super) async fn handle_response_logging_static(
        buffer: &[u8], state: &mut ResponseState, logger: &Arc<Mutex<Option<Arc<HttpLogger>>>>,
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::io::{
        AsyncReadExt,
        AsyncWriteExt,
    };
    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    use super::super::TcpForwarder;

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
}
