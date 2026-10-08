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
