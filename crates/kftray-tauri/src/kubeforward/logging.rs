use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use bytes::Bytes;
use flate2::read::GzDecoder;
use httparse::{
    Request,
    Response,
    Status,
};
use image::DynamicImage;
use image_ascii::TextGenerator;
use k8s_openapi::chrono::Utc;
use serde_json::Value;
use tokio::fs::{
    self,
    File,
    OpenOptions,
};
use tokio::io::AsyncWriteExt;
use tokio::sync::{
    mpsc,
    RwLock,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

#[derive(Clone)]
pub struct Logger {
    sender: mpsc::Sender<LogEntry>,
    handle: Arc<JoinHandle<()>>,
    #[allow(dead_code)]
    trace_map: Arc<RwLock<HashMap<String, String>>>,
}

enum LogEntry {
    Request(Bytes, String),
    Response(Bytes, String),
}

impl Logger {
    pub async fn new(log_file_path: PathBuf) -> anyhow::Result<Self> {
        let (sender, mut receiver) = mpsc::channel(100);
        let log_file = Arc::new(RwLock::new(
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(&log_file_path)
                .await?,
        ));
        let trace_map = Arc::new(RwLock::new(HashMap::new()));

        let handle = tokio::spawn({
            let log_file = log_file.clone();
            let trace_map = trace_map.clone();
            async move {
                while let Some(entry) = receiver.recv().await {
                    let log_file = log_file.clone();
                    let trace_map = trace_map.clone();
                    match entry {
                        LogEntry::Request(buffer, request_id) => {
                            let trace_id = Uuid::new_v4().to_string();
                            trace_map
                                .write()
                                .await
                                .insert(request_id.clone(), trace_id.clone());
                            if let Err(e) = log_request(&buffer, &log_file, &trace_id).await {
                                eprintln!("Failed to log request: {:?}", e);
                            }
                        }
                        LogEntry::Response(buffer, request_id) => {
                            if let Some(trace_id) = trace_map.read().await.get(&request_id).cloned()
                            {
                                if let Err(e) = log_response(&buffer, &log_file, &trace_id).await {
                                    eprintln!("Failed to log response: {:?}", e);
                                }
                                trace_map.write().await.remove(&request_id);
                            } else {
                                eprintln!("Trace ID not found for request ID: {}", request_id);
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            sender,
            handle: Arc::new(handle),
            trace_map,
        })
    }

    pub async fn log_request(&self, buffer: Bytes) -> String {
        let request_id = Uuid::new_v4().to_string();
        let _ = self
            .sender
            .send(LogEntry::Request(buffer, request_id.clone()))
            .await;
        request_id
    }

    pub async fn log_response(&self, buffer: Bytes, request_id: String) {
        let _ = self
            .sender
            .send(LogEntry::Response(buffer, request_id))
            .await;
    }

    pub async fn join_handle(&self) {
        let handle = Arc::clone(&self.handle);
        match Arc::try_unwrap(handle) {
            Ok(join_handle) => {
                join_handle.await.unwrap();
            }
            Err(_) => {
                eprintln!("Failed to unwrap join handle");
            }
        }
    }
}

pub async fn create_log_file_path(config_id: i64, local_port: u16) -> anyhow::Result<PathBuf> {
    let mut path = dirs::home_dir().context("Failed to get home directory")?;

    path.push(".kftray/http_logs");
    fs::create_dir_all(&path).await?;
    path.push(format!("{}_{}.log", config_id, local_port));
    Ok(path)
}

async fn log_request(
    buffer: &[u8], log_file: &Arc<RwLock<File>>, trace_id: &str,
) -> anyhow::Result<()> {
    let mut log_entry = String::new();
    log_entry.push_str("\n----------------------------------------\n");
    log_entry.push_str(&format!("Trace ID: {}\n", trace_id));
    log_entry.push_str(&format!("Request at: {}\n", Utc::now().to_rfc3339()));

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = Request::new(&mut headers);
    match req.parse(buffer) {
        Ok(Status::Complete(_)) => {
            log_entry.push_str(&format!("Method: {}\n", req.method.unwrap_or("")));
            log_entry.push_str(&format!("Path: {}\n", req.path.unwrap_or("")));
            log_entry.push_str(&format!("Version: {}\n", req.version.unwrap_or(0)));
            log_entry.push_str("\n\nHeaders:\n");
            for header in req.headers.iter() {
                log_entry.push_str(&format!(
                    "{}: {}\n",
                    header.name,
                    std::str::from_utf8(header.value).unwrap_or("")
                ));
            }
        }
        Ok(Status::Partial) => {
            // Do nothing if the request is incomplete
            return Ok(());
        }
        Err(_) => {
            // Do nothing if there's an error parsing the request
            return Ok(());
        }
    }

    let mut log_file = log_file.write().await;
    log_file.write_all(log_entry.as_bytes()).await?;
    log_file.flush().await?;
    Ok(())
}

async fn log_response(
    buffer: &[u8], log_file: &Arc<RwLock<File>>, trace_id: &str,
) -> anyhow::Result<()> {
    let mut log_entry = String::new();
    log_entry.push_str("\n----------------------------------------\n");
    log_entry.push_str(&format!("Trace ID: {}\n", trace_id));
    log_entry.push_str(&format!("Response at: {}\n", Utc::now().to_rfc3339()));

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut res = Response::new(&mut headers);
    match res.parse(buffer) {
        Ok(Status::Complete(_)) => {
            log_entry.push_str(&format!("Status: {}\n", res.code.unwrap_or(0)));
            log_entry.push_str("\n\nHeaders:\n");
            for header in res.headers.iter() {
                log_entry.push_str(&format!(
                    "{}: {}\n",
                    header.name,
                    std::str::from_utf8(header.value).unwrap_or("")
                ));
            }

            let headers_len = buffer
                .iter()
                .position(|&b| b == b'\r')
                .unwrap_or(buffer.len())
                + 4;
            if let Some(body) = buffer.get(headers_len..) {
                if res.headers.iter().any(|h| {
                    h.name.eq_ignore_ascii_case("content-encoding")
                        && h.value.eq_ignore_ascii_case(b"gzip")
                }) {
                    match decompress_gzip(body) {
                        Ok(decompressed_body) => {
                            log_body(&decompressed_body, &mut log_entry, res.headers).await?;
                        }
                        Err(e) => {
                            log_entry.push_str(&format!("Failed to decompress body: {:?}\n", e))
                        }
                    }
                } else {
                    log_body(body, &mut log_entry, res.headers).await?;
                }
            }
        }
        Ok(Status::Partial) => {
            // Do nothing if the response is incomplete
            return Ok(());
        }
        Err(_) => {
            // Do nothing if there's an error parsing the response
            return Ok(());
        }
    }

    let mut log_file = log_file.write().await;
    log_file.write_all(log_entry.as_bytes()).await?;
    log_file.flush().await?;
    Ok(())
}

async fn log_body<'a>(
    body: &[u8], log_entry: &mut String, headers: &[httparse::Header<'a>],
) -> anyhow::Result<()> {
    if !body.is_empty() {
        log_entry.push_str("\nBody:\n");
        if let Ok(body_str) = std::str::from_utf8(body) {
            if let Ok(json_value) = serde_json::from_str::<Value>(body_str) {
                log_entry.push_str(&format!("{}\n", serde_json::to_string_pretty(&json_value)?));
            } else {
                log_entry.push_str(&format!("{}\n", body_str.trim_end()));
            }
        } else if is_image(headers) {
            if let Ok(image) = image::load_from_memory(body) {
                let ascii_art = convert_image_to_ascii(&image)?;
                log_entry.push_str(&format!("{}\n", ascii_art));
            } else {
                log_entry.push_str("Failed to convert image to ascii\n");
            }
        } else {
            log_entry.push_str(&format!("Body (as bytes): {:?}\n", body));
        }
    } else {
        log_entry.push_str("Body is empty\n");
    }
    Ok(())
}

fn decompress_gzip(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed_data = Vec::new();
    decoder.read_to_end(&mut decompressed_data)?;
    Ok(decompressed_data)
}

fn is_image(headers: &[httparse::Header<'_>]) -> bool {
    headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("Accept") && h.value.starts_with(b"image/"))
        || headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value.starts_with(b"image/"))
}

fn convert_image_to_ascii(image: &DynamicImage) -> anyhow::Result<String> {
    let ascii_art = TextGenerator::new(image).generate();
    Ok(ascii_art)
}
