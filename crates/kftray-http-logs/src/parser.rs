use std::io::Read;
use std::str;

use anyhow::Result;
use brotli::Decompressor;
use flate2::read::GzDecoder;
use httparse::{
    Header,
    Request as HttpRequest,
    Response as HttpResponse,
    Status as ParseStatus,
    EMPTY_HEADER,
};
use tokio::task;
use tracing::debug;

const MAX_HEADERS: usize = 64;

#[derive(Debug, PartialEq)]
enum ContentCategory {
    Empty,
    Binary,
    Json,
    JavaScript,
    Css,
    Html,
    Xml,
    Svg,
    Text,
    Font,
}

lazy_static::lazy_static! {
    static ref HEADERS_CACHE: dashmap::DashMap<&'static str, &'static str> = {
        let map = dashmap::DashMap::new();
        map.insert("content-type", "content-type");
        map.insert("content-length", "content-length");
        map.insert("content-encoding", "content-encoding");
        map.insert("transfer-encoding", "transfer-encoding");
        map
    };
}

#[derive(Debug)]
pub enum ChunkParseResult<'a> {
    Complete {
        chunk_data: &'a [u8],
        next_pos: usize,
    },
    EndOfChunks {
        next_pos: usize,
    },
    Incomplete {
        chunk_data: Option<&'a [u8]>,
        consumed: usize,
    },
    Skip {
        consumed: usize,
    },
}

pub type RequestParseResult<'a> = (
    Option<&'a str>,
    Option<&'a str>,
    Option<u16>,
    Vec<Header<'a>>,
);

pub struct RequestParser;

impl RequestParser {
    pub fn parse(buffer: &[u8]) -> Result<RequestParseResult> {
        let mut headers = [EMPTY_HEADER; MAX_HEADERS];
        let mut req = HttpRequest::new(&mut headers);

        match req.parse(buffer)? {
            ParseStatus::Complete(_) => Ok((
                req.method,
                req.path,
                req.version.map(|v| v as u16),
                req.headers.to_vec(),
            )),
            ParseStatus::Partial => Ok((None, None, None, Vec::new())),
        }
    }

