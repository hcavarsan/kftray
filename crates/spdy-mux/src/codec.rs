use bytes::{
    Bytes,
    BytesMut,
};
use flate2::{
    Compress,
    Decompress,
    FlushCompress,
    FlushDecompress,
    Status,
};

use crate::dictionary::SPDY_DICT;
use crate::error::Error;

const SPDY_VERSION: u16 = 0x8003;
const SYN_STREAM_TYPE: u16 = 0x0001;
const SYN_REPLY_TYPE: u16 = 0x0002;
const RST_STREAM_TYPE: u16 = 0x0003;
const SETTINGS_TYPE: u16 = 0x0004;
const PING_TYPE: u16 = 0x0006;
const GOAWAY_TYPE: u16 = 0x0007;
const WINDOW_UPDATE_TYPE: u16 = 0x0009;

const FLAG_FIN: u8 = 0x01;

const SETTINGS_INITIAL_WINDOW_SIZE: u32 = 7;
const SETTINGS_MAX_CONCURRENT_STREAMS: u32 = 4;

/// Decoded SPDY frame.
#[derive(Debug)]
pub(crate) enum Frame {
    SynStream {
        stream_id: u32,
        headers: Vec<(String, String)>,
        fin: bool,
    },
    SynReply {
        stream_id: u32,
        headers: Vec<(String, String)>,
        fin: bool,
    },
    Data {
        stream_id: u32,
        payload: Bytes,
        fin: bool,
    },
    RstStream {
        stream_id: u32,
        status: u32,
    },
    Ping {
        id: u32,
    },
    GoAway {
        last_good_stream_id: u32,
        status: u32,
    },
    WindowUpdate {
        stream_id: u32,
        delta_window_size: u32,
    },
    Settings {
        initial_window_size: Option<u32>,
        max_concurrent_streams: Option<u32>,
        max_frame_size: Option<u32>,
    },
    Unknown,
}

/// Stateful SPDY/3.1 codec handling zlib-compressed header blocks.
///
/// The compressor and decompressor maintain state across frames (flushed,
/// not reset between frames) per the SPDY specification.
pub(crate) struct SpdyCodec {
    compressor: Compress,
    decompressor: Decompress,
    compress_buf: Vec<u8>,
    dict_set_compress: bool,
    dict_set_decompress: bool,
    /// Maximum incoming frame payload size. Frames exceeding this are rejected.
    max_frame_size: u32,
}

impl SpdyCodec {
    pub fn with_max_frame_size(max_frame_size: u32) -> Self {
        Self {
            compressor: Compress::new(flate2::Compression::best(), true),
            decompressor: Decompress::new(true),
            compress_buf: Vec::with_capacity(4096),
            dict_set_compress: false,
            dict_set_decompress: false,
            max_frame_size,
        }
    }

    /// Update the max frame size (e.g. after receiving peer SETTINGS).
    pub fn set_max_frame_size(&mut self, size: u32) {
        self.max_frame_size = size;
    }

    /// Encode a SYN_STREAM control frame.
    ///
    /// Priority field is set to 0 for all port-forward streams.
    /// If heterogeneous priorities are needed in the future, the writer's
    /// cmd_tx should be replaced with a priority queue.
    pub fn encode_syn_stream(
        &mut self, stream_id: u32, headers: &[(String, String)], fin: bool,
    ) -> Result<Vec<u8>, Error> {
        self.encode_syn_stream_with_priority(stream_id, headers, fin, 0)
    }

    /// Encode a SYN_STREAM control frame with configurable priority (0-7,
    /// lower = higher priority).
    pub fn encode_syn_stream_with_priority(
        &mut self, stream_id: u32, headers: &[(String, String)], fin: bool, priority: u8,
    ) -> Result<Vec<u8>, Error> {
        let compressed_headers = self.compress_headers(headers)?;

        // SYN_STREAM payload: stream_id(4) + assoc_id(4) + priority(1) + slot(1) +
        // headers
        let payload_len = 10 + compressed_headers.len();
        let mut frame = Vec::with_capacity(8 + payload_len);

        // Control frame header
        frame.extend_from_slice(&SPDY_VERSION.to_be_bytes());
        frame.extend_from_slice(&SYN_STREAM_TYPE.to_be_bytes());
        let flags_len = ((if fin { FLAG_FIN } else { 0 } as u32) << 24) | (payload_len as u32);
        frame.extend_from_slice(&flags_len.to_be_bytes());

        // SYN_STREAM payload
        frame.extend_from_slice(&stream_id.to_be_bytes());
        frame.extend_from_slice(&0u32.to_be_bytes()); // associated stream ID
        frame.push((priority & 0x07) << 5); // priority in top 3 bits
        frame.push(0); // slot
        frame.extend_from_slice(&compressed_headers);

        Ok(frame)
    }

