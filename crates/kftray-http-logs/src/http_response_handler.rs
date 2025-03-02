use std::sync::Arc;

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::Mutex;
use tokio::time::{
    Duration,
    Instant,
};
use tracing::{
    debug,
    error,
    trace,
    warn,
};

fn find_headers_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

use crate::http_response_analyzer::{
    HttpResponseAnalyzer,
    ResponseAnalyzerConfig,
};
use crate::models::HttpLogState;
use crate::HttpLogger;

pub const DEFAULT_MIN_LOG_SYNC_MS: u64 = 50;

pub struct ResponseLoggingState {
    pub complete_response: Vec<u8>,
    pub already_logged: bool,
    pub logging_enabled: bool,
    pub is_chunked: bool,
}

impl ResponseLoggingState {
    pub fn new() -> Self {
        Self {
            complete_response: Vec::new(),
            already_logged: false,
            logging_enabled: false,
            is_chunked: false,
        }
    }
}

pub struct ResponseChunkContext {
    pub complete_response: Vec<u8>,
    pub is_chunked: bool,
    pub found_end_marker: bool,
    pub total_chunks_received: usize,
    pub response_logged: Arc<Mutex<bool>>,
    pub request_id: Arc<Mutex<Option<String>>>,
}

impl ResponseChunkContext {
    pub fn new(response_logged: Arc<Mutex<bool>>, request_id: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            complete_response: Vec::new(),
            is_chunked: false,
            found_end_marker: false,
            total_chunks_received: 0,
            response_logged,
            request_id,
        }
    }
}

