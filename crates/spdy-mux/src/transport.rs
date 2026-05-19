//! Transport abstraction for the SPDY multiplexer.
//!
//! The mux is generic over [`WsFrameWriter`] and [`WsFrameReader`] traits,
//! decoupling SPDY framing from the concrete transport. Two adapters ship
//! with the crate:
//!
//! - [`FastWsWriter`] / [`FastWsReader`]: WebSocket transport via the
//!   `fastwebsockets` library. Used when SPDY frames are tunnelled inside
//!   WebSocket binary messages (`SPDY/3.1+portforward.k8s.io`).
//! - [`RawSpdyWriter`] / [`RawSpdyReader`]: raw transport over any `AsyncRead`
//!   / `AsyncWrite`. Used when SPDY frames flow directly over an HTTP-upgraded
//!   connection (legacy `kubectl port-forward` wire protocol, no WebSocket
//!   framing).
//!
//! # Masking copy budget
//!
//! WebSocket client role mandates frame masking. The adapter incurs exactly
//! one copy per write:
//!
//! - `Payload::Borrowed`: `write_frame` copies the borrowed slice into an
//!   internal buffer and masks there. One allocation plus one copy.
//! - `Payload::Bytes(BytesMut)`: `write_frame` masks the `BytesMut` in-place.
//!   One allocation (the `Bytes` to `BytesMut` conversion) plus zero copy for
//!   masking. Net: one copy total, same as Borrowed.
//!
//! `Payload::Borrowed` keeps the simpler path; both copy once. If profiling
//! reveals masking as a bottleneck, switch to `Payload::Bytes` with
//! `BytesMut::from(&payload[..])` to eliminate the second allocation inside
//! fastwebsockets (mask in-place instead of allocate-copy-mask).

use std::future::Future;

use bytes::{
    Bytes,
    BytesMut,
};
use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    AsyncWrite,
    AsyncWriteExt,
};

/// Transport-level error for WebSocket read/write operations.
#[derive(Debug)]
pub enum TransportError {
    /// Error from the fastwebsockets library.
    FastWebSocket(fastwebsockets::WebSocketError),
    /// Generic I/O error (e.g., broken pipe, connection reset).
    Io(std::io::Error),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FastWebSocket(e) => write!(f, "fastwebsockets: {e}"),
            Self::Io(e) => write!(f, "transport I/O: {e}"),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FastWebSocket(e) => Some(e),
            Self::Io(e) => Some(e),
        }
    }
}

impl From<fastwebsockets::WebSocketError> for TransportError {
    fn from(e: fastwebsockets::WebSocketError) -> Self {
        Self::FastWebSocket(e)
    }
}

impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Decoded WebSocket message returned by [`WsFrameReader`].
///
/// The mux reader matches on these variants to dispatch SPDY frames, handle
/// WS-level keepalive (Ping triggers Pong), and detect connection close.
///
/// Skip-class frames (Pong, Text, Continuation) are absorbed inside the
/// adapter's read loop and never reach the mux. The mux reader's idle timer
/// resets on any successful return from `read_message()`, so absorbed frames
/// still count as activity.
pub enum WsMessage {
    /// Binary payload containing one or more SPDY frames.
    Binary(Bytes),
    /// WebSocket-level PING from the peer. The mux reader enqueues a
    /// `SendWsPong` command so the writer responds.
    Ping(Bytes),
    /// WebSocket close frame. The mux reader breaks its loop.
    Close,
}

/// Async writer for WebSocket binary frames.
///
/// The fastwebsockets adapter writes directly to the socket. Callers MUST
/// call [`flush`](WsFrameWriter::flush) after a batch of
/// [`write_binary`](WsFrameWriter::write_binary) calls to ensure data reaches
/// the peer.
pub trait WsFrameWriter: Send {
    /// Write a binary WebSocket frame. May buffer internally.
    ///
    /// The `payload` is the complete SPDY frame (header + data), already
    /// encoded by the codec. The adapter wraps it in a WS binary frame.
    fn write_binary(
        &mut self, payload: Bytes,
    ) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// Write a WebSocket PONG frame in response to a peer PING.
    fn write_pong(
        &mut self, payload: Bytes,
    ) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// Flush all buffered data to the underlying transport.
    ///
    /// For tungstenite, flushes the Sink's internal buffer.
    /// For fastwebsockets, this is a no-op. See [`FastWsWriter::flush`]
    /// for the rationale.
    fn flush(&mut self) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// Send a WebSocket Close frame and shut down the write half.
    fn close(&mut self) -> impl Future<Output = Result<(), TransportError>> + Send;
}

