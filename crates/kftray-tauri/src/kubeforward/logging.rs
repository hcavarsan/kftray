use std::fs::{
    self,
    File,
    OpenOptions,
};
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

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
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct Logger {
    sender: mpsc::Sender<LogEntry>,
    handle: Arc<JoinHandle<()>>,
}

enum LogEntry {
    Request(Vec<u8>),
    Response(Vec<u8>),
}

impl Logger {
    pub fn new(log_file_path: PathBuf) -> anyhow::Result<Self> {
        let (sender, mut receiver) = mpsc::channel(100);
        let log_file_path = Arc::new(log_file_path);

        let handle = tokio::spawn(async move {
            while let Some(entry) = receiver.recv().await {
                let log_file_path = log_file_path.clone();
                match entry {
                    LogEntry::Request(buffer) => {
                        if let Err(e) = log_request(&buffer, &log_file_path).await {
                            eprintln!("Failed to log request: {:?}", e);
                        }
                    }
                    LogEntry::Response(buffer) => {
                        if let Err(e) = log_response(&buffer, &log_file_path).await {
                            eprintln!("Failed to log response: {:?}", e);
                        }
                    }
                }
            }
        });

        Ok(Self {
            sender,
            handle: Arc::new(handle),
        })
    }

    pub async fn log_request(&self, buffer: Vec<u8>) {
        let _ = self.sender.send(LogEntry::Request(buffer)).await;
    }

    pub async fn log_response(&self, buffer: Vec<u8>) {
        let _ = self.sender.send(LogEntry::Response(buffer)).await;
    }

    pub async fn join_handle(&self) {
        let handle = Arc::clone(&self.handle);
        match Arc::try_unwrap(handle) {
            Ok(join_handle) => {
                join_handle.await.unwrap();
            }
            Err(_) => {
                eprintln!("Failed to join handle: multiple references to Arc");
            }
        }
    }
}

pub fn create_log_file_path(config_id: i64, local_port: u16) -> anyhow::Result<PathBuf> {
    let mut path = dirs::home_dir().unwrap();

    path.push(".kftray/http_logs");
    fs::create_dir_all(&path)?;
    path.push(format!("{}_{}.log", config_id, local_port));
    Ok(path)
}

async fn log_request(buffer: &[u8], log_file_path: &PathBuf) -> anyhow::Result<()> {
    let mut log_file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_file_path)?;
    writeln!(log_file, "\n----------------------------------------")?;
    writeln!(log_file, "Request at: {}", Utc::now().to_rfc3339())?;

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = Request::new(&mut headers);
    match req.parse(buffer) {
        Ok(Status::Complete(_)) => {
            writeln!(log_file, "Method: {}", req.method.unwrap_or(""))?;
            writeln!(log_file, "Path: {}", req.path.unwrap_or(""))?;
            writeln!(log_file, "Version: {}", req.version.unwrap_or(0))?;
            writeln!(log_file, "\nHeaders:")?;
            for header in req.headers.iter() {
                writeln!(
                    log_file,
                    "{}: {}",
                    header.name,
                    std::str::from_utf8(header.value).unwrap_or("")
                )?;
            }
        }
        Ok(Status::Partial) => {
            writeln!(log_file, "Incomplete request")?;
        }
        Err(e) => {
            writeln!(log_file, "Failed to parse request: {:?}", e)?;
        }
    }

    log_file.flush()?;
    Ok(())
}

async fn log_response(buffer: &[u8], log_file_path: &PathBuf) -> anyhow::Result<()> {
    let mut log_file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_file_path)?;

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut res = Response::new(&mut headers);
    match res.parse(buffer) {
        Ok(Status::Complete(_)) => {
            writeln!(log_file, "\n----------------------------------------")?;
            writeln!(log_file, "Response at: {}", Utc::now().to_rfc3339())?;
            writeln!(log_file, "Status: {}", res.code.unwrap_or(0))?;
            writeln!(log_file, "\nHeaders:")?;
            for header in res.headers.iter() {
                writeln!(
                    log_file,
                    "{}: {}",
                    header.name,
                    std::str::from_utf8(header.value).unwrap_or("")
                )?;
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
                            log_body(&decompressed_body, &mut log_file, res.headers).await?;
                        }
                        Err(e) => writeln!(log_file, "Failed to decompress body: {:?}", e)?,
                    }
                } else {
                    log_body(body, &mut log_file, res.headers).await?;
                }
            }
        }
        Ok(Status::Partial) => {
            return Ok(());
        }
        Err(_) => {
            return Ok(());
        }
    }

    log_file.flush()?;
    Ok(())
}

async fn log_body<'a>(
    body: &[u8], log_file: &mut File, headers: &[httparse::Header<'a>],
) -> anyhow::Result<()> {
    if !body.is_empty() {
        writeln!(log_file, "\nBody:")?;
        if let Ok(body_str) = std::str::from_utf8(body) {
            if let Ok(json_value) = serde_json::from_str::<Value>(body_str) {
                writeln!(log_file, "{}", serde_json::to_string_pretty(&json_value)?)?;
            } else {
                writeln!(log_file, "{}", body_str.trim_end())?;
            }
        } else if is_image(headers) {
            if let Ok(image) = image::load_from_memory(body) {
                let ascii_art = convert_image_to_ascii(&image)?;
                writeln!(log_file, "{}", ascii_art)?;
            } else {
                writeln!(log_file, "Failed to convert image to ascii")?;
            }
        } else {
            writeln!(log_file, "Body (as bytes): {:?}", body)?;
        }
    } else {
        writeln!(log_file, "Body is empty")?;
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
