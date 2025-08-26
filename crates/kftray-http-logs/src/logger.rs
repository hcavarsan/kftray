use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{
    Context,
    Result,
};
use bytes::{
    BufMut,
    Bytes,
    BytesMut,
};
use chrono::{
    DateTime,
    Utc,
};
use dashmap::DashMap;
use lazy_static::lazy_static;
use tokio::fs::{
    File,
    OpenOptions,
};
use tokio::io::{
    AsyncWriteExt,
    BufWriter,
};
use tokio::sync::mpsc::{
    self,
    Sender,
};
use tokio::sync::RwLock;
use tokio::time::Duration;
use tracing::{
    debug,
    error,
    trace,
};
use uuid::Uuid;

use crate::config::LogConfig;
use crate::formatter::MessageFormatter;
use crate::message::LogMessage;

#[derive(Debug, Clone)]
pub struct TraceInfo {
    pub trace_id: String,
    pub timestamp: DateTime<Utc>,
}

pub fn calculate_time_diff(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    (end - start).num_milliseconds()
}

lazy_static! {
    static ref BUFFER_POOL: Arc<tokio::sync::Mutex<Vec<BytesMut>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::with_capacity(32)));
}

const CHANNEL_CAPACITY: usize = 256;
const BATCH_SIZE_THRESHOLD: usize = 10;
const FLUSH_INTERVAL_MS: u64 = 100;
const TRACE_CLEANUP_INTERVAL_SECS: u64 = 5;
const TRACE_EXPIRY_SECS: i64 = 1800;

type TraceMap = Arc<DashMap<String, TraceInfo>>;

