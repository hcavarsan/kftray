use std::io::Read;

use anyhow::Result;
use brotli::Decompressor;
use bytes::Bytes;
use chrono::{
    DateTime,
    Utc,
};
use httparse::Header;
use tokio::task;
use tracing::{
    debug,
    trace,
};

use crate::message::LogMessage;
use crate::parser::{
    BodyParser,
    RequestParser,
    ResponseParser,
};

pub struct MessageFormatter;

impl MessageFormatter {
    pub async fn format_request(
        buffer: &Bytes, trace_id: &str, timestamp: DateTime<Utc>,
    ) -> Result<LogMessage> {
        debug!("Formatting request with trace ID: {}", trace_id);

        let mut log_entry = Self::create_metadata_header(trace_id, timestamp, None);

        let (method, path, version, headers) = RequestParser::parse(buffer)?;

        if let (Some(method), Some(path), Some(version)) = (method, path, version) {
            log_entry.push_str(&format!("{method} {path} HTTP/1.{version}\n"));

            Self::append_headers(&headers, &mut log_entry);

            log_entry.push('\n');

            if let Some(body) = RequestParser::extract_body(buffer) {
                Self::format_body(body, &headers, &mut log_entry).await?;
                Self::append_log_separator(&mut log_entry);
            } else {
                Self::append_log_separator(&mut log_entry);
            }
        }

        Ok(LogMessage::Request(log_entry))
    }

    pub async fn format_response(
        buffer: &Bytes, trace_id: &str, timestamp: DateTime<Utc>, took: i64,
    ) -> Result<LogMessage> {
        debug!("Formatting response with trace ID: {}", trace_id);

        let mut log_entry = Self::create_metadata_header(trace_id, timestamp, Some(took));

        let (status_code, headers) = ResponseParser::parse(buffer)?;

        if let Some(status) = status_code {
            log_entry.push_str(&format!(
                "HTTP/1.1 {} {}\n",
                status,
                Self::status_text(status)
            ));

            Self::append_headers(&headers, &mut log_entry);
            log_entry.push('\n');

            if let Some(body) = RequestParser::extract_body(buffer) {
                let processed_body = Self::process_response_body(body, &headers);
                Self::format_body(&processed_body, &headers, &mut log_entry).await?;
                Self::append_log_separator(&mut log_entry);
            } else {
                Self::append_log_separator(&mut log_entry);
            }
        }

        Ok(LogMessage::Response(log_entry))
    }

    pub fn format_preformatted_response(
        trace_id: &str, timestamp: DateTime<Utc>, took: i64, buffer: &Bytes,
    ) -> String {
        debug!(
            "Formatting preformatted response with trace ID: {}",
            trace_id
        );

        let mut log_entry = Self::create_metadata_header(trace_id, timestamp, Some(took));

        if buffer.len() > 5 && &buffer[0..5] == b"HTTP/" {
            if let Some(headers_end) = Self::find_headers_end(buffer) {
                let headers_bytes = &buffer[..headers_end];
                let body_bytes = &buffer[headers_end..];

                if let Ok(headers_str) = std::str::from_utf8(headers_bytes) {
                    log_entry.push_str(headers_str);
                    log_entry.push('\n');
                } else {
                    log_entry.push_str("HTTP/1.1 200 OK\n<unparseable headers>\n\n");
                }

                if !body_bytes.is_empty() {
                    let mut content_type = None;
                    let mut is_gzipped = false;
                    let mut is_chunked = false;

                    if let Ok(headers_str) = std::str::from_utf8(headers_bytes) {
                        for line in headers_str.lines() {
                            let line_lower = line.to_lowercase();
                            if line_lower.starts_with("content-type:") {
                                content_type = line.split(':').nth(1).map(|s| s.trim());
                            }
                            if line_lower.starts_with("content-encoding:")
                                && line_lower.contains("gzip")
                            {
                                is_gzipped = true;
                            }
                            if line_lower.starts_with("transfer-encoding:")
                                && line_lower.contains("chunked")
                            {
                                is_chunked = true;
                            }
                        }
                    }

                    debug!(
                        "Content-Type: {:?}, gzipped: {}, chunked: {}",
                        content_type, is_gzipped, is_chunked
                    );

                    let mut processed_body = if is_chunked {
                        debug!("Dechunking body");
                        Self::dechunk_body(body_bytes)
                    } else {
                        body_bytes.to_vec()
                    };

                    if is_gzipped {
                        debug!("Decompressing gzipped body");
                        if let Ok(decompressed) = Self::decompress_gzip(&processed_body) {
                            debug!(
                                "Successfully decompressed gzip content: {} -> {} bytes",
                                processed_body.len(),
                                decompressed.len()
                            );
                            processed_body = decompressed;
                        } else {
                            debug!("Failed to decompress gzip content");
                        }
                    }

                    let content_type_str = content_type.unwrap_or("text/plain");
                    Self::format_content(&mut log_entry, content_type_str, &processed_body);
                }
            } else if let Ok(content) = std::str::from_utf8(buffer) {
                log_entry.push_str(content);
            } else {
                log_entry.push_str("<binary content>");
            }
        } else if let Ok(content) = std::str::from_utf8(buffer) {
            log_entry.push_str(content);
        } else {
            log_entry.push_str("<binary content>");
        }

        Self::append_log_separator(&mut log_entry);
        log_entry
    }