/// Async reader for WebSocket frames.
///
/// Returns [`WsMessage`] variants for binary data, pings, and close.
/// Skip-class frames (Pong, Text, Continuation) are absorbed inside the
/// read loop; the mux never sees them.
pub trait WsFrameReader: Send {
    /// Read the next actionable WebSocket message.
    ///
    /// Returns `None` when the connection is closed (EOF). Returns
    /// `Some(Err(_))` on transport error. The mux reader breaks its loop
    /// on both cases.
    ///
    /// Pong, Text, and Continuation frames are silently consumed inside
    /// the adapter's loop and never returned.
    fn read_message(
        &mut self,
    ) -> impl Future<Output = Option<Result<WsMessage, TransportError>>> + Send;
}

/// fastwebsockets write half adapter.
///
/// Wraps `WebSocketWrite<WriteHalf<S>>`. Each `write_binary` call writes
/// directly to the underlying stream via `AsyncWrite::write_all` /
/// `write_vectored` (no WS-level buffering).
pub struct FastWsWriter<S: AsyncWrite + Unpin + Send> {
    ws: fastwebsockets::WebSocketWrite<S>,
}

impl<S: AsyncWrite + Unpin + Send> WsFrameWriter for FastWsWriter<S> {
    async fn write_binary(&mut self, payload: Bytes) -> Result<(), TransportError> {
        // Payload::Borrowed(&payload) borrows from the function parameter.
        // The borrow lives for the duration of write_frame. `payload` is owned
        // by this stack frame and outlives the await point.
        //
        // fastwebsockets copies the borrowed slice during masking (one copy,
        // unavoidable for WS client role). See module-level doc for the full
        // copy budget analysis.
        let frame = fastwebsockets::Frame::binary(fastwebsockets::Payload::Borrowed(&payload));
        self.ws.write_frame(frame).await?;
        Ok(())
    }

    async fn write_pong(&mut self, payload: Bytes) -> Result<(), TransportError> {
        // Same lifetime reasoning as write_binary: payload is owned by this
        // stack frame, borrow is valid across the write_frame await.
        let frame = fastwebsockets::Frame::pong(fastwebsockets::Payload::Borrowed(&payload));
        self.ws.write_frame(frame).await?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), TransportError> {
        // No-op. fastwebsockets writes through AsyncWrite::write_all (for
        // small frames) or write_vectored (for frames > writev_threshold),
        // both of which guarantee data has been written to the underlying
        // stream before returning.
        //
        // No application-level buffer exists at this layer:
        //
        // - fastwebsockets has no internal write queue. write_frame writes directly to
        //   the stream and returns only after write_all completes.
        //
        // - The TLS layer (openssl, configured by kube-rs) emits a TLS record per write
        //   call. There is no record-coalescing buffer that flush() would drain.
        //
        // - The TCP layer has TCP_NODELAY set (in client.rs socket config), disabling
        //   Nagle's algorithm. Data enters the kernel send buffer and is transmitted
        //   immediately.
        //
        // If a future change introduces a buffering layer between this
        // adapter and the socket (e.g., BufWriter for write coalescing),
        // this method must be updated to propagate flush.
        Ok(())
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        let frame = fastwebsockets::Frame::close(1000, &[]);
        self.ws.write_frame(frame).await?;
        Ok(())
    }
}

/// fastwebsockets read half adapter.
///
/// Wraps `FragmentCollectorRead<ReadHalf<S>>` for safe handling of any
/// unexpected WS fragmentation (negligible overhead in the common non-
/// fragmented case: one branch per frame, no allocation).
///
/// `auto_pong` and `auto_close` are disabled. PING and Close frames are
/// surfaced as [`WsMessage::Ping`] and [`WsMessage::Close`] so the mux
/// reader handles them through the existing command path (SendWsPong,
/// break loop). Pong, Text, and Continuation frames are absorbed in the
/// read loop; the mux never sees them.
pub struct FastWsReader<S: AsyncRead + Unpin + Send> {
    ws: fastwebsockets::FragmentCollectorRead<S>,
}

