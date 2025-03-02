use std::sync::Arc;

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::Mutex;
use tracing::{
    error,
    trace,
};

use crate::models::HttpLogState;
use crate::HttpLogger;

pub struct HttpRequestHandler {
    config_id: i64,
}

impl HttpRequestHandler {
    pub fn new(config_id: i64) -> Self {
        Self { config_id }
    }

    pub async fn handle_request_logging(
        &self, request_buffer: &[u8], already_logged: &mut bool, logger: &Option<HttpLogger>,
        http_log_state: &HttpLogState, request_id: &Arc<Mutex<Option<String>>>,
    ) -> Result<()> {
        match http_log_state.get_http_logs(self.config_id).await {
            Ok(true) => {
                *already_logged = true;

                if let Some(logger) = logger {
                    let mut req_id_guard = request_id.lock().await;
                    let req_data = Bytes::copy_from_slice(request_buffer);
                    let new_request_id = logger.log_request(req_data).await;
                    {
                        trace!("Generated new request ID: {}", new_request_id);
                        *req_id_guard = Some(new_request_id);
                    }
                }
                Ok(())
            }
            Ok(false) => {
                *already_logged = true;
                Ok(())
            }
            Err(e) => {
                error!("Failed to check HTTP logging state: {:?}", e);
                *already_logged = true;
                Err(e)
            }
        }
    }

    pub async fn is_logging_enabled(&self, http_log_state: &HttpLogState) -> Result<bool> {
        http_log_state.get_http_logs(self.config_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::HttpLogState;

    #[tokio::test]
    async fn test_is_logging_enabled() {
        let http_log_state = HttpLogState::new();
        let handler = HttpRequestHandler::new(123);

        assert!(!handler.is_logging_enabled(&http_log_state).await.unwrap());

        http_log_state.set_http_logs(123, true).await.unwrap();
        assert!(handler.is_logging_enabled(&http_log_state).await.unwrap());
    }
}