    /// Encode a DATA frame. Does NOT split; the caller must ensure the
    /// payload fits within max_frame_size (see `split_data_frames`).
    pub fn encode_data(&self, stream_id: u32, payload: &[u8], fin: bool) -> Vec<u8> {
        let mut frame = Vec::with_capacity(8 + payload.len());

        // Data frame: stream_id with MSB=0
        frame.extend_from_slice(&(stream_id & 0x7FFF_FFFF).to_be_bytes());
        let flags_len = ((if fin { FLAG_FIN } else { 0 } as u32) << 24) | (payload.len() as u32);
        frame.extend_from_slice(&flags_len.to_be_bytes());
        frame.extend_from_slice(payload);

        frame
    }

    /// Encode a RST_STREAM control frame.
    pub fn encode_rst_stream(&self, stream_id: u32, status: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity(16);

        frame.extend_from_slice(&SPDY_VERSION.to_be_bytes());
        frame.extend_from_slice(&RST_STREAM_TYPE.to_be_bytes());
        // flags=0, length=8
        frame.extend_from_slice(&8u32.to_be_bytes());
        frame.extend_from_slice(&stream_id.to_be_bytes());
        frame.extend_from_slice(&status.to_be_bytes());

        frame
    }

    /// Encode a PING frame.
    pub fn encode_ping(&self, id: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity(12);

        frame.extend_from_slice(&SPDY_VERSION.to_be_bytes());
        frame.extend_from_slice(&PING_TYPE.to_be_bytes());
        // flags=0, length=4
        frame.extend_from_slice(&4u32.to_be_bytes());
        frame.extend_from_slice(&id.to_be_bytes());

        frame
    }

    /// Encode a WINDOW_UPDATE control frame.
    pub fn encode_window_update(&self, stream_id: u32, delta: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity(16);

        frame.extend_from_slice(&SPDY_VERSION.to_be_bytes());
        frame.extend_from_slice(&WINDOW_UPDATE_TYPE.to_be_bytes());
        // flags=0, length=8
        frame.extend_from_slice(&8u32.to_be_bytes());
        frame.extend_from_slice(&stream_id.to_be_bytes());
        frame.extend_from_slice(&delta.to_be_bytes());

        frame
    }

    /// Encode a GOAWAY control frame.
    pub fn encode_goaway(&self, last_good_stream_id: u32, status: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity(16);

        frame.extend_from_slice(&SPDY_VERSION.to_be_bytes());
        frame.extend_from_slice(&GOAWAY_TYPE.to_be_bytes());
        // flags=0, length=8
        frame.extend_from_slice(&8u32.to_be_bytes());
        frame.extend_from_slice(&last_good_stream_id.to_be_bytes());
        frame.extend_from_slice(&status.to_be_bytes());

        frame
    }

    /// Encode a SETTINGS control frame.
    ///
    /// SPDY/3.1 SETTINGS format:
    ///   4-byte entry count, then N entries of (flags:1 + id:3 + value:4) = 8
    /// bytes each.
    pub fn encode_settings(&self, entries: &[(u32, u32)]) -> Vec<u8> {
        let payload_len = 4 + entries.len() * 8;
        let mut frame = Vec::with_capacity(8 + payload_len);

        // Control frame header
        frame.extend_from_slice(&SPDY_VERSION.to_be_bytes());
        frame.extend_from_slice(&SETTINGS_TYPE.to_be_bytes());
        // flags=0, length=payload_len
        frame.extend_from_slice(&(payload_len as u32).to_be_bytes());

        // Entry count
        frame.extend_from_slice(&(entries.len() as u32).to_be_bytes());

        for &(id, value) in entries {
            // flags (1 byte) = 0, id (3 bytes, big-endian)
            frame.push(0); // flags
            let id_bytes = id.to_be_bytes();
            frame.extend_from_slice(&id_bytes[1..4]); // 3 bytes of id
            frame.extend_from_slice(&value.to_be_bytes());
        }

        frame
    }

