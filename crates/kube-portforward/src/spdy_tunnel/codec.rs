use bytes::Bytes;
use flate2::{Compress, Decompress, FlushCompress, FlushDecompress, Status};

use super::dictionary::SPDY_DICT;
use super::error::Error;

const SPDY_VERSION: u16 = 0x8003;
const SYN_STREAM_TYPE: u16 = 0x0001;
const SYN_REPLY_TYPE: u16 = 0x0002;
const RST_STREAM_TYPE: u16 = 0x0003;
const PING_TYPE: u16 = 0x0006;

const FLAG_FIN: u8 = 0x01;

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
    Unknown,
}

/// Stateful SPDY/3.1 codec handling zlib-compressed header blocks.
///
/// The compressor and decompressor maintain state across frames (flushed,
/// not reset between frames) per the SPDY specification.
pub(crate) struct SpdyCodec {
    compressor: Compress,
    decompressor: Decompress,
    /// Reusable buffer for compression output.
    compress_buf: Vec<u8>,
    /// Whether the dictionary has been set on the compressor.
    dict_set_compress: bool,
    /// Whether the dictionary has been set on the decompressor.
    dict_set_decompress: bool,
}

impl SpdyCodec {
    pub(crate) fn new() -> Self {
        Self {
            compressor: Compress::new(flate2::Compression::best(), true),
            decompressor: Decompress::new(true),
            compress_buf: Vec::with_capacity(4096),
            dict_set_compress: false,
            dict_set_decompress: false,
        }
    }

    /// Encode a SYN_STREAM control frame.
    pub(crate) fn encode_syn_stream(
        &mut self, stream_id: u32, headers: &[(String, String)], fin: bool,
    ) -> Result<Vec<u8>, Error> {
        let compressed_headers = self.compress_headers(headers)?;

        // SYN_STREAM payload: stream_id(4) + assoc_id(4) + priority(1) + slot(1) + headers
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
        frame.push(0); // priority << 5
        frame.push(0); // slot
        frame.extend_from_slice(&compressed_headers);

        Ok(frame)
    }

    /// Encode a DATA frame.
    pub(crate) fn encode_data(&mut self, stream_id: u32, payload: &[u8], fin: bool) -> Vec<u8> {
        let mut frame = Vec::with_capacity(8 + payload.len());

        // Data frame: stream_id with MSB=0
        frame.extend_from_slice(&(stream_id & 0x7FFF_FFFF).to_be_bytes());
        let flags_len = ((if fin { FLAG_FIN } else { 0 } as u32) << 24) | (payload.len() as u32);
        frame.extend_from_slice(&flags_len.to_be_bytes());
        frame.extend_from_slice(payload);

        frame
    }

    /// Encode a RST_STREAM control frame.
    pub(crate) fn encode_rst_stream(&mut self, stream_id: u32, status: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity(16);

        frame.extend_from_slice(&SPDY_VERSION.to_be_bytes());
        frame.extend_from_slice(&RST_STREAM_TYPE.to_be_bytes());
        // flags=0, length=8
        frame.extend_from_slice(&8u32.to_be_bytes());
        frame.extend_from_slice(&stream_id.to_be_bytes());
        frame.extend_from_slice(&status.to_be_bytes());

        frame
    }

    /// Encode a PING frame (for responding to server pings).
    pub(crate) fn encode_ping(&mut self, id: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity(12);

        frame.extend_from_slice(&SPDY_VERSION.to_be_bytes());
        frame.extend_from_slice(&PING_TYPE.to_be_bytes());
        // flags=0, length=4
        frame.extend_from_slice(&4u32.to_be_bytes());
        frame.extend_from_slice(&id.to_be_bytes());

        frame
    }

