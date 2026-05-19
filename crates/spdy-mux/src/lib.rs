//! SPDY/3.1 stream multiplexer over WebSocket transports.
//!
//! Provides a full SPDY/3.1 codec, flow-controlled stream multiplexing, and a
//! fastwebsockets transport adapter. Designed for Kubernetes port-forward
//! tunneling but usable with any SPDY/3.1 peer.

mod codec;
mod dictionary;
mod error;
mod mux;
mod session;
mod stream;
mod transport;

pub use crate::error::Error;
pub use crate::mux::MuxConfig;
pub use crate::session::Session;
pub use crate::stream::{
    DataStream,
    ErrorStream,
    Stream,
};
pub use crate::transport::{
    FastWsReader,
    FastWsWriter,
    RawSpdyReader,
    RawSpdyWriter,
    WsFrameReader,
    WsFrameWriter,
    WsMessage,
    split_fastws,
    split_raw_spdy,
};
