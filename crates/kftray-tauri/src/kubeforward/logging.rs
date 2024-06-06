use std::fs::{
    self,
};
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use flate2::read::GzDecoder;
use k8s_openapi::chrono::Utc;
use serde_json::Value;
use tokio::sync::Mutex;

pub fn create_log_file_path(config_id: i64, local_port: u16) -> anyhow::Result<PathBuf> {
    let mut path = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    path.push(".kftray/sniff");
    fs::create_dir_all(&path)?;
    path.push(format!("{}_{}.log", config_id, local_port));
    Ok(path)
}

pub async fn log_request(
    buffer: &[u8], log_file: &Arc<Mutex<std::fs::File>>,
) -> anyhow::Result<()> {
    let mut log_file = log_file.lock().await;
    writeln!(log_file, "\n----------------------------------------")?;
    if let Ok(request) = std::str::from_utf8(buffer) {
        let (protocol, headers_body) = request.split_once("\r\n").unwrap_or(("", request));
        let (method, path, version) = parse_request_line(protocol);
        writeln!(log_file, "\nRequest:")?;
        writeln!(log_file, "{} - {} | {}", method, path, version)?;
        writeln!(log_file, "\n{}", Utc::now().to_rfc3339())?;
        writeln!(log_file, "\nHeaders:")?;
        writeln!(log_file, "{}", headers_body.trim_end())?;
    } else {
        writeln!(log_file, "Binary data: {:?}", buffer)?;
    }
    log_file.flush()?;
    Ok(())
}

pub async fn log_response(
    buffer: &[u8], log_file: &Arc<Mutex<std::fs::File>>,
) -> anyhow::Result<()> {
    let mut log_file = log_file.lock().await;
    writeln!(log_file, "\n----------------------------------------")?;

    // Separate headers and body
    if let Some((headers, body)) = separate_headers_and_body(buffer) {
        if let Ok(headers_str) = std::str::from_utf8(headers) {
            let (protocol, headers) = headers_str.split_once("\r\n").unwrap_or(("", headers_str));
            writeln!(log_file, "\nResponse:")?;
            writeln!(log_file, "{}", protocol)?;
            writeln!(log_file, "\n{}", Utc::now().to_rfc3339())?;
            writeln!(log_file, "\nHeaders:")?;
            writeln!(log_file, "{}", headers.trim_end())?;

            // Check if the response is compressed
            if headers_str.contains("content-encoding: gzip") {
                match decompress_gzip(body) {
                    Ok(decompressed_body) => log_body(&decompressed_body, &mut log_file).await?,
                    Err(e) => writeln!(log_file, "Failed to decompress body: {:?}", e)?,
                }
            } else {
                log_body(body, &mut log_file).await?;
            }
        } else {
            writeln!(log_file, "\nBinary headers: {:?}", headers)?;
            log_body(body, &mut log_file).await?;
        }
    } else {
        writeln!(log_file, "\nBinary data: {:?}", buffer)?;
    }

    log_file.flush()?;
    Ok(())
}

async fn log_body(
    body: &[u8], log_file: &mut tokio::sync::MutexGuard<'_, std::fs::File>,
) -> anyhow::Result<()> {
    if !body.is_empty() {
        writeln!(log_file, "\nBody:")?;
        if let Ok(body_str) = std::str::from_utf8(body) {
            if let Ok(json_value) = serde_json::from_str::<Value>(body_str) {
                writeln!(log_file, "{}", serde_json::to_string_pretty(&json_value)?)?;
            } else {
                writeln!(log_file, "{}", body_str.trim_end())?;
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
    let mut headers_end = None;
    for i in 0..buffer.len() - 3 {
        if &buffer[i..i + 4] == b"\r\n\r\n" {
            headers_end = Some(i + 4);
            break;
        }
    }

    headers_end.map(|end| buffer.split_at(end))
}

fn parse_request_line(request_line: &str) -> (&str, &str, &str) {
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() == 3 {
        (parts[0], parts[1], parts[2])
    } else {
        ("", "", "")
    }
}
