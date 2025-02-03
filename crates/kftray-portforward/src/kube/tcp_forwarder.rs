use std::sync::Arc;
use std::time::Duration;

use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::time::timeout;
use tracing::{
    error,
    trace,
};

use crate::http_logs::HttpLogState;
use crate::http_logs::Logger;

const BUFFER_SIZE: usize = 131072;

#[derive(Clone)]
pub struct TcpForwarder {
    config_id: i64,
    workload_type: String,
}

impl TcpForwarder {
    pub fn new(config_id: i64, workload_type: String) -> Self {
        Self {
            config_id,
            workload_type,
        }
    }

    pub async fn forward_connection(
        &self, client_conn: Arc<Mutex<TcpStream>>,
        upstream_conn: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        http_log_state: Arc<HttpLogState>, cancel_notifier: Arc<Notify>, local_port: u16,
    ) -> anyhow::Result<()> {
        let logger = if self.workload_type == "service" || self.workload_type == "pod" {
            let log_file_path =
                crate::http_logs::logging::create_log_file_path(self.config_id, local_port).await?;
            let logger = Logger::new(log_file_path).await?;
            Some(logger)
        } else {
            None
        };

        let request_id = Arc::new(Mutex::new(None));

        let mut client_conn_guard = client_conn.lock().await;
        client_conn_guard.set_nodelay(true)?;
        let (mut client_reader, mut client_writer) = tokio::io::split(&mut *client_conn_guard);

        let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream_conn);

        let client_to_upstream = self.handle_client_to_upstream(
            &mut client_reader,
            &mut upstream_writer,
            logger.clone(),
            &http_log_state,
            Arc::clone(&request_id),
            cancel_notifier.clone(),
        );

        let upstream_to_client = self.handle_upstream_to_client(
            &mut upstream_reader,
            &mut client_writer,
            logger.clone(),
            &http_log_state,
            Arc::clone(&request_id),
            cancel_notifier.clone(),
        );

        match tokio::try_join!(client_to_upstream, upstream_to_client) {
            Ok(_) => {
                trace!("Connection closed normally");
            }
            Err(e) => {
                error!(
                    error = e.as_ref() as &dyn std::error::Error,
                    "Connection closed with error"
                );
                return Err(e);
            }
        }

        Ok(())
    }

    async fn handle_client_to_upstream<'a>(
        &'a self, client_reader: &'a mut (impl AsyncReadExt + Unpin),
        upstream_writer: &'a mut (impl AsyncWriteExt + Unpin), logger: Option<Logger>,
        http_log_state: &HttpLogState, request_id: Arc<Mutex<Option<String>>>,
        cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut timeout_duration = Duration::from_secs(600);
        let mut request_buffer = Vec::new();

        loop {
            tokio::select! {
                n = timeout(timeout_duration, client_reader.read(&mut buffer)) => {
                    let n = match n {
                        Ok(Ok(n)) => n,
                        Ok(Err(e)) => {
                            error!("Error reading from client: {:?}", e);
                            return Err(e.into());
                        }
                        Err(_) => {
                            error!("Timeout reading from client");
                            return Err(anyhow::anyhow!("Timeout reading from client"));
                        }
                    };

                    if n == 0 {
                        break;
                    }

                    trace!("Read {} bytes from client", n);
                    request_buffer.extend_from_slice(&buffer[..n]);

                    match http_log_state.get_http_logs(self.config_id).await {
                        Ok(true) => {
                            if let Some(logger) = &logger {
                                let mut req_id_guard = request_id.lock().await;
                                let new_request_id = logger.log_request(request_buffer.clone().into()).await;
                                trace!("Generated new request ID: {}", new_request_id);
                                *req_id_guard = Some(new_request_id);
                            }
                        }
                        Ok(false) => {},
                        Err(e) => {
                            error!("Failed to check HTTP logging state: {:?}", e);
                        }
                    }

                    if let Err(e) = upstream_writer.write_all(&request_buffer).await {
                        error!("Error writing to upstream: {:?}", e);
                        return Err(e.into());
                    }
                    request_buffer.clear();
                },
                _ = cancel_notifier.notified() => {
                    trace!("Client to upstream task cancelled");
                    break;
                }
            }

            timeout_duration = Duration::from_secs(600);
        }

        if let Err(e) = upstream_writer.shutdown().await {
            error!("Error shutting down upstream writer: {:?}", e);
        }

        Ok(())
    }

    async fn handle_upstream_to_client<'a>(
        &'a self, upstream_reader: &'a mut (impl AsyncReadExt + Unpin),
        client_writer: &'a mut (impl AsyncWriteExt + Unpin), logger: Option<Logger>,
        http_log_state: &HttpLogState, request_id: Arc<Mutex<Option<String>>>,
        cancel_notifier: Arc<Notify>,
    ) -> anyhow::Result<()> {
        let mut buffer = [0; BUFFER_SIZE];
        let mut timeout_duration = Duration::from_secs(600);
        let mut response_buffer = Vec::new();

        loop {
            tokio::select! {
                n = timeout(timeout_duration, upstream_reader.read(&mut buffer)) => {
                    let n = match n {
                        Ok(Ok(n)) => n,
                        Ok(Err(e)) => {
                            error!("Error reading from upstream: {:?}", e);
                            return Err(e.into());
                        }
                        Err(_) => {
                            error!("Timeout reading from upstream");
                            return Err(anyhow::anyhow!("Timeout reading from upstream"));
                        }
                    };

                    if n == 0 {
                        break;
                    }

                    trace!("Read {} bytes from upstream", n);
                    response_buffer.extend_from_slice(&buffer[..n]);

                    match http_log_state.get_http_logs(self.config_id).await {
                        Ok(true) => {
                            if let Some(logger) = &logger {
                                let req_id_guard = request_id.lock().await;
                                if let Some(req_id) = &*req_id_guard {
                                    trace!("Logging response for request ID: {}", req_id);
                                    logger
                                        .log_response(response_buffer.clone().into(), req_id.clone())
                                        .await;
                                }
                            }
                        }
                        Ok(false) => {},
                        Err(e) => {
                            error!("Failed to check HTTP logging state: {:?}", e);
                        }
                    }

                    if let Err(e) = client_writer.write_all(&response_buffer).await {
                        error!("Error writing to client: {:?}", e);
                        return Err(e.into());
                    }

                    response_buffer.clear();
                },
                _ = cancel_notifier.notified() => {
                    trace!("Upstream to client task cancelled");
                    break;
                }
            }

            timeout_duration = Duration::from_secs(600);
        }

        if let Err(e) = client_writer.shutdown().await {
            error!("Error shutting down client writer: {:?}", e);
        }

        Ok(())
    }
}