    /// Attempt to decode one SPDY frame from a `BytesMut` buffer.
    ///
    /// For DATA frames the payload is split off zero-copy via
    /// `BytesMut::split_to` + `freeze()`, avoiding allocation.
    ///
    /// Enforces `max_frame_size` on incoming payloads. Oversized frames
    /// return `Err(Error::FrameTooLarge { .. })`.
    ///
    /// Returns `Ok(Some(frame))` when a complete frame is available (the
    /// consumed bytes are removed from `buf`), `Ok(None)` when more data is
    /// needed, or `Err` on a protocol error.
    pub fn decode_frame(&mut self, buf: &mut BytesMut) -> Result<Option<Frame>, Error> {
        if buf.len() < 8 {
            return Ok(None);
        }

        let first_u16 = u16::from_be_bytes([buf[0], buf[1]]);
        let is_control = (first_u16 & 0x8000) != 0;

        let flags_len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let flags = (flags_len >> 24) as u8;
        let payload_len = (flags_len & 0x00FF_FFFF) as usize;

        // Frame size enforcement for DATA frames (control frames are
        // typically small and exempt from this check per SPDY spec).
        if !is_control && payload_len > self.max_frame_size as usize {
            let stream_id = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) & 0x7FFF_FFFF;
            return Err(Error::FrameTooLarge {
                stream_id,
                size: payload_len,
                max: self.max_frame_size,
            });
        }

        let total = 8 + payload_len;
        if buf.len() < total {
            return Ok(None);
        }

