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
        #[serde(with = "header_pairs")]
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    HttpResponse {
        id: String,
        status: u16,
        #[serde(with = "header_pairs")]
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    Ping,
    Pong,
    Error {
        id: Option<String>,
        message: String,
    },
}

/// Headers go on the wire as a JSON object with one entry per value, so a
/// repeated name repeats the key. Peers that still read headers into a map
/// parse the message and keep the last value.
mod header_pairs {
    use std::fmt;

    use serde::de::{
        MapAccess,
        Visitor,
    };
    use serde::ser::SerializeMap;
    use serde::{
        Deserializer,
        Serializer,
    };

    pub fn serialize<S: Serializer>(
        headers: &[(String, String)], serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(headers.len()))?;
        for (name, value) in headers {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<(String, String)>, D::Error> {
        struct PairsVisitor;

        impl<'de> Visitor<'de> for PairsVisitor {
            type Value = Vec<(String, String)>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of header names to values")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut headers = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some(entry) = map.next_entry()? {
                    headers.push(entry);
                }
                Ok(headers)
            }
        }

        deserializer.deserialize_map(PairsVisitor)
    }
}

impl TunnelMessage {
    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
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
        let headers = vec![("Content-Type".to_string(), "application/json".to_string())];

        let msg = TunnelMessage::HttpRequest {
            id: "req-123".to_string(),
            method: "GET".to_string(),
            path: "/api/test".to_string(),
            headers: headers.clone(),
            body: vec![1, 2, 3],
        };

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
        let headers = vec![("Content-Type".to_string(), "text/html".to_string())];

        let msg = TunnelMessage::HttpResponse {
            id: "req-123".to_string(),
            status: 200,
            headers: headers.clone(),
            body: vec![72, 101, 108, 108, 111],
        };

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
    fn repeated_headers_keep_every_value_and_still_parse_as_a_map() {
        use std::collections::HashMap;

        #[derive(Deserialize)]
        #[serde(tag = "type")]
        enum MapPeerMessage {
            HttpResponse { headers: HashMap<String, String> },
        }

        let headers = vec![
            ("set-cookie".to_string(), "session=abc".to_string()),
            ("content-type".to_string(), "text/plain".to_string()),
            ("set-cookie".to_string(), "csrf=xyz".to_string()),
        ];
        let serialized = TunnelMessage::HttpResponse {
            id: "req-1".to_string(),
            status: 200,
            headers: headers.clone(),
            body: Vec::new(),
        }
        .serialize()
        .unwrap();

        match TunnelMessage::deserialize(&serialized).unwrap() {
            TunnelMessage::HttpResponse { headers: h, .. } => assert_eq!(h, headers),
            other => panic!("Expected HttpResponse message, got {other:?}"),
        }

        let MapPeerMessage::HttpResponse { headers: map } =
            serde_json::from_slice(&serialized).unwrap();
        assert_eq!(map["set-cookie"], "csrf=xyz");
        assert_eq!(map["content-type"], "text/plain");
    }

    #[test]
    fn test_serialize_deserialize_error() {
        let msg = TunnelMessage::Error {
            id: Some("req-456".to_string()),
            message: "Test error".to_string(),
        };

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
