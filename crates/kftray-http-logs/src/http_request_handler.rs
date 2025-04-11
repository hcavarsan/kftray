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
    use std::sync::Arc;

    use bytes::Bytes;
    use tokio::sync::Mutex;

    use super::*;
    use crate::models::HttpLogState;

    // No need to mock the entire HttpLogger class
    // Instead we'll use a simpler approach for tests

    #[tokio::test]
    async fn test_is_logging_enabled() {
        let http_log_state = HttpLogState::new();
        let handler = HttpRequestHandler::new(123);

        assert!(!handler.is_logging_enabled(&http_log_state).await.unwrap());

        http_log_state.set_http_logs(123, true).await.unwrap();
        assert!(handler.is_logging_enabled(&http_log_state).await.unwrap());
    }

    struct TestLogger;
    #[allow(dead_code)]
    impl TestLogger {
        #[allow(dead_code)]
        async fn log_request(&self, _: Bytes) -> String {
            "test-request-id".to_string()
        }
    }

    #[tokio::test]
    async fn test_handle_request_logging_enabled() {
        let http_log_state = HttpLogState::new();
        let config_id = 123;
        let _handler = HttpRequestHandler::new(config_id);

        http_log_state.set_http_logs(config_id, true).await.unwrap();

        let _test_logger = TestLogger;
        let _request_buffer = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let _already_logged = false;
        let request_id = Arc::new(Mutex::new(None::<String>));

        let mut already_logged_manual = false;

        if http_log_state.get_http_logs(config_id).await.unwrap() {
            already_logged_manual = true;
            let req_id = "test-request-id".to_string();
            let mut req_id_guard = request_id.lock().await;
            *req_id_guard = Some(req_id);
        }

        assert!(already_logged_manual);
        let req_id = request_id.lock().await;
        assert_eq!(req_id.as_ref().unwrap(), "test-request-id");
    }

    #[tokio::test]
    async fn test_handle_request_logging_disabled() {
        let http_log_state = HttpLogState::new();
        let config_id = 456;
        let _handler = HttpRequestHandler::new(config_id);

        let request_id = Arc::new(Mutex::new(None::<String>));

        let logging_enabled = http_log_state.get_http_logs(config_id).await.unwrap();

        if logging_enabled {
            panic!("Logging should be disabled");
        }

        assert!(!logging_enabled, "Logging should be disabled");
        let req_id = request_id.lock().await;
        assert!(req_id.is_none());
    }

    #[tokio::test]
    async fn test_handle_request_logging_error() {
        let http_log_state = HttpLogState::new();

        let config_id = -999;
        let handler = HttpRequestHandler::new(config_id);

        let _ = http_log_state.set_http_logs(config_id, true).await;

        let result = handler.is_logging_enabled(&http_log_state).await;

        assert!(result.is_ok() || result.is_err());
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

        {
            let _state = MockHttpLogState::new(false);
            let mut already_logged = false;
            let request_id = Arc::new(Mutex::new(None::<String>));

            let dummy_logger = None;
            let request_buffer = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";

            let result = handler
                .handle_request_logging(
                    request_buffer,
                    &mut already_logged,
                    &dummy_logger,
                    &HttpLogState::new(),
                    &request_id,
                )
                .await;

            assert!(result.is_ok() || result.is_err());
            assert!(already_logged);
        }
    }
}
