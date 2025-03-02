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
        &self, buffer: &[u8], n: usize, complete_response: &mut Vec<u8>, already_logged: &mut bool,
        logging_enabled: &mut bool, is_chunked: &mut bool, http_log_state: &HttpLogState,
    ) -> Result<()> {
        match http_log_state.get_http_logs(self.config_id).await {
            Ok(true) => {
                *logging_enabled = true;
                *already_logged = true;

                complete_response.extend_from_slice(&buffer[..n]);

                if complete_response.len() >= 16 {
                    *is_chunked = HttpResponseAnalyzer::detect_chunked_encoding(complete_response);
                    if *is_chunked {
                        trace!("Detected chunked transfer encoding in response - will wait for all chunks");
                    }
                } else {
                    trace!("Initial response too small to determine encoding type ({}B), will check on subsequent chunks",
                         complete_response.len());
                }

                if let Some(headers_end) = find_headers_end(complete_response) {
                    let headers = &complete_response[..headers_end];
                    if let Ok(headers_str) = std::str::from_utf8(headers) {
                        let h_lower = headers_str.to_lowercase();
                        if h_lower.contains("upgrade: websocket")
                            && h_lower.contains("connection: upgrade")
                        {
                            debug!("Initial detection of WebSocket upgrade response");

                            *is_chunked = false;
                        }
                    }
                }

                Ok(())
            }
            Ok(false) => {
                *already_logged = true;
                Ok(())
            }
            Err(e) => {
                error!("Failed to check HTTP logging state: {:?}", e);
                *already_logged = true;
                Err(e)
            }
        }
    }

    pub async fn process_response_chunk(
        &self, buffer: &[u8], n: usize, complete_response: &mut Vec<u8>, is_chunked: &mut bool,
        found_end_marker: &mut bool, total_chunks_received: &mut usize,
        response_logged: &Arc<Mutex<bool>>, request_id: &Arc<Mutex<Option<String>>>,
        logger: &Option<HttpLogger>,
    ) -> Result<()> {
        complete_response.extend_from_slice(&buffer[..n]);

        let response_size = complete_response.len();
        if response_size > 500_000 && *total_chunks_received % 10 == 0 {
            debug!(
                "Large response accumulating: {}B after {} chunks",
                response_size, *total_chunks_received
            );
        }

        if !*is_chunked && complete_response.len() >= 64 {
            let detected_chunked = HttpResponseAnalyzer::detect_chunked_encoding(complete_response);
            if detected_chunked {
                debug!("Detected chunked encoding in subsequent response data");
                *is_chunked = true;
            }
        }

        HttpResponseAnalyzer::process_chunk(
            &buffer[..n],
            *is_chunked,
            found_end_marker,
            total_chunks_received,
        );

        let first_chunk_time = if *total_chunks_received <= 1 {
            Some(Instant::now())
        } else {
            None
        };

        self.check_and_log_complete_response(
            complete_response,
            *is_chunked,
            *found_end_marker,
            response_logged,
            request_id,
            logger,
            first_chunk_time,
        )
        .await?;

        Ok(())
    }

    pub async fn check_and_log_complete_response(
        &self, complete_response: &mut Vec<u8>, is_chunked: bool, found_end_marker: bool,
        response_logged: &Arc<Mutex<bool>>, request_id: &Arc<Mutex<Option<String>>>,
        logger: &Option<HttpLogger>, first_chunk_time: Option<Instant>,
    ) -> Result<()> {
        let mut response_logged_guard = response_logged.lock().await;

        if !complete_response.is_empty() && !*response_logged_guard {
            let is_websocket_upgrade =
                if let Some(headers_end) = find_headers_end(complete_response) {
                    let headers = &complete_response[..headers_end];
                    let headers_str = std::str::from_utf8(headers).unwrap_or("");

                    headers_str.to_lowercase().contains("upgrade: websocket")
                        && headers_str.to_lowercase().contains("connection: upgrade")
                        && headers_str.to_lowercase().contains("sec-websocket-accept:")
                } else {
                    false
                };

            if is_websocket_upgrade {
                debug!("WebSocket upgrade response detected - logging immediately");
                *response_logged_guard = true;

                drop(response_logged_guard);

                self.log_response(complete_response, response_logged, request_id, logger)
                    .await?;

                return Ok(());
            }

            let ready_by_content = self.analyzer.is_ready_for_logging_with_config(
                complete_response,
                is_chunked,
                found_end_marker,
            );

            let ready_by_time = if let Some(start_time) = first_chunk_time {
                self.check_time_based_logging(complete_response, start_time)
            } else {
                false
            };

            if ready_by_content || ready_by_time {
                *response_logged_guard = true;

                drop(response_logged_guard);

                self.log_response(complete_response, response_logged, request_id, logger)
                    .await?;

                return Ok(());
            }
        }

        drop(response_logged_guard);
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
                formatted.push_str("\n");
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
            formatted.push_str("\n");

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
        &self, complete_response: &mut Vec<u8>, _response_logged: &Arc<Mutex<bool>>,
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
        &self, complete_response: &Vec<u8>, logging_enabled: bool,
        response_logged: &Arc<Mutex<bool>>, request_id: &Arc<Mutex<Option<String>>>,
        logger: &Option<HttpLogger>,
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
        let mut complete_response = Vec::new();
        let mut already_logged = false;
        let mut logging_enabled = false;
        let mut is_chunked = false;

        handler
            .check_response_logging_status(
                buffer,
                buffer.len(),
                &mut complete_response,
                &mut already_logged,
                &mut logging_enabled,
                &mut is_chunked,
                &http_log_state,
            )
            .await
            .unwrap();

        assert!(already_logged);
        assert!(logging_enabled);
        assert!(is_chunked);
        assert_eq!(complete_response, buffer);
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