        let frame = if is_control {
            let frame_type = u16::from_be_bytes([buf[2], buf[3]]);
            let header_and_payload = buf.split_to(total);
            let payload = &header_and_payload[8..];
            self.decode_control_frame(frame_type, flags, payload)?
        } else {
            let stream_id = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) & 0x7FFF_FFFF;
            let fin = (flags & FLAG_FIN) != 0;
            // Split off the entire frame, then slice off the 8-byte header
            // to get a zero-copy Bytes handle to the payload.
            let mut frame_bytes = buf.split_to(total);
            let payload = frame_bytes.split_off(8).freeze();
            Frame::Data {
                stream_id,
                payload,
                fin,
            }
        };

        Ok(Some(frame))
    }

    fn decode_control_frame(
        &mut self, frame_type: u16, flags: u8, payload: &[u8],
    ) -> Result<Frame, Error> {
        match frame_type {
            SYN_STREAM_TYPE => {
                if payload.len() < 10 {
                    return Err(Error::InvalidFrame("SYN_STREAM payload too short"));
                }
                let stream_id =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let headers = self.decompress_headers(&payload[10..])?;
                Ok(Frame::SynStream {
                    stream_id,
                    headers,
                    fin: (flags & FLAG_FIN) != 0,
                })
            }
            SYN_REPLY_TYPE => {
                if payload.len() < 4 {
                    return Err(Error::InvalidFrame("SYN_REPLY payload too short"));
                }
                let stream_id =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let headers = self.decompress_headers(&payload[4..])?;
                Ok(Frame::SynReply {
                    stream_id,
                    headers,
                    fin: (flags & FLAG_FIN) != 0,
                })
            }
            RST_STREAM_TYPE => {
                if payload.len() < 8 {
                    return Err(Error::InvalidFrame("RST_STREAM payload too short"));
                }
                let stream_id =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let status = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                Ok(Frame::RstStream { stream_id, status })
            }
            PING_TYPE => {
                if payload.len() < 4 {
                    return Err(Error::InvalidFrame("PING payload too short"));
                }
                let id = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                Ok(Frame::Ping { id })
            }
            GOAWAY_TYPE => {
                if payload.len() < 8 {
                    return Err(Error::InvalidFrame("GOAWAY payload too short"));
                }
                let last_good_stream_id =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let status = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                Ok(Frame::GoAway {
                    last_good_stream_id,
                    status,
                })
            }
            SETTINGS_TYPE => {
                // SPDY/3.1 SETTINGS: 4-byte entry count, then N entries
                // of (flags:1 + id:3 + value:4) = 8 bytes each.
                let mut initial_window_size = None;
                let mut max_concurrent_streams = None;
                let mut max_frame_size = None;
                if payload.len() >= 4 {
                    let num_entries =
                        u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]])
                            as usize;
                    let mut offset = 4;
                    for _ in 0..num_entries {
                        if offset + 8 > payload.len() {
                            break;
                        }
                        // id is in bytes [offset+1..offset+4] (3 bytes, big-endian,
                        // first byte is flags per SPDY/3.1 spec)
                        let id = u32::from_be_bytes([
                            0,
                            payload[offset + 1],
                            payload[offset + 2],
                            payload[offset + 3],
                        ]);
                        let value = u32::from_be_bytes([
                            payload[offset + 4],
                            payload[offset + 5],
                            payload[offset + 6],
                            payload[offset + 7],
                        ]);
                        match id {
                            SETTINGS_INITIAL_WINDOW_SIZE => {
                                initial_window_size = Some(value);
                            }
                            SETTINGS_MAX_CONCURRENT_STREAMS => {
                                max_concurrent_streams = Some(value);
                            }
                            // Non-standard: some peers advertise max frame size as
                            // setting id 5. SPDY/3.1 itself defines settings 1-4 and
                            // 7-8; id 5 is left for peer-specific extensions.
                            5 => {
                                max_frame_size = Some(value);
                            }
                            _ => {}
                        }
                        offset += 8;
                    }
                }
                Ok(Frame::Settings {
                    initial_window_size,
                    max_concurrent_streams,
                    max_frame_size,
                })
            }
            WINDOW_UPDATE_TYPE => {
                if payload.len() < 8 {
                    return Err(Error::InvalidFrame("WINDOW_UPDATE payload too short"));
                }
                let stream_id =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let delta = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                Ok(Frame::WindowUpdate {
                    stream_id,
                    delta_window_size: delta,
                })
            }
            _ => Ok(Frame::Unknown),
        }
    }

    /// Compress a header block using the stateful zlib compressor with the
    /// SPDY dictionary. Header names are lowercased to match the SPDY/3.1
    /// spec; receivers reject frames whose header names contain uppercase
    /// characters.
    fn compress_headers(&mut self, headers: &[(String, String)]) -> Result<Vec<u8>, Error> {
        // Build uncompressed header block (names MUST be lowercased)
        let mut block = Vec::new();
        let num_headers = headers.len() as u32;
        block.extend_from_slice(&num_headers.to_be_bytes());
        for (name, value) in headers {
            let lower_name = name.to_ascii_lowercase();
            block.extend_from_slice(&(lower_name.len() as u32).to_be_bytes());
            block.extend_from_slice(lower_name.as_bytes());
            block.extend_from_slice(&(value.len() as u32).to_be_bytes());
            block.extend_from_slice(value.as_bytes());
        }

        // Set dictionary on first use
        if !self.dict_set_compress {
            self.compressor
                .set_dictionary(SPDY_DICT)
                .map_err(|e| Error::Compression(e.to_string()))?;
            self.dict_set_compress = true;
        }

        // Compress with SyncFlush (not full reset)
        self.compress_buf.clear();
        self.compress_buf.resize(block.len() + 512, 0);

        let mut total_in = 0;
        let mut total_out = 0;

        loop {
            let before_in = self.compressor.total_in() as usize;
            let before_out = self.compressor.total_out() as usize;

            let status = self
                .compressor
                .compress(
                    &block[total_in..],
                    &mut self.compress_buf[total_out..],
                    FlushCompress::Sync,
                )
                .map_err(|e| Error::Compression(e.to_string()))?;

            let consumed = self.compressor.total_in() as usize - before_in;
            let produced = self.compressor.total_out() as usize - before_out;
            total_in += consumed;
            total_out += produced;

            match status {
                Status::Ok | Status::BufError => {
                    if total_in >= block.len() && produced == 0 {
                        break;
                    }
                    // Need more output space
                    if total_out >= self.compress_buf.len().saturating_sub(64) {
                        self.compress_buf.resize(self.compress_buf.len() * 2, 0);
                    }
                }
                Status::StreamEnd => break,
            }
        }

        Ok(self.compress_buf[..total_out].to_vec())
    }

    /// Decompress a header block using the stateful zlib decompressor with SPDY
    /// dictionary.
    fn decompress_headers(&mut self, compressed: &[u8]) -> Result<Vec<(String, String)>, Error> {
        if compressed.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = vec![0u8; compressed.len() * 4 + 1024];

        // Track absolute positions via the decompressor's counters.
        let base_in = self.decompressor.total_in() as usize;
        let base_out = self.decompressor.total_out() as usize;

        loop {
            let cur_in = self.decompressor.total_in() as usize - base_in;
            let cur_out = self.decompressor.total_out() as usize - base_out;

            if cur_out >= output.len().saturating_sub(256) {
                output.resize(output.len() * 2, 0);
            }

            let result = self.decompressor.decompress(
                &compressed[cur_in..],
                &mut output[cur_out..],
                FlushDecompress::Sync,
            );

            match result {
                Ok(status) => {
                    let new_in = self.decompressor.total_in() as usize - base_in;
                    let new_out = self.decompressor.total_out() as usize - base_out;
                    let produced = new_out - cur_out;

                    match status {
                        Status::Ok => {
                            if new_in >= compressed.len() && produced == 0 {
                                return parse_header_block(&output[..new_out]);
                            }
                        }
                        Status::BufError => {
                            if new_in >= compressed.len() {
                                return parse_header_block(&output[..new_out]);
                            }
                            output.resize(output.len() * 2, 0);
                        }
                        Status::StreamEnd => {
                            return parse_header_block(&output[..new_out]);
                        }
                    }
                }
                Err(e) => {
                    // flate2 signals "need dictionary" via DecompressError.
                    if e.needs_dictionary().is_some() && !self.dict_set_decompress {
                        self.decompressor
                            .set_dictionary(SPDY_DICT)
                            .map_err(|e| Error::Compression(e.to_string()))?;
                        self.dict_set_decompress = true;
                        // Continue the loop; retry decompression from where we
                        // left off.
                    } else {
                        return Err(Error::Compression(e.to_string()));
                    }
                }
            }
        }
    }
}

