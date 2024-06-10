use std::fs::{
    self,
    File,
};
use std::io::{
    Read,
    Write,
};
use std::path::PathBuf;
use std::sync::Arc;

use flate2::read::GzDecoder;
use image::DynamicImage;
use image_ascii::TextGenerator;
use k8s_openapi::chrono::Utc;
use serde_json::Value;
use tokio::sync::Mutex;

pub fn create_log_file_path(config_id: i64, local_port: u16) -> anyhow::Result<PathBuf> {
    let mut path = dirs::home_dir().unwrap();

    path.push(".kftray/http_logs");
    fs::create_dir_all(&path)?;
    path.push(format!("{}_{}.log", config_id, local_port));
    Ok(path)
}

pub async fn log_request(buffer: &[u8], log_file: &Arc<Mutex<File>>) -> anyhow::Result<()> {
    let mut log_file = log_file.lock().await;
    writeln!(log_file, "\n----------------------------------------")?;
    if let Ok(request) = std::str::from_utf8(buffer) {
        let (protocol, headers_body) = request.split_once("\r\n").unwrap_or(("", request));
        let (method, path, version) = parse_request_line(protocol);
        writeln!(log_file, "\nRequest:")?;
        writeln!(log_file, "{} - {} | {}", method, path, version)?;
        writeln!(log_file, "{}", Utc::now().to_rfc3339())?;
        writeln!(log_file, "\nHeaders:")?;
        writeln!(log_file, "{}", headers_body.trim_end())?;
    } else {
        writeln!(log_file, "Binary data: {:?}", buffer)?;
    }
    log_file.flush()?;
    Ok(())
}

pub async fn log_response(buffer: &[u8], log_file: &Arc<Mutex<File>>) -> anyhow::Result<()> {
    let mut log_file = log_file.lock().await;
    writeln!(log_file, "\n----------------------------------------")?;

    if let Some((headers, body)) = separate_headers_and_body(buffer) {
        if let Ok(headers_str) = std::str::from_utf8(headers) {
            let (protocol, headers) = headers_str.split_once("\r\n").unwrap_or(("", headers_str));
            writeln!(log_file, "\nResponse:")?;
            writeln!(log_file, "{}", protocol)?;
            writeln!(log_file, "{}", Utc::now().to_rfc3339())?;
            writeln!(log_file, "\nHeaders:")?;
            writeln!(log_file, "{}", headers.trim_end())?;

            if headers_str.contains("content-encoding: gzip") {
                match decompress_gzip(body) {
                    Ok(decompressed_body) => {
                        log_body(&decompressed_body, &mut log_file, headers_str).await?
                    }
                    Err(e) => writeln!(log_file, "Failed to decompress body: {:?}", e)?,
                }
            } else {
                log_body(body, &mut log_file, headers_str).await?;
            }
        } else {
            writeln!(log_file, "\nBinary headers: {:?}", headers)?;
            log_body(body, &mut log_file, "").await?;
        }
    } else {
        writeln!(log_file, "\nBinary data: {:?}", buffer)?;
    }

    log_file.flush()?;
    Ok(())
}

async fn log_body(
    body: &[u8], log_file: &mut tokio::sync::MutexGuard<'_, File>, headers: &str,
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
                writeln!(log_file, "Binary body: {:?}", body)?;
            }
        } else {
            writeln!(log_file, "Binary body: {:?}", body)?;
        }
    }
    Ok(())
}

fn decompress_gzip(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed_data = Vec::new();
    decoder.read_to_end(&mut decompressed_data)?;
    Ok(decompressed_data)
}

fn separate_headers_and_body(buffer: &[u8]) -> Option<(&[u8], &[u8])> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|pos| buffer.split_at(pos + 4))
}

fn parse_request_line(request_line: &str) -> (&str, &str, &str) {
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() == 3 {
        (parts[0], parts[1], parts[2])
    } else {
        ("", "", "")
    }
}

fn is_image(headers: &str) -> bool {
    headers.contains("Accept: image/") || headers.contains("content-type: image/")
}

fn convert_image_to_ascii(image: &DynamicImage) -> anyhow::Result<String> {
    let ascii_art = TextGenerator::new(image).generate();
    Ok(ascii_art)
}
