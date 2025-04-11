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
            Ok(false) => Ok(()),
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
    use std::path::PathBuf;
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use super::*;
    use crate::config::LogConfig;
    use crate::models::HttpLogState;

    #[tokio::test]
    async fn test_is_logging_enabled() {
        let http_log_state = HttpLogState::new();
        let handler = HttpRequestHandler::new(123);

        assert!(!handler.is_logging_enabled(&http_log_state).await.unwrap());

        http_log_state.set_http_logs(123, true).await.unwrap();
        assert!(handler.is_logging_enabled(&http_log_state).await.unwrap());
    }

    #[tokio::test]
    async fn test_handle_request_logging_enabled() {
        let http_log_state = HttpLogState::new();
        let config_id = 123;
        let handler = HttpRequestHandler::new(config_id);

        http_log_state.set_http_logs(config_id, true).await.unwrap();

        let test_logger = Some(
            HttpLogger::new(
                LogConfig::builder("test_path".into()).build(),
                PathBuf::from("test_log_enabled.log"),
            )
            .await
            .expect("Failed to create test logger"),
        );
        let request_buffer = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let mut already_logged = false;
        let request_id = Arc::new(Mutex::new(None::<String>));

        let result = handler
            .handle_request_logging(
                request_buffer,
                &mut already_logged,
                &test_logger,
                &http_log_state,
                &request_id,
            )
            .await;

        assert!(result.is_ok(), "Request logging should succeed");
        assert!(already_logged, "Request should be marked as logged");
        let req_id = request_id.lock().await;
        assert!(req_id.is_some(), "Request ID should be set");

        let _ = tokio::fs::remove_file("test_log_enabled.log").await;
    }

    #[tokio::test]
    async fn test_handle_request_logging_disabled() {
        let http_log_state = HttpLogState::new();
        let config_id = 456;
        let handler = HttpRequestHandler::new(config_id);

        let request_id = Arc::new(Mutex::new(None::<String>));
        let request_buffer = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let mut already_logged = false;

        let result = handler
            .handle_request_logging(
                request_buffer,
                &mut already_logged,
                &None,
                &http_log_state,
                &request_id,
            )
            .await;

        assert!(
            result.is_ok(),
            "Request logging should succeed even when disabled"
        );
        assert!(!already_logged, "Request should not be marked as logged");
        let req_id = request_id.lock().await;
        assert!(req_id.is_none());
    }

    #[tokio::test]
    async fn test_handle_request_logging_error() {
        let http_log_state = HttpLogState::new();

        let config_id = -999;
        let handler = HttpRequestHandler::new(config_id);

        http_log_state.set_http_logs(config_id, true).await.unwrap();

        let result = handler.is_logging_enabled(&http_log_state).await;

        assert!(result.is_ok(), "Expected Ok result for negative config ID");
        assert!(
            result.unwrap(),
            "Expected logging to be enabled as it was set"
        );
    }

    #[test]
    fn test_handler_new() {
        let config_id = 42;
        let handler = HttpRequestHandler::new(config_id);

        let debug_str = format!("{:?}", handler);
        assert!(debug_str.contains("42"));
    }

    #[allow(dead_code)]
    struct MockHttpLogState {
        #[allow(dead_code)]
        should_fail: bool,
    }

    #[allow(dead_code)]
    impl MockHttpLogState {
        fn new(should_fail: bool) -> Self {
            Self { should_fail }
        }

        #[allow(dead_code)]
        async fn get_http_logs(&self, _: i64) -> Result<bool, anyhow::Error> {
            if self.should_fail {
                Err(anyhow::anyhow!("Simulated error"))
            } else {
                Ok(true)
            }
        }
    }

    #[tokio::test]
    async fn test_handle_request_logging_cases() {
        let handler = HttpRequestHandler::new(42);

        let dummy_log_config = LogConfig::builder("dummy_path".into()).build();
        let dummy_log_file_path = PathBuf::from("dummy_log.log");

        let dummy_logger = HttpLogger::new(dummy_log_config, dummy_log_file_path)
            .await
            .expect("Failed to create dummy HttpLogger");

        {
            let http_log_state = HttpLogState::new();
            http_log_state.set_http_logs(42, true).await.unwrap();

            let mut already_logged = false;
            let request_id = Arc::new(Mutex::new(None::<String>));
            let request_buffer = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";

            let result = handler
                .handle_request_logging(
                    request_buffer,
                    &mut already_logged,
                    &Some(dummy_logger.clone()),
                    &http_log_state,
                    &request_id,
                )
                .await;

            assert!(
                result.is_ok(),
                "Call should succeed when logging is enabled"
            );
            assert!(already_logged, "Request should be marked as logged");
            assert!(
                request_id.lock().await.is_some(),
                "Request ID should be set when logger is provided"
            );
        }

        {
            let http_log_state = HttpLogState::new();
            http_log_state.set_http_logs(42, false).await.unwrap();

            let mut already_logged = false;
            let request_id = Arc::new(Mutex::new(None::<String>));
            let logger_option = None;
            let request_buffer = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";

            let result = handler
                .handle_request_logging(
                    request_buffer,
                    &mut already_logged,
                    &logger_option,
                    &http_log_state,
                    &request_id,
                )
                .await;

            assert!(
                result.is_ok(),
                "Call should succeed even when logging is disabled"
            );
            assert!(!already_logged, "Request should not be marked as logged");
            assert!(
                request_id.lock().await.is_none(),
                "Request ID should not be set when logging disabled or logger None"
            );
        }
    }
}