/// Parse an uncompressed SPDY header block into name/value pairs.
fn parse_header_block(data: &[u8]) -> Result<Vec<(String, String)>, Error> {
    if data.len() < 4 {
        return Err(Error::InvalidFrame("header block too short"));
    }

    let num_headers = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let mut headers = Vec::with_capacity(num_headers);
    let mut offset = 4;

    for _ in 0..num_headers {
        if offset + 4 > data.len() {
            return Err(Error::InvalidFrame("header block truncated at name length"));
        }
        let name_len = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + name_len > data.len() {
            return Err(Error::InvalidFrame("header block truncated at name"));
        }
        let name = String::from_utf8_lossy(&data[offset..offset + name_len]).into_owned();
        offset += name_len;

        if offset + 4 > data.len() {
            return Err(Error::InvalidFrame(
                "header block truncated at value length",
            ));
        }
        let value_len = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + value_len > data.len() {
            return Err(Error::InvalidFrame("header block truncated at value"));
        }
        let value = String::from_utf8_lossy(&data[offset..offset + value_len]).into_owned();
        offset += value_len;

        headers.push((name, value));
    }

    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_codec() -> SpdyCodec {
        SpdyCodec::with_max_frame_size(16_384)
    }

    /// Decode one complete frame; panic if the buffer is incomplete.
    fn decode_one(codec: &mut SpdyCodec, data: &[u8]) -> Frame {
        let mut buf = BytesMut::from(data);
        codec
            .decode_frame(&mut buf)
            .expect("decode error")
            .expect("incomplete frame")
    }

    #[test]
    fn encode_decode_data_frame() {
        let mut codec = fresh_codec();
        let payload = b"hello world";
        let encoded = codec.encode_data(3, payload, false);

        match decode_one(&mut codec, &encoded) {
            Frame::Data {
                stream_id,
                payload: p,
                fin,
            } => {
                assert_eq!(stream_id, 3);
                assert_eq!(&p[..], payload);
                assert!(!fin);
            }
            _ => panic!("expected Data frame"),
        }
    }

    #[test]
    fn encode_decode_data_frame_with_fin() {
        let mut codec = fresh_codec();
        let encoded = codec.encode_data(7, b"", true);

        match decode_one(&mut codec, &encoded) {
            Frame::Data { stream_id, fin, .. } => {
                assert_eq!(stream_id, 7);
                assert!(fin);
            }
            _ => panic!("expected Data frame"),
        }
    }

    #[test]
    fn encode_decode_rst_stream() {
        let mut codec = fresh_codec();
        let encoded = codec.encode_rst_stream(5, 2);

        match decode_one(&mut codec, &encoded) {
            Frame::RstStream { stream_id, status } => {
                assert_eq!(stream_id, 5);
                assert_eq!(status, 2);
            }
            _ => panic!("expected RstStream frame"),
        }
    }

    #[test]
    fn encode_decode_syn_stream_roundtrip() {
        let mut codec = fresh_codec();
        let headers = vec![
            ("streamtype".to_string(), "data".to_string()),
            ("port".to_string(), "8080".to_string()),
        ];

        let encoded = codec.encode_syn_stream(1, &headers, false).unwrap();

        match decode_one(&mut codec, &encoded) {
            Frame::SynStream {
                stream_id,
                headers: decoded_headers,
                fin,
            } => {
                assert_eq!(stream_id, 1);
                assert!(!fin);
                assert_eq!(decoded_headers, headers);
            }
            _ => panic!("expected SynStream frame"),
        }
    }

    #[test]
    fn encode_decode_syn_stream_with_fin() {
        let mut codec = fresh_codec();
        let headers = vec![
            ("streamtype".to_string(), "error".to_string()),
            ("port".to_string(), "9200".to_string()),
            ("requestid".to_string(), "0".to_string()),
        ];

        let encoded = codec.encode_syn_stream(1, &headers, true).unwrap();

        match decode_one(&mut codec, &encoded) {
            Frame::SynStream {
                stream_id,
                fin,
                headers: h,
                ..
            } => {
                assert_eq!(stream_id, 1);
                assert!(fin);
                assert_eq!(h, headers);
            }
            _ => panic!("expected SynStream frame"),
        }
    }

    #[test]
    fn multiple_syn_streams_share_compressor_state() {
        let mut codec = fresh_codec();

        let h1 = vec![
            ("streamtype".to_string(), "error".to_string()),
            ("port".to_string(), "8080".to_string()),
            ("requestid".to_string(), "0".to_string()),
        ];
        let h2 = vec![
            ("streamtype".to_string(), "data".to_string()),
            ("port".to_string(), "8080".to_string()),
            ("requestid".to_string(), "0".to_string()),
        ];

        let enc1 = codec.encode_syn_stream(1, &h1, true).unwrap();
        let enc2 = codec.encode_syn_stream(3, &h2, false).unwrap();

        match decode_one(&mut codec, &enc1) {
            Frame::SynStream {
                stream_id,
                headers,
                fin,
            } => {
                assert_eq!(stream_id, 1);
                assert!(fin);
                assert_eq!(headers, h1);
            }
            _ => panic!("expected SynStream"),
        }
        match decode_one(&mut codec, &enc2) {
            Frame::SynStream {
                stream_id,
                headers,
                fin,
            } => {
                assert_eq!(stream_id, 3);
                assert!(!fin);
                assert_eq!(headers, h2);
            }
            _ => panic!("expected SynStream"),
        }
    }

    #[test]
    fn encode_decode_ping() {
        let mut codec = fresh_codec();
        let encoded = codec.encode_ping(42);

        match decode_one(&mut codec, &encoded) {
            Frame::Ping { id } => assert_eq!(id, 42),
            _ => panic!("expected Ping frame"),
        }
    }

    #[test]
    fn encode_decode_window_update() {
        let mut codec = fresh_codec();
        let encoded = codec.encode_window_update(5, 32768);

        match decode_one(&mut codec, &encoded) {
            Frame::WindowUpdate {
                stream_id,
                delta_window_size,
            } => {
                assert_eq!(stream_id, 5);
                assert_eq!(delta_window_size, 32768);
            }
            _ => panic!("expected WindowUpdate frame"),
        }
    }

    #[test]
    fn incomplete_buffer_returns_none() {
        let mut codec = fresh_codec();
        // Less than 8 bytes, not enough for any frame header
        let mut buf = BytesMut::from(&[0u8; 4][..]);
        assert!(codec.decode_frame(&mut buf).unwrap().is_none());

        // Full header but truncated payload
        let encoded = codec.encode_data(1, b"hello", false);
        let mut buf = BytesMut::from(&encoded[..10]);
        assert!(codec.decode_frame(&mut buf).unwrap().is_none());
    }

    #[test]
    fn decode_goaway_frame() {
        let mut codec = fresh_codec();
        let mut frame = Vec::new();
        frame.extend_from_slice(&0x8003u16.to_be_bytes());
        frame.extend_from_slice(&0x0007u16.to_be_bytes());
        frame.extend_from_slice(&8u32.to_be_bytes());
        frame.extend_from_slice(&42u32.to_be_bytes());
        frame.extend_from_slice(&0u32.to_be_bytes());

        match decode_one(&mut codec, &frame) {
            Frame::GoAway {
                last_good_stream_id,
                status,
            } => {
                assert_eq!(last_good_stream_id, 42);
                assert_eq!(status, 0);
            }
            _ => panic!("expected GoAway frame"),
        }
    }

    #[test]
    fn streaming_decode_multiple_frames() {
        let mut codec = fresh_codec();
        let f1 = codec.encode_data(1, b"aaa", false);
        let f2 = codec.encode_data(3, b"bbb", true);
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&f1);
        buf.extend_from_slice(&f2);

        let original_len = buf.len();
        let frame1 = codec.decode_frame(&mut buf).unwrap().unwrap();
        assert_eq!(original_len - buf.len(), f1.len());
        match frame1 {
            Frame::Data { stream_id, .. } => assert_eq!(stream_id, 1),
            _ => panic!("expected Data"),
        }

        let frame2 = codec.decode_frame(&mut buf).unwrap().unwrap();
        assert!(buf.is_empty());
        match frame2 {
            Frame::Data { stream_id, fin, .. } => {
                assert_eq!(stream_id, 3);
                assert!(fin);
            }
            _ => panic!("expected Data"),
        }
    }

    #[test]
    fn encode_decode_goaway_roundtrip() {
        let mut codec = fresh_codec();
        let encoded = codec.encode_goaway(99, 0);
        match decode_one(&mut codec, &encoded) {
            Frame::GoAway {
                last_good_stream_id,
                status,
            } => {
                assert_eq!(last_good_stream_id, 99);
                assert_eq!(status, 0);
            }
            _ => panic!("expected GoAway frame"),
        }
    }

    #[test]
    fn encode_decode_settings_roundtrip() {
        let mut codec = fresh_codec();
        let entries = vec![(7, 131072), (4, 200)];
        let encoded = codec.encode_settings(&entries);
        match decode_one(&mut codec, &encoded) {
            Frame::Settings {
                initial_window_size,
                max_concurrent_streams,
                ..
            } => {
                assert_eq!(initial_window_size, Some(131072));
                assert_eq!(max_concurrent_streams, Some(200));
            }
            _ => panic!("expected Settings frame"),
        }
    }

    #[test]
    fn frame_too_large_rejected() {
        let mut codec = SpdyCodec::with_max_frame_size(10);
        // Encode a data frame with 20 bytes payload (exceeds max 10)
        let encoded = fresh_codec().encode_data(1, &[0u8; 20], false);
        let mut buf = BytesMut::from(&encoded[..]);
        match codec.decode_frame(&mut buf) {
            Err(Error::FrameTooLarge {
                stream_id,
                size,
                max,
            }) => {
                assert_eq!(stream_id, 1);
                assert_eq!(size, 20);
                assert_eq!(max, 10);
            }
            other => panic!("expected FrameTooLarge, got {other:?}"),
        }
    }
}