    fn format_content(log_entry: &mut String, content_type: &str, body: &[u8]) {
        if body.is_empty() {
            return;
        }

        let content_type_lower = content_type.to_lowercase();

        if content_type_lower.contains("javascript") {
            debug!("Formatting as JavaScript");
            log_entry.push_str(&String::from_utf8_lossy(body));
            return;
        }

        if content_type_lower.contains("json") {
            debug!("Attempting to format as JSON");
            if let Ok(text) = std::str::from_utf8(body) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(text.trim()) {
                    if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                        log_entry.push_str(&pretty);
                        return;
                    }
                }
                log_entry.push_str(text);
            } else {
                log_entry.push_str(&String::from_utf8_lossy(body));
            }
            return;
        }

        match content_type_lower {
            t if t.contains("html") => {
                debug!("Formatting as HTML");
                log_entry.push_str(&String::from_utf8_lossy(body));
            }
            t if t.starts_with("text/") => {
                debug!("Formatting as text");
                log_entry.push_str(&String::from_utf8_lossy(body));
            }
            t if t.contains("xml") => {
                debug!("Formatting as XML");
                log_entry.push_str(&String::from_utf8_lossy(body));
            }
            _ => {
                if body
                    .iter()
                    .any(|&b| b < 32 && b != b'\n' && b != b'\r' && b != b'\t')
                {
                    debug!("Detected binary content");
                    log_entry.push_str(&format!("# <binary content: {} bytes>", body.len()));
                } else {
                    log_entry.push_str(&String::from_utf8_lossy(body));
                }
            }
        }
    }

    fn decompress_gzip(data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 2 || data[0] != 0x1f || data[1] != 0x8b {
            return Err(anyhow::anyhow!("Not a valid gzip header"));
        }

        debug!("Decompressing gzip data of {} bytes", data.len());

        let mut decoder = flate2::read::GzDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;

        if decompressed.is_empty() {
            return Err(anyhow::anyhow!("Decompression produced empty result"));
        }

        Ok(decompressed)
    }

    fn dechunk_body(body: &[u8]) -> Vec<u8> {
        if body.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();
        let mut pos = 0;

        while pos < body.len() && (body[pos] == b' ' || body[pos] == b'\r' || body[pos] == b'\n') {
            pos += 1;
        }

        while pos < body.len() {
            let chunk_size_end_pos = match body[pos..].iter().position(|&b| b == b'\r') {
                Some(p) => pos + p,
                None => {
                    debug!("No CRLF found in chunked data, returning partial result");
                    return result;
                }
            };

            if chunk_size_end_pos + 1 >= body.len() || body[chunk_size_end_pos + 1] != b'\n' {
                debug!("Invalid chunk format (missing LF after CR)");
                return result;
            }

            let chunk_size_str = match std::str::from_utf8(&body[pos..chunk_size_end_pos]) {
                Ok(s) => s.trim(),
                Err(_) => {
                    debug!("Invalid UTF-8 in chunk size");
                    return result;
                }
            };

            let chunk_size_str = chunk_size_str.split(';').next().unwrap_or("").trim();

            let chunk_size = match usize::from_str_radix(chunk_size_str, 16) {
                Ok(size) => size,
                Err(e) => {
                    debug!("Failed to parse chunk size '{}': {}", chunk_size_str, e);
                    return result;
                }
            };

            if chunk_size == 0 {
                debug!("Found end chunk marker");
                break;
            }

            let chunk_data_start = chunk_size_end_pos + 2;

            if chunk_data_start + chunk_size + 2 > body.len() {
                debug!(
                    "Incomplete chunk: need {} bytes, have {}",
                    chunk_size,
                    body.len() - chunk_data_start
                );

                if chunk_data_start < body.len() {
                    result.extend_from_slice(&body[chunk_data_start..]);
                }
                break;
            }

            result.extend_from_slice(&body[chunk_data_start..(chunk_data_start + chunk_size)]);

            pos = chunk_data_start + chunk_size + 2;
        }

        result
    }

    fn create_metadata_header(
        trace_id: &str, timestamp: DateTime<Utc>, took_ms: Option<i64>,
    ) -> String {
        let initial_capacity = 128 + trace_id.len() + 64;
        let mut header = String::with_capacity(initial_capacity);

        header.push_str("\n# ----------------------------------------\n");
        header.push_str("# Trace ID: ");
        header.push_str(trace_id);
        header.push('\n');

        if let Some(took) = took_ms {
            header.push_str("# Response at: ");
            header.push_str(&timestamp.to_rfc3339());
            header.push('\n');
            header.push_str("# Took: ");
            header.push_str(&took.to_string());
            header.push_str(" ms\n");
        } else {
            header.push_str("# Request at: ");
            header.push_str(&timestamp.to_rfc3339());
            header.push('\n');
        }

        header
    }

    fn append_headers(headers: &[Header<'_>], log_entry: &mut String) {
        let additional_capacity = headers
            .iter()
            .map(|h| h.name.len() + h.value.len() + 3)
            .sum::<usize>();

        let current_len = log_entry.len();
        if log_entry.capacity() < current_len + additional_capacity {
            log_entry.reserve(additional_capacity);
        }

        for header in headers {
            log_entry.push_str(header.name);
            log_entry.push_str(": ");
            if let Ok(value) = std::str::from_utf8(header.value) {
                log_entry.push_str(value);
            }
            log_entry.push('\n');
        }
    }

    fn process_response_body(body: &[u8], headers: &[Header<'_>]) -> Vec<u8> {
        let is_chunked = RequestParser::is_chunked_transfer(headers);

        if !is_chunked {
            return body.to_vec();
        }

        trace!(
            "Processing chunked transfer encoded body of {} bytes",
            body.len()
        );

        let processed_body = RequestParser::process_chunked_body(body);

        if processed_body.len() != body.len() {
            trace!(
                "Chunked body processed: {} bytes original, {} bytes after dechunking",
                body.len(),
                processed_body.len()
            );
        }

        processed_body
    }

    fn append_log_separator(log_entry: &mut String) {
        log_entry.push_str("\n\n###\n");
    }

    async fn format_body(
        body: &[u8], headers: &[Header<'_>], log_entry: &mut String,
    ) -> Result<()> {
        let content_length = RequestParser::get_content_length(headers);

        if BodyParser::is_content_too_large(content_length) {
            log_entry.push_str("<content too large>");
            return Ok(());
        }

        let body_content = match Self::try_decompress_body(body, headers).await {
            Ok(content) => content,
            Err(e) => Self::handle_decompression_error(body, headers, log_entry, e)?,
        };

        let content_type = Self::extract_content_type(headers);
        let formatted_body = BodyParser::format_body(&body_content, content_type)?;
        log_entry.push_str(&formatted_body);

        Ok(())
    }

    fn extract_content_type<'a>(headers: &'a [Header<'_>]) -> Option<&'a str> {
        headers.iter().find_map(|h| {
            if h.name.eq_ignore_ascii_case("content-type") {
                std::str::from_utf8(h.value).ok()
            } else {
                None
            }
        })
    }

    fn handle_decompression_error(
        body: &[u8], headers: &[Header<'_>], log_entry: &mut String, error: anyhow::Error,
    ) -> Result<Vec<u8>> {
        log_entry.push_str(&format!(
            "<!-- Debug: Failed to process content: {error} -->"
        ));

        if let Some(content_type) = Self::extract_content_type(headers) {
            if content_type.contains("javascript") {
                Self::insert_js_placeholder(content_type, log_entry);
                return Ok(Vec::new());
            }
        }

        if let Ok(body_str) = std::str::from_utf8(body) {
            if !body_str.is_empty() {
                log_entry.push_str(body_str);
                return Ok(Vec::new());
            }
        }

        Ok(body.to_vec())
    }

    fn insert_js_placeholder(content_type: &str, log_entry: &mut String) {
        log_entry.push_str("/**\n * This JavaScript content could not be fully decompressed.\n * Original response was compressed with gzip and chunked transfer encoding.\n * \n * The HTTP logging system captured the content but could not properly decompress it.\n * This is a placeholder to maintain HTTP log file compatibility with REST clients.\n */\n\n// Content metadata:\n// Content-Type: ");
        log_entry.push_str(content_type);
        log_entry.push_str("\n// Content-Encoding: gzip\n// Transfer-Encoding: chunked\n\n");
        log_entry
            .push_str("console.log('Content placeholder for compressed and chunked JavaScript');");
    }

    async fn try_decompress_body(body: &[u8], headers: &[Header<'_>]) -> Result<Vec<u8>> {
        if body.is_empty() || body.len() < 16 {
            return Ok(body.to_vec());
        }

        let content_encoding = headers.iter().find_map(|h| {
            if h.name.eq_ignore_ascii_case("content-encoding") {
                std::str::from_utf8(h.value).ok()
            } else {
                None
            }
        });

        if content_encoding.is_none() {
            return Ok(body.to_vec());
        }

        let content_encoding = content_encoding.unwrap().to_lowercase();

        let mut current_data = body.to_vec();

        let encodings: Vec<&str> = content_encoding
            .split(',')
            .map(|s| s.trim())
            .rev()
            .collect();

        for encoding in encodings {
            match encoding {
                "gzip" => {
                    debug!("Decompressing gzip encoded data");
                    match Self::decompress_gzip_async(&current_data).await {
                        Ok(decompressed) if !decompressed.is_empty() => {
                            debug!(
                                "Successfully decompressed gzip data: {} bytes -> {} bytes",
                                current_data.len(),
                                decompressed.len()
                            );
                            current_data = decompressed;
                        }
                        Ok(_) => {
                            debug!("Gzip decompression produced empty result, keeping original");
                        }
                        Err(e) => {
                            debug!("Gzip decompression failed: {:?}, keeping original", e);
                        }
                    }
                }
                "br" => {
                    debug!("Decompressing brotli encoded data");
                    match Self::try_decompress_brotli(&current_data).await {
                        Ok(decompressed) if !decompressed.is_empty() => {
                            debug!(
                                "Successfully decompressed brotli data: {} bytes -> {} bytes",
                                current_data.len(),
                                decompressed.len()
                            );
                            current_data = decompressed;
                        }
                        Ok(_) => {
                            debug!("Brotli decompression produced empty result, keeping original");
                        }
                        Err(e) => {
                            debug!("Brotli decompression failed: {:?}, keeping original", e);
                        }
                    }
                }
                "deflate" => {
                    debug!("Decompressing deflate encoded data");
                    match Self::try_decompress_deflate(&current_data).await {
                        Ok(decompressed) if !decompressed.is_empty() => {
                            debug!(
                                "Successfully decompressed deflate data: {} bytes -> {} bytes",
                                current_data.len(),
                                decompressed.len()
                            );
                            current_data = decompressed;
                        }
                        Ok(_) => {
                            debug!("Deflate decompression produced empty result, keeping original");
                        }
                        Err(e) => {
                            debug!("Deflate decompression failed: {:?}, keeping original", e);
                        }
                    }
                }
                "identity" => {
                    debug!("Identity encoding - no decompression needed");
                }
                _ => {
                    debug!("Unknown content encoding: {}, keeping original", encoding);
                }
            }
        }

        Ok(current_data)
    }

    async fn decompress_gzip_async(data: &[u8]) -> Result<Vec<u8>> {
        let data_owned = data.to_vec();
        task::spawn_blocking(move || {
            let mut decoder = flate2::read::GzDecoder::new(&data_owned[..]);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)?;
            Ok(decompressed)
        })
        .await?
    }

    async fn try_decompress_brotli(body: &[u8]) -> Result<Vec<u8>> {
        let body_owned = body.to_vec();
        task::spawn_blocking(move || {
            let cursor = std::io::Cursor::new(body_owned);
            let mut reader = Decompressor::new(cursor, 4096);
            let mut decompressed_data = Vec::new();
            reader.read_to_end(&mut decompressed_data)?;
            Ok(decompressed_data)
        })
        .await?
    }

    async fn try_decompress_deflate(body: &[u8]) -> Result<Vec<u8>> {
        let body_owned = body.to_vec();
        task::spawn_blocking(move || {
            let cursor = std::io::Cursor::new(body_owned);
            let mut decompressed_data = Vec::new();
            let mut deflater = flate2::read::DeflateDecoder::new(cursor);
            deflater.read_to_end(&mut decompressed_data)?;
            Ok(decompressed_data)
        })
        .await?
    }

    fn find_headers_end(data: &[u8]) -> Option<usize> {
        let mut i = 0;
        while i + 3 < data.len() {
            if data[i] == b'\r'
                && data[i + 1] == b'\n'
                && data[i + 2] == b'\r'
                && data[i + 3] == b'\n'
            {
                return Some(i + 4);
            }
            i += 1;
        }

        let mut i = 0;
        while i + 1 < data.len() {
            if data[i] == b'\n' && data[i + 1] == b'\n' {
                return Some(i + 2);
            }
            i += 1;
        }

        None
    }

    fn status_text(status: u16) -> &'static str {
        match status {
            100 => "Continue",
            101 => "Switching Protocols",
            102 => "Processing",
            200 => "OK",
            201 => "Created",
            202 => "Accepted",
            203 => "Non-Authoritative Information",
            204 => "No Content",
            205 => "Reset Content",
            206 => "Partial Content",
            207 => "Multi-Status",
            300 => "Multiple Choices",
            301 => "Moved Permanently",
            302 => "Found",
            303 => "See Other",
            304 => "Not Modified",
            305 => "Use Proxy",
            307 => "Temporary Redirect",
            308 => "Permanent Redirect",
            400 => "Bad Request",
            401 => "Unauthorized",
            402 => "Payment Required",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            406 => "Not Acceptable",
            407 => "Proxy Authentication Required",
            408 => "Request Timeout",
            409 => "Conflict",
            410 => "Gone",
            411 => "Length Required",
            412 => "Precondition Failed",
            413 => "Payload Too Large",
            414 => "URI Too Long",
            415 => "Unsupported Media Type",
            416 => "Range Not Satisfiable",
            417 => "Expectation Failed",
            418 => "I'm a teapot",
            422 => "Unprocessable Entity",
            423 => "Locked",
            424 => "Failed Dependency",
            426 => "Upgrade Required",
            428 => "Precondition Required",
            429 => "Too Many Requests",
            431 => "Request Header Fields Too Large",
            451 => "Unavailable For Legal Reasons",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            504 => "Gateway Timeout",
            505 => "HTTP Version Not Supported",
            506 => "Variant Also Negotiates",
            507 => "Insufficient Storage",
            508 => "Loop Detected",
            510 => "Not Extended",
            511 => "Network Authentication Required",
            _ => "Unknown Status",
        }
    }
}

#[test]
fn test_format_content_json_pretty() {
    let mut log_entry = String::new();
    let json_body = b"{\"name\":\"test\", \"value\":123}";
    MessageFormatter::format_content(&mut log_entry, "application/json", json_body);
    let expected = "{\n  \"name\": \"test\",\n  \"value\": 123\n}";
    assert_eq!(log_entry.trim(), expected.trim());
}

#[test]
fn test_format_content_json_with_charset() {
    let mut log_entry = String::new();
    let json_body = b"{\"name\":\"test\"}";
    MessageFormatter::format_content(&mut log_entry, "application/json; charset=utf-8", json_body);
    let expected = "{\n  \"name\": \"test\"\n}";
    assert_eq!(log_entry.trim(), expected.trim());
}

#[test]
fn test_format_content_javascript() {
    let mut log_entry = String::new();
    let js_body = b"function test() { console.log('hello'); }";
    MessageFormatter::format_content(&mut log_entry, "application/javascript", js_body);
    assert_eq!(log_entry, "function test() { console.log('hello'); }");

    log_entry.clear();
    MessageFormatter::format_content(&mut log_entry, "text/javascript", js_body);
    assert_eq!(log_entry, "function test() { console.log('hello'); }");
}

#[test]
fn test_format_content_html() {
    let mut log_entry = String::new();
    let html_body = b"<html><body><h1>Title</h1></body></html>";
    MessageFormatter::format_content(&mut log_entry, "text/html", html_body);
    assert_eq!(log_entry, "<html><body><h1>Title</h1></body></html>");
}

#[test]
fn test_format_content_xml() {
    let mut log_entry = String::new();
    let xml_body = b"<root><item>value</item></root>";
    MessageFormatter::format_content(&mut log_entry, "application/xml", xml_body);
    assert_eq!(log_entry, "<root><item>value</item></root>");

    log_entry.clear();
    MessageFormatter::format_content(&mut log_entry, "text/xml", xml_body);
    assert_eq!(log_entry, "<root><item>value</item></root>");
}

#[test]
fn test_format_content_plain_text() {
    let mut log_entry = String::new();
    let text_body = b"Just some plain text.";
    MessageFormatter::format_content(&mut log_entry, "text/plain", text_body);
    assert_eq!(log_entry, "Just some plain text.");
}

#[test]
fn test_format_content_other_text() {
    let mut log_entry = String::new();
    let csv_body = b"col1,col2\nval1,val2";
    MessageFormatter::format_content(&mut log_entry, "text/csv", csv_body);
    assert_eq!(log_entry, "col1,col2\nval1,val2");
}

#[test]
fn test_format_content_binary() {
    let mut log_entry = String::new();
    let binary_body = &[0x01, 0x02, 0x03, 0x00, 0x1f, 0x8b];
    MessageFormatter::format_content(&mut log_entry, "application/octet-stream", binary_body);
    assert_eq!(
        log_entry,
        format!("# <binary content: {} bytes>", binary_body.len())
    );
}

#[test]
fn test_format_content_pseudo_binary_text() {
    let mut log_entry = String::new();
    let text_body = "Contains \t tab and \n newline and \r CR".as_bytes();
    MessageFormatter::format_content(&mut log_entry, "application/unknown", text_body);
    assert_eq!(log_entry, "Contains \t tab and \n newline and \r CR");
}

#[test]
fn test_format_content_empty_body() {
    let mut log_entry = String::new();
    let empty_body = b"";
    MessageFormatter::format_content(&mut log_entry, "text/plain", empty_body);
    assert!(log_entry.is_empty());

    log_entry.clear();
    MessageFormatter::format_content(&mut log_entry, "application/json", empty_body);
    assert!(log_entry.is_empty());
}

#[cfg(test)]
mod formatter_tests {
    use bytes::Bytes;
    use chrono::Utc;
    use httparse::Header;

    use super::*;

    #[tokio::test]
    async fn test_decompress_gzip_async() {
        let test_data = b"test text";
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, test_data).unwrap();
        let gzip_data = encoder.finish().unwrap();

        assert!(gzip_data.len() > 2 && gzip_data[0] == 0x1f && gzip_data[1] == 0x8b);

        let result = MessageFormatter::decompress_gzip_async(&gzip_data).await;
        assert!(
            result.is_ok(),
            "Gzip decompression failed: {:?}",
            result.err()
        );
        let decompressed = result.unwrap();
        assert_eq!(decompressed, test_data);
    }

    #[tokio::test]
    async fn test_decompress_gzip_invalid_header() {
        let invalid_data = vec![0x01, 0x02, 0x03, 0x04, 0x05];

        let result = MessageFormatter::decompress_gzip(&invalid_data);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_try_decompress_brotli() {
        let test_data = b"hello world";
        let mut encoder = brotli::CompressorReader::new(test_data.as_slice(), 4096, 11, 22);
        let mut compressed = Vec::new();
        std::io::copy(&mut encoder, &mut compressed).unwrap();

        let result = MessageFormatter::try_decompress_brotli(&compressed).await;
        assert!(
            result.is_ok(),
            "Brotli decompression failed: {:?}",
            result.err()
        );
        let decompressed = result.unwrap();
        assert_eq!(decompressed, test_data);
    }

    #[tokio::test]
    async fn test_try_decompress_deflate() {
        let test_data = b"test data for deflate";
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, test_data).unwrap();
        let deflate_data = encoder.finish().unwrap();

        let result = MessageFormatter::try_decompress_deflate(&deflate_data).await;
        assert!(
            result.is_ok(),
            "Deflate decompression failed: {:?}",
            result.err()
        );
        let decompressed = result.unwrap();
        assert_eq!(decompressed, test_data);
    }

    #[tokio::test]
    async fn test_dechunk_body() {
        let chunked_body = b"4\r\ntest\r\n5\r\nchunk\r\n0\r\n\r\n";

        let result = MessageFormatter::dechunk_body(chunked_body);
        assert_eq!(result, b"testchunk");
    }

    #[tokio::test]
    async fn test_dechunk_body_with_extension() {
        let chunked_body = b"4;extension=value\r\ntest\r\n0\r\n\r\n";

        let result = MessageFormatter::dechunk_body(chunked_body);
        assert_eq!(result, b"test");
    }

    #[tokio::test]
    async fn test_dechunk_body_incomplete() {
        let chunked_body = b"4\r\ntest\r\n5\r\nchun";

        let result = MessageFormatter::dechunk_body(chunked_body);
        assert_eq!(result, b"testchun");
    }

    #[tokio::test]
    async fn test_dechunk_body_invalid_size() {
        let chunked_body = b"XYZ\r\ntest\r\n";

        let result = MessageFormatter::dechunk_body(chunked_body);
        assert_eq!(result, Vec::<u8>::new());
    }

    #[test]
    fn test_find_headers_end() {
        let headers = b"Header1: Value1\r\nHeader2: Value2\r\n\r\nBody";

        let pos = MessageFormatter::find_headers_end(headers);
        println!("CRLF CRLF position: {pos:?}");

        assert_eq!(pos, Some(36));

        let headers = b"Header1: Value1\nHeader2: Value2\n\nBody";

        let pos = MessageFormatter::find_headers_end(headers);
        println!("LF LF position: {pos:?}");

        assert_eq!(pos, Some(33));

        let headers = b"Header1: Value1\nHeader2: Value2\nBody";
        assert_eq!(MessageFormatter::find_headers_end(headers), None);
    }

    #[test]
    fn test_create_metadata_header() {
        let trace_id = "abc123";
        let timestamp = Utc::now();

        let request_header = MessageFormatter::create_metadata_header(trace_id, timestamp, None);
        assert!(request_header.contains("# Trace ID: abc123"));
        assert!(request_header.contains("# Request at:"));
        assert!(!request_header.contains("# Took:"));

        let took = 150;
        let response_header =
            MessageFormatter::create_metadata_header(trace_id, timestamp, Some(took));
        assert!(response_header.contains("# Trace ID: abc123"));
        assert!(response_header.contains("# Response at:"));
        assert!(response_header.contains("# Took: 150 ms"));
    }

    #[test]
    fn test_append_headers() {
        let mut log_entry = String::new();

        let header1_name = "Content-Type";
        let header1_value = b"application/json";
        let header2_name = "Content-Length";
        let header2_value = b"42";

        let headers = vec![
            Header {
                name: header1_name,
                value: header1_value,
            },
            Header {
                name: header2_name,
                value: header2_value,
            },
        ];

        MessageFormatter::append_headers(&headers, &mut log_entry);

        assert!(log_entry.contains("Content-Type: application/json\n"));
        assert!(log_entry.contains("Content-Length: 42\n"));
    }

    #[test]
    fn test_status_text() {
        assert_eq!(MessageFormatter::status_text(200), "OK");
        assert_eq!(MessageFormatter::status_text(404), "Not Found");
        assert_eq!(MessageFormatter::status_text(500), "Internal Server Error");
        assert_eq!(MessageFormatter::status_text(999), "Unknown Status");
    }

    #[test]
    fn test_extract_content_type() {
        let header1_name = "Content-Type";
        let header1_value = b"application/json";
        let header2_name = "Content-Length";
        let header2_value = b"42";

        let headers = vec![
            Header {
                name: header1_name,
                value: header1_value,
            },
            Header {
                name: header2_name,
                value: header2_value,
            },
        ];

        let content_type = MessageFormatter::extract_content_type(&headers);
        assert_eq!(content_type, Some("application/json"));

        let headers_no_ct = vec![Header {
            name: "Content-Length",
            value: b"42",
        }];

        let content_type = MessageFormatter::extract_content_type(&headers_no_ct);
        assert_eq!(content_type, None);
    }

    #[tokio::test]
    async fn test_format_request() {
        let request = b"GET /api/test HTTP/1.1\r\nHost: example.com\r\nContent-Type: application/json\r\n\r\n{\"test\": true}";
        let trace_id = "req123";
        let timestamp = Utc::now();

        let result =
            MessageFormatter::format_request(&Bytes::from(&request[..]), trace_id, timestamp)
                .await
                .unwrap();

        if let LogMessage::Request(formatted) = result {
            assert!(
                formatted.contains("# Trace ID: req123"),
                "Should contain trace ID"
            );
            assert!(
                formatted.contains("GET /api/test HTTP/1.1"),
                "Should contain request line"
            );
            assert!(
                formatted.contains("Host: example.com"),
                "Should contain host header"
            );
            assert!(
                formatted.contains("Content-Type: application/json"),
                "Should contain content-type header"
            );

            assert!(formatted.contains("test"), "Should contain JSON key");
            assert!(formatted.contains("true"), "Should contain JSON value");

            println!("Formatted request: {formatted}");
        } else {
            panic!("Expected Request LogMessage");
        }
    }

    #[tokio::test]
    async fn test_format_response() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\n\r\n{\"status\":\"ok\"}";
        let trace_id = "resp123";
        let timestamp = Utc::now();
        let took = 100;

        let result = MessageFormatter::format_response(
            &Bytes::from(&response[..]),
            trace_id,
            timestamp,
            took,
        )
        .await
        .unwrap();

        if let LogMessage::Response(formatted) = result {
            assert!(formatted.contains("# Trace ID: resp123"));
            assert!(formatted.contains("# Took: 100 ms"));
            assert!(formatted.contains("HTTP/1.1 200 OK"));
            assert!(formatted.contains("Content-Type: application/json"));
            assert!(formatted.contains("{\n  \"status\": \"ok\"\n}"));
        } else {
            panic!("Expected Response LogMessage");
        }
    }

    #[test]
    fn test_format_preformatted_response() {
        let response =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nHello";
        let trace_id = "pre123";
        let timestamp = Utc::now();
        let took = 50;

        let formatted = MessageFormatter::format_preformatted_response(
            trace_id,
            timestamp,
            took,
            &Bytes::from(&response[..]),
        );

        assert!(formatted.contains("# Trace ID: pre123"));
        assert!(formatted.contains("# Took: 50 ms"));
        assert!(formatted.contains("HTTP/1.1 200 OK"));
        assert!(formatted.contains("Content-Type: text/plain"));
        assert!(formatted.contains("Hello"));
    }

    #[test]
    fn test_format_preformatted_response_gzipped() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\n\r\n\x1f\x8b";
        let trace_id = "gzip123";
        let timestamp = Utc::now();
        let took = 75;

        let formatted = MessageFormatter::format_preformatted_response(
            trace_id,
            timestamp,
            took,
            &Bytes::from(&response[..]),
        );

        assert!(formatted.contains("# Trace ID: gzip123"));
        assert!(formatted.contains("Content-Type: application/json"));
        assert!(formatted.contains("Content-Encoding: gzip"));
    }

    #[test]
    fn test_process_response_body() {
        let headers = vec![Header {
            name: "Transfer-Encoding",
            value: b"chunked",
        }];

        let chunked_body = b"4\r\ntest\r\n5\r\nchunk\r\n0\r\n\r\n";

        let processed = MessageFormatter::process_response_body(chunked_body, &headers);

        assert_eq!(processed, b"testchunk");

        let normal_body = b"normal body";
        let headers_normal = vec![Header {
            name: "Content-Length",
            value: b"11",
        }];

        let processed_normal =
            MessageFormatter::process_response_body(normal_body, &headers_normal);
        assert_eq!(processed_normal, normal_body);
    }

    #[tokio::test]
    async fn test_handle_decompression_error() {
        let mut log_entry = String::new();
        let body = b"plain text body";
        let headers = vec![Header {
            name: "Content-Type",
            value: b"text/plain",
        }];

        let error = anyhow::anyhow!("Test decompression error");

        let result =
            MessageFormatter::handle_decompression_error(body, &headers, &mut log_entry, error)
                .unwrap();

        assert!(log_entry.contains("Failed to process content: Test decompression error"));

        assert!(
            log_entry.contains("plain text body"),
            "Log entry should contain the body text"
        );
        assert_eq!(
            result,
            Vec::<u8>::new(),
            "Result should be empty vec for text content"
        );

        let mut js_log_entry = String::new();
        let js_headers = vec![Header {
            name: "Content-Type",
            value: b"application/javascript",
        }];

        let js_result = MessageFormatter::handle_decompression_error(
            b"console.log('test')",
            &js_headers,
            &mut js_log_entry,
            anyhow::anyhow!("JS error"),
        )
        .unwrap();

        assert!(js_log_entry.contains("JavaScript content could not be fully decompressed"));
        assert!(js_result.is_empty());

        let mut bin_log_entry = String::new();
        let bin_headers = vec![Header {
            name: "Content-Type",
            value: b"application/octet-stream",
        }];

        let binary_body = &[0xFF, 0xFE, 0xFD];
        let bin_result = MessageFormatter::handle_decompression_error(
            binary_body,
            &bin_headers,
            &mut bin_log_entry,
            anyhow::anyhow!("Binary error"),
        )
        .unwrap();

        assert!(bin_log_entry.contains("Failed to process content: Binary error"));
        assert_eq!(bin_result, binary_body);
    }

    #[tokio::test]
    async fn test_try_decompress_body() {
        let body = b"plain body";
        let headers = vec![Header {
            name: "Content-Type",
            value: b"text/plain",
        }];

        let result = MessageFormatter::try_decompress_body(body, &headers)
            .await
            .unwrap();
        assert_eq!(result, body);

        let identity_headers = vec![Header {
            name: "Content-Encoding",
            value: b"identity",
        }];

        let identity_result = MessageFormatter::try_decompress_body(body, &identity_headers)
            .await
            .unwrap();
        assert_eq!(identity_result, body);

        let unsupported_headers = vec![Header {
            name: "Content-Encoding",
            value: b"unknown-encoding",
        }];

        let unsupported_result = MessageFormatter::try_decompress_body(body, &unsupported_headers)
            .await
            .unwrap();
        assert_eq!(unsupported_result, body);
    }
}
