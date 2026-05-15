use bytes::{
    BufMut,
    Bytes,
    BytesMut,
};

use crate::error::Error;

pub(crate) const CLOSE_SIGNAL_BYTE: u8 = 0xFF;

pub(crate) fn encode_channel_frame(channel: u8, payload: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(1 + payload.len());
    buf.put_u8(channel);
    buf.put_slice(payload);
    buf.freeze()
}

pub(crate) fn encode_close_signal(channel: u8) -> Bytes {
    let mut buf = BytesMut::with_capacity(2);
    buf.put_u8(CLOSE_SIGNAL_BYTE);
    buf.put_u8(channel);
    buf.freeze()
}

pub(crate) fn split_channel_byte(mut payload: Bytes) -> Result<(u8, Bytes), Error> {
    if payload.is_empty() {
        return Err(Error::ProtocolViolation {
            context: "split_channel_byte",
            detail: "empty frame".into(),
        });
    }
    let channel = payload.split_to(1)[0];
    Ok((channel, payload))
}

pub(crate) fn parse_close_signal(payload: &Bytes) -> Option<u8> {
    if payload.len() == 2 && payload[0] == CLOSE_SIGNAL_BYTE {
        Some(payload[1])
    } else {
        None
    }
}

pub(crate) fn parse_initial_port_frame(payload: &Bytes) -> Result<u16, Error> {
    if payload.len() < 2 {
        return Err(Error::ProtocolViolation {
            context: "parse_initial_port_frame",
            detail: format!("expected 2 bytes, got {}", payload.len()),
        });
    }
    Ok(u16::from_le_bytes([payload[0], payload[1]]))
}

pub(crate) fn bytes_to_message(payload: Bytes) -> tungstenite::Message {
    tungstenite::Message::Binary(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_channel_frame() {
        let payload = b"hello world";
        let encoded = encode_channel_frame(7, payload);
        let (ch, body) = split_channel_byte(encoded).expect("split");
        assert_eq!(ch, 7);
        assert_eq!(&body[..], payload);
    }

    #[test]
    fn close_signal_round_trip() {
        let encoded = encode_close_signal(42);
        assert_eq!(parse_close_signal(&encoded), Some(42));
    }

    #[test]
    fn close_signal_does_not_match_normal_frame() {
        let encoded = encode_channel_frame(0xFF, &[1, 2, 3]);
        assert_eq!(parse_close_signal(&encoded), None);
    }

    #[test]
    fn split_empty_errors() {
        assert!(split_channel_byte(Bytes::new()).is_err());
    }

    #[test]
    fn initial_port_frame_parses_le() {
        let bytes = Bytes::from_static(&[0xF0, 0x23]);
        assert_eq!(parse_initial_port_frame(&bytes).unwrap(), 0x23F0);
    }

    #[test]
    fn initial_port_frame_short_errors() {
        let bytes = Bytes::from_static(&[0x01]);
        assert!(parse_initial_port_frame(&bytes).is_err());
    }
}