    pub fn get_content_length(headers: &[Header<'_>]) -> usize {
        if let Some(content_length) = HEADERS_CACHE.get("content-length") {
            let header_name = content_length.value();
            for h in headers {
                if h.name.eq_ignore_ascii_case(header_name) {
                    if let Ok(v) = str::from_utf8(h.value) {
                        if let Ok(len) = v.parse::<usize>() {
                            return len;
                        }
                    }
                    break;
                }
            }
        }
        0
    }

    pub fn get_content_encoding<'a>(headers: &'a [Header<'_>]) -> Option<&'a str> {
        for header in headers {
            if header.name.eq_ignore_ascii_case("content-encoding") {
                if let Ok(value) = std::str::from_utf8(header.value) {
                    return Some(value.trim());
                }
            }
        }
        None
    }

    pub fn is_gzip_encoded(headers: &[Header<'_>]) -> bool {
        if let Some(encoding) = Self::get_content_encoding(headers) {
            return encoding == "gzip" || encoding.eq_ignore_ascii_case("gzip");
        }
        false
    }

    pub fn is_brotli_encoded(headers: &[Header<'_>]) -> bool {
        if let Some(encoding) = Self::get_content_encoding(headers) {
            return encoding == "br" || encoding.eq_ignore_ascii_case("br");
        }
        false
    }

    pub fn is_chunked_transfer(headers: &[Header<'_>]) -> bool {
        if let Some(transfer_encoding) = HEADERS_CACHE.get("transfer-encoding") {
            let header_name = transfer_encoding.value();
            for h in headers {
                if h.name.eq_ignore_ascii_case(header_name)
                    && (h.value == b"chunked" || h.value.eq_ignore_ascii_case(b"chunked"))
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn extract_body(buffer: &[u8]) -> Option<&[u8]> {
        buffer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|headers_end| {
                let body_start = headers_end + 4;
                &buffer[body_start..]
            })
    }

    pub fn process_chunked_body(body: &[u8]) -> Vec<u8> {
        if body.is_empty() {
            return Vec::new();
        }

        let estimated_capacity = body.len() * 9 / 10;
        let mut result = Vec::with_capacity(estimated_capacity);
        let mut pos = 0;
        let mut chunks_found = 0;
        let mut incomplete_chunk = false;
        let mut last_incomplete_data: Option<&[u8]> = None;
        let mut last_chunk_size: Option<usize> = None;

        while pos < body.len() {
            match Self::parse_chunk(&body[pos..], &mut chunks_found) {
                Ok(ChunkParseResult::Complete {
                    chunk_data,
                    next_pos,
                }) => {
                    result.extend_from_slice(chunk_data);
                    pos += next_pos;
                    last_incomplete_data = None;
                    last_chunk_size = None;
                }
                Ok(ChunkParseResult::EndOfChunks { next_pos }) => {
                    pos += next_pos;
                    if pos < body.len() {
                        debug!(
                            "Found end marker with {} bytes of trailing data",
                            body.len() - pos
                        );
                    }
                    break;
                }
                Ok(ChunkParseResult::Incomplete {
                    chunk_data,
                    consumed,
                }) => {
                    if let Some(data) = chunk_data {
                        last_incomplete_data = Some(data);
                        if let Some(chunk_line_end) = data.windows(2).position(|w| w == b"\r\n") {
                            if let Ok(size_str) = std::str::from_utf8(&data[..chunk_line_end]) {
                                if let Ok(size) = usize::from_str_radix(size_str.trim(), 16) {
                                    last_chunk_size = Some(size);
                                }
                            }
                        }
                        result.extend_from_slice(data);
                    }
                    pos += consumed;
                    incomplete_chunk = true;

                    if pos >= body.len() {
                        break;
                    }
                }
                Ok(ChunkParseResult::Skip { consumed }) => {
                    pos += consumed;
                }
                Err(e) => {
                    debug!("Chunk parsing error: {:?} at position {}", e, pos);

                    if pos < body.len() && body.len() - pos > 4 {
                        if incomplete_chunk
                            && last_incomplete_data.is_some()
                            && last_chunk_size.is_some()
                        {
                            let remaining = &body[pos..];
                            let last_size = last_chunk_size.unwrap();
                            let last_data_len = last_incomplete_data.unwrap().len();

                            if last_data_len + remaining.len() >= last_size {
                                debug!("Adding remaining data to complete partial chunk");
                                result.extend_from_slice(remaining);
                            }
                        } else {
                            result.extend_from_slice(&body[pos..]);
                        }
                    }
                    break;
                }
            }
        }

        if chunks_found > 0 && !result.is_empty() && !incomplete_chunk {
            return result;
        }

        Self::handle_chunked_edge_cases(body, result, chunks_found, incomplete_chunk)
    }

    fn parse_chunk<'a>(
        chunk_data: &'a [u8], chunks_found: &mut usize,
    ) -> Result<ChunkParseResult<'a>> {
        let line_end = match chunk_data.windows(2).position(|w| w == b"\r\n") {
            Some(pos) => pos,
            None => return Err(anyhow::anyhow!("No CRLF found in chunk header")),
        };

        if line_end == 0 {
            return Ok(ChunkParseResult::Skip { consumed: 2 });
        }

        let hex_str = match std::str::from_utf8(&chunk_data[..line_end]) {
            Ok(s) => s.trim(),
            Err(_) => {
                return Ok(ChunkParseResult::Skip {
                    consumed: line_end + 2,
                })
            }
        };

        if hex_str.is_empty() {
            return Ok(ChunkParseResult::Skip {
                consumed: line_end + 2,
            });
        }

        let size_part = if let Some(semi_idx) = hex_str.find(';') {
            &hex_str[..semi_idx]
        } else {
            hex_str
        };

        let chunk_size = match usize::from_str_radix(size_part, 16) {
            Ok(size) => size,
            Err(_) => {
                return Ok(ChunkParseResult::Skip {
                    consumed: line_end + 2,
                })
            }
        };

        *chunks_found += 1;

        if chunk_size == 0 {
            return Ok(ChunkParseResult::EndOfChunks {
                next_pos: line_end + 2 + 2,
            });
        }

        let chunk_start = line_end + 2;

        if chunk_start + chunk_size <= chunk_data.len() {
            if chunk_start + chunk_size + 2 <= chunk_data.len() {
                Ok(ChunkParseResult::Complete {
                    chunk_data: &chunk_data[chunk_start..chunk_start + chunk_size],
                    next_pos: chunk_start + chunk_size + 2,
                })
            } else {
                Ok(ChunkParseResult::Incomplete {
                    chunk_data: Some(&chunk_data[chunk_start..chunk_start + chunk_size]),
                    consumed: chunk_data.len(),
                })
            }
        } else if chunk_start < chunk_data.len() {
            Ok(ChunkParseResult::Incomplete {
                chunk_data: Some(&chunk_data[chunk_start..]),
                consumed: chunk_data.len(),
            })
        } else {
            Ok(ChunkParseResult::Incomplete {
                chunk_data: None,
                consumed: chunk_data.len(),
            })
        }
    }

    fn handle_chunked_edge_cases(
        body: &[u8], result: Vec<u8>, chunks_found: usize, incomplete_chunk: bool,
    ) -> Vec<u8> {
        if chunks_found == 0 && result.is_empty() && !body.is_empty() {
            return body.to_vec();
        }

        if incomplete_chunk && body.len() > result.len() {
            let mut combined = result;

            if combined.len() < body.len() {
                let unparsed_start = body
                    .iter()
                    .zip(combined.iter())
                    .position(|(a, b)| a != b)
                    .unwrap_or(combined.len().min(body.len()));

                if unparsed_start < body.len() {
                    combined.extend_from_slice(&body[unparsed_start..]);
                }
            }

            return combined;
        }

        if result.is_empty() && !body.is_empty() {
            body.to_vec()
        } else {
            result
        }
    }
}

pub struct ResponseParser;

impl ResponseParser {
    pub fn parse(buffer: &[u8]) -> Result<(Option<u16>, Vec<Header<'_>>)> {
        let mut headers = [EMPTY_HEADER; MAX_HEADERS];
        let mut res = HttpResponse::new(&mut headers);

        match res.parse(buffer)? {
            ParseStatus::Complete(_) => Ok((res.code, res.headers.to_vec())),
            ParseStatus::Partial => Ok((None, Vec::new())),
        }
    }
}

pub struct BodyParser;

impl BodyParser {
    pub fn is_image(headers: &[Header<'_>]) -> bool {
        headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value.starts_with(b"image/"))
    }

    pub fn get_content_encoding<'a>(headers: &'a [Header<'_>]) -> Option<&'a str> {
        for header in headers {
            if header.name.eq_ignore_ascii_case("content-encoding") {
                if let Ok(value) = std::str::from_utf8(header.value) {
                    return Some(value.trim());
                }
            }
        }
        None
    }

    pub fn get_content_type<'a>(headers: &'a [Header<'_>]) -> Option<&'a str> {
        for header in headers {
            if header.name.eq_ignore_ascii_case("content-type") {
                if let Ok(value) = str::from_utf8(header.value) {
                    return Some(value.trim());
                }
            }
        }
        None
    }

    pub async fn process_response_body(body: &[u8], headers: &[Header<'_>]) -> Result<Vec<u8>> {
        let is_chunked = RequestParser::is_chunked_transfer(headers);
        let dechunked_body = if is_chunked {
            debug!(
                "Processing chunked response body, size before dechunking: {}",
                body.len()
            );
            let result = RequestParser::process_chunked_body(body);
            debug!(
                "Dechunked response body size: {} -> {}",
                body.len(),
                result.len()
            );
            result
        } else {
            body.to_vec()
        };

        let content_encoding = Self::get_content_encoding(headers);

        if let Some(encoding) = content_encoding {
            debug!("Detected content encoding: {}", encoding);
        }

        match content_encoding {
            Some(encoding) if encoding == "gzip" || encoding.eq_ignore_ascii_case("gzip") => {
                debug!(
                    "Decompressing gzip encoded content, size: {}",
                    dechunked_body.len()
                );

                let mut decompressed = None;

                match Self::decompress_gzip(&dechunked_body).await {
                    Ok(result) if !result.is_empty() && result.len() > dechunked_body.len() / 2 => {
                        debug!(
                            "Successfully decompressed gzip content: {} bytes -> {} bytes",
                            dechunked_body.len(),
                            result.len()
                        );
                        decompressed = Some(result);
                    }
                    Ok(result) if !result.is_empty() => {
                        debug!("Partial gzip decompression: {} bytes -> {} bytes, trying alternative approaches",
                              dechunked_body.len(), result.len());
                        decompressed = Some(result);
                    }
                    _ => {
                        debug!("Standard gzip decompression failed, trying alternative approaches");
                    }
                }

                if decompressed.is_none() || decompressed.as_ref().unwrap().is_empty() {
                    if let Some(gzip_start) = Self::find_gzip_header(&dechunked_body) {
                        if gzip_start > 0 {
                            debug!("Found gzip header at offset {}, attempting alternative decompression", gzip_start);
                            match Self::decompress_gzip(&dechunked_body[gzip_start..]).await {
                                Ok(result) if !result.is_empty() => {
                                    debug!(
                                        "Alternative gzip decompression succeeded: {} bytes",
                                        result.len()
                                    );
                                    decompressed = Some(result);
                                }
                                _ => {
                                    debug!("Alternative gzip decompression failed or produced empty output");
                                }
                            }
                        }
                    }
                }

                match decompressed {
                    Some(result) if !result.is_empty() => {
                        debug!("Final decompressed size: {} bytes", result.len());
                        Ok(result)
                    }
                    _ => {
                        debug!(
                            "All gzip decompression attempts failed, returning original content"
                        );
                        Ok(dechunked_body)
                    }
                }
            }
            Some(encoding) if encoding == "br" || encoding.eq_ignore_ascii_case("br") => {
                debug!(
                    "Decompressing brotli encoded content, size: {}",
                    dechunked_body.len()
                );
                match Self::decompress_brotli(&dechunked_body).await {
                    Ok(decompressed) => {
                        debug!(
                            "Successfully decompressed brotli content: {} bytes -> {} bytes",
                            dechunked_body.len(),
                            decompressed.len()
                        );
                        Ok(decompressed)
                    }
                    Err(e) => {
                        debug!(
                            "Brotli decompression failed: {:?}, returning original content",
                            e
                        );
                        Ok(dechunked_body)
                    }
                }
            }
            Some(encoding) => {
                debug!("Unknown content encoding: {}, returning as-is", encoding);
                Ok(dechunked_body)
            }
            None => Ok(dechunked_body),
        }
    }

    fn find_gzip_header(data: &[u8]) -> Option<usize> {
        data.windows(2)
            .position(|window| window[0] == 0x1f && window[1] == 0x8b)
    }

    pub async fn decompress_gzip(data: &[u8]) -> Result<Vec<u8>> {
        let data = data.to_vec();

        if data.len() < 8 {
            return Ok(data);
        }

        if data.len() >= 2 && data[0] == 0x1F && data[1] == 0x8B {
            task::spawn_blocking(move || {
                let mut decoder = GzDecoder::new(&data[..]);
                let mut decompressed_data = Vec::new();
                if decoder.read_to_end(&mut decompressed_data).is_err() {
                    if decompressed_data.is_empty() {
                        return Ok(data);
                    } else {
                        return Ok(decompressed_data);
                    }
                }
                Ok(decompressed_data)
            })
            .await?
        } else {
            Ok(data)
        }
    }

    pub async fn decompress_brotli(data: &[u8]) -> Result<Vec<u8>> {
        if data.is_empty() || data.len() < 4 {
            return Ok(data.to_vec());
        }

        let data = data.to_vec();
        task::spawn_blocking(move || {
            let mut reader = Decompressor::new(&data[..], data.len());
            let mut decompressed_data = Vec::new();

            match reader.read_to_end(&mut decompressed_data) {
                Ok(_) => {
                    if !decompressed_data.is_empty() {
                        debug!(
                            "Successfully decompressed brotli data: {} -> {} bytes",
                            data.len(),
                            decompressed_data.len()
                        );
                        Ok(decompressed_data)
                    } else {
                        debug!("Brotli decompression produced empty result, using original");
                        Ok(data)
                    }
                }
                Err(e) => {
                    debug!("Brotli decompression error: {:?}", e);

                    let mut reader = Decompressor::new(&data[..], 65536);
                    let mut decompressed_data = Vec::new();

                    match reader.read_to_end(&mut decompressed_data) {
                        Ok(_) if !decompressed_data.is_empty() => {
                            debug!(
                                "Alternative brotli decompression succeeded: {} bytes",
                                decompressed_data.len()
                            );
                            Ok(decompressed_data)
                        }
                        _ => {
                            debug!("All brotli decompression attempts failed, returning original");
                            Ok(data)
                        }
                    }
                }
            }
        })
        .await?
    }

    fn bytes_to_utf8_string(bytes: &[u8]) -> String {
        if let Ok(s) = std::str::from_utf8(bytes) {
            return s.to_string();
        }

        String::from_utf8_lossy(bytes).to_string()
    }

    fn identify_content_category(content_type: Option<&str>, body: &[u8]) -> ContentCategory {
        if body.is_empty() {
            return ContentCategory::Empty;
        }

        if let Some(ct) = content_type {
            let ct_lower = ct.to_lowercase();

            if ct_lower.contains("svg") {
                return ContentCategory::Svg;
            }

            if ct_lower.starts_with("image/") {
                return ContentCategory::Binary;
            }
            if ct_lower.starts_with("audio/") {
                return ContentCategory::Binary;
            }
            if ct_lower.starts_with("video/") {
                return ContentCategory::Binary;
            }
            if ct_lower.contains("font/") || ct_lower.contains("application/font") {
                return ContentCategory::Font;
            }
            if ct_lower.contains("application/pdf")
                || ct_lower.contains("application/msword")
                || ct_lower.contains("octet-stream")
            {
                return ContentCategory::Binary;
            }

            if ct_lower.contains("json") {
                return ContentCategory::Json;
            }
            if ct_lower.contains("javascript") {
                return ContentCategory::JavaScript;
            }
            if ct_lower.contains("css") {
                return ContentCategory::Css;
            }
            if ct_lower.contains("html") {
                return ContentCategory::Html;
            }
            if ct_lower.contains("xml") {
                return ContentCategory::Xml;
            }
            if ct_lower.starts_with("text/") {
                return ContentCategory::Text;
            }
        }

        if body.len() > 2 {
            if body.windows(4).any(|w| w == b"<svg") {
                return ContentCategory::Svg;
            }

            if (body[0] == b'{' && body.contains(&b'}'))
                || (body[0] == b'[' && body.contains(&b']'))
            {
                return ContentCategory::Json;
            }

            if body.len() > 5
                && (body.starts_with(b"<?xml")
                    || body.starts_with(b"<!DOC")
                    || body.windows(5).any(|w| w == b"<html")
                    || body.windows(5).any(|w| w == b"<body")
                    || body.windows(5).any(|w| w == b"<head"))
            {
                if body.windows(5).any(|w| w == b"<html") || body.windows(5).any(|w| w == b"<body")
                {
                    return ContentCategory::Html;
                }

                return ContentCategory::Xml;
            }

            if body.starts_with(&[0xFF, 0xD8])
                || body.starts_with(&[0x89, 0x50, 0x4E, 0x47])
                || body.starts_with(&[0x47, 0x49, 0x46])
            {
                return ContentCategory::Binary;
            }

            let likely_binary = body
                .iter()
                .take(32)
                .filter(|&&b| b < 9 && b != b'\t' && b != b'\n' && b != b'\r')
                .count()
                > 8;
            if likely_binary {
                return ContentCategory::Binary;
            }
        }

        ContentCategory::Text
    }

    pub async fn format_body_async(body: &[u8], headers: &[Header<'_>]) -> Result<String> {
        debug!(
            "Processing response body for formatting, size: {} bytes",
            body.len()
        );

        let processed_body = match Self::process_response_body(body, headers).await {
            Ok(processed) => {
                debug!(
                    "Successfully processed body: {} bytes -> {} bytes",
                    body.len(),
                    processed.len()
                );
                processed
            }
            Err(e) => {
                debug!("Failed to process body: {:?}, using original", e);
                body.to_vec()
            }
        };

        let content_type = Self::get_content_type(headers);
        if let Some(ct) = content_type {
            debug!("Content-Type: {}", ct);
        } else {
            debug!("No Content-Type header found");
        }

        match Self::format_body(&processed_body, content_type) {
            Ok(formatted) => {
                debug!("Successfully formatted body: {} bytes", formatted.len());
                Ok(formatted)
            }
            Err(e) => {
                debug!(
                    "Failed to format body: {:?}, trying direct string conversion",
                    e
                );

                if let Ok(text) = std::str::from_utf8(&processed_body) {
                    debug!("Direct string conversion succeeded");
                    Ok(text.to_string())
                } else {
                    debug!("Direct string conversion failed, using lossy conversion");
                    Ok(String::from_utf8_lossy(&processed_body).to_string())
                }
            }
        }
    }

    pub fn format_body(body: &[u8], content_type: Option<&str>) -> Result<String> {
        let category = Self::identify_content_category(content_type, body);
        debug!("Content category identified as: {:?}", category);

        let result = match category {
            ContentCategory::Empty => "# <empty body>".to_string(),

            ContentCategory::Binary => {
                let content_desc = content_type.unwrap_or("binary");
                let preview = if !body.is_empty() {
                    let preview_size = std::cmp::min(body.len(), 64);
                    let bytes: Vec<String> = body[..preview_size]
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect();
                    format!("\n# Preview: {} ", bytes.join(" "))
                } else {
                    "".to_string()
                };
                format!(
                    "# <binary data: {} format, {} bytes>{}",
                    content_desc,
                    body.len(),
                    preview
                )
            }

            ContentCategory::Font => {
                let font_type = content_type.unwrap_or("font");
                format!(
                    "# <binary data: {} format, {} bytes>",
                    font_type,
                    body.len()
                )
            }

            ContentCategory::Json => {
                let body_str = Self::bytes_to_utf8_string(body);
                let trimmed = body_str.trim();
                debug!(
                    "Attempting to format JSON content, size: {} bytes",
                    trimmed.len()
                );

                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(json_value) => {
                        debug!("Successfully parsed JSON, pretty-printing");
                        match serde_json::to_string_pretty(&json_value) {
                            Ok(pretty_json) => {
                                debug!(
                                    "Successfully pretty-printed JSON: {} bytes",
                                    pretty_json.len()
                                );
                                pretty_json
                            }
                            Err(e) => {
                                debug!("Error pretty-printing JSON: {:?}", e);
                                format!("# <JSON content - error formatting>\n{}", trimmed)
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Error parsing JSON: {:?}", e);

                        if trimmed.contains("}\n{") || trimmed.contains("}\r\n{") {
                            debug!("Detected potential JSON stream, formatting line by line");
                            let mut formatted = String::new();
                            let mut line_count = 0;

                            for line in trimmed.lines() {
                                let line = line.trim();
                                if line.is_empty() {
                                    continue;
                                }

                                line_count += 1;
                                if line_count > 100 {
                                    formatted.push_str("\n# ... more JSON objects omitted ...");
                                    break;
                                }

                                match serde_json::from_str::<serde_json::Value>(line) {
                                    Ok(json_value) => {
                                        if let Ok(pretty) =
                                            serde_json::to_string_pretty(&json_value)
                                        {
                                            formatted.push_str(&pretty);
                                        } else {
                                            formatted.push_str(line);
                                        }
                                    }
                                    Err(_) => {
                                        formatted.push_str(line);
                                    }
                                }
                                formatted.push_str("\n\n");
                            }

                            if !formatted.is_empty() {
                                debug!("Formatted JSON stream: {} lines", line_count);
                                formatted
                            } else {
                                format!("# <JSON content - invalid JSON>\n{}", trimmed)
                            }
                        } else {
                            format!("# <JSON content - invalid JSON>\n{}", trimmed)
                        }
                    }
                }
            }

            ContentCategory::JavaScript => {
                let js_str = Self::bytes_to_utf8_string(body);
                debug!(
                    "Formatting JavaScript content, size: {} bytes",
                    js_str.len()
                );

                let mut formatted = String::new();
                let mut indent_level: usize = 0;
                let mut in_string = false;
                let mut prev_char = '\0';

                for c in js_str.chars() {
                    if (c == '"' || c == '\'') && prev_char != '\\' {
                        in_string = !in_string;
                    }

                    if !in_string {
                        if c == '{' || c == '[' {
                            formatted.push(c);
                            formatted.push('\n');
                            indent_level += 1;
                            for _ in 0..indent_level {
                                formatted.push_str("  ");
                            }
                            continue;
                        } else if c == '}' || c == ']' {
                            formatted.push('\n');
                            indent_level = indent_level.saturating_sub(1);
                            for _ in 0..indent_level {
                                formatted.push_str("  ");
                            }
                            formatted.push(c);
                            continue;
                        } else if c == ';' {
                            formatted.push(c);
                            formatted.push('\n');
                            for _ in 0..indent_level {
                                formatted.push_str("  ");
                            }
                            continue;
                        }
                    }

                    formatted.push(c);
                    prev_char = c;
                }

                if formatted.len() > js_str.len() * 2 || formatted.len() < js_str.len() / 2 {
                    debug!("JavaScript formatting produced unusual result, using original");
                    js_str
                } else {
                    debug!(
                        "Successfully formatted JavaScript: {} bytes",
                        formatted.len()
                    );
                    formatted
                }
            }

            ContentCategory::Css => {
                let css_str = Self::bytes_to_utf8_string(body);
                format!("# <CSS content>\n{}", css_str.trim())
            }

            ContentCategory::Html => {
                let html_str = Self::bytes_to_utf8_string(body);
                format!(
                    "# <HTML content ({} bytes)>\n{}",
                    body.len(),
                    html_str.trim()
                )
            }

            ContentCategory::Xml => {
                let xml_str = Self::bytes_to_utf8_string(body);
                format!("# <XML content>\n{}", xml_str.trim())
            }

            ContentCategory::Svg => {
                let svg_str = Self::bytes_to_utf8_string(body);
                format!("# <SVG content>\n{}", svg_str.trim())
            }

            ContentCategory::Text => {
                let text_str = Self::bytes_to_utf8_string(body);

                let type_info = if let Some(ct) = content_type {
                    format!(" of type {}", ct)
                } else {
                    "".to_string()
                };

                if content_type.is_some()
                    && !content_type.unwrap().to_lowercase().starts_with("text/")
                {
                    format!("# <Unknown content{}>\n{}", type_info, text_str.trim())
                } else {
                    text_str.trim().to_string()
                }
            }
        };

        Ok(result)
    }

    pub fn is_content_too_large(content_length: usize) -> bool {
        content_length > 100 * 1024 * 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_parser() {
        let request =
            b"GET /api/users HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\nHello";
        let (method, path, version, headers) = RequestParser::parse(request).unwrap();

        assert_eq!(method, Some("GET"));
        assert_eq!(path, Some("/api/users"));
        assert_eq!(version, Some(1));
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].name, "Host");
        assert_eq!(
            std::str::from_utf8(headers[0].value).unwrap(),
            "example.com"
        );
    }

    #[test]
    fn test_request_parser_partial() {
        let partial_request = b"GET /api/";
        let (method, path, version, headers) = RequestParser::parse(partial_request).unwrap();

        assert_eq!(method, None);
        assert_eq!(path, None);
        assert_eq!(version, None);
        assert!(headers.is_empty());
    }

    #[test]
    fn test_get_content_length() {
        let mut headers = [EMPTY_HEADER; 2];
        headers[0].name = "Content-Length";
        headers[0].value = b"1024";
        headers[1].name = "Host";
        headers[1].value = b"example.com";

        let length = RequestParser::get_content_length(&headers);
        assert_eq!(length, 1024);

        let empty_headers: [Header; 0] = [];
        assert_eq!(RequestParser::get_content_length(&empty_headers), 0);

        headers[0].value = b"invalid";
        assert_eq!(RequestParser::get_content_length(&headers), 0);
    }

    #[test]
    fn test_get_content_encoding() {
        let mut headers = [EMPTY_HEADER; 2];
        headers[0].name = "Content-Encoding";
        headers[0].value = b"gzip";
        headers[1].name = "Host";
        headers[1].value = b"example.com";

        let encoding = RequestParser::get_content_encoding(&headers);
        assert_eq!(encoding, Some("gzip"));

        headers[0].value = b"br";
        assert_eq!(RequestParser::get_content_encoding(&headers), Some("br"));

        let empty_headers: [Header; 0] = [];
        assert_eq!(RequestParser::get_content_encoding(&empty_headers), None);
    }

    #[test]
    fn test_is_gzip_encoded() {
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Encoding";
        headers[0].value = b"gzip";

        assert!(RequestParser::is_gzip_encoded(&headers));

        headers[0].value = b"br";
        assert!(!RequestParser::is_gzip_encoded(&headers));

        let empty_headers: [Header; 0] = [];
        assert!(!RequestParser::is_gzip_encoded(&empty_headers));
    }

    #[test]
    fn test_is_brotli_encoded() {
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Encoding";
        headers[0].value = b"br";

        assert!(RequestParser::is_brotli_encoded(&headers));

        headers[0].value = b"gzip";
        assert!(!RequestParser::is_brotli_encoded(&headers));

        let empty_headers: [Header; 0] = [];
        assert!(!RequestParser::is_brotli_encoded(&empty_headers));
    }

    #[test]
    fn test_is_chunked_transfer() {
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Transfer-Encoding";
        headers[0].value = b"chunked";

        assert!(RequestParser::is_chunked_transfer(&headers));

        headers[0].value = b"compress";
        assert!(!RequestParser::is_chunked_transfer(&headers));

        let empty_headers: [Header; 0] = [];
        assert!(!RequestParser::is_chunked_transfer(&empty_headers));
    }

    #[test]
    fn test_extract_body() {
        let request = b"GET /api/users HTTP/1.1\r\nHost: example.com\r\n\r\nHello World";
        let body = RequestParser::extract_body(request);
        assert_eq!(body, Some(b"Hello World" as &[u8]));

        let no_body = b"GET /api/users HTTP/1.1\r\nHost: example.com";
        assert_eq!(RequestParser::extract_body(no_body), None);
    }

    #[test]
    fn test_process_chunked_body() {
        let chunked_body = b"7\r\nHello, \r\n5\r\nWorld\r\n0\r\n\r\n";
        let processed = RequestParser::process_chunked_body(chunked_body);
        assert_eq!(processed, b"Hello, World");

        let empty_body = b"";
        let processed = RequestParser::process_chunked_body(empty_body);
        assert!(processed.is_empty());

        let incomplete = b"7\r\nHello,";
        let processed = RequestParser::process_chunked_body(incomplete);
        assert!(processed.len() >= 6);
        assert!(processed.starts_with(b"Hello,"));

        let invalid = b"XYZ\r\nHello";
        let processed = RequestParser::process_chunked_body(invalid);
        assert!(processed.len() >= 5);
        assert!(processed.ends_with(b"Hello"));

        let complex = b"5\r\nHello\r\n5\r\nWorld\r\n0\r\n\r\nExtra";
        let processed = RequestParser::process_chunked_body(complex);
        assert_eq!(processed, b"HelloWorld");
    }

    #[test]
    fn test_parse_chunk() {
        let mut chunks_found = 0;

        let data = b"5\r\nHello\r\n";
        let result = RequestParser::parse_chunk(data, &mut chunks_found).unwrap();
        match result {
            ChunkParseResult::Complete {
                chunk_data,
                next_pos,
            } => {
                assert_eq!(chunk_data, b"Hello");
                assert_eq!(next_pos, data.len());
                assert_eq!(chunks_found, 1);
            }
            _ => panic!("Expected Complete result"),
        }

        chunks_found = 0;
        let data = b"0\r\n\r\n";
        let result = RequestParser::parse_chunk(data, &mut chunks_found).unwrap();
        match result {
            ChunkParseResult::EndOfChunks { next_pos } => {
                assert_eq!(next_pos, 5);
                assert_eq!(chunks_found, 1);
            }
            _ => panic!("Expected EndOfChunks result"),
        }

        chunks_found = 0;
        let data = b"5\r\nHel";
        let result = RequestParser::parse_chunk(data, &mut chunks_found).unwrap();
        match result {
            ChunkParseResult::Incomplete {
                chunk_data,
                consumed,
            } => {
                assert_eq!(chunk_data, Some(b"Hel" as &[u8]));
                assert_eq!(consumed, data.len());
                assert_eq!(chunks_found, 1);
            }
            _ => panic!("Expected Incomplete result"),
        }

        chunks_found = 0;
        let data = b"\r\nignore";
        let result = RequestParser::parse_chunk(data, &mut chunks_found).unwrap();
        match result {
            ChunkParseResult::Skip { consumed } => {
                assert_eq!(consumed, 2);
                assert_eq!(chunks_found, 0);
            }
            _ => panic!("Expected Skip result"),
        }

        chunks_found = 0;
        let data = b"A;extension=value\r\n1234567890\r\n";
        let result = RequestParser::parse_chunk(data, &mut chunks_found).unwrap();
        match result {
            ChunkParseResult::Complete {
                chunk_data,
                next_pos,
            } => {
                assert_eq!(chunk_data, b"1234567890");
                assert_eq!(next_pos, data.len());
                assert_eq!(chunks_found, 1);
            }
            _ => panic!("Expected Complete result"),
        }
    }

    #[test]
    fn test_handle_chunked_edge_cases() {
        let body = b"";
        let result = Vec::new();
        let output = RequestParser::handle_chunked_edge_cases(body, result, 0, false);
        assert!(output.is_empty());

        let body = b"Hello World";
        let result = Vec::new();
        let output = RequestParser::handle_chunked_edge_cases(body, result, 0, false);
        assert_eq!(output, body);

        let body = b"5\r\nHello\r\n3\r\nWor";
        let mut result = Vec::new();
        result.extend_from_slice(b"Hello");
        let output = RequestParser::handle_chunked_edge_cases(body, result, 1, true);
        assert!(output.contains(&b'H'));
        assert!(output.contains(&b'e'));
        assert!(output.contains(&b'l'));
        assert!(output.contains(&b'o'));
        assert!(output.contains(&b'W'));
        assert!(output.contains(&b'o'));
        assert!(output.contains(&b'r'));
    }

    #[test]
    fn test_response_parser() {
        let response =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
        let (status, headers) = ResponseParser::parse(response).unwrap();

        assert_eq!(status, Some(200));
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].name, "Content-Type");
        assert_eq!(
            std::str::from_utf8(headers[0].value).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_response_parser_partial() {
        let partial_response = b"HTTP/1.1 20";
        let (status, headers) = ResponseParser::parse(partial_response).unwrap();

        assert_eq!(status, None);
        assert!(headers.is_empty());
    }

    #[test]
    fn test_is_image() {
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Type";
        headers[0].value = b"image/jpeg";

        assert!(BodyParser::is_image(&headers));

        headers[0].value = b"text/plain";
        assert!(!BodyParser::is_image(&headers));
    }

    #[test]
    fn test_get_content_type() {
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Type";
        headers[0].value = b"application/json";

        let content_type = BodyParser::get_content_type(&headers);
        assert_eq!(content_type, Some("application/json"));

        headers[0].value = b"text/html; charset=utf-8";
        let content_type = BodyParser::get_content_type(&headers);
        assert_eq!(content_type, Some("text/html; charset=utf-8"));

        let empty_headers: [Header; 0] = [];
        assert_eq!(BodyParser::get_content_type(&empty_headers), None);
    }

    #[tokio::test]
    async fn test_gzip_decompression() {
        let short_data = &[0x1F];
        let result = BodyParser::decompress_gzip(short_data).await.unwrap();
        assert_eq!(result, short_data);

        let non_gzip_data = b"Hello World";
        let result = BodyParser::decompress_gzip(non_gzip_data).await.unwrap();
        assert_eq!(result, non_gzip_data);

        let data_with_gzip_header = b"prefix\x1F\x8Bgzipped";
        let index = BodyParser::find_gzip_header(data_with_gzip_header);
        assert_eq!(index, Some(6));
    }

    #[tokio::test]
    async fn test_brotli_decompression() {
        let empty_data = &[];
        let result = BodyParser::decompress_brotli(empty_data).await.unwrap();
        assert_eq!(result, empty_data);

        let short_data = &[0x01, 0x02, 0x03];
        let result = BodyParser::decompress_brotli(short_data).await.unwrap();
        assert_eq!(result, short_data);

        let non_br_data = b"Hello World";
        let result = BodyParser::decompress_brotli(non_br_data).await.unwrap();
        assert_eq!(result, non_br_data);
    }

    #[test]
    fn test_bytes_to_utf8_string() {
        let valid_utf8 = b"Hello World";
        let result = BodyParser::bytes_to_utf8_string(valid_utf8);
        assert_eq!(result, "Hello World");

        let invalid_utf8 = &[0xFF, 0xFE, 0xFD];
        let result = BodyParser::bytes_to_utf8_string(invalid_utf8);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_identify_content_category() {
        // Empty body test
        assert_eq!(
            BodyParser::identify_content_category(None, b""),
            ContentCategory::Empty
        );

        // Content-Type based tests
        assert_eq!(
            BodyParser::identify_content_category(Some("image/png"), b"data"),
            ContentCategory::Binary
        );

        assert_eq!(
            BodyParser::identify_content_category(Some("application/json"), b"data"),
            ContentCategory::Json
        );

        assert_eq!(
            BodyParser::identify_content_category(Some("text/javascript"), b"data"),
            ContentCategory::JavaScript
        );

        assert_eq!(
            BodyParser::identify_content_category(Some("text/css"), b"data"),
            ContentCategory::Css
        );

        assert_eq!(
            BodyParser::identify_content_category(Some("text/html"), b"data"),
            ContentCategory::Html
        );

        assert_eq!(
            BodyParser::identify_content_category(Some("text/xml"), b"data"),
            ContentCategory::Xml
        );

        assert_eq!(
            BodyParser::identify_content_category(Some("image/svg+xml"), b"data"),
            ContentCategory::Svg
        );

        assert_eq!(
            BodyParser::identify_content_category(Some("text/plain"), b"data"),
            ContentCategory::Text
        );

        assert_eq!(
            BodyParser::identify_content_category(Some("font/ttf"), b"data"),
            ContentCategory::Font
        );

        // Content-based detection
        assert_eq!(
            BodyParser::identify_content_category(None, b"{\"key\":\"value\"}"),
            ContentCategory::Json
        );

        assert_eq!(
            BodyParser::identify_content_category(None, b"<html><body>Hello</body></html>"),
            ContentCategory::Html
        );

        assert_eq!(
            BodyParser::identify_content_category(None, b"<svg width=\"100\"></svg>"),
            ContentCategory::Svg
        );

        // Binary detection based on magic numbers
        assert_eq!(
            BodyParser::identify_content_category(None, &[0xFF, 0xD8, 0xFF]),
            ContentCategory::Binary
        );

        assert_eq!(
            BodyParser::identify_content_category(None, &[0x89, 0x50, 0x4E, 0x47]),
            ContentCategory::Binary
        );

        assert_eq!(
            BodyParser::identify_content_category(None, &[0x47, 0x49, 0x46]),
            ContentCategory::Binary
        );
    }

    #[tokio::test]
    async fn test_process_response_body() {
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Transfer-Encoding";
        headers[0].value = b"chunked";

        let chunked_body = b"5\r\nHello\r\n6\r\n World\r\n0\r\n\r\n";
        let result = BodyParser::process_response_body(chunked_body, &headers)
            .await
            .unwrap();
        assert_eq!(result, b"Hello World");

        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Encoding";
        headers[0].value = b"gzip";

        let headers: [Header; 0] = [];
        let body = b"Hello World";
        let result = BodyParser::process_response_body(body, &headers)
            .await
            .unwrap();
        assert_eq!(result, body);
    }

    #[tokio::test]
    async fn test_format_body() {
        let json_body = br#"{"name":"test","value":123}"#;
        let formatted = BodyParser::format_body_async(json_body, &[]).await.unwrap();
        assert!(formatted.contains("{\n"));

        let text_body = b"Hello world";
        let formatted = BodyParser::format_body_async(text_body, &[]).await.unwrap();
        assert_eq!(formatted, "Hello world");

        let empty_body = b"";
        let formatted = BodyParser::format_body_async(empty_body, &[])
            .await
            .unwrap();
        assert_eq!(formatted, "# <empty body>");

        let binary_data = &[0xFF, 0xD8, 0xFF, 0xE0];
        let formatted = BodyParser::format_body_async(binary_data, &[])
            .await
            .unwrap();
        assert!(
            formatted.contains("binary"),
            "Expected binary format message, got: {}",
            formatted
        );

        let jpeg_binary_data = &[0xFF, 0xD8, 0xFF, 0xE0];
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Type";
        headers[0].value = b"image/jpeg";

        let formatted = BodyParser::format_body_async(jpeg_binary_data, &headers)
            .await
            .unwrap();
        assert!(
            formatted.contains("image"),
            "Expected image format message, got: {}",
            formatted
        );

        let css_body = b"body { color: red; }";
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Type";
        headers[0].value = b"text/css";

        let formatted = BodyParser::format_body_async(css_body, &headers)
            .await
            .unwrap();
        assert!(formatted.contains("CSS content"));

        let html_body = b"<html><body>Hello</body></html>";
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Type";
        headers[0].value = b"text/html";

        let formatted = BodyParser::format_body_async(html_body, &headers)
            .await
            .unwrap();
        assert!(formatted.contains("HTML content"));

        let xml_body = b"<?xml version=\"1.0\"?><root><item>test</item></root>";
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Type";
        headers[0].value = b"application/xml";

        let formatted = BodyParser::format_body_async(xml_body, &headers)
            .await
            .unwrap();
        assert!(formatted.contains("XML content"));

        let svg_body = b"<svg width=\"100\" height=\"100\"></svg>";
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Type";
        headers[0].value = b"image/svg+xml";

        let formatted = BodyParser::format_body_async(svg_body, &headers)
            .await
            .unwrap();
        assert!(formatted.contains("SVG content"));

        let js_body = b"function test() { return 1; }";
        let mut headers = [EMPTY_HEADER; 1];
        headers[0].name = "Content-Type";
        headers[0].value = b"application/javascript";

        let formatted = BodyParser::format_body_async(js_body, &headers)
            .await
            .unwrap();
        assert!(formatted.contains("function"));
    }

    #[test]
    fn test_is_content_too_large() {
        assert!(BodyParser::is_content_too_large(101 * 1024 * 1024));

        assert!(!BodyParser::is_content_too_large(10 * 1024 * 1024));
    }
}
