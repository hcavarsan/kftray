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
