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
            log_entry.push_str(&format!("{} {} HTTP/1.{}\n", method, path, version));

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
            "<!-- Debug: Failed to process content: {} -->",
            error
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