pub struct ResponseLoggingContext {
    pub complete_response: Vec<u8>,
    pub is_chunked: bool,
    pub found_end_marker: bool,
    pub response_logged: Arc<Mutex<bool>>,
    pub request_id: Arc<Mutex<Option<String>>>,
    pub first_chunk_time: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct ResponseHandlerConfig {
    pub analyzer_config: ResponseAnalyzerConfig,
    pub min_log_sync_ms: u64,
}

impl Default for ResponseHandlerConfig {
    fn default() -> Self {
        Self {
            analyzer_config: ResponseAnalyzerConfig::default(),
            min_log_sync_ms: DEFAULT_MIN_LOG_SYNC_MS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponseHandler {
    config_id: i64,
    analyzer: HttpResponseAnalyzer,
    config: ResponseHandlerConfig,
}

impl HttpResponseHandler {
    pub fn new(config_id: i64) -> Self {
        Self::with_config(config_id, ResponseHandlerConfig::default())
    }

    pub fn with_config(config_id: i64, config: ResponseHandlerConfig) -> Self {
        Self {
            config_id,
            analyzer: HttpResponseAnalyzer::new(config.analyzer_config.clone()),
            config,
        }
    }

    pub fn config(&self) -> &ResponseHandlerConfig {
        &self.config
    }

    pub async fn check_response_logging_status(
        &self, buffer: &[u8], n: usize, state: &mut ResponseLoggingState,
        http_log_state: &HttpLogState,
    ) -> Result<()> {
        match http_log_state.get_http_logs(self.config_id).await {
            Ok(true) => {
                state.logging_enabled = true;
                state.already_logged = true;

                if n > 0 {
                    state.complete_response.extend_from_slice(&buffer[..n]);

                    if let Some(headers_end) = find_headers_end(&state.complete_response) {
                        let headers_data = &state.complete_response[..headers_end];
                        state.is_chunked = Self::is_chunked_transfer(headers_data);
                    }
                }

                Ok(())
            }
            Ok(false) => {
                state.already_logged = true;
                Ok(())
            }
            Err(e) => {
                warn!("Failed to check HTTP logging status: {}", e);
                state.already_logged = true;
                Ok(())
            }
        }
    }

    pub async fn process_response_chunk(
        &self, buffer: &[u8], n: usize, context: &mut ResponseChunkContext,
        logger: &Option<HttpLogger>,
    ) -> Result<()> {
        context.complete_response.extend_from_slice(&buffer[..n]);
        context.total_chunks_received += 1;

        let response_size = context.complete_response.len();
        trace!(
            "Processing response chunk #{}: {}B, total: {}B",
            context.total_chunks_received,
            n,
            response_size
        );

        if !context.is_chunked {
            if let Some(headers_end) = find_headers_end(&context.complete_response) {
                let headers_data = &context.complete_response[..headers_end];
                context.is_chunked = Self::is_chunked_transfer(headers_data);
            }
        }

        if context.is_chunked && !context.found_end_marker {
            context.found_end_marker =
                HttpResponseAnalyzer::has_chunked_end_marker(&context.complete_response);
            if context.found_end_marker {
                debug!(
                    "Found end marker in chunked response after {} chunks, {}B",
                    context.total_chunks_received, response_size
                );

                let mut logging_context = ResponseLoggingContext {
                    complete_response: std::mem::take(&mut context.complete_response),
                    is_chunked: context.is_chunked,
                    found_end_marker: context.found_end_marker,
                    response_logged: context.response_logged.clone(),
                    request_id: context.request_id.clone(),
                    first_chunk_time: None,
                };

                let result = self
                    .check_and_log_complete_response(&mut logging_context, logger)
                    .await;

                // Restore the complete_response
                context.complete_response = logging_context.complete_response;

                result?;
            }
        }

        Ok(())
    }

    pub async fn check_and_log_complete_response(
        &self, context: &mut ResponseLoggingContext, logger: &Option<HttpLogger>,
    ) -> Result<()> {
        let mut response_logged_guard = context.response_logged.lock().await;

        if !context.complete_response.is_empty() && !*response_logged_guard {
            let should_log = if context.is_chunked {
                context.found_end_marker
                    || self.analyzer.is_ready_for_logging_with_config(
                        &context.complete_response,
                        context.is_chunked,
                        context.found_end_marker,
                    )
            } else if let Some(start_time) = context.first_chunk_time {
                self.check_time_based_logging(&context.complete_response, start_time)
            } else {
                self.analyzer.appears_complete(
                    &context.complete_response,
                    context.is_chunked,
                    context.found_end_marker,
                )
            };

            if should_log {
                debug!(
                    "Logging complete response: {}B, chunked: {}, end marker: {}",
                    context.complete_response.len(),
                    context.is_chunked,
                    context.found_end_marker
                );

                self.log_response(
                    &mut context.complete_response,
                    &context.response_logged,
                    &context.request_id,
                    logger,
                )
                .await?;

                *response_logged_guard = true;
            }
        }

        Ok(())
    }

    fn check_time_based_logging(&self, response_data: &[u8], start_time: Instant) -> bool {
        if let Some(headers_end) = find_headers_end(response_data) {
            let headers = &response_data[..headers_end];

            if response_data.len() > headers_end + 4 {
                if let Ok(headers_str) = std::str::from_utf8(headers) {
                    let h_lower = headers_str.to_lowercase();
                    if h_lower.contains("upgrade: websocket")
                        && h_lower.contains("connection: upgrade")
                    {
                        if h_lower.contains("sec-websocket-accept:") {
                            debug!("Logging WebSocket upgrade response immediately - complete with all required headers");
                            return true;
                        } else {
                            debug!("Potential WebSocket upgrade response detected but missing accept header");
                        }
                    }
                }
            }

            if let Ok(headers_str) = std::str::from_utf8(headers) {
                let first_line = headers_str.lines().next().unwrap_or("");
                let parts: Vec<&str> = first_line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(status) = parts[1].parse::<u16>() {
                        if (100..200).contains(&status) || status == 204 || status == 304 {
                            debug!("Logging status {} response which never has a body", status);
                            return true;
                        }
                    }
                }
            }

            if std::str::from_utf8(headers)
                .map(|h| h.to_lowercase().contains("connection: close"))
                .unwrap_or(false)
            {
                debug!("Connection: close header found, logging response at connection end");
                return true;
            }

            if let Ok(headers_str) = std::str::from_utf8(headers) {
                if headers_str.starts_with("HTTP/1.0")
                    && !headers_str.to_lowercase().contains("content-length:")
                    && !headers_str
                        .to_lowercase()
                        .contains("transfer-encoding: chunked")
                {
                    debug!(
                        "HTTP/1.0 response without length indicators - logging at connection end"
                    );
                    return true;
                }
            }

            let elapsed = start_time.elapsed();
            if elapsed.as_secs() >= 10 && response_data.len() > 5000 {
                debug!("Long-lived connection detected ({}s, {}B) - logging accumulated data to prevent being stuck",
                      elapsed.as_secs(), response_data.len());
                return true;
            }

            if response_data.len() > 1_000_000 {
                debug!(
                    "Large response detected ({}B) - logging to prevent memory issues",
                    response_data.len()
                );
                return true;
            }

            if response_data.len() > 100_000 && elapsed.as_secs() > 30 {
                debug!(
                    "Long-running HTTP stream detected ({}s, {}B) - forcing log",
                    elapsed.as_secs(),
                    response_data.len()
                );
                return true;
            }
        }

        false
    }

    async fn format_response_for_logging(&self, response_data: &[u8]) -> Result<String> {
        debug!(
            "Formatting response for logging, size: {} bytes",
            response_data.len()
        );

        if let Some(headers_end) = find_headers_end(response_data) {
            debug!("Found headers end at position: {}", headers_end);

            let headers_bytes = &response_data[..headers_end];
            let body_start = headers_end + 4;
            let body = &response_data[body_start..];

            debug!(
                "Headers size: {} bytes, Body size: {} bytes",
                headers_bytes.len(),
                body.len()
            );

            let (status, headers) = match crate::parser::ResponseParser::parse(headers_bytes) {
                Ok((status, headers)) => {
                    debug!(
                        "Successfully parsed headers, status: {:?}, headers count: {}",
                        status,
                        headers.len()
                    );
                    (status, headers)
                }
                Err(e) => {
                    error!("Failed to parse headers: {:?}", e);
                    return Err(e);
                }
            };

            let mut formatted = String::new();
            if let Some(_status_code) = status {
                let status_line = match std::str::from_utf8(
                    &headers_bytes[..headers_bytes
                        .windows(2)
                        .position(|w| w == b"\r\n")
                        .unwrap_or(headers_bytes.len())],
                ) {
                    Ok(line) => {
                        debug!("Status line: {}", line);
                        line
                    }
                    Err(e) => {
                        error!("Failed to convert status line to UTF-8: {:?}", e);
                        "HTTP/1.1 200 OK"
                    }
                };
                formatted.push_str(status_line);
                formatted.push('\n');
            } else {
                debug!("No status line found, adding default");
                formatted.push_str("HTTP/1.1 200 OK\n");
            }

            for header in &headers {
                if let (Ok(name), Ok(value)) = (
                    std::str::from_utf8(header.name.as_bytes()),
                    std::str::from_utf8(header.value),
                ) {
                    formatted.push_str(&format!("{}: {}\n", name, value));
                }
            }
            formatted.push('\n');

            if !body.is_empty() {
                debug!("Processing response body with size: {} bytes", body.len());

                let is_chunked = crate::parser::RequestParser::is_chunked_transfer(&headers);
                let is_gzipped = crate::parser::RequestParser::is_gzip_encoded(&headers);
                let is_brotli = crate::parser::RequestParser::is_brotli_encoded(&headers);

                if is_chunked {
                    debug!("Response is chunked encoded");
                }
                if is_gzipped {
                    debug!("Response is gzip compressed");
                }
                if is_brotli {
                    debug!("Response is brotli compressed");
                }

                let processed_body =
                    match crate::parser::BodyParser::process_response_body(body, &headers).await {
                        Ok(processed) => {
                            debug!(
                                "Successfully processed response body: {} bytes -> {} bytes",
                                body.len(),
                                processed.len()
                            );
                            processed
                        }
                        Err(e) => {
                            debug!(
                                "Error processing response body: {:?}, using original body",
                                e
                            );
                            body.to_vec()
                        }
                    };

                let content_type = crate::parser::BodyParser::get_content_type(&headers);
                debug!("Content type for body formatting: {:?}", content_type);

                let body_formatted =
                    match crate::parser::BodyParser::format_body(&processed_body, content_type) {
                        Ok(formatted) => {
                            debug!(
                                "Successfully formatted response body: {} bytes",
                                formatted.len()
                            );
                            formatted
                        }
                        Err(e) => {
                            debug!(
                            "Error formatting response body: {:?}, trying direct string conversion",
                            e
                        );

                            if let Ok(text) = std::str::from_utf8(&processed_body) {
                                debug!("Direct string conversion succeeded");
                                text.to_string()
                            } else {
                                debug!("Direct string conversion failed, using lossy conversion");
                                String::from_utf8_lossy(&processed_body).to_string()
                            }
                        }
                    };

                formatted.push_str(&body_formatted);
            } else {
                formatted.push_str("#<empty body>");
            }

            debug!("Formatted response size: {} bytes", formatted.len());
            Ok(formatted)
        } else {
            debug!("Could not find headers end, returning raw data as string");
            Ok(String::from_utf8_lossy(response_data).to_string())
        }
    }

    pub async fn log_response(
        &self, complete_response: &mut [u8], _response_logged: &Arc<Mutex<bool>>,
        request_id: &Arc<Mutex<Option<String>>>, logger: &Option<HttpLogger>,
    ) -> Result<()> {
        if let Some(logger) = logger {
            let req_id_guard = request_id.lock().await;
            if let Some(req_id) = &*req_id_guard {
                debug!(
                    "Logging response for request ID: {}, response size: {} bytes",
                    req_id,
                    complete_response.len()
                );

                let formatted_response =
                    match self.format_response_for_logging(complete_response).await {
                        Ok(formatted) => {
                            debug!(
                                "Successfully formatted response for logging, size: {} bytes",
                                formatted.len()
                            );

                            if formatted.starts_with("HTTP/") {
                                formatted
                            } else {
                                debug!("Response doesn't start with HTTP/, adding basic header");
                                let mut enhanced = String::from("HTTP/1.1 200 OK\n\n");
                                enhanced.push_str(&formatted);
                                enhanced
                            }
                        }
                        Err(e) => {
                            error!("Failed to format response: {:?}", e);
                            let mut basic_response = String::from("HTTP/1.1 200 OK\n");
                            basic_response.push_str(&format!("X-Formatting-Error: {}\n\n", e));
                            basic_response.push_str("# Failed to format response properly\n");
                            basic_response.push_str(&format!("# Error: {}\n", e));
                            basic_response.push_str("# Raw content follows (first 1000 bytes):\n");

                            let preview_size = std::cmp::min(complete_response.len(), 1000);
                            basic_response.push_str(&String::from_utf8_lossy(
                                &complete_response[..preview_size],
                            ));
                            basic_response
                        }
                    };

                let bytes = Bytes::from(formatted_response.into_bytes());
                debug!("Sending response to logger, size: {} bytes", bytes.len());

                logger.log_response(bytes, req_id.clone()).await;
                debug!("Response successfully logged for request ID: {}", req_id);
            } else {
                error!("No request ID available for logging response");
            }
            drop(req_id_guard);
        } else {
            debug!("No logger available for logging response");
        }

        Ok(())
    }

    pub async fn handle_remaining_response_data(
        &self, complete_response: &[u8], logging_enabled: bool, response_logged: &Arc<Mutex<bool>>,
        request_id: &Arc<Mutex<Option<String>>>, logger: &Option<HttpLogger>,
    ) -> Result<()> {
        if complete_response.is_empty() {
            debug!("No response data to log at connection end");
            return Ok(());
        }

        let mut response_logged_guard = response_logged.lock().await;
        let already_logged = *response_logged_guard;

        if logging_enabled && !already_logged {
            *response_logged_guard = true;
            debug!(
                "Marking response as logged (size: {}B)",
                complete_response.len()
            );
            drop(response_logged_guard);

            let req_id_guard = request_id.lock().await;
            if let Some(req_id) = &*req_id_guard {
                debug!(
                    "Connection closing with response data (size: {} bytes) for request ID: {}",
                    complete_response.len(),
                    req_id
                );

                if let Some(log_instance) = logger.as_ref() {
                    let req_clone = req_id.clone();
                    let response_data = Bytes::copy_from_slice(complete_response);
                    let response_size = response_data.len();

                    drop(req_id_guard);

                    debug!("Logging HTTP response at connection close - this ensures all data is captured");

                    let timeout_duration = Duration::from_secs(5);
                    match tokio::time::timeout(
                        timeout_duration,
                        log_instance.log_response(response_data, req_clone),
                    )
                    .await
                    {
                        Ok(_) => {
                            debug!(
                                "Final response ({}B) successfully logged (connection end)",
                                response_size
                            );
                        }
                        Err(_) => {
                            error!("Response logging timed out after 5 seconds");
                        }
                    }
                } else {
                    drop(req_id_guard);
                    error!("No logger instance available for logging response");
                }

                let wait_time = Duration::from_millis(self.config.min_log_sync_ms * 2);
                tokio::time::sleep(wait_time).await;
                debug!("Completed final response logging before connection close");
            } else {
                drop(req_id_guard);
                debug!("No request ID found for response at connection end");
            }
        } else {
            drop(response_logged_guard);

            if logging_enabled && already_logged {
                debug!("Response already logged earlier - connection closing");
            } else if !logging_enabled {
                debug!("HTTP logging disabled for this connection");
            }
        }

        Ok(())
    }

    fn is_chunked_transfer(headers_data: &[u8]) -> bool {
        if let Ok(headers_str) = std::str::from_utf8(headers_data) {
            let h_lower = headers_str.to_lowercase();
            if h_lower.contains("transfer-encoding: chunked") {
                trace!("Detected chunked transfer encoding in response - will wait for all chunks");
                return true;
            }

            if h_lower.contains("upgrade: websocket") && h_lower.contains("connection: upgrade") {
                debug!("Initial detection of WebSocket upgrade response");
                return false;
            }
        }
        false
    }
}

pub use DEFAULT_MIN_LOG_SYNC_MS as MIN_LOG_SYNC_MS;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::HttpLogState;

    #[tokio::test]
    async fn test_check_response_logging_status() {
        let http_log_state = HttpLogState::new();
        let handler = HttpResponseHandler::new(123);

        http_log_state.set_http_logs(123, true).await.unwrap();

        let buffer = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\ndata";
        let mut state = ResponseLoggingState::new();

        handler
            .check_response_logging_status(buffer, buffer.len(), &mut state, &http_log_state)
            .await
            .unwrap();

        assert!(state.already_logged);
        assert!(state.logging_enabled);
        assert!(state.is_chunked);
        assert_eq!(state.complete_response, buffer);
    }

    #[tokio::test]
    async fn test_custom_config() {
        let config = ResponseHandlerConfig {
            min_log_sync_ms: 25,
            ..Default::default()
        };

        let handler = HttpResponseHandler::with_config(123, config);

        assert_eq!(handler.config().min_log_sync_ms, 25);

        let response_data = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\nSome data";

        assert!(handler.check_time_based_logging(response_data, Instant::now()));

        let default_handler = HttpResponseHandler::new(123);

        let regular_response = b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nSome data";

        assert!(!default_handler.check_time_based_logging(regular_response, Instant::now()));
    }
}
