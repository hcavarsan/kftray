use std::time::Duration;

/// Configurable limits for the SPDY multiplexer. Replaces all former
/// hardcoded constants. Passed through `MuxHandle::spawn()` and
/// `open_portforward_pair()`.
#[derive(Clone)]
pub struct MuxConfig {
    /// Number of parallel WebSocket connections per session. Each gets its own
    /// reader/writer task pair. Streams are distributed round-robin across the
    /// pool for parallel TLS writes. Default 1 (single connection).
    /// Read by the caller (`Session::with_config`) to determine pool sizing;
    /// not consumed within the mux layer itself.
    pub pool_size: usize,
    /// Initial per-stream send/recv window size (bytes).
    pub initial_window_size: u32,
    /// PROTOCOL hard cap: maximum concurrent stream *pairs* the SPDY peer
    /// accepts. Exceeding this is a protocol violation.
    pub max_concurrent_streams: u32,
    /// SCHEDULING cap: maximum concurrent stream pairs the open path will
    /// allow before returning `CapacityExhausted`. Provides headroom below
    /// the hard cap so control frames (RST_STREAM, WINDOW_UPDATE) always
    /// have capacity.
    pub operating_max_streams: u32,
    /// Maximum SPDY DATA frame payload size. Outgoing frames are split to
    /// respect the peer's limit; incoming frames exceeding this are rejected.
    pub max_frame_size: u32,
    /// Capacity of the data command channel (writer task inbox).
    pub cmd_buffer_size: usize,
    /// Capacity of the control command channel (writer task inbox).
    /// Control commands (CloseStream, GoAway, PING, PONG, WINDOW_UPDATE)
    /// are drained with priority over data commands.
    pub control_buffer_size: usize,
    /// Capacity of the open-registration channel (per-worker inbox).
    /// Each of the 5 workers gets its own channel with this capacity.
    pub reg_buffer_size: usize,
    /// Capacity of the close-registration channel (per-worker inbox).
    /// Separate from open registrations so teardown is never blocked by
    /// a burst of opens.
    pub close_reg_buffer_size: usize,
    /// Per-worker bounded queue capacity for inbound frames.
    pub worker_queue_size: usize,
    /// Interval between keepalive PINGs.
    pub ping_interval: Duration,
    /// Time to wait for a PING response before tearing down.
    pub ping_timeout: Duration,
    /// Maximum duration for `sink.flush()` before tearing down.
    pub write_timeout: Duration,
    /// Time without any incoming frame before sending a probe PING.
    pub idle_timeout: Duration,
    /// Capacity of the dedicated WINDOW_UPDATE channel. Separating
    /// flow-control frames from control_tx ensures they are never lost
    /// due to contention with PING/PONG/RST_STREAM/CloseStream traffic.
    /// Default 256, enough for operating_max_streams * 4 plus session-level.
    pub window_buffer_size: usize,
    /// Per-stream inbound data channel capacity. When full, the worker
    /// withholds WINDOW_UPDATE for this stream so the peer backs off naturally.
    pub stream_data_buffer: usize,
    /// Per-stream inbound error channel capacity.
    pub stream_error_buffer: usize,
}

impl Default for MuxConfig {
    fn default() -> Self {
        Self {
            pool_size: 1,
            // 1MB initial window. SPDY/3.1 spec default is 64KB. At our
            // measured base RTT (~158ms) any response consuming more than
            // 50% of the window triggers a WINDOW_UPDATE round-trip stall.
            // With 256KB, benchmarks showed bimodal latency: normal requests
            // at 150-260ms, but periodic spikes to 500-1400ms when multiple
            // streams exhausted their windows simultaneously and
            // WINDOW_UPDATEs cascaded through the reader, worker, and waker
            // path. 1MB eliminates WINDOW_UPDATE round-trips for all but
            // the largest responses, matching max_frame_size for symmetry.
            initial_window_size: 1024 * 1024,
            max_concurrent_streams: 100,
            // Operating cap: 64 pairs leaves headroom below the 100 hard
            // cap so control frames always have capacity. At pool=6 this
            // gives 6×64 = 384 total operating pairs.
            operating_max_streams: 64,
            // 1MB max frame size. SPDY/3.1 allows up to 16MB. The previous
            // 16KB default fragmented every 64KB TCP read into four frames,
            // quadrupling writer overhead.
            max_frame_size: 1024 * 1024,
            cmd_buffer_size: 512,
            control_buffer_size: 256,
            reg_buffer_size: 128,
            close_reg_buffer_size: 64,
            worker_queue_size: 128,
            ping_interval: Duration::from_secs(30),
            ping_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(120),
            window_buffer_size: 256,
            // Per-stream buffer: when full, the worker stops issuing
            // WINDOW_UPDATE for this stream so the peer naturally pauses.
            // 64 elements keeps backpressure tight, preventing bufferbloat
            // that manifests as p99 spikes. Backpressure propagates to every
            // stream on the same mux handle since the worker is shared.
            // Memory ceiling: 64 frames × 1MB max = 64MB worst case;
            // typical frames under 16KB land around 1MB.
            stream_data_buffer: 64,
            stream_error_buffer: 8,
        }
    }
}
