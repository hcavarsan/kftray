use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use bytes::Bytes;
use dashmap::DashMap;
use flate2::read::GzDecoder;
use httparse::Status;
use k8s_openapi::chrono::{
    DateTime,
    Utc,
};
use kftray_commons::utils::config_dir::get_log_folder_path;
use serde_json::Value;
use tokio::fs::{
    self,
    File,
    OpenOptions,
};
use tokio::io::{
    AsyncWriteExt,
    BufWriter,
};
use tokio::spawn;
use tokio::sync::mpsc::{
    self,
    Sender,
};
use tokio::sync::RwLock;
use tokio::task;
use tokio::time::Duration;
use tracing::{
    debug,
    error,
};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Logger {
    log_sender: Sender<LogMessage>,
    trace_map: TraceMap,
    shutdown: Arc<tokio::sync::watch::Sender<()>>,
}

type TraceMap = Arc<DashMap<String, TraceInfo>>;

#[derive(Clone, Debug)]
struct TraceInfo {
    trace_id: String,
    timestamp: DateTime<Utc>,
}

impl Logger {
    pub async fn new(log_file_path: PathBuf) -> anyhow::Result<Self> {
        let (log_sender, mut log_receiver) = mpsc::channel(100);
        let log_file = Arc::new(RwLock::new(BufWriter::new(
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(&log_file_path)
                .await?,
        )));
        let trace_map: TraceMap = Arc::new(DashMap::new());

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(());
        let mut shutdown_rx_writer = shutdown_rx.clone();

        // Spawn log writer with shutdown handling
        let _log_writer = {
            let log_file = log_file.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        Some(log_message) = log_receiver.recv() => {
                            if let Err(e) = write_log(&log_file, log_message).await {
                                error!("Failed to write log: {:?}", e);
                            }
                        }
                        _ = shutdown_rx_writer.changed() => {
                            debug!("Shutting down log writer");
                            break;
                        }
                    }
                }
            })
        };

        // Spawn cleanup task with shutdown handling
        let _cleanup_task = {
            let trace_map = trace_map.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(300));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let now = Utc::now();
                            trace_map.retain(|_, trace_info| {
                                now.signed_duration_since(trace_info.timestamp)
                                    .num_minutes() < 30
                            });
                        }
                        _ = shutdown_rx.changed() => {
                            debug!("Shutting down cleanup task");
                            break;
                        }
                    }
                }
            })
        };

        Ok(Self {
            log_sender,
            trace_map,
            shutdown: Arc::new(shutdown_tx),
        })
    }

    pub async fn log_request(&self, buffer: Bytes) -> String {
        let request_id = Uuid::new_v4().to_string();
        let timestamp = Utc::now();
        let trace_id = request_id.clone();

        // Log request before spawning async task
        let log_sender = self.log_sender.clone();
        if let Err(e) = log_request(
            buffer.clone(),
            log_sender.clone(),
            trace_id.clone(),
            timestamp,
        )
        .await
        {
            error!("Failed to log request: {:?}", e);
        }

        self.trace_map.insert(
            request_id.clone(),
            TraceInfo {
                trace_id: trace_id.clone(),
                timestamp,
            },
        );

        request_id
    }

    pub async fn log_response(&self, buffer: Bytes, request_id: String) {
        let timestamp = Utc::now();
        debug!("Logging response for request ID: {}", request_id);

        if let Some((_, trace_info)) = self.trace_map.remove(&request_id) {
            let took = calculate_time_diff(trace_info.timestamp, timestamp);
            let log_sender = self.log_sender.clone();
            spawn(async move {
                log_response(buffer, log_sender, trace_info.trace_id, timestamp, took)
                    .await
                    .unwrap_or_else(|e| error!("Failed to log response: {:?}", e));
            });
        } else {
            error!("Trace ID not found for request ID: {}", request_id);
        }
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown.send(());
    }
}

pub async fn create_log_file_path(
    config_id: i64, local_port: u16,
) -> Result<PathBuf, anyhow::Error> {
    let mut path = get_log_folder_path()
        .map_err(|e| anyhow::anyhow!(e))
        .context("Failed to get log folder path")?;

    fs::create_dir_all(&path)
        .await
        .context("Failed to create log directory")?;

    path.push(format!("{}_{}.log", config_id, local_port));

    Ok(path)
}

