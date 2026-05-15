use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("invalid SPDY frame: {0}")]
    InvalidFrame(&'static str),

    #[error("zlib compression error: {0}")]
    Compression(#[source] std::io::Error),

    #[error("stream {0} not found")]
    StreamNotFound(u32),

    #[error("stream {0} reset by peer: status {1}")]
    StreamReset(u32, u32),

    #[error("mux closed")]
    MuxClosed,

    #[error("SYN_REPLY timeout for stream {0}")]
    SynReplyTimeout(u32),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
