use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid SPDY frame: {0}")]
    InvalidFrame(&'static str),

    #[error("zlib compression error: {0}")]
    Compression(String),

    #[error("stream {0} not found")]
    StreamNotFound(u32),

    #[error("stream {0} reset by peer: status {1}")]
    StreamReset(u32, u32),

    #[error("mux closed")]
    MuxClosed,

    #[error("SYN_REPLY timeout for stream {0}")]
    SynReplyTimeout(u32),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("capacity exhausted: {in_use} active pairs, limit {limit}")]
    CapacityExhausted { in_use: usize, limit: u32 },

    #[error("stream ID space exhausted (last valid: {0})")]
    StreamIdExhausted(u32),

    #[error("frame too large on stream {stream_id}: {size} bytes exceeds max {max}")]
    FrameTooLarge {
        stream_id: u32,
        size: usize,
        max: u32,
    },

    #[error("GOAWAY received: last_good_stream={last_good_stream_id}, status={status}")]
    GoAway {
        last_good_stream_id: u32,
        status: u32,
    },

    #[error("ping timeout: no response within {0:?}")]
    PingTimeout(std::time::Duration),

    #[error("idle timeout: no frames received for {0:?}")]
    IdleTimeout(std::time::Duration),

    #[error("transport error: {0}")]
    Transport(#[from] crate::transport::TransportError),
}