async fn log_request(
    buffer: Bytes, log_sender: Sender<LogMessage>, trace_id: String, timestamp: DateTime<Utc>,
) -> anyhow::Result<()> {
    debug!("Logging request with trace ID: {}", trace_id);
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    req.parse(&buffer)?;

    let log_entry = format_request_log(&buffer, &trace_id, timestamp).await?;
    if log_sender.try_send(LogMessage::Request(log_entry)).is_err() {
        error!("Log channel is full, dropping log message");
    }
    Ok(())
}

async fn log_response(
    buffer: Bytes, log_sender: tokio::sync::mpsc::Sender<LogMessage>, trace_id: String,
    timestamp: DateTime<Utc>, took: i64,
) -> anyhow::Result<()> {
    debug!("Logging response with trace ID: {}", trace_id);
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut res = httparse::Response::new(&mut headers);
    res.parse(&buffer)?;

    let log_entry = format_response_log(&buffer, &trace_id, timestamp, took).await?;
    if log_sender
        .try_send(LogMessage::Response(log_entry))
        .is_err()
    {
        error!("Log channel is full, dropping log message");
    }
    Ok(())
}

async fn format_request_log(
    buffer: &[u8], trace_id: &str, timestamp: DateTime<Utc>,
) -> anyhow::Result<String> {
    let mut log_entry = format!(
        "\n----------------------------------------\n\
         Trace ID: {}\n\
         Request at: {}\n",
        trace_id,
        timestamp.to_rfc3339()
    );

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    if let Ok(Status::Complete(_)) = req.parse(buffer) {
        log_entry.push_str(&format!(
            "Method: {}\nPath: {}\nVersion: {}\n\nHeaders:\n",
            req.method.unwrap_or(""),
            req.path.unwrap_or(""),
            req.version.unwrap_or(0)
        ));
        for header in req.headers.iter() {
            log_entry.push_str(&format!(
                "{}: {}\n",
                header.name,
                std::str::from_utf8(header.value).unwrap_or("")
            ));
        }

        if let Some(headers_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let body_start = headers_end + 4;
            let content_length = req
                .headers
                .iter()
                .find_map(|h| {
                    if h.name.eq_ignore_ascii_case("content-length") {
                        std::str::from_utf8(h.value)
                            .ok()
                            .and_then(|v| v.parse::<usize>().ok())
                    } else {
                        None
                    }
                })
                .unwrap_or(0);

            let body_end = body_start.saturating_add(content_length);
            if body_end > buffer.len() {
                log_entry.push_str("\n\nBody:\n<invalid content length>\n");
            } else if let Some(body) = buffer.get(body_start..body_end) {
                if req.headers.iter().any(|h| {
                    h.name.eq_ignore_ascii_case("content-encoding")
                        && h.value.eq_ignore_ascii_case(b"gzip")
                }) {
                    match decompress_gzip(body).await {
                        Ok(decompressed_body) => {
                            log_body(&decompressed_body, &mut log_entry, req.headers).await?;
                        }
                        Err(e) => {
                            log_entry
                                .push_str(&format!("Failed to decompress request body: {:?}\n", e));
                        }
                    }
                } else {
                    log_body(body, &mut log_entry, req.headers).await?;
                }
            }
        }
    }

    Ok(log_entry)
}

