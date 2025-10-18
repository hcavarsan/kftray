use std::collections::HashMap;

use serde::{
    Deserialize,
    Serialize,
};

/// Messages sent over WebSocket tunnel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TunnelMessage {
    HttpRequest {
        id: String,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    },
    HttpResponse {
        id: String,
        status: u16,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    },
    Ping,
    Pong,
    Error {
        id: Option<String>,
        message: String,
    },
}

impl TunnelMessage {
    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}

#[allow(dead_code)]
pub fn create_http_request(
    id: String, method: String, path: String, headers: HashMap<String, String>, body: Vec<u8>,
) -> TunnelMessage {
    TunnelMessage::HttpRequest {
        id,
        method,
        path,
        headers,
        body,
    }
}

#[allow(dead_code)]
pub fn create_http_response(
    id: String, status: u16, headers: HashMap<String, String>, body: Vec<u8>,
) -> TunnelMessage {
    TunnelMessage::HttpResponse {
        id,
        status,
        headers,
        body,
    }
}

#[allow(dead_code)]
pub fn create_error(id: Option<String>, message: String) -> TunnelMessage {
    TunnelMessage::Error { id, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_ping() {
        let msg = TunnelMessage::Ping;
        let serialized = msg.serialize().unwrap();
        let deserialized = TunnelMessage::deserialize(&serialized).unwrap();

        match deserialized {
            TunnelMessage::Ping => {}
            _ => panic!("Expected Ping message"),
        }
    }

    #[test]
    fn test_serialize_deserialize_http_request() {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let msg = create_http_request(
            "req-123".to_string(),
            "GET".to_string(),
            "/api/test".to_string(),
            headers.clone(),
            vec![1, 2, 3],
        );

        let serialized = msg.serialize().unwrap();
        let deserialized = TunnelMessage::deserialize(&serialized).unwrap();

        match deserialized {
            TunnelMessage::HttpRequest {
                id,
                method,
                path,
                headers: h,
                body,
            } => {
                assert_eq!(id, "req-123");
                assert_eq!(method, "GET");
                assert_eq!(path, "/api/test");
                assert_eq!(h, headers);
                assert_eq!(body, vec![1, 2, 3]);
            }
            _ => panic!("Expected HttpRequest message"),
        }
    }

    #[test]
    fn test_serialize_deserialize_http_response() {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "text/html".to_string());

        let msg = create_http_response(
            "req-123".to_string(),
            200,
            headers.clone(),
            vec![72, 101, 108, 108, 111],
        );

        let serialized = msg.serialize().unwrap();
        let deserialized = TunnelMessage::deserialize(&serialized).unwrap();

        match deserialized {
            TunnelMessage::HttpResponse {
                id,
                status,
                headers: h,
                body,
            } => {
                assert_eq!(id, "req-123");
                assert_eq!(status, 200);
                assert_eq!(h, headers);
                assert_eq!(body, vec![72, 101, 108, 108, 111]);
            }
            _ => panic!("Expected HttpResponse message"),
        }
    }

    #[test]
    fn test_serialize_deserialize_error() {
        let msg = create_error(Some("req-456".to_string()), "Test error".to_string());

        let serialized = msg.serialize().unwrap();
        let deserialized = TunnelMessage::deserialize(&serialized).unwrap();

        match deserialized {
            TunnelMessage::Error { id, message } => {
                assert_eq!(id, Some("req-456".to_string()));
                assert_eq!(message, "Test error");
            }
            _ => panic!("Expected Error message"),
        }
    }
}
