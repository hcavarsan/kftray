//! IAP relay frames carried in WebSocket binary messages. Pure and total:
//! no input makes it panic.

use super::consts::{
    MAX_DATA_FRAME,
    TAG_ACK,
    TAG_CONNECT_SUCCESS_SID,
    TAG_DATA,
    TAG_RECONNECT_SUCCESS_ACK,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Frame<'a> {
    ConnectSuccessSid(&'a [u8]),
    ReconnectSuccessAck(u64),
    Data(&'a [u8]),
    Ack(u64),
    Unknown(u16),
}

impl Frame<'_> {
    /// Name for logs; never includes the payload.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ConnectSuccessSid(_) => "CONNECT_SUCCESS_SID",
            Self::ReconnectSuccessAck(_) => "RECONNECT_SUCCESS_ACK",
            Self::Data(_) => "DATA",
            Self::Ack(_) => "ACK",
            Self::Unknown(_) => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("truncated frame")]
    Truncated,
    #[error("DATA frame of {0} bytes exceeds the {MAX_DATA_FRAME} byte limit")]
    Oversized(usize),
}

fn take<const N: usize>(buf: &[u8], at: usize) -> Result<[u8; N], FrameError> {
    buf.get(at..at + N)
        .and_then(|s| s.try_into().ok())
        .ok_or(FrameError::Truncated)
}

/// Decodes the frame at the start of `buf`; returns it with the bytes it
/// used. A frame with an unknown tag has no known length, so it uses all of `buf`.
pub fn decode(buf: &[u8]) -> Result<(Frame<'_>, usize), FrameError> {
    let tag = u16::from_be_bytes(take(buf, 0)?);
    match tag {
        TAG_CONNECT_SUCCESS_SID | TAG_DATA => {
            let len = u32::from_be_bytes(take(buf, 2)?) as usize;
            if tag == TAG_DATA && len > MAX_DATA_FRAME {
                return Err(FrameError::Oversized(len));
            }
            let end = 6usize.checked_add(len).ok_or(FrameError::Truncated)?;
            let body = buf.get(6..end).ok_or(FrameError::Truncated)?;
            let frame = if tag == TAG_DATA {
                Frame::Data(body)
            } else {
                Frame::ConnectSuccessSid(body)
            };
            Ok((frame, end))
        }
        TAG_RECONNECT_SUCCESS_ACK => Ok((
            Frame::ReconnectSuccessAck(u64::from_be_bytes(take(buf, 2)?)),
            10,
        )),
        TAG_ACK => Ok((Frame::Ack(u64::from_be_bytes(take(buf, 2)?)), 10)),
        other => Ok((Frame::Unknown(other), buf.len())),
    }
}

