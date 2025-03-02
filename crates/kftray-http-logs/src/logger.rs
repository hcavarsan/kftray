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
use crate::models::{
    calculate_time_diff,
    TraceInfo,
};

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

        let log_entry = MessageFormatter::format_request(&buffer, &trace_id, timestamp).await?;

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
            MessageFormatter::format_response(&buffer, &trace_id, timestamp, took_ms).await?;

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
