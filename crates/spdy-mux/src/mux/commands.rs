use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::{
    mpsc,
    oneshot,
};

use super::window::SendWindow;

/// Command sent to the writer task via `cmd_tx`.
pub(crate) enum MuxCommand {
    /// Open a paired stream (an "error" stream half-closed at open time +
    /// a "data" stream that carries application bytes) and emit the
    /// first DATA frame on the data stream, all under a single writer
    /// command. Stream IDs are allocated by the caller under the open
    /// sequencer immediately before enqueuing this command, so the wire
    /// sees `SYN_STREAM(error)`, `SYN_STREAM(data)`, the empty
    /// `DATA(error, fin=true)`, then `DATA(data, first_payload)` in
    /// monotonic ID order.
    ///
    /// Header content is caller-supplied; the codec does not interpret
    /// the keys/values. This keeps the multiplexer agnostic to any
    /// particular SPDY/3.1 peer (Kubernetes port-forward, custom RPC
    /// servers, etc.).
    ///
    /// Lazy allocation: pre-opened spare streams do not allocate IDs
    /// or send any frames until the consumer actually writes the first
    /// byte. Useful for any peer that creates an upstream connection
    /// eagerly on SYN_STREAM (idle pre-opens otherwise time out).
    OpenStreamPairAndWrite {
        error_id: u32,
        data_id: u32,
        error_headers: Vec<(String, String)>,
        data_headers: Vec<(String, String)>,
        /// First chunk of application data to send on `data_id` after the
        /// two SYN_STREAM frames. Always sent with fin=false; the consumer
        /// half-closes via `poll_shutdown` later if needed.
        first_payload: Bytes,
    },
    /// Send a DATA frame (encoded by the codec).
    SendData {
        stream_id: u32,
        payload: Bytes,
        fin: bool,
    },
    /// Pre-encoded SPDY DATA frame. Bypasses the codec; sent directly.
    SendRawFrame { frame: Bytes },
    /// Close a stream with a RST_STREAM frame.
    CloseStream { stream_id: u32, status: u32 },
    /// Encode and send an SPDY PING.
    EncodePing { id: u32 },
    /// Send a WebSocket-level PONG.
    SendWsPong { payload: Bytes },
    /// Encode and send a WINDOW_UPDATE frame.
    EncodeWindowUpdate { stream_id: u32, delta: u32 },
    /// Send a GOAWAY frame and prepare for graceful shutdown.
    /// Used by the open path for stream ID exhaustion and available for
    /// external graceful shutdown triggers.
    GoAway { last_good_stream_id: u32 },
}

/// Sent by the caller to the appropriate frame worker BEFORE OpenStream goes
/// to the writer, or to notify the worker that a stream has been closed from
/// the client side.
pub(crate) enum StreamRegistration {
    /// Register a new stream with the worker.
    Open {
        stream_id: u32,
        data_tx: mpsc::Sender<Bytes>,
        reply_tx: oneshot::Sender<Result<(), crate::error::Error>>,
        send_window: Arc<SendWindow>,
    },
    /// Notify the worker that a stream was closed by the client.
    Close { stream_id: u32 },
    /// Broadcast from reader when peer SETTINGS changes initial_window_size.
    /// Each worker applies the delta to its streams' send_windows.
    SettingsWindowDelta { delta: i64 },
    /// Broadcast from reader when GOAWAY is received. Each worker cleans up
    /// streams with id > last_good_stream_id in its shard.
    GoAway {
        last_good_stream_id: u32,
        status: u32,
    },
}

/// State protected by the per-handle open sequencer. Holds the monotonic
/// SPDY stream ID counter. Wrapped in `tokio::sync::Mutex` so the open
/// path can `.await` while holding it (reg_tx + cmd_tx sends are all
/// `.await`).
pub(super) struct OpenState {
    /// Next SPDY client-initiated stream ID. SPDY/3.1 requires client
    /// streams to be odd and monotonically increasing. Initial value 1,
    /// increment by 2 per allocation.
    pub next_stream_id: u32,
}