impl<S: AsyncRead + Unpin + Send> FastWsReader<S> {
    pub const fn new(ws: fastwebsockets::FragmentCollectorRead<S>) -> Self {
        Self { ws }
    }
}

impl<S: AsyncRead + Unpin + Send> WsFrameReader for FastWsReader<S> {
    async fn read_message(&mut self) -> Option<Result<WsMessage, TransportError>> {
        // Loop absorbs skip-class frames (Pong, Text, Continuation).
        // The mux reader's idle timer resets on any successful return,
        // so absorbed frames still count as activity.
        loop {
            // No-op send_fn: auto_pong and auto_close are disabled, so
            // the callback is never invoked. PING/PONG is handled at the
            // mux level via MuxCommand::SendWsPong.
            let frame = match self
                .ws
                .read_frame(&mut |_| async { Ok::<(), fastwebsockets::WebSocketError>(()) })
                .await
            {
                Ok(f) => f,
                // ConnectionClose: peer initiated a clean close, treat as EOF.
                Err(fastwebsockets::WebSocketError::ConnectionClosed) => return None,
                // All other errors (IoError, protocol violations) are
                // transport failures. Both EOF (None) and transport error
                // (Some(Err)) reach the same mux teardown: the reader breaks
                // its loop, calls cancel.cancel(), the supervisor fires,
                // pending_replies fail with MuxClosed, and send windows are
                // poisoned. The distinction affects only the log level
                // (debug for EOF, warn for error).
                Err(e) => return Some(Err(TransportError::FastWebSocket(e))),
            };

            match frame.opcode {
                fastwebsockets::OpCode::Binary => {
                    // Zero-copy path: fastwebsockets stores read payloads as
                    // Payload::Bytes(BytesMut) via split_to from internal
                    // buffer. freeze() converts to Bytes via refcount bump,
                    // O(1), no copy.
                    let bytes = payload_to_bytes(frame.payload);
                    return Some(Ok(WsMessage::Binary(bytes)));
                }
                fastwebsockets::OpCode::Ping => {
                    let bytes = payload_to_bytes(frame.payload);
                    return Some(Ok(WsMessage::Ping(bytes)));
                }
                fastwebsockets::OpCode::Close => return Some(Ok(WsMessage::Close)),
                // Skip-class: absorb and re-read. The mux reader's idle
                // timer resets on any successful read_message() return,
                // not on specific variants.
                fastwebsockets::OpCode::Pong => {}
                fastwebsockets::OpCode::Text | fastwebsockets::OpCode::Continuation => {
                    tracing::warn!(
                        opcode = ?frame.opcode,
                        "fastwebsockets reader: unexpected opcode from K8s apiserver, \
                         absorbing (SPDY tunnel expects only Binary frames)"
                    );
                }
            }
        }
    }
}

/// Extract `Bytes` from a fastwebsockets `Payload` with minimal copying.
///
/// - `Payload::Bytes(BytesMut)`: `freeze()`, O(1) zero-copy.
/// - `Payload::Owned(Vec<u8>)`: `Bytes::from(vec)`, O(1) takes ownership.
/// - `Payload::Borrowed` / `BorrowedMut`: forces a copy. Should not happen on
///   the read path since fastwebsockets always returns `Payload::Bytes` for
///   data read from the socket.
fn payload_to_bytes(payload: fastwebsockets::Payload<'_>) -> Bytes {
    match payload {
        fastwebsockets::Payload::Bytes(bm) => bm.freeze(),
        fastwebsockets::Payload::Owned(v) => Bytes::from(v),
        fastwebsockets::Payload::Borrowed(b) => Bytes::copy_from_slice(b),
        fastwebsockets::Payload::BorrowedMut(b) => Bytes::copy_from_slice(b),
    }
}

