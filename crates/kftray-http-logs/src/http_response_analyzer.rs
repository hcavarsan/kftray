use std::sync::Arc;

use tracing::debug;
use tracing::trace;
pub const DEFAULT_MIN_VALID_HEADERS_SIZE: usize = 16;

#[derive(Debug, Clone)]
pub struct ResponseAnalyzerConfig {
    pub min_headers_size: usize,
}

impl Default for ResponseAnalyzerConfig {
    fn default() -> Self {
        Self {
            min_headers_size: DEFAULT_MIN_VALID_HEADERS_SIZE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponseAnalyzer {
    config: Arc<ResponseAnalyzerConfig>,
}

impl Default for HttpResponseAnalyzer {
    fn default() -> Self {
        Self::new(ResponseAnalyzerConfig::default())
    }
}

impl HttpResponseAnalyzer {
    pub fn new(config: ResponseAnalyzerConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub fn config(&self) -> &ResponseAnalyzerConfig {
        &self.config
    }

    pub fn detect_chunked_encoding(response_data: &[u8]) -> bool {
        if let Some(headers_end) = find_headers_end(response_data) {
            let header_section = &response_data[..headers_end];
            return std::str::from_utf8(header_section)
                .map(|h| h.to_lowercase().contains("transfer-encoding: chunked"))
                .unwrap_or(false);
        }
        false
    }

    pub fn check_content_length_match(response_data: &[u8]) -> bool {
        if let Some(headers_end) = find_headers_end(response_data) {
            let headers = &response_data[..headers_end];
            if let Some(content_length) = parse_content_length(headers) {
                let body_size = response_data.len() - headers_end;
                return body_size >= content_length;
            }
        }
        false
    }

    pub fn has_chunked_end_marker(chunk_data: &[u8]) -> bool {
        let standard_markers = chunk_data.windows(5).any(|w| w == b"0\r\n\r\n");

        let leading_crlf_markers = chunk_data.windows(7).any(|w| w == b"\r\n0\r\n\r\n");

        let with_trailers = if let Some(pos) = chunk_data.windows(3).position(|w| w == b"0\r\n") {
            let end_pos = pos + 3;
            if end_pos < chunk_data.len() {
                chunk_data[end_pos..].windows(4).any(|w| w == b"\r\n\r\n")
            } else {
                false
            }
        } else {
            false
        };

        standard_markers || leading_crlf_markers || with_trailers
    }

    pub fn is_websocket_upgrade(response_data: &[u8]) -> bool {
        if let Some(headers_end) = find_headers_end(response_data) {
            let header_section = &response_data[..headers_end];
            return std::str::from_utf8(header_section)
                .map(|h| {
                    let h_lower = h.to_lowercase();
                    h_lower.contains("upgrade: websocket")
                        && h_lower.contains("connection: upgrade")
                        && h_lower.contains("sec-websocket-accept:")
                })
                .unwrap_or(false);
        }
        false
    }

    pub fn appears_complete(
        &self, response_data: &[u8], is_chunked: bool, found_end_marker: bool,
    ) -> bool {
        if Self::is_websocket_upgrade(response_data) {
            trace!("Detected WebSocket upgrade response - considering complete for logging");
            return true;
        }

        if is_chunked {
            return found_end_marker;
        }

        Self::check_content_length_match(response_data)
    }

    pub fn is_ready_for_logging(
        response_data: &[u8], is_chunked: bool, found_end_marker: bool,
    ) -> bool {
        HttpResponseAnalyzer::default().is_ready_for_logging_with_config(
            response_data,
            is_chunked,
            found_end_marker,
        )
    }

    pub fn is_ready_for_logging_with_config(
        &self, response_data: &[u8], is_chunked: bool, found_end_marker: bool,
    ) -> bool {
        if let Some(headers_end) = find_headers_end(response_data) {
            let headers = &response_data[..headers_end];
            if let Ok(headers_str) = std::str::from_utf8(headers) {
                let h_lower = headers_str.to_lowercase();
                if h_lower.contains("upgrade: websocket")
                    && h_lower.contains("connection: upgrade")
                    && h_lower.contains("sec-websocket-accept:")
                {
                    trace!("WebSocket upgrade response fully confirmed with all necessary headers - complete");
                    return true;
                }
            }

            if is_chunked {
                if found_end_marker {
                    trace!("Chunked response with end marker detected - complete");
                    return true;
                }

                if response_data.len() > 100_000_000 {
                    debug!("Very large chunked response ({}B) - forcing log despite missing end marker",
                          response_data.len());
                    return true;
                }

                return false;
            }

            let headers = &response_data[..headers_end];
            if let Some(content_length) = parse_content_length(headers) {
                let body_size = response_data.len() - (headers_end + 4);

                if body_size == content_length {
                    trace!("Response with exact Content-Length match - complete");
                    return true;
                }

                if body_size >= content_length {
                    trace!("Response with sufficient Content-Length - complete");
                    return true;
                }

                return false;
            }

            let headers_str = std::str::from_utf8(headers).unwrap_or("");
            if headers_str.to_lowercase().contains("connection: close") {
                trace!("Response with Connection: close header - complete");
                return true;
            }

            if headers_str.starts_with("HTTP/1.0")
                && !headers_str.to_lowercase().contains("content-length:")
            {
                trace!("HTTP/1.0 response without Content-Length - complete");
                return true;
            }

            if let Some(status_code) = Self::extract_status_code(headers_str) {
                if (100..200).contains(&status_code) || status_code == 204 || status_code == 304 {
                    trace!(
                        "Response with no-body status code {} - complete",
                        status_code
                    );
                    return true;
                }
            }
        }

        false
    }

    fn extract_status_code(header_str: &str) -> Option<u16> {
        let lines: Vec<&str> = header_str.split("\r\n").collect();
        if lines.is_empty() {
            return None;
        }

        let status_line = lines[0];
        let parts: Vec<&str> = status_line.split_whitespace().collect();
        if parts.len() < 2 {
            return None;
        }

        parts[1].parse::<u16>().ok()
    }

    pub fn process_chunk(
        chunk_data: &[u8], is_chunked: bool, found_end_marker: &mut bool,
        total_chunks_received: &mut usize,
    ) {
        if is_chunked && !*found_end_marker {
            *total_chunks_received += 1;

            if Self::has_chunked_end_marker(chunk_data) {
                *found_end_marker = true;
                trace!(
                    "Found end marker for chunked response after {} chunks, {}B total",
                    total_chunks_received,
                    chunk_data.len()
                );
            }
        }
    }

    pub fn is_multipart_response(headers: &[u8]) -> bool {
        if let Ok(headers_str) = std::str::from_utf8(headers) {
            return headers_str
                .to_lowercase()
                .contains("content-type: multipart/");
        }
        false
    }

    pub fn is_streaming_response(headers: &[u8]) -> bool {
        if let Ok(headers_str) = std::str::from_utf8(headers) {
            let h_lower = headers_str.to_lowercase();
            return h_lower.contains("content-type: text/event-stream")
                || h_lower.contains("content-type: application/x-ndjson")
                || (h_lower.contains("content-type: application/json")
                    && h_lower.contains("transfer-encoding: chunked"));
        }
        false
    }
}

fn find_headers_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_content_length(headers: &[u8]) -> Option<usize> {
    std::str::from_utf8(headers)
        .ok()
        .and_then(|h| h.to_lowercase().find("content-length:"))
        .and_then(|pos| {
            let remaining = &headers[pos + 15..];
            let end_pos = remaining
                .iter()
                .position(|&b| b == b'\r')
                .unwrap_or(remaining.len());
            std::str::from_utf8(&remaining[..end_pos])
                .ok()
                .and_then(|s| s.trim().parse::<usize>().ok())
        })
}

pub use DEFAULT_MIN_VALID_HEADERS_SIZE as MIN_VALID_HEADERS_SIZE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_chunked_encoding() {
        let chunked_response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\ndata";
        let non_chunked_response = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\ndata";

        assert!(HttpResponseAnalyzer::detect_chunked_encoding(
            chunked_response
        ));
        assert!(!HttpResponseAnalyzer::detect_chunked_encoding(
            non_chunked_response
        ));
    }

    #[test]
    fn test_check_content_length_match() {
        let complete_response = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ndata";
        let incomplete_response = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\ndata";

        assert!(HttpResponseAnalyzer::check_content_length_match(
            complete_response
        ));
        assert!(!HttpResponseAnalyzer::check_content_length_match(
            incomplete_response
        ));
    }

    #[test]
    fn test_has_chunked_end_marker() {
        let with_end_marker = b"5\r\nhello\r\n0\r\n\r\n";
        let without_end_marker = b"5\r\nhello\r\n";

        assert!(HttpResponseAnalyzer::has_chunked_end_marker(
            with_end_marker
        ));
        assert!(!HttpResponseAnalyzer::has_chunked_end_marker(
            without_end_marker
        ));
    }

    #[test]
    fn test_is_websocket_upgrade() {
        let websocket_response = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n";
        let normal_response = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\ndata";

        assert!(HttpResponseAnalyzer::is_websocket_upgrade(
            websocket_response
        ));
        assert!(!HttpResponseAnalyzer::is_websocket_upgrade(normal_response));
    }

    #[test]
    fn test_parse_content_length() {
        let headers = b"HTTP/1.1 200 OK\r\nContent-Length: 42\r\nContent-Type: text/plain\r\n";
        assert_eq!(parse_content_length(headers), Some(42));

        let no_cl_headers = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n";
        assert_eq!(parse_content_length(no_cl_headers), None);
    }
}
