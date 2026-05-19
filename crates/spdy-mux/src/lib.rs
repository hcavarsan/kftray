//! SPDY/3.1 stream multiplexer over WebSocket transports.
//!
//! Provides a full SPDY/3.1 codec, flow-controlled stream multiplexing, and a
//! fastwebsockets transport adapter. Designed for Kubernetes port-forward
//! tunneling but usable with any SPDY/3.1 peer.

// SPDY/3.1 wire framing is specified in terms of u32 stream IDs, u32 payload
// lengths (24-bit, packed into a 32-bit flags/length word), u8 priorities, and
// i64-derived flow-control deltas. The codec, multiplexer, and flow-control
// machinery deliberately move between fixed-width integer types per the spec
// (e.g. `len as u32` after a checked bound, `u32 as usize` for buffer math).
// Every cast here is bounded by either a SPDY-layer enforcement or a prior
// size check; flagging each one individually would just produce dozens of
// `#[allow]`s on protocol-correct code, so the relaxation is crate-wide.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

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