async fn format_response_log(
    buffer: &[u8], trace_id: &str, timestamp: DateTime<Utc>, took: i64,
) -> anyhow::Result<String> {
    let mut log_entry = format!(
        "\n----------------------------------------\n\
         Trace ID: {}\n\
         Response at: {}\n\
         Took: {} ms\n",
        trace_id,
        timestamp.to_rfc3339(),
        took
    );

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut res = httparse::Response::new(&mut headers);
    if let Ok(Status::Complete(_)) = res.parse(buffer) {
        log_entry.push_str(&format!("Status: {}\n\nHeaders:\n", res.code.unwrap_or(0)));
        for header in res.headers.iter() {
            log_entry.push_str(&format!(
                "{}: {}\n",
                header.name,
                std::str::from_utf8(header.value).unwrap_or("")
            ));
        }

        if let Some(headers_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let body_start = headers_end + 4;
            let content_length = res
                .headers
                .iter()
                .find_map(|h| {
                    if h.name.eq_ignore_ascii_case("content-length") {
                        std::str::from_utf8(h.value)
                            .ok()
                            .and_then(|v| v.parse::<usize>().ok())
                    } else {
                        None
                    }
                })
                .unwrap_or(0);

            let body_end = body_start.saturating_add(content_length);
            if body_end > buffer.len() {
                log_entry.push_str("\n\nBody:\n<invalid content length>\n");
            } else if let Some(body) = buffer.get(body_start..body_end) {
                if res.headers.iter().any(|h| {
                    h.name.eq_ignore_ascii_case("content-encoding")
                        && h.value.eq_ignore_ascii_case(b"gzip")
                }) {
                    match decompress_gzip(body).await {
                        Ok(decompressed_body) => {
                            log_body(&decompressed_body, &mut log_entry, res.headers).await?;
                        }
                        Err(e) => {
                            log_entry.push_str(&format!(
                                "Failed to decompress response body: {:?}\n",
                                e
                            ));
                        }
                    }
                } else {
                    log_body(body, &mut log_entry, res.headers).await?;
                }
            }
        }
    }

    Ok(log_entry)
}

const MAX_CONTENT_LENGTH: usize = 10 * 1024 * 1024; // 10MB

async fn log_body(
    body: &[u8], log_entry: &mut String, headers: &[httparse::Header<'_>],
) -> anyhow::Result<()> {
    let content_length = headers
        .iter()
        .find_map(|h| {
            if h.name.eq_ignore_ascii_case("content-length") {
                std::str::from_utf8(h.value)
                    .ok()
                    .and_then(|v| v.parse::<usize>().ok())
            } else {
                None
            }
        })
        .unwrap_or(0);

    if content_length > MAX_CONTENT_LENGTH {
        log_entry.push_str("\n\nBody:\n<content too large>\n");
        return Ok(());
    }

    if !body.is_empty() {
        log_entry.push_str("\n\nBody:\n");
        if let Ok(body_str) = std::str::from_utf8(body) {
            if let Ok(json_value) = serde_json::from_str::<Value>(body_str) {
                log_entry.push_str(&format!("{}\n", serde_json::to_string_pretty(&json_value)?));
            } else {
                log_entry.push_str(&format!("{}\n", body_str.trim_end()));
            }
        } else if is_image(headers) {
            log_entry.push_str("<image>\n");
        } else {
            log_entry.push_str("<binary>\n");
        }
    } else {
        log_entry.push_str("\n\nBody:\n<empty>\n");
    }
    Ok(())
}

async fn decompress_gzip(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let data = data.to_vec();
    task::spawn_blocking(move || {
        let mut decoder = GzDecoder::new(&data[..]);
        let mut decompressed_data = Vec::new();
        decoder.read_to_end(&mut decompressed_data)?;
        Ok(decompressed_data)
    })
    .await?
}

fn is_image(headers: &[httparse::Header<'_>]) -> bool {
    headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value.starts_with(b"image/"))
}

async fn write_log(
    log_file: &Arc<RwLock<BufWriter<File>>>, log_message: LogMessage,
) -> anyhow::Result<()> {
    let mut log_file = log_file.write().await;
    debug!("Acquired write lock for log file");

    log_file
        .write_all(log_message.as_bytes())
        .await
        .context("Failed to write log entry to file")?;
    debug!("Wrote log entry to file");

    log_file
        .flush()
        .await
        .context("Failed to flush log entry to file")?;
    debug!("Flushed log entry to file");

    Ok(())
}

enum LogMessage {
    Request(String),
    Response(String),
}

impl LogMessage {
    fn as_bytes(&self) -> &[u8] {
        match self {
            LogMessage::Request(log) => log.as_bytes(),
            LogMessage::Response(log) => log.as_bytes(),
        }
    }
}

fn calculate_time_diff(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    (end - start).num_milliseconds()
}