fn with_payload(tag: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(6 + payload.len());
    frame.extend_from_slice(&tag.to_be_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn with_counter(tag: u16, counter: u64) -> Vec<u8> {
    let mut frame = Vec::with_capacity(10);
    frame.extend_from_slice(&tag.to_be_bytes());
    frame.extend_from_slice(&counter.to_be_bytes());
    frame
}

/// The caller guarantees `payload.len() <= MAX_DATA_FRAME`.
pub fn encode_data(payload: &[u8]) -> Vec<u8> {
    debug_assert!(payload.len() <= MAX_DATA_FRAME);
    with_payload(TAG_DATA, payload)
}

pub fn encode_ack(total_received: u64) -> Vec<u8> {
    with_counter(TAG_ACK, total_received)
}

/// Server-side frame; used by the test double.
pub fn encode_connect_success_sid(sid: &[u8]) -> Vec<u8> {
    with_payload(TAG_CONNECT_SUCCESS_SID, sid)
}

/// Server-side frame; used by the test double.
pub fn encode_reconnect_success_ack(total_received: u64) -> Vec<u8> {
    with_counter(TAG_RECONNECT_SUCCESS_ACK, total_received)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn decode_should_read_connect_success_sid_when_frame_is_complete() {
        let bytes = [0, 1, 0, 0, 0, 3, b'a', b'b', b'c'];
        assert_eq!(
            decode(&bytes),
            Ok((Frame::ConnectSuccessSid(b"abc"), bytes.len()))
        );
    }

    #[test]
    fn decode_should_read_reconnect_success_ack_when_frame_is_complete() {
        let bytes = [0, 2, 0, 0, 0, 0, 0, 0, 1, 0];
        assert_eq!(decode(&bytes), Ok((Frame::ReconnectSuccessAck(256), 10)));
    }

    #[test]
    fn decode_should_read_data_when_frame_is_complete() {
        let bytes = [0, 4, 0, 0, 0, 2, 9, 8];
        assert_eq!(decode(&bytes), Ok((Frame::Data(&[9, 8]), 8)));
    }

    #[test]
    fn decode_should_read_ack_when_frame_is_complete() {
        let bytes = [0, 7, 0, 0, 0, 0, 0, 0, 0x80, 0];
        assert_eq!(decode(&bytes), Ok((Frame::Ack(32_768), 10)));
    }

    #[test]
    fn decode_should_report_unknown_tag_when_tag_is_not_defined() {
        assert_eq!(decode(&[0, 9, 1, 2]), Ok((Frame::Unknown(9), 4)));
    }

    #[test]
    fn decode_should_leave_trailing_bytes_when_buffer_has_more_than_one_frame() {
        let bytes = [0, 4, 0, 0, 0, 1, 7, 0xff];
        assert_eq!(decode(&bytes), Ok((Frame::Data(&[7]), 7)));
    }

    #[test]
    fn decode_should_reject_data_when_length_exceeds_the_limit() {
        let bytes = [0, 4, 0, 0, 0x40, 1];
        assert_eq!(
            decode(&bytes),
            Err(FrameError::Oversized(MAX_DATA_FRAME + 1))
        );
    }

    #[test]
    fn decode_should_not_overflow_when_length_is_u32_max() {
        let bytes = [0, 1, 0xff, 0xff, 0xff, 0xff];
        assert_eq!(decode(&bytes), Err(FrameError::Truncated));
    }

    #[test]
    fn encode_should_match_fixed_vectors_when_given_each_tag() {
        assert_eq!(encode_data(&[9, 8]), [0, 4, 0, 0, 0, 2, 9, 8]);
        assert_eq!(encode_ack(256), [0, 7, 0, 0, 0, 0, 0, 0, 1, 0]);
        assert_eq!(
            encode_connect_success_sid(b"abc"),
            [0, 1, 0, 0, 0, 3, 97, 98, 99]
        );
        assert_eq!(
            encode_reconnect_success_ack(1),
            [0, 2, 0, 0, 0, 0, 0, 0, 0, 1]
        );
    }

    fn assert_every_prefix_is_truncated(frame: &[u8]) {
        for cut in 0..frame.len() {
            assert_eq!(
                decode(&frame[..cut]),
                Err(FrameError::Truncated),
                "cut at {cut}"
            );
        }
    }

    proptest! {
        #[test]
        fn data_should_round_trip_when_payload_fits(
            payload in proptest::collection::vec(any::<u8>(), 0..=MAX_DATA_FRAME)
        ) {
            let frame = encode_data(&payload);
            prop_assert_eq!(decode(&frame), Ok((Frame::Data(&payload), frame.len())));
        }

        #[test]
        fn ack_should_round_trip_when_given_any_counter(n in any::<u64>()) {
            let frame = encode_ack(n);
            prop_assert_eq!(decode(&frame), Ok((Frame::Ack(n), 10)));
        }

        #[test]
        fn sid_and_reconnect_ack_should_round_trip_when_given_any_value(
            sid in proptest::collection::vec(any::<u8>(), 0..64),
            n in any::<u64>(),
        ) {
            let frame = encode_connect_success_sid(&sid);
            prop_assert_eq!(decode(&frame), Ok((Frame::ConnectSuccessSid(&sid), frame.len())));
            let frame = encode_reconnect_success_ack(n);
            prop_assert_eq!(decode(&frame), Ok((Frame::ReconnectSuccessAck(n), 10)));
        }

        #[test]
        fn decode_should_report_truncated_when_valid_frame_is_cut(
            payload in proptest::collection::vec(any::<u8>(), 0..256),
            n in any::<u64>(),
        ) {
            assert_every_prefix_is_truncated(&encode_data(&payload));
            assert_every_prefix_is_truncated(&encode_connect_success_sid(&payload));
            assert_every_prefix_is_truncated(&encode_ack(n));
            assert_every_prefix_is_truncated(&encode_reconnect_success_ack(n));
        }

        #[test]
        fn decode_should_not_panic_when_input_is_arbitrary(
            bytes in proptest::collection::vec(any::<u8>(), 0..64)
        ) {
            if let Ok((_, used)) = decode(&bytes) {
                prop_assert!(used <= bytes.len());
            }
        }
    }
}
