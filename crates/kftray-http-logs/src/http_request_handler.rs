use std::sync::Arc;

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::Mutex;
use tracing::trace;

use crate::HttpLogger;

#[derive(Debug)]
pub struct HttpRequestHandler {
    config_id: i64,
}

impl HttpRequestHandler {
    pub fn new(config_id: i64) -> Self {
        Self { config_id }
    }

    pub async fn handle_request_logging(
        &self, request_buffer: &[u8], already_logged: &mut bool, logger: &Option<HttpLogger>,
        request_id: &Arc<Mutex<Option<String>>>,
    ) -> Result<()> {
        let is_enabled =
            match kftray_commons::utils::http_logs_config::get_http_logs_config(self.config_id)
                .await
            {
                Ok(config) => config.enabled,
                Err(_) => false,
            };

        if is_enabled {
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
        }

        Ok(())
    }

    pub async fn is_logging_enabled(&self) -> Result<bool> {
        match kftray_commons::utils::http_logs_config::get_http_logs_config(self.config_id).await {
            Ok(config) => Ok(config.enabled),
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // HttpLogState removed - using direct database access

    #[tokio::test]
    async fn test_is_logging_enabled() {
        let handler = HttpRequestHandler::new(123);

        assert!(!handler.is_logging_enabled().await.unwrap());
    }

    #[test]
    fn test_handler_new() {
        let config_id = 42;
        let handler = HttpRequestHandler::new(config_id);

        let debug_str = format!("{handler:?}");
        assert!(debug_str.contains("42"));
    }
}
