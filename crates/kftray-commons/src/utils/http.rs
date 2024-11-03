use std::path::PathBuf;
use std::sync::Arc;
use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use dashmap::DashMap;
use tokio::fs::{
    self,
    OpenOptions,
};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{
    self,
    Sender,
};
use uuid::Uuid;

use crate::error::Result;
use crate::utils::paths;

#[derive(Debug, Clone)]
struct TraceInfo {
    trace_id: String,
    timestamp: u128,
}

#[derive(Debug, Clone)]
pub struct HttpLogger {
    trace_map: Arc<DashMap<String, TraceInfo>>,
    log_sender: Arc<Sender<LogEntry>>,
}

#[derive(Debug)]
struct LogEntry {
    trace_id: String,
    timestamp: u128,
    duration_ms: u128,
    content: Vec<u8>,
    is_request: bool,
}

impl HttpLogger {
    pub async fn new(config_id: i64, local_port: u16) -> Result<Self> {
        let (sender, mut receiver) = mpsc::channel(100);
        let log_sender = Arc::new(sender);
        let trace_map = Arc::new(DashMap::new());

        let log_path = create_log_file_path(config_id, local_port).await?;
        let logger_clone = log_sender.clone();
        let trace_map_clone = trace_map.clone();

        tokio::spawn(async move {
            while let Some(entry) = receiver.recv().await {
                if let Err(e) = write_log_entry(&log_path, &entry).await {
                    log::error!("Failed to write log entry: {}", e);
                }
            }
        });

        Ok(Self {
            trace_map: trace_map_clone,
            log_sender: logger_clone,
        })
    }

    pub fn track_request(&self, request_id: String) -> String {
        let trace_id = Uuid::new_v4().to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();

        self.trace_map.insert(
            request_id,
            TraceInfo {
                trace_id: trace_id.clone(),
                timestamp,
            },
        );
        trace_id
    }

    pub async fn log_request(&self, request_id: &str, buffer: Vec<u8>) -> Result<()> {
        if let Some(trace_info) = self.trace_map.get(request_id) {
            let entry = LogEntry {
                trace_id: trace_info.trace_id.clone(),
                timestamp: trace_info.timestamp,
                duration_ms: 0,
                content: buffer,
                is_request: true,
            };

            self.log_sender.send(entry).await.map_err(|e| {
                log::error!("Failed to send log entry: {}", e);
                crate::error::Error::config("Failed to log request")
            })?;
        }
        Ok(())
    }

    pub async fn log_response(&self, request_id: &str, buffer: Vec<u8>) -> Result<()> {
        if let Some((_, trace_info)) = self.trace_map.remove(request_id) {
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();

            let duration = current_time.saturating_sub(trace_info.timestamp);

            let entry = LogEntry {
                trace_id: trace_info.trace_id,
                timestamp: current_time,
                duration_ms: duration,
                content: buffer,
                is_request: false,
            };

            self.log_sender.send(entry).await.map_err(|e| {
                log::error!("Failed to send log entry: {}", e);
                crate::error::Error::config("Failed to log response")
            })?;
        }
        Ok(())
    }
}

async fn write_log_entry(log_path: &PathBuf, entry: &LogEntry) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await?;

    let log_line = format!(
        "[{}] {} {} {}ms\n{}\n",
        entry.trace_id,
        if entry.is_request {
            "REQUEST"
        } else {
            "RESPONSE"
        },
        entry.timestamp,
        entry.duration_ms,
        String::from_utf8_lossy(&entry.content)
    );

    file.write_all(log_line.as_bytes()).await?;
    Ok(())
}

pub async fn create_log_file_path(config_id: i64, local_port: u16) -> Result<PathBuf> {
    let mut path = paths::get_log_dir().await?;
    fs::create_dir_all(&path).await?;
    path.push(format!("{}_{}.log", config_id, local_port));
    Ok(path)
}

pub async fn cleanup_old_logs(max_age_days: u64) -> Result<()> {
    let log_dir = paths::get_log_dir().await?;
    if !log_dir.exists() {
        return Ok(());
    }

    let now = SystemTime::now();
    let max_age = std::time::Duration::from_secs(max_age_days * 24 * 60 * 60);

    let mut entries = fs::read_dir(&log_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = now.duration_since(modified) {
                if duration > max_age {
                    fs::remove_file(entry.path()).await?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_http_logger() {
        let logger = HttpLogger::new(1, 8080).await.unwrap();
        let request_id = "test-request".to_string();

        let trace_id = logger.track_request(request_id.clone());
        assert!(!trace_id.is_empty());

        logger
            .log_request(&request_id, b"test request".to_vec())
            .await
            .unwrap();
        logger
            .log_response(&request_id, b"test response".to_vec())
            .await
            .unwrap();
    }
}
