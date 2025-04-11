use bytes::Bytes;
use chrono::{
    DateTime,
    Utc,
};

use crate::formatter::MessageFormatter;

#[derive(Debug, Clone)]
pub enum HttpMessage {
    Request {
        trace_id: String,
        timestamp: DateTime<Utc>,
        buffer: Bytes,
    },
    Response {
        trace_id: String,
        timestamp: DateTime<Utc>,
        took_ms: i64,
        buffer: Bytes,
    },
}

#[derive(Debug, Clone)]
pub enum LogMessage {
    Request(String),
    Response(String),
    PreformattedResponse(String),
    TriggerFlush,
}

impl LogMessage {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            LogMessage::Request(log) => log.as_bytes(),
            LogMessage::Response(log) => log.as_bytes(),
            LogMessage::PreformattedResponse(log) => log.as_bytes(),
            LogMessage::TriggerFlush => &[],
        }
    }

    pub fn size(&self) -> usize {
        match self {
            LogMessage::TriggerFlush => 0,
            _ => self.as_bytes().len(),
        }
    }

    pub fn message_type(&self) -> &'static str {
        match self {
            LogMessage::Request(_) => "Request",
            LogMessage::Response(_) => "Response",
            LogMessage::PreformattedResponse(_) => "PreformattedResponse",
            LogMessage::TriggerFlush => "TriggerFlush",
        }
    }

    pub fn is_response(&self) -> bool {
        matches!(
            self,
            LogMessage::Response(_) | LogMessage::PreformattedResponse(_)
        )
    }

    pub fn is_flush_trigger(&self) -> bool {
        matches!(self, LogMessage::TriggerFlush)
    }

    pub fn new_preformatted_response(
        trace_id: String, timestamp: DateTime<Utc>, took_ms: i64, buffer: Bytes,
    ) -> Self {
        let formatted =
            MessageFormatter::format_preformatted_response(&trace_id, timestamp, took_ms, &buffer);

        LogMessage::PreformattedResponse(formatted)
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use chrono::Utc;

    use super::*;

    #[test]
    fn test_http_message_request() {
        let trace_id = "test-trace-123".to_string();
        let timestamp = Utc::now();
        let buffer = Bytes::from("GET / HTTP/1.1\r\nHost: example.com\r\n\r\n");

        let msg = HttpMessage::Request {
            trace_id: trace_id.clone(),
            timestamp,
            buffer: buffer.clone(),
        };

        if let HttpMessage::Request {
            trace_id: tid,
            timestamp: ts,
            buffer: buf,
        } = msg
        {
            assert_eq!(tid, trace_id);
            assert_eq!(ts, timestamp);
            assert_eq!(buf, buffer);
        } else {
            panic!("Expected HttpMessage::Request");
        }
    }

    #[test]
    fn test_http_message_response() {
        let trace_id = "test-trace-456".to_string();
        let timestamp = Utc::now();
        let took_ms = 123;
        let buffer = Bytes::from("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nHello");

        let msg = HttpMessage::Response {
            trace_id: trace_id.clone(),
            timestamp,
            took_ms,
            buffer: buffer.clone(),
        };

        if let HttpMessage::Response {
            trace_id: tid,
            timestamp: ts,
            took_ms: tm,
            buffer: buf,
        } = msg
        {
            assert_eq!(tid, trace_id);
            assert_eq!(ts, timestamp);
            assert_eq!(tm, took_ms);
            assert_eq!(buf, buffer);
        } else {
            panic!("Expected HttpMessage::Response");
        }
    }

    #[test]
    fn test_log_message_as_bytes() {
        let req_content = "Request content".to_string();
        let resp_content = "Response content".to_string();
        let preformatted = "Preformatted content".to_string();

        let req_msg = LogMessage::Request(req_content.clone());
        let resp_msg = LogMessage::Response(resp_content.clone());
        let preformatted_msg = LogMessage::PreformattedResponse(preformatted.clone());
        let flush_msg = LogMessage::TriggerFlush;

        assert_eq!(req_msg.as_bytes(), req_content.as_bytes());
        assert_eq!(resp_msg.as_bytes(), resp_content.as_bytes());
        assert_eq!(preformatted_msg.as_bytes(), preformatted.as_bytes());

        // Use Vec type explicitly to fix the error
        let empty_vec: Vec<u8> = Vec::new();
        assert_eq!(flush_msg.as_bytes(), empty_vec.as_slice());
    }

    #[test]
    fn test_log_message_size() {
        let req_content = "Request content".to_string();
        let resp_content = "Response content".to_string();
        let preformatted = "Preformatted content".to_string();

        let req_msg = LogMessage::Request(req_content.clone());
        let resp_msg = LogMessage::Response(resp_content.clone());
        let preformatted_msg = LogMessage::PreformattedResponse(preformatted.clone());
        let flush_msg = LogMessage::TriggerFlush;

        assert_eq!(req_msg.size(), req_content.len());
        assert_eq!(resp_msg.size(), resp_content.len());
        assert_eq!(preformatted_msg.size(), preformatted.len());
        assert_eq!(flush_msg.size(), 0);
    }

    #[test]
    fn test_log_message_type() {
        let req_msg = LogMessage::Request("req".to_string());
        let resp_msg = LogMessage::Response("resp".to_string());
        let preformatted_msg = LogMessage::PreformattedResponse("pre".to_string());
        let flush_msg = LogMessage::TriggerFlush;

        assert_eq!(req_msg.message_type(), "Request");
        assert_eq!(resp_msg.message_type(), "Response");
        assert_eq!(preformatted_msg.message_type(), "PreformattedResponse");
        assert_eq!(flush_msg.message_type(), "TriggerFlush");
    }

    #[test]
    fn test_is_response() {
        let req_msg = LogMessage::Request("req".to_string());
        let resp_msg = LogMessage::Response("resp".to_string());
        let preformatted_msg = LogMessage::PreformattedResponse("pre".to_string());
        let flush_msg = LogMessage::TriggerFlush;

        assert!(!req_msg.is_response());
        assert!(resp_msg.is_response());
        assert!(preformatted_msg.is_response());
        assert!(!flush_msg.is_response());
    }

    #[test]
    fn test_is_flush_trigger() {
        let req_msg = LogMessage::Request("req".to_string());
        let resp_msg = LogMessage::Response("resp".to_string());
        let preformatted_msg = LogMessage::PreformattedResponse("pre".to_string());
        let flush_msg = LogMessage::TriggerFlush;

        assert!(!req_msg.is_flush_trigger());
        assert!(!resp_msg.is_flush_trigger());
        assert!(!preformatted_msg.is_flush_trigger());
        assert!(flush_msg.is_flush_trigger());
    }

    #[test]
    fn test_new_preformatted_response() {
        let formatted = "Test formatted response".to_string();
        let msg = LogMessage::PreformattedResponse(formatted);

        if let LogMessage::PreformattedResponse(content) = msg {
            assert_eq!(content, "Test formatted response");
        } else {
            panic!("Expected LogMessage::PreformattedResponse");
        }
    }
}
