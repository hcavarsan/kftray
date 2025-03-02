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

const BUFFER_SIZE: usize = 131072;

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
        http_log_state: Arc<HttpLogState>, cancel_notifier: Arc<Notify>, _local_port: u16,
    ) -> anyhow::Result<()> {
        let request_id = Arc::new(Mutex::new(None));

        let mut client_conn_guard = client_conn.lock().await;
        client_conn_guard.set_nodelay(true)?;
        let (mut client_reader, mut client_writer) = tokio::io::split(&mut *client_conn_guard);

        let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

        let client_to_upstream = self.handle_client_to_upstream(
            &mut client_reader,
            &mut upstream_writer,
            self.logger.clone(),
            &http_log_state,
            Arc::clone(&request_id),
            cancel_notifier.clone(),
        );

        let upstream_to_client = self.handle_upstream_to_client(
            &mut upstream_reader,
            &mut client_writer,
            self.logger.clone(),
            &http_log_state,
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
        http_log_state: &HttpLogState, request_id: Arc<Mutex<Option<String>>>,
        cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut timeout_duration = Duration::from_secs(600);
        let mut request_buffer = Vec::new();

        let logging_enabled = match http_log_state.get_http_logs(self.config_id).await {
            Ok(enabled) => enabled,
            Err(e) => {
                error!("Failed to check HTTP logging state: {:?}", e);
                false
            }
        };

        // Only proceed with logging if both enabled and logger is available
        let should_log = logging_enabled && logger.is_some();
        if should_log {
            debug!("HTTP logging is enabled for this connection");
        }

        loop {
            tokio::select! {
                n = timeout(timeout_duration, client_reader.read(&mut buffer)) => {
                    let n = match n {
                        Ok(Ok(n)) => n,
                        Ok(Err(e)) => {
                            error!("Error reading from client: {:?}", e);
                            return Err(e.into());
                        }
                        Err(_) => {
                            error!("Timeout reading from client");
                            return Err(anyhow::anyhow!("Timeout reading from client"));
                        }
                    };

                    if n == 0 {
                        break;
                    }

                    debug!("Read {} bytes from client", n);
                    request_buffer.extend_from_slice(&buffer[..n]);

                    // Only log if HTTP logging is enabled and we have a logger
                    if should_log {
                        if let Some(logger) = &logger {
                            let mut req_id_guard = request_id.lock().await;
                            let new_request_id = logger.log_request(request_buffer.clone().into()).await;
                            debug!("Generated new request ID: {}", new_request_id);
                            *req_id_guard = Some(new_request_id);
                        }
                    }

                    if let Err(e) = upstream_writer.write_all(&request_buffer).await {
                        error!("Error writing to upstream: {:?}", e);
                        return Err(e.into());
                    }
                    request_buffer.clear();
                },
                _ = cancel_notifier.notified() => {
                    debug!("Client to upstream task cancelled");
                    break;
                }
            }

            timeout_duration = Duration::from_secs(600);
        }

        if let Err(e) = upstream_writer.shutdown().await {
            error!("Error shutting down upstream writer: {:?}", e);
        }

        Ok(())
    }

    async fn handle_upstream_to_client<'a>(
        &'a self, upstream_reader: &'a mut (impl AsyncReadExt + Unpin),
        client_writer: &'a mut (impl AsyncWriteExt + Unpin), logger: Option<Logger>,
        http_log_state: &HttpLogState, request_id: Arc<Mutex<Option<String>>>,
        cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut timeout_duration = Duration::from_secs(600);
        let mut response_buffer = Vec::new();

        let mut is_chunked = false;
        let mut found_end_marker = false;
        let mut total_chunks_received = 0;

        let mut current_response_id: Option<String> = None;
        let mut current_response_logged = false;

        let mut first_chunk_time: Option<tokio::time::Instant> = None;
        let mut force_log_time: Option<tokio::time::Instant> = None;

        let logging_enabled = match http_log_state.get_http_logs(self.config_id).await {
            Ok(enabled) => enabled,
            Err(e) => {
                error!("Failed to check HTTP logging state: {:?}", e);
                false
            }
        };

        let should_log = logging_enabled && logger.is_some();
        if should_log {
            debug!("HTTP logging is enabled for this connection");
        }

        loop {
            tokio::select! {
                n = timeout(timeout_duration, upstream_reader.read(&mut buffer)) => {
                    let n = match n {
                        Ok(Ok(n)) => n,
                        Ok(Err(e)) => {
                            error!("Error reading from upstream: {:?}", e);
                            return Err(e.into());
                        }
                        Err(_) => {
                            error!("Timeout reading from upstream");
                            return Err(anyhow::anyhow!("Timeout reading from upstream"));
                        }
                    };

                    if n == 0 {
                        if !response_buffer.is_empty() && !current_response_logged && should_log {
                            if let Some(logger) = &logger {
                                let req_id_guard = request_id.lock().await;
                                if let Some(req_id) = &*req_id_guard {
                                    debug!("Connection closed, logging final response data for request ID: {}", req_id);
                                    logger
                                        .log_response(response_buffer.clone().into(), req_id.clone())
                                        .await;
                                }
                                drop(req_id_guard);
                            }
                        }
                        break;
                    }

                    debug!("Read {} bytes from upstream", n);

                    let req_id_guard = request_id.lock().await;
                    let current_req_id = req_id_guard.clone();
                    drop(req_id_guard);

                    let is_new_response = match (&current_response_id, &current_req_id) {
                        (Some(current_id), Some(req_id)) => current_id != req_id,
                        (None, Some(_)) => true,
                        _ => false
                    };

                    if is_new_response {
                        debug!("Detected new response for request ID: {:?}", current_req_id);
                        response_buffer.clear();
                        is_chunked = false;
                        found_end_marker = false;
                        total_chunks_received = 0;
                        current_response_logged = false;
                        current_response_id = current_req_id.clone();
                        first_chunk_time = Some(tokio::time::Instant::now());
                        force_log_time = None;
                    }

                    if response_buffer.is_empty() && n > 0 {
                        if first_chunk_time.is_none() {
                            first_chunk_time = Some(tokio::time::Instant::now());
                        }

                        is_chunked = kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::detect_chunked_encoding(&buffer[..n]);
                        if is_chunked {
                            debug!("Detected chunked encoding in response");
                        }
                    }

                    kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::process_chunk(
                        &buffer[..n],
                        is_chunked,
                        &mut found_end_marker,
                        &mut total_chunks_received
                    );

                    response_buffer.extend_from_slice(&buffer[..n]);

                    if !current_response_logged && should_log {
                        if let Some(logger) = &logger {
                            let mut should_log = false;

                            if is_chunked && found_end_marker && force_log_time.is_none() {
                                debug!("Found end marker for chunked response after {} chunks", total_chunks_received);
                                let delay = if !response_buffer.is_empty() &&
                                             response_buffer.len() < 40_000 &&
                                             total_chunks_received < 20 {
                                    debug!("Setting delayed logging for chunked response to collect all data");
                                    tokio::time::Instant::now() + tokio::time::Duration::from_millis(50)
                                } else {
                                    debug!("Forcing immediate logging of chunked response");
                                    tokio::time::Instant::now()
                                };

                                force_log_time = Some(delay);

                                should_log = delay <= tokio::time::Instant::now();
                            }

                            if is_chunked {
                                if let Some(force_time) = force_log_time {
                                    let now = tokio::time::Instant::now();
                                    if now >= force_time {
                                        let needs_more_time = is_chunked &&
                                            response_buffer.windows(18).any(|w| w == b"Content-Encoding: gzip" ||
                                                                             w == b"content-encoding: gzip") &&
                                            total_chunks_received < 20 &&
                                            now.saturating_duration_since(force_time) < tokio::time::Duration::from_millis(30);

                                        if needs_more_time {
                                            debug!("Temporarily delaying gzipped chunked response logging to ensure all data arrived");
                                        } else {
                                            debug!("Logging chunked response after waiting for additional chunks (chunks: {})",
                                                  total_chunks_received);
                                            should_log = true;
                                        }
                                    }
                                } else if let Some(first_time) = first_chunk_time {
                                    let elapsed = first_time.elapsed();
                                    if elapsed.as_secs() > 5 {
                                        debug!("Logging chunked response after {}s timeout", elapsed.as_secs());
                                        should_log = true;
                                    }
                                }
                            } else {
                                should_log = kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::check_content_length_match(&response_buffer);

                                if !should_log {
                                    should_log = kftray_http_logs::http_response_analyzer::HttpResponseAnalyzer::is_websocket_upgrade(&response_buffer);
                                }
                            }

                            if should_log && current_response_id.is_some() && !current_response_logged {
                                let response_id = current_response_id.as_ref().unwrap().clone();
                                debug!("Logging response for request ID: {} (chunked: {}, found_end: {})",
                                      response_id, is_chunked, found_end_marker);

                                current_response_logged = true;

                                let buffer_for_logging = response_buffer.clone();

                                logger
                                    .log_response(buffer_for_logging.into(), response_id.clone())
                                    .await;

                                debug!("Response successfully logged for ID: {}", response_id);

                                let can_clear_buffer = found_end_marker || !is_chunked;
                                if can_clear_buffer {
                                    debug!("Response fully logged, resetting buffer for next response");
                                    response_buffer.clear();
                                    is_chunked = false;
                                    found_end_marker = false;
                                    total_chunks_received = 0;
                                    first_chunk_time = None;
                                    force_log_time = None;
                                } else {
                                    debug!("Keeping buffer for potential additional data");
                                }
                            }
                        }
                    }

                    if let Err(e) = client_writer.write_all(&buffer[..n]).await {
                        error!("Error writing to client: {:?}", e);
                        return Err(e.into());
                    }
                },
                _ = cancel_notifier.notified() => {
                    debug!("Upstream to client task cancelled");
                    break;
                }
            }

            timeout_duration = Duration::from_secs(600);
        }

        if let Err(e) = client_writer.shutdown().await {
            error!("Error shutting down client writer: {:?}", e);
        }

        Ok(())
    }
}