#[derive(Clone, Debug)]
pub struct HttpLogger {
    log_sender: Sender<LogMessage>,
    trace_map: TraceMap,
    shutdown: Arc<tokio::sync::watch::Sender<()>>,
    #[allow(dead_code)]
    config: LogConfig,
    #[allow(dead_code)]
    writer_task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    #[allow(dead_code)]
    cleanup_task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl HttpLogger {
    pub async fn new(log_config: LogConfig, log_file_path: PathBuf) -> Result<Self> {
        let (log_sender, mut log_receiver) = mpsc::channel::<LogMessage>(CHANNEL_CAPACITY);

        let log_file = Arc::new(RwLock::new(BufWriter::with_capacity(
            64 * 1024,
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(&log_file_path)
                .await
                .context("Failed to open log file")?,
        )));

        let trace_map: TraceMap = Arc::new(DashMap::with_capacity(1024));
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(());
        let mut shutdown_rx_writer = shutdown_rx.clone();

        let writer_task = tokio::spawn({
            let log_file = log_file.clone();
            async move {
                let mut flush_interval =
                    tokio::time::interval(Duration::from_millis(FLUSH_INTERVAL_MS));
                let mut message_batch = Vec::with_capacity(BATCH_SIZE_THRESHOLD * 2);
                let mut last_flush = Utc::now();

                loop {
                    tokio::select! {
                        Some(log_message) = log_receiver.recv() => {
                            if log_message.is_flush_trigger() {
                                if let Ok(mut file) = log_file.try_write() {
                                    let _ = file.flush().await;
                                }
                                continue;
                            }

                            let is_response = log_message.is_response();
                            message_batch.push(log_message);

                            let now = Utc::now();
                            let batch_too_old = now.signed_duration_since(last_flush).num_milliseconds() > FLUSH_INTERVAL_MS as i64;
                            let force_write = message_batch.len() >= BATCH_SIZE_THRESHOLD || is_response || batch_too_old;

                            if force_write {
                                let batch_size = message_batch.len();
                                debug!("Writing log batch of {} messages (contains response: {})",
                                      batch_size, is_response);

                                if let Err(e) = Self::write_log_batch(&log_file, &message_batch).await {
                                    error!("Failed to write log batch: {:?}", e);
                                } else {
                                    debug!("Successfully wrote log batch");

                                    if is_response {
                                        if let Ok(mut file) = log_file.try_write() {
                                            if let Err(e) = file.get_mut().sync_data().await {
                                                error!("Failed to sync response log to disk: {:?}", e);
                                            }
                                        }
                                    }
                                }
                                message_batch.clear();
                                last_flush = Utc::now();
                            }
                        }
                        _ = flush_interval.tick() => {
                            let now = Utc::now();
                            if !message_batch.is_empty() {
                                debug!("Timer flush: writing batch of {} messages after {}ms",
                                      message_batch.len(),
                                      now.signed_duration_since(last_flush).num_milliseconds());

                                if let Err(e) = Self::write_log_batch(&log_file, &message_batch).await {
                                    error!("Failed to write log batch: {:?}", e);
                                }
                                message_batch.clear();
                                last_flush = now;
                            }

                            if let Ok(mut file) = log_file.try_write() {
                                if let Err(e) = file.flush().await {
                                    error!("Failed to flush log file in timer: {:?}", e);
                                } else {
                                    trace!("Successfully flushed log file during periodic tick");
                                }
                            }
                        }
                        _ = shutdown_rx_writer.changed() => {
                            debug!("Shutting down log writer - processing final messages");

                            if !message_batch.is_empty() {
                                debug!("Writing final batch of {} messages during shutdown", message_batch.len());

                                if let Err(e) = Self::write_log_batch(&log_file, &message_batch).await {
                                    error!("Failed to write final log batch: {:?}", e);

                                    for (i, msg) in message_batch.iter().enumerate() {
                                        debug!("Attempting to write individual message {} during shutdown", i+1);
                                        if let Err(e) = Self::write_single_log(&log_file, msg).await {
                                            error!("Failed to write message {}: {:?}", i+1, e);
                                        }
                                    }
                                }
                            }

                            debug!("Performing final sync to ensure data durability");
                            let mut file = log_file.write().await;
                            if let Err(e) = file.flush().await {
                                error!("Failed to flush log file during shutdown: {:?}", e);
                            } else if let Err(e) = file.get_mut().sync_all().await {
                                error!("Failed to sync log file during shutdown: {:?}", e);
                            } else {
                                debug!("Successfully flushed and synced log file during shutdown");
                            }

                            debug!("Log writer shutdown complete");
                            break;
                        }
                    }
                }
            }
        });

        let cleanup_task = tokio::spawn({
            let trace_map = trace_map.clone();
            async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(TRACE_CLEANUP_INTERVAL_SECS));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let now = Utc::now();
                            trace_map.retain(|_, trace_info| {
                                now.signed_duration_since(trace_info.timestamp).num_seconds() < TRACE_EXPIRY_SECS
                            });
                        }
                        _ = shutdown_rx.changed() => {
                            debug!("Shutting down cleanup task");
                            break;
                        }
                    }
                }
            }
        });

        let writer_task_handle = Arc::new(tokio::sync::Mutex::new(Some(writer_task)));
        let cleanup_task_handle = Arc::new(tokio::sync::Mutex::new(Some(cleanup_task)));

        Ok(Self {
            log_sender,
            trace_map,
            shutdown: Arc::new(shutdown_tx),
            config: log_config,
            writer_task: writer_task_handle,
            cleanup_task: cleanup_task_handle,
        })
    }

    pub async fn for_config(config_id: i64, local_port: u16) -> Result<Self> {
        let log_config = LogConfig::new(LogConfig::default_log_directory()?);
        let log_path = log_config
            .create_log_file_path(config_id, local_port)
            .await?;
        Self::new(log_config, log_path).await
    }

    pub async fn log_request(&self, buffer: Bytes) -> String {
        let request_id = Uuid::new_v4().to_string();
        let timestamp = Utc::now();
        let trace_id = request_id.clone();

        if let Err(e) = self
            .send_request_log(buffer, trace_id.clone(), timestamp)
            .await
        {
            error!("Failed to log request: {:?}", e);
        }

        self.trace_map.insert(
            request_id.clone(),
            TraceInfo {
                trace_id,
                timestamp,
            },
        );

        request_id
    }

    pub async fn log_response(&self, buffer: Bytes, request_id: String) {
        let timestamp = Utc::now();
        let is_preformatted = buffer.len() > 5 && &buffer[0..5] == b"HTTP/";

        if let Some(trace_info) = self.trace_map.get(&request_id) {
            let took_ms = calculate_time_diff(trace_info.timestamp, timestamp);
            self.send_response_log_internal(
                buffer,
                request_id,
                timestamp,
                took_ms,
                is_preformatted,
            )
            .await;
        } else {
            debug!("No trace info found for request ID: {}", request_id);
            self.send_response_log_internal(buffer, request_id, timestamp, 0, is_preformatted)
                .await;
        }
    }

    async fn send_response_log_internal(
        &self, buffer: Bytes, request_id: String, timestamp: DateTime<Utc>, took_ms: i64,
        is_preformatted: bool,
    ) {
        let result = if is_preformatted {
            self.send_preformatted_response_log(buffer, request_id.clone(), timestamp, took_ms)
                .await
        } else {
            self.send_response_log(buffer, request_id.clone(), timestamp, took_ms)
                .await
        };

        if let Err(e) = result {
            error!("Failed to send response log: {:?}", e);
        }
    }

    async fn send_request_log(
        &self, buffer: Bytes, trace_id: String, timestamp: DateTime<Utc>,
    ) -> Result<()> {
        debug!("Formatting request log for trace ID: {}", trace_id);

        let log_entry = match MessageFormatter::format_request(&buffer, &trace_id, timestamp).await
        {
            Ok(entry) => entry,
            Err(e) => {
                debug!("Failed to parse HTTP request for logging: {:?}", e);
                return Ok(());
            }
        };

        match self.log_sender.send(log_entry).await {
            Ok(_) => debug!("Successfully sent request log message to channel"),
            Err(e) => error!("Failed to send log message: {:?}", e),
        }

        Ok(())
    }

    async fn send_response_log(
        &self, buffer: Bytes, trace_id: String, timestamp: DateTime<Utc>, took_ms: i64,
    ) -> Result<()> {
        debug!(
            "Formatting response log for trace ID: {} (size: {}B)",
            trace_id,
            buffer.len()
        );

        let is_valid_http =
            buffer.len() > 16 && matches!(buffer.get(..5), Some(prefix) if prefix == b"HTTP/");

        if !is_valid_http {
            debug!("Response doesn't appear to be a valid HTTP response, but will log anyway");
        }

        let log_entry =
            match MessageFormatter::format_response(&buffer, &trace_id, timestamp, took_ms).await {
                Ok(entry) => entry,
                Err(e) => {
                    debug!("Failed to parse HTTP response for logging: {:?}", e);
                    return Ok(());
                }
            };

        let message_size = log_entry.size();
        match self.log_sender.send(log_entry).await {
            Ok(_) => debug!(
                "Successfully sent response log message to channel (size: {}B)",
                message_size
            ),
            Err(e) => {
                error!(
                    "Failed to send response log message (size: {}B): {:?}",
                    message_size, e
                );
                return Err(anyhow::anyhow!(
                    "Failed to send response log message: {:?}",
                    e
                ));
            }
        }

        Ok(())
    }

    async fn send_preformatted_response_log(
        &self, buffer: Bytes, trace_id: String, timestamp: DateTime<Utc>, took_ms: i64,
    ) -> Result<()> {
        let message = LogMessage::new_preformatted_response(trace_id, timestamp, took_ms, buffer);

        if let Err(e) = self.log_sender.send(message).await {
            error!("Failed to send preformatted response log message: {:?}", e);
            return Err(anyhow::anyhow!("Failed to send log message"));
        }

        Ok(())
    }

    pub async fn flush(&self) -> Result<()> {
        let trigger = LogMessage::TriggerFlush;
        self.log_sender
            .send(trigger)
            .await
            .map_err(|_| anyhow::anyhow!("Failed to send flush trigger"))
    }

    async fn write_log_batch(
        log_file: &Arc<RwLock<BufWriter<File>>>, messages: &[LogMessage],
    ) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }

        let mut total_size = 0;
        let mut response_count = 0;

        for message in messages {
            total_size += message.as_bytes().len();
            if message.is_response() {
                response_count += 1;
            }
        }

        let mut combined_buffer = BytesMut::with_capacity(total_size);

        if response_count > 0 {
            debug!("Processing {} response messages in batch", response_count);

            for message in messages.iter() {
                if message.is_response() {
                    let bytes = message.as_bytes();
                    combined_buffer.put_slice(bytes);
                    trace!(
                        "Added response message to write buffer: {} bytes",
                        bytes.len()
                    );
                }
            }
        }

        for message in messages.iter() {
            if !message.is_response() {
                combined_buffer.put_slice(message.as_bytes());
            }
        }

        let mut log_file = log_file.write().await;
        debug!(
            "Acquired write lock for log file batch of {} messages (buffer size: {}B)",
            messages.len(),
            combined_buffer.len()
        );

        log_file
            .write_all(&combined_buffer)
            .await
            .context("Failed to write log entries to file")?;

        log_file
            .flush()
            .await
            .context("Failed to flush log entries to file")?;

        if response_count > 0 {
            debug!("Syncing {} response messages to disk", response_count);

            if let Err(e) = log_file.get_mut().sync_data().await {
                error!("Failed to sync log file data to disk: {:?}", e);
            } else {
                debug!("Successfully synced log file with responses to disk");
            }
        }

        debug!(
            "Successfully wrote and flushed batch of {} messages (responses: {}, total bytes: {})",
            messages.len(),
            response_count,
            combined_buffer.len()
        );

        Ok(())
    }

    async fn write_single_log(
        log_file: &Arc<RwLock<BufWriter<File>>>, message: &LogMessage,
    ) -> Result<()> {
        let mut log_file = log_file.write().await;
        trace!(
            "Acquired write lock for single {} message",
            message.message_type()
        );

        log_file
            .write_all(message.as_bytes())
            .await
            .context("Failed to write single log entry to file")?;

        log_file
            .flush()
            .await
            .context("Failed to flush single log entry to file")?;

        trace!(
            "Wrote and flushed single {} message",
            message.message_type()
        );

        Ok(())
    }

    pub async fn shutdown(&self) {
        debug!("Initiating HTTP logger shutdown sequence");

        let _ = self.flush().await;

        let shutdown_signal = self.shutdown.clone();

        let _ = shutdown_signal.send(());
        debug!("Sent shutdown signal to logger tasks");

        let timeout = Duration::from_secs(1);

        let writer_handle = {
            let mut guard = self.writer_task.lock().await;
            guard.take()
        };

        if let Some(handle) = writer_handle {
            debug!("Awaiting writer task completion");
            match tokio::time::timeout(timeout, handle).await {
                Ok(result) => {
                    if let Err(e) = result {
                        error!("Error awaiting writer task: {:?}", e);
                    }
                }
                Err(_) => error!("Writer task shutdown timed out"),
            }
        }

        let cleanup_handle = {
            let mut guard = self.cleanup_task.lock().await;
            guard.take()
        };

        if let Some(handle) = cleanup_handle {
            debug!("Awaiting cleanup task completion");
            match tokio::time::timeout(timeout, handle).await {
                Ok(result) => {
                    if let Err(e) = result {
                        error!("Error awaiting cleanup task: {:?}", e);
                    }
                }
                Err(_) => error!("Cleanup task shutdown timed out"),
            }
        }

        debug!("HTTP logger shutdown sequence completed");
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use std::io::Write;

    use chrono::Utc;
    use mockall::mock;
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;

    use super::*;

    mock! {
        pub MockFile {}
    }

    #[test]
    fn test_trace_map_operations() {
        let trace_map: Arc<DashMap<String, TraceInfo>> = Arc::new(DashMap::with_capacity(10));

        let old_time = Utc::now() - chrono::Duration::seconds(TRACE_EXPIRY_SECS + 10);

        for i in 0..5 {
            let trace_id = format!("old-trace-{i}");
            trace_map.insert(
                trace_id.clone(),
                TraceInfo {
                    trace_id,
                    timestamp: old_time,
                },
            );
        }

        for i in 0..5 {
            let trace_id = format!("recent-trace-{i}");
            trace_map.insert(
                trace_id.clone(),
                TraceInfo {
                    trace_id,
                    timestamp: Utc::now(),
                },
            );
        }

        assert_eq!(trace_map.len(), 10);

        let now = Utc::now();
        trace_map.retain(|_, info| {
            let age_secs = (now - info.timestamp).num_seconds();
            age_secs <= TRACE_EXPIRY_SECS
        });

        assert_eq!(trace_map.len(), 5);

        for i in 0..5 {
            let old_id = format!("old-trace-{i}");
            let recent_id = format!("recent-trace-{i}");

            assert!(!trace_map.contains_key(&old_id));
            assert!(trace_map.contains_key(&recent_id));
        }
    }

    #[test]
    fn test_trace_info() {
        let trace_id = "test-trace-123".to_string();
        let timestamp = Utc::now();

        let info = TraceInfo {
            trace_id: trace_id.clone(),
            timestamp,
        };

        assert_eq!(info.trace_id, trace_id);
        assert_eq!(info.timestamp, timestamp);
    }

    #[test]
    fn test_calculate_time_diff() {
        let start = Utc::now();
        let end = start + chrono::Duration::milliseconds(500);

        let diff = calculate_time_diff(start, end);
        assert_eq!(diff, 500);
    }

    #[test]
    fn test_log_message_types() {
        let req_content = "REQUEST: GET /test HTTP/1.1";
        let resp_content = "RESPONSE: HTTP/1.1 200 OK";
        let preformatted = "PREFORMATTED: Some special response";

        let req_msg = LogMessage::Request(req_content.to_string());
        let resp_msg = LogMessage::Response(resp_content.to_string());
        let preformatted_msg = LogMessage::PreformattedResponse(preformatted.to_string());
        let flush_msg = LogMessage::TriggerFlush;

        assert_eq!(req_msg.message_type(), "Request");
        assert_eq!(resp_msg.message_type(), "Response");
        assert_eq!(preformatted_msg.message_type(), "PreformattedResponse");
        assert_eq!(flush_msg.message_type(), "TriggerFlush");

        assert!(!req_msg.is_response());
        assert!(resp_msg.is_response());
        assert!(preformatted_msg.is_response());
        assert!(!flush_msg.is_response());

        assert!(!req_msg.is_flush_trigger());
        assert!(!resp_msg.is_flush_trigger());
        assert!(!preformatted_msg.is_flush_trigger());
        assert!(flush_msg.is_flush_trigger());

        assert_eq!(req_msg.size(), req_content.len());
        assert_eq!(resp_msg.size(), resp_content.len());
        assert_eq!(preformatted_msg.size(), preformatted.len());
        assert_eq!(flush_msg.size(), 0);
    }

    #[tokio::test]
    async fn test_sender_receiver_pattern() {
        let (tx, mut rx) = mpsc::channel::<LogMessage>(10);

        let test_request = LogMessage::Request("Test request".to_string());
        let test_response = LogMessage::Response("Test response".to_string());

        tx.send(test_request.clone()).await.unwrap();
        tx.send(test_response.clone()).await.unwrap();

        let received_request = rx.recv().await.unwrap();
        let received_response = rx.recv().await.unwrap();

        if let LogMessage::Request(content) = received_request {
            assert_eq!(content, "Test request");
        } else {
            panic!("Expected LogMessage::Request");
        }

        if let LogMessage::Response(content) = received_response {
            assert_eq!(content, "Test response");
        } else {
            panic!("Expected LogMessage::Response");
        }
    }

    #[test]
    fn test_create_unique_trace_id() {
        let mut trace_ids = Vec::with_capacity(100);

        for _ in 0..100 {
            let id = Uuid::new_v4().to_string();
            trace_ids.push(id);
        }

        use std::collections::HashSet;
        let trace_id_set: HashSet<_> = trace_ids.into_iter().collect();

        assert_eq!(trace_id_set.len(), 100);
    }

    #[tokio::test]
    async fn test_http_logger_creation() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test_log.txt");

        let config = LogConfig::new(temp_dir.path().to_path_buf());
        let logger = HttpLogger::new(config, file_path.clone()).await.unwrap();

        assert!(logger.log_sender.capacity() >= CHANNEL_CAPACITY);
        assert!(logger.trace_map.capacity() >= 1024);

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_log_write_batch() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("batch_test.log");

        let file = File::create(&file_path).await.unwrap();
        let buf_writer = BufWriter::new(file);

        let log_file = Arc::new(RwLock::new(buf_writer));

        let messages = vec![
            LogMessage::Request("Test request 1".to_string()),
            LogMessage::Request("Test request 2".to_string()),
            LogMessage::Response("Test response".to_string()),
        ];

        HttpLogger::write_log_batch(&log_file, &messages)
            .await
            .unwrap();

        {
            let mut writer = log_file.write().await;
            writer.flush().await.unwrap();
        }

        let contents = tokio::fs::read_to_string(&file_path).await.unwrap();

        assert!(contents.contains("Test request 1"));
        assert!(contents.contains("Test request 2"));
        assert!(contents.contains("Test response"));

        let response_pos = contents.find("Test response").unwrap();
        let request1_pos = contents.find("Test request 1").unwrap();

        assert!(
            response_pos < request1_pos,
            "Responses should appear before requests in the log file"
        );
    }

    #[tokio::test]
    async fn test_request_response_logging() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("req_resp_test.log");

        let config = LogConfig::new(temp_dir.path().to_path_buf());
        let logger = HttpLogger::new(config, file_path.clone()).await.unwrap();

        let request_data = Bytes::from_static(b"GET /test HTTP/1.1\r\nHost: example.com\r\n\r\n");
        let response_data =
            Bytes::from_static(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nSuccess");

        let request_id = logger.log_request(request_data).await;

        assert!(!request_id.is_empty());
        assert!(logger.trace_map.contains_key(&request_id));

        tokio::time::sleep(Duration::from_millis(10)).await;

        logger.log_response(response_data, request_id.clone()).await;

        // Add a longer delay to ensure the response is written to disk
        tokio::time::sleep(Duration::from_millis(50)).await;

        logger.flush().await.unwrap();

        // Add another small delay after flushing
        tokio::time::sleep(Duration::from_millis(50)).await;

        logger.shutdown().await;

        // Add delay after shutdown
        tokio::time::sleep(Duration::from_millis(50)).await;

        let contents = tokio::fs::read_to_string(&file_path).await.unwrap();

        println!("Log file contents: {contents}");

        assert!(contents.contains("GET /test") || contents.contains("/test"));
        assert!(contents.contains("200") || contents.contains("OK"));

        let trace_info = logger.trace_map.get(&request_id).unwrap();
        assert_eq!(trace_info.trace_id, request_id);
    }

    #[tokio::test]
    async fn test_write_single_log() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("single_log.txt");

        let file = File::create(&file_path).await.unwrap();
        let buf_writer = BufWriter::new(file);

        let log_file = Arc::new(RwLock::new(buf_writer));

        let message = LogMessage::Request("Single log test message".to_string());

        HttpLogger::write_single_log(&log_file, &message)
            .await
            .unwrap();

        {
            let mut writer = log_file.write().await;
            writer.flush().await.unwrap();
        }

        let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(contents.contains("Single log test message"));
    }
}
