//! SPDY/3.1 multiplexer: reader, frame workers, writer, supervisor, and handle.
//!
//! # Architecture
//!
//! Each WebSocket connection runs:
//!   - 1 reader task: decodes SPDY frames from the wire, handles session-level
//!     frames (PING, SETTINGS, GOAWAY, session WINDOW_UPDATE) inline, and
//!     routes stream-keyed frames to one of 5 frame worker tasks.
//!   - 5 frame worker tasks: each owns a shard of streams partitioned by
//!     `stream_id % FRAME_WORKERS`. No shared mutable state across workers.
//!   - 1 writer task: encodes and sends frames to the wire.
//!   - 1 supervisor task: watches all 7 tasks; on any unexpected exit, cancels
//!     the session.
//!
//! # Transport break contract
//!
//! When the WebSocket closes or errors, **all** streams receive `BrokenPipe`.
//! There is no transparent reconnection. The `Forwarder` layer above handles
//! reconnection by opening a new session. This is the stated, intentional
//! contract: transport failure propagates to every stream as an I/O error.
//!
//! # Stream priority
//!
//! SPDY SYN_STREAM carries a priority field (3 bits, 0-7, lower = higher).
//! For port-forwarding, all streams use priority 0. The writer's FIFO `cmd_tx`
//! is correct for equal-priority streams. If heterogeneous priorities are
//! needed in the future, `cmd_tx` should be replaced with a priority queue.
//!
//! # Panic recovery
//!
//! If any task panics, the supervisor logs the panic and marks the session
//! poisoned via the cancellation token. A panic kills the session
//! permanently. Restart at the Forwarder layer.

// Private modules
mod config;
mod handle;
mod supervisor;

// Crate-internal modules
mod commands;
mod reader;
mod window;
mod worker;
mod writer;

// Task scheduling constants

/// Number of frame worker tasks per WebSocket connection. Five workers
/// balance contention against memory: each owns a disjoint shard of streams,
/// so no shared mutable state crosses worker boundaries. Frames are
/// partitioned by stream_id % FRAME_WORKERS for parallel processing.
pub(super) const FRAME_WORKERS: usize = 5;

/// Maximum outbound commands coalesced into one `feed…flush` batch.
/// With pool=6 and 200 req/s, each writer handles ~33 bidirectional frames/s.
/// Batching 64 amortizes flush cost across more frames, reducing per-frame
/// overhead and avoiding head-of-line blocking when one flush stalls on TCP
/// backpressure from the apiserver. Worst-case added latency per frame is
/// ~2ms (64 frames × ~30μs encode each), acceptable for port-forwarding.
pub(super) const WRITE_BATCH_CAP: usize = 64;

// SPDY/3.1 protocol constants

/// RST_STREAM status code: peer cancelled the stream (no protocol error).
pub(super) const RST_STATUS_CANCEL: u32 = 5;

/// RST_STREAM status code: flow control window violated.
pub(super) const RST_STATUS_FLOW_CONTROL: u32 = 7;

/// RST_STREAM status code: stream ID is not valid for this session.
pub(super) const RST_STATUS_INVALID_STREAM: u32 = 2;

/// RST_STREAM status code: peer refused to open the stream.
pub(super) const RST_STATUS_REFUSED_STREAM: u32 = 3;

/// GOAWAY status code: graceful shutdown.
pub(super) const GOAWAY_STATUS_OK: u32 = 0;

/// Maximum valid SPDY stream ID (31-bit unsigned).
pub(super) const MAX_STREAM_ID: u32 = 0x7FFF_FFFF;

// Re-exports
pub(crate) use commands::{
    MuxCommand,
    StreamRegistration,
};
pub use config::MuxConfig;
pub(crate) use handle::MuxHandle;
pub(crate) use window::SendWindow;
