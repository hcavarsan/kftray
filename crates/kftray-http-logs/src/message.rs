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

#[derive(Debug)]
pub enum LogMessage {
    Request(String),
    Response(String),
    PreformattedResponse(String),
}

impl LogMessage {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            LogMessage::Request(log) => log.as_bytes(),
            LogMessage::Response(log) => log.as_bytes(),
            LogMessage::PreformattedResponse(log) => log.as_bytes(),
        }
    }

    pub fn size(&self) -> usize {
        self.as_bytes().len()
    }

    pub fn message_type(&self) -> &'static str {
        match self {
            LogMessage::Request(_) => "Request",
            LogMessage::Response(_) => "Response",
            LogMessage::PreformattedResponse(_) => "PreformattedResponse",
        }
    }

    pub fn is_response(&self) -> bool {
        matches!(
            self,
            LogMessage::Response(_) | LogMessage::PreformattedResponse(_)
        )
    }

    pub fn new_preformatted_response(
        trace_id: String, timestamp: DateTime<Utc>, took_ms: i64, buffer: Bytes,
    ) -> Self {
        let formatted =
            MessageFormatter::format_preformatted_response(&trace_id, timestamp, took_ms, &buffer);

        LogMessage::PreformattedResponse(formatted)
    }
}