/// Construct fastwebsockets writer/reader adapters from a raw upgraded
/// HTTP connection.
///
/// Uses [`fastwebsockets::after_handshake_split`] to create pre-split
/// read/write halves from the already-upgraded stream. Configures the
/// WebSocket for SPDY tunnel use:
///
/// - `auto_pong = false`: PING/PONG handled at the mux level.
/// - `auto_close = false`: Close handled at the mux level.
/// - `set_auto_apply_mask = true` (default): Client role masking.
/// - `set_writev = true` (default): Vectored I/O for large frames.
pub fn split_fastws<S>(
    stream: S,
) -> (
    FastWsWriter<tokio::io::WriteHalf<S>>,
    FastWsReader<tokio::io::ReadHalf<S>>,
)
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let (read_half, write_half) = tokio::io::split(stream);

    let (mut ws_read, ws_write) =
        fastwebsockets::after_handshake_split(read_half, write_half, fastwebsockets::Role::Client);

    // Disable auto_pong/auto_close on the read half so PING and Close
    // frames surface as WsMessage variants for the mux to handle.
    ws_read.set_auto_pong(false);
    ws_read.set_auto_close(false);

    let frag_read = fastwebsockets::FragmentCollectorRead::new(ws_read);

    (FastWsWriter { ws: ws_write }, FastWsReader::new(frag_read))
}

/// Read buffer size for the raw SPDY transport. Sized to drain a typical
/// 256 KiB socket receive buffer in a few reads while keeping the per-task
/// stack allocation modest.
const RAW_READ_BUF_SIZE: usize = 16 * 1024;

/// Raw-transport write half: writes SPDY frame bytes directly to the
/// underlying [`AsyncWrite`] with no WebSocket framing.
///
/// Used when SPDY/3.1 flows over an HTTP/1.1 upgraded connection (the
/// legacy `kubectl port-forward` wire protocol). Each `write_binary` call
/// writes one full SPDY frame.
pub struct RawSpdyWriter<W: AsyncWrite + Unpin + Send> {
    inner: W,
}

impl<W: AsyncWrite + Unpin + Send> RawSpdyWriter<W> {
    pub const fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: AsyncWrite + Unpin + Send> WsFrameWriter for RawSpdyWriter<W> {
    async fn write_binary(&mut self, payload: Bytes) -> Result<(), TransportError> {
        // SPDY frames are self-delimiting (8-byte header carries the
        // payload length), so writing the encoded frame bytes verbatim
        // is correct.
        self.inner.write_all(&payload).await?;
        Ok(())
    }

    async fn write_pong(&mut self, _payload: Bytes) -> Result<(), TransportError> {
        // Raw SPDY has no WebSocket-level PING/PONG. The mux only invokes
        // this in response to a `WsMessage::Ping`, which the raw reader
        // never produces, so this path is unreachable in practice.
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), TransportError> {
        self.inner.flush().await?;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.inner.shutdown().await?;
        Ok(())
    }
}

/// Raw-transport read half: reads SPDY frame bytes from the underlying
/// [`AsyncRead`] and yields them as [`WsMessage::Binary`] chunks. The mux
/// reader accumulates the chunks into its own buffer and decodes complete
/// SPDY frames from there, so chunk boundaries need not align with frame
/// boundaries.
pub struct RawSpdyReader<R: AsyncRead + Unpin + Send> {
    inner: R,
    buf: BytesMut,
}

impl<R: AsyncRead + Unpin + Send> RawSpdyReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buf: BytesMut::with_capacity(RAW_READ_BUF_SIZE),
        }
    }
}

impl<R: AsyncRead + Unpin + Send> WsFrameReader for RawSpdyReader<R> {
    async fn read_message(&mut self) -> Option<Result<WsMessage, TransportError>> {
        // Ensure the buffer has spare capacity. read_buf appends in-place
        // and grows the buffer if needed.
        if self.buf.capacity() == self.buf.len() {
            self.buf.reserve(RAW_READ_BUF_SIZE);
        }
        match self.inner.read_buf(&mut self.buf).await {
            Ok(0) => None,
            Ok(_) => {
                let chunk = self.buf.split().freeze();
                Some(Ok(WsMessage::Binary(chunk)))
            }
            Err(e) => Some(Err(TransportError::Io(e))),
        }
    }
}

/// Construct raw SPDY writer/reader adapters from an already-upgraded
/// connection. Use this for the legacy `Upgrade: SPDY/3.1` path where the
/// apiserver speaks raw SPDY frames (no WebSocket envelope).
pub fn split_raw_spdy<S>(
    stream: S,
) -> (
    RawSpdyWriter<tokio::io::WriteHalf<S>>,
    RawSpdyReader<tokio::io::ReadHalf<S>>,
)
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let (read_half, write_half) = tokio::io::split(stream);
    (
        RawSpdyWriter::new(write_half),
        RawSpdyReader::new(read_half),
    )
}