    /// Decode a SPDY frame from raw bytes.
    pub(crate) fn decode_frame(&mut self, data: &[u8]) -> Result<Frame, Error> {
        if data.len() < 8 {
            return Err(Error::InvalidFrame("frame too short"));
        }

        let first_u16 = u16::from_be_bytes([data[0], data[1]]);
        let is_control = (first_u16 & 0x8000) != 0;

        let flags_len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let flags = (flags_len >> 24) as u8;
        let payload_len = (flags_len & 0x00FF_FFFF) as usize;

        if data.len() < 8 + payload_len {
            return Err(Error::InvalidFrame("frame truncated"));
        }

        let payload = &data[8..8 + payload_len];

        if is_control {
            let frame_type = u16::from_be_bytes([data[2], data[3]]);
            self.decode_control_frame(frame_type, flags, payload)
        } else {
            let stream_id = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) & 0x7FFF_FFFF;
            Ok(Frame::Data {
                stream_id,
                payload: Bytes::copy_from_slice(payload),
                fin: (flags & FLAG_FIN) != 0,
            })
        }
    }

    fn decode_control_frame(
        &mut self, frame_type: u16, flags: u8, payload: &[u8],
    ) -> Result<Frame, Error> {
        match frame_type {
            SYN_STREAM_TYPE => {
                if payload.len() < 10 {
                    return Err(Error::InvalidFrame("SYN_STREAM payload too short"));
                }
                let stream_id = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
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
                let stream_id = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
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
                let stream_id = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
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
            _ => Ok(Frame::Unknown),
        }
    }

    /// Compress a header block using the stateful zlib compressor with SPDY dictionary.
    fn compress_headers(&mut self, headers: &[(String, String)]) -> Result<Vec<u8>, Error> {
        // Build uncompressed header block
        let mut block = Vec::new();
        let num_headers = headers.len() as u32;
        block.extend_from_slice(&num_headers.to_be_bytes());
        for (name, value) in headers {
            block.extend_from_slice(&(name.len() as u32).to_be_bytes());
            block.extend_from_slice(name.as_bytes());
            block.extend_from_slice(&(value.len() as u32).to_be_bytes());
            block.extend_from_slice(value.as_bytes());
        }

        // Set dictionary on first use
        if !self.dict_set_compress {
            self.compressor
                .set_dictionary(SPDY_DICT)
                .map_err(compress_io_error)
                .map_err(Error::Compression)?;
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
                .map_err(compress_io_error)
                .map_err(Error::Compression)?;

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
                    if total_out >= self.compress_buf.len() - 64 {
                        self.compress_buf.resize(self.compress_buf.len() * 2, 0);
                    }
                }
                Status::StreamEnd => break,
            }
        }

        Ok(self.compress_buf[..total_out].to_vec())
    }

    /// Decompress a header block using the stateful zlib decompressor with SPDY dictionary.
    fn decompress_headers(
        &mut self, compressed: &[u8],
    ) -> Result<Vec<(String, String)>, Error> {
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
                            .map_err(decompress_io_error)
                            .map_err(Error::Compression)?;
                        self.dict_set_decompress = true;
                        // Continue the loop — retry decompression from where we left off.
                    } else {
                        return Err(Error::Compression(decompress_io_error(e)));
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
        let name_len =
            u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
                as usize;
        offset += 4;

        if offset + name_len > data.len() {
            return Err(Error::InvalidFrame("header block truncated at name"));
        }
        let name = String::from_utf8_lossy(&data[offset..offset + name_len]).into_owned();
        offset += name_len;

        if offset + 4 > data.len() {
            return Err(Error::InvalidFrame("header block truncated at value length"));
        }
        let value_len =
            u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
                as usize;
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

fn compress_io_error(e: flate2::CompressError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}

fn decompress_io_error(e: flate2::DecompressError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_data_frame() {
        let mut codec = SpdyCodec::new();
        let payload = b"hello world";
        let encoded = codec.encode_data(3, payload, false);

        let frame = codec.decode_frame(&encoded).unwrap();
        match frame {
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
        let mut codec = SpdyCodec::new();
        let encoded = codec.encode_data(7, b"", true);

        let frame = codec.decode_frame(&encoded).unwrap();
        match frame {
            Frame::Data {
                stream_id, fin, ..
            } => {
                assert_eq!(stream_id, 7);
                assert!(fin);
            }
            _ => panic!("expected Data frame"),
        }
    }

    #[test]
    fn encode_decode_rst_stream() {
        let mut codec = SpdyCodec::new();
        let encoded = codec.encode_rst_stream(5, 2);

        let frame = codec.decode_frame(&encoded).unwrap();
        match frame {
            Frame::RstStream { stream_id, status } => {
                assert_eq!(stream_id, 5);
                assert_eq!(status, 2);
            }
            _ => panic!("expected RstStream frame"),
        }
    }

    #[test]
    fn encode_decode_syn_stream_roundtrip() {
        let mut codec = SpdyCodec::new();
        let headers = vec![
            ("streamtype".to_string(), "data".to_string()),
            ("port".to_string(), "8080".to_string()),
        ];

        let encoded = codec.encode_syn_stream(1, &headers, false).unwrap();
        let frame = codec.decode_frame(&encoded).unwrap();

        match frame {
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
        let mut codec = SpdyCodec::new();
        let headers = vec![
            ("streamtype".to_string(), "error".to_string()),
            ("port".to_string(), "9200".to_string()),
            ("requestid".to_string(), "0".to_string()),
        ];

        let encoded = codec.encode_syn_stream(1, &headers, true).unwrap();
        let frame = codec.decode_frame(&encoded).unwrap();

        match frame {
            Frame::SynStream {
                stream_id, fin, headers: h, ..
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
        // The compressor is stateful: second frame should benefit from shared state
        let mut codec = SpdyCodec::new();

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

        // Both should decode correctly with the same codec
        let f1 = codec.decode_frame(&enc1).unwrap();
        let f2 = codec.decode_frame(&enc2).unwrap();

        match f1 {
            Frame::SynStream { stream_id, headers, fin } => {
                assert_eq!(stream_id, 1);
                assert!(fin);
                assert_eq!(headers, h1);
            }
            _ => panic!("expected SynStream"),
        }
        match f2 {
            Frame::SynStream { stream_id, headers, fin } => {
                assert_eq!(stream_id, 3);
                assert!(!fin);
                assert_eq!(headers, h2);
            }
            _ => panic!("expected SynStream"),
        }
    }

    #[test]
    fn encode_decode_ping() {
        let mut codec = SpdyCodec::new();
        let encoded = codec.encode_ping(42);

        let frame = codec.decode_frame(&encoded).unwrap();
        match frame {
            Frame::Ping { id } => assert_eq!(id, 42),
            _ => panic!("expected Ping frame"),
        }
    }

    #[test]
    fn frame_too_short_returns_error() {
        let mut codec = SpdyCodec::new();
        let result = codec.decode_frame(&[0u8; 4]);
        assert!(result.is_err());
    }
}
