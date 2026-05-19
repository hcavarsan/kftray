use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::task::{
    Context,
    Poll,
};

use bytes::{
    Buf,
    Bytes,
    BytesMut,
};
use tokio::io::{
    AsyncBufRead,
    AsyncRead,
    AsyncWrite,
    ReadBuf,
};
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;

use crate::error::Error;
use crate::mux::{
    MuxCommand,
    MuxHandle,
    SendWindow,
    StreamRegistration,
};

/// Bidirectional SPDY port-forward stream backed by a (data, error) stream
/// pair.
///
/// Streams are **lazily opened on the wire**:
/// `MuxHandle::open_portforward_pair` reserves a session slot and creates the
/// per-stream channels, but no SPDY `SYN_STREAM` frame is sent until the relay
/// actually writes its first byte. This avoids the idle-pod-TCP-close race for
/// fast-closing target servers (e.g. `static-web-server`) while preserving the
/// pre-opened spare-stream throughput optimization.
///
/// Implements `AsyncRead + AsyncWrite` on the data half. The error half is
/// available via `split()`.
pub struct Stream {
    state: StreamState,
}

enum StreamState {
    /// Pair reserved, channels created, but no SPDY stream IDs allocated
    /// and no `SYN_STREAM` on the wire yet. Transitions to `Opened` on the
    /// first non-empty `poll_write`.
    Unopened {
        port: u16,
        mux: MuxHandle,
        data_rx: mpsc::Receiver<Bytes>,
        error_rx: mpsc::Receiver<Bytes>,
        /// Sender handed to the data worker at realize time. `Option` so it
        /// can be moved out without re-creating the channel.
        pending_data_tx: Option<mpsc::Sender<Bytes>>,
        pending_error_tx: Option<mpsc::Sender<Bytes>>,
        max_frame_size: u32,
        read_buf: Option<Bytes>,
        read_eof: bool,
        /// In-flight lazy-open future. `Some` once `poll_write` started
        /// a realize call and the first poll returned `Pending`. The
        /// future owns a clone of the first payload so cancellation safety
        /// is preserved across re-polls.
        open_in_progress: Option<LazyOpenFuture>,
        /// Guard that releases `active_pairs` on drop. Always present in
        /// the Unopened state.
        release_guard: Option<PairReleaseGuard>,
    },
    Opened {
        data_id: u32,
        data_rx: mpsc::Receiver<Bytes>,
        error_rx: mpsc::Receiver<Bytes>,
        mux: MuxHandle,
        write_tx: PollSender<MuxCommand>,
        send_window: Arc<SendWindow>,
        max_frame_size: u32,
        read_buf: Option<Bytes>,
        read_eof: bool,
        graceful_shutdown: Arc<AtomicBool>,
        guard: StreamGuard,
    },
    /// Terminal state used while moving out of `Unopened` during realize.
    /// Should never be observed by a user.
    Transitioning,
}

/// Boxed future driving a single lazy-open attempt.
type LazyOpenFuture = Pin<Box<dyn Future<Output = Result<OpenedStreamParts, Error>> + Send>>;

/// Guards the session's `active_pairs` counter for unopened streams.
/// Once the stream realizes, the counter is owned by `StreamGuard` instead
/// and this guard is disarmed so drop becomes a no-op.
struct PairReleaseGuard {
    mux: MuxHandle,
    armed: bool,
}

impl PairReleaseGuard {
    fn new(mux: MuxHandle) -> Self {
        Self { mux, armed: true }
    }

    /// Disarm without releasing. Use when ownership of the pair counter
    /// transfers to a `StreamGuard`.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PairReleaseGuard {
    fn drop(&mut self) {
        if self.armed {
            self.mux.release_pair();
        }
    }
}

/// Open-stream guard owning the IDs, drop permits, and graceful-shutdown
/// flag. Replaces the previous `StreamGuard` and carries the same RST /
/// worker-close contract.
struct StreamGuard {
    data_id: u32,
    error_id: u32,
    mux: MuxHandle,
    ctrl_permit_error: Option<mpsc::OwnedPermit<MuxCommand>>,
    ctrl_permit_data: Option<mpsc::OwnedPermit<MuxCommand>>,
    close_reg_permit_error: Option<mpsc::OwnedPermit<StreamRegistration>>,
    close_reg_permit_data: Option<mpsc::OwnedPermit<StreamRegistration>>,
    /// Set to true when `poll_shutdown()` sends DATA+FIN (graceful half-close).
    /// When true, `Drop` skips RST_STREAM for the data stream. The peer
    /// already knows we're done writing and will close its end naturally.
    /// This mirrors TCP semantics: shutdown(SHUT_WR) + close() sends FIN,
    /// not RST.
    graceful_shutdown: Arc<AtomicBool>,
}

/// RST_STREAM status code for CANCEL.
const RST_STATUS_CANCEL: u32 = 5;

impl Drop for StreamGuard {
    fn drop(&mut self) {
        // GUARANTEED path: use pre-reserved permits for infallible delivery.
        // OwnedPermit::send() is synchronous, so no async is needed in Drop.
        let graceful = self.graceful_shutdown.load(Ordering::Acquire);

        // The error stream is ALREADY half-closed at open time: the
        // `OpenPortForwardAndWrite` writer command emitted an empty
        // DATA+FIN on `error_id` right after the two SYN_STREAM frames
        // (matching kubectl's `errorStream.Close()` behavior). Sending
        // RST_STREAM here would be wrong — we never use the error stream
        // for writes after open, and the peer interprets RST_STREAM as
        // an abnormal termination. Drop the permit unused.
        //
        // Data stream: skip RST if poll_shutdown() already sent DATA+FIN
        // (graceful half-close).
        let _ = self.ctrl_permit_error.take();
        if !graceful && let Some(permit) = self.ctrl_permit_data.take() {
            permit.send(MuxCommand::CloseStream {
                stream_id: self.data_id,
                status: RST_STATUS_CANCEL,
            });
        }

        // 2. Notify workers via close-reg channels (stream entry cleanup and
        //    send-window poisoning).
        if let Some(permit) = self.close_reg_permit_error.take() {
            permit.send(StreamRegistration::Close {
                stream_id: self.error_id,
            });
        }
        if let Some(permit) = self.close_reg_permit_data.take() {
            permit.send(StreamRegistration::Close {
                stream_id: self.data_id,
            });
        }

        // 3. Release the session slot.
        self.mux.release_pair();
    }
}

/// Runtime handles needed to construct a lazily-opened SPDY stream.
pub(crate) struct UnopenedStreamParts {
    pub port: u16,
    pub mux: MuxHandle,
    pub data_rx: mpsc::Receiver<Bytes>,
    pub error_rx: mpsc::Receiver<Bytes>,
    pub pending_data_tx: mpsc::Sender<Bytes>,
    pub pending_error_tx: mpsc::Sender<Bytes>,
    pub max_frame_size: u32,
}

/// Result of a successful realize call: the wire-visible bits a stream
/// needs to switch into `Opened` state.
pub(crate) struct OpenedStreamParts {
    pub data_id: u32,
    pub error_id: u32,
    pub send_window: Arc<SendWindow>,
    pub ctrl_permit_error: mpsc::OwnedPermit<MuxCommand>,
    pub ctrl_permit_data: mpsc::OwnedPermit<MuxCommand>,
    pub close_reg_permit_error: mpsc::OwnedPermit<StreamRegistration>,
    pub close_reg_permit_data: mpsc::OwnedPermit<StreamRegistration>,
}

impl Stream {
    pub(crate) fn new_unopened(parts: UnopenedStreamParts) -> Self {
        let UnopenedStreamParts {
            port,
            mux,
            data_rx,
            error_rx,
            pending_data_tx,
            pending_error_tx,
            max_frame_size,
        } = parts;
        let release_guard = PairReleaseGuard::new(mux.clone());
        Self {
            state: StreamState::Unopened {
                port,
                mux,
                data_rx,
                error_rx,
                pending_data_tx: Some(pending_data_tx),
                pending_error_tx: Some(pending_error_tx),
                max_frame_size,
                read_buf: None,
                read_eof: false,
                open_in_progress: None,
                release_guard: Some(release_guard),
            },
        }
    }

    /// Returns true if the remote has already closed this stream's read
    /// side (FIN or RST received while idle). Used by spare-stream checkout
    /// to discard stale pre-opened streams.
    ///
    /// Unopened streams are never stale: no `SYN_STREAM` was sent yet, so
    /// the apiserver has not created a backing pod TCP connection.
    pub fn is_read_closed(&self) -> bool {
        match &self.state {
            StreamState::Unopened {
                read_eof, data_rx, ..
            } => *read_eof || data_rx.is_closed(),
            StreamState::Opened {
                read_eof, data_rx, ..
            } => *read_eof || data_rx.is_closed(),
            StreamState::Transitioning => false,
        }
    }

    /// Split into data half (AsyncRead + AsyncWrite) and error half
    /// (AsyncRead).
    ///
    /// Splitting an unopened stream is supported: both halves share a
    /// single `LazyOpenSlot` driven by the data half's first write.
    pub fn split(self) -> (DataStream, ErrorStream) {
        match self.state {
            StreamState::Unopened {
                port,
                mux,
                data_rx,
                error_rx,
                pending_data_tx,
                pending_error_tx,
                max_frame_size,
                read_buf,
                read_eof,
                open_in_progress,
                release_guard,
            } => {
                let shared = Arc::new(parking_lot::Mutex::new(SharedSplitState::Unopened(
                    UnopenedShared {
                        port,
                        mux: mux.clone(),
                        pending_data_tx,
                        pending_error_tx,
                        open_in_progress,
                        release_guard,
                    },
                )));
                (
                    DataStream {
                        data_rx,
                        max_frame_size,
                        read_buf,
                        read_eof,
                        shared: Arc::clone(&shared),
                    },
                    ErrorStream {
                        error_rx,
                        error_buf: None,
                        error_eof: false,
                        shared,
                    },
                )
            }
            StreamState::Opened {
                data_id,
                data_rx,
                error_rx,
                mux,
                write_tx,
                send_window,
                max_frame_size,
                read_buf,
                read_eof,
                graceful_shutdown,
                guard,
            } => {
                let opened = OpenedShared {
                    data_id,
                    mux,
                    write_tx,
                    send_window,
                    graceful_shutdown,
                    guard,
                };
                let shared = Arc::new(parking_lot::Mutex::new(SharedSplitState::Opened(opened)));
                (
                    DataStream {
                        data_rx,
                        max_frame_size,
                        read_buf,
                        read_eof,
                        shared: Arc::clone(&shared),
                    },
                    ErrorStream {
                        error_rx,
                        error_buf: None,
                        error_eof: false,
                        shared,
                    },
                )
            }
            StreamState::Transitioning => {
                unreachable!("split() called on transitioning stream")
            }
        }
    }
}

impl Unpin for Stream {}

/// Shared `poll_read` logic for channel-backed streams.
fn poll_read_channel(
    rx: &mut mpsc::Receiver<Bytes>, read_buf: &mut Option<Bytes>, read_eof: &mut bool,
    cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
) -> Poll<io::Result<()>> {
    if *read_eof {
        return Poll::Ready(Ok(()));
    }

    // Drain buffered data first
    if let Some(ref mut remaining) = *read_buf {
        let to_copy = remaining.len().min(buf.remaining());
        buf.put_slice(&remaining[..to_copy]);
        if to_copy >= remaining.len() {
            *read_buf = None;
        } else {
            *remaining = remaining.slice(to_copy..);
        }
        return Poll::Ready(Ok(()));
    }

    // Poll channel for more data
    match rx.poll_recv(cx) {
        Poll::Ready(Some(data)) => {
            let to_copy = data.len().min(buf.remaining());
            buf.put_slice(&data[..to_copy]);
            if to_copy < data.len() {
                *read_buf = Some(data.slice(to_copy..));
            }
            Poll::Ready(Ok(()))
        }
        Poll::Ready(None) => {
            *read_eof = true;
            Poll::Ready(Ok(()))
        }
        Poll::Pending => Poll::Pending,
    }
}

/// Shared `consume` logic for `AsyncBufRead`.
fn consume_channel_buf(read_buf: &mut Option<Bytes>, amt: usize) {
    if let Some(ref mut bytes) = *read_buf {
        let consumed = amt.min(bytes.len());
        bytes.advance(consumed);
        if bytes.is_empty() {
            *read_buf = None;
        }
    }
}

/// Shared `poll_fill_buf` logic for channel-backed streams.
fn poll_fill_buf_channel<'a>(
    rx: &'a mut mpsc::Receiver<Bytes>, read_buf: &'a mut Option<Bytes>, read_eof: &'a mut bool,
    cx: &mut Context<'_>,
) -> Poll<io::Result<&'a [u8]>> {
    loop {
        if read_buf.as_ref().is_some_and(|b| !b.is_empty()) {
            return Poll::Ready(Ok(read_buf.as_deref().unwrap()));
        }
        if read_buf.is_some() {
            *read_buf = None;
        }
        if *read_eof {
            return Poll::Ready(Ok(&[]));
        }
        match rx.poll_recv(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => {
                *read_eof = true;
                return Poll::Ready(Ok(&[]));
            }
            Poll::Ready(Some(b)) => {
                *read_buf = Some(b);
            }
        }
    }
}

/// Send DATA+FIN on a fully opened stream and mark the guard graceful so
/// `Drop` skips RST_STREAM for the data half.
fn poll_shutdown_opened(
    graceful_shutdown: &AtomicBool, mux: &MuxHandle, data_id: u32,
) -> Poll<io::Result<()>> {
    graceful_shutdown.store(true, Ordering::Release);
    let _ = mux.send_data_nonblocking(data_id, Bytes::new(), true);
    Poll::Ready(Ok(()))
}

fn broken_pipe() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "mux closed")
}

/// Build a complete SPDY DATA frame in a single allocation and send it as a
/// pre-encoded raw frame, enforcing per-stream send window flow control.
///
/// Session-level send window is intentionally NOT enforced. The peer (kubelet
/// apiserver) never sends session-level WINDOW_UPDATE with stream_id=0, so
/// enforcing it would deadlock once the initial window drains. Per-stream
/// windows still provide proper backpressure.
///
/// Clamps write size to max_frame_size - 8 (the 8-byte SPDY DATA header).
///
/// Ordering invariant (prevents window leak on Pending):
///   1. `poll_reserve` cmd_tx permit (may return Pending; no side effects)
///   2. Read send window, compute n = min(buf.len(), stream_window,
///      max_payload)
///   3. If n == 0: register waker, return Pending
///   4. `stream_window.consume(n)`: debit committed
///   5. `send_item(frame)`: infallible after successful reserve
///   6. return Ready(Ok(n))
fn poll_write_via_sender(
    write_tx: &mut PollSender<MuxCommand>, stream_id: u32, send_window: &SendWindow,
    max_frame_size: u32, cx: &mut Context<'_>, buf: &[u8],
) -> Poll<io::Result<usize>> {
    // Early check: stream was closed (window poisoned by reader)
    if send_window.is_closed() {
        return Poll::Ready(Err(broken_pipe()));
    }

    // Acquire cmd_tx permit. No side effects on Pending.
    match write_tx.poll_reserve(cx) {
        Poll::Ready(Ok(())) => {}
        Poll::Ready(Err(_)) => return Poll::Ready(Err(broken_pipe())),
        Poll::Pending => return Poll::Pending,
    }

    // Maximum DATA payload is max_frame_size - 8 (8-byte SPDY DATA header).
    let max_payload = (max_frame_size as usize).saturating_sub(8);
    let max_payload = if max_payload == 0 {
        buf.len()
    } else {
        max_payload
    };

    // Compute write size, clamped to per-stream window AND max_frame_size.
    let stream_avail = send_window.available().max(0) as usize;
    let mut n = buf.len().min(stream_avail).min(max_payload);

    if n == 0 {
        // Per-stream window exhausted. Register waker.
        send_window.register_waker(cx.waker());

        // Re-check for poisoning
        if send_window.is_closed() {
            return Poll::Ready(Err(broken_pipe()));
        }
        // Re-check window after registering waker (lost wake guard)
        let stream_avail = send_window.available().max(0) as usize;
        n = buf.len().min(stream_avail).min(max_payload);
        if n == 0 {
            return Poll::Pending;
        }
    }

    // Debit per-stream window via CAS.
    if !send_window.consume(n) {
        return Poll::Ready(Err(broken_pipe()));
    }

    // Build DATA frame and send via the reserved permit.
    let write_buf = &buf[..n];
    let mut frame = BytesMut::with_capacity(8 + n);
    frame.extend_from_slice(&(stream_id & 0x7FFF_FFFF).to_be_bytes());
    let flags_len = (n as u32) & 0x00FF_FFFF;
    frame.extend_from_slice(&flags_len.to_be_bytes());
    frame.extend_from_slice(write_buf);

    let cmd = MuxCommand::SendRawFrame {
        frame: frame.freeze(),
    };
    match write_tx.send_item(cmd) {
        Ok(()) => Poll::Ready(Ok(n)),
        Err(_) => Poll::Ready(Err(broken_pipe())),
    }
}

/// Borrowed arguments for [`poll_lazy_open`]. Bundles the per-stream lazy
/// state into one borrow so the function signature stays under the
/// `clippy::too_many_arguments` threshold and the call sites read as one
/// logical unit instead of six positional arguments.
struct LazyOpenArgs<'a> {
    port: u16,
    max_frame_size: u32,
    mux: &'a MuxHandle,
    pending_data_tx: &'a mut Option<mpsc::Sender<Bytes>>,
    pending_error_tx: &'a mut Option<mpsc::Sender<Bytes>>,
    open_in_progress: &'a mut Option<LazyOpenFuture>,
}

/// Drive the lazy-open path for the data half of a stream. On success the
/// caller transitions `Stream` (or the split `DataStream`'s shared slot)
/// into `Opened` state and returns the number of bytes accepted from `buf`.
///
/// Returns:
/// - `Ready(Ok(n))` if open completed and `n` bytes were committed to the
///   atomic open+write batch (the bytes are owned by the writer now).
/// - `Pending` if the realize future has not completed yet.
/// - `Ready(Err(_))` on fatal mux error.
///
/// The first payload is capped to one SPDY DATA frame (`max_frame_size - 8`)
/// because `SpdyCodec::encode_data` does not split. Anything beyond that in
/// the caller's first `write_all()` lands in subsequent normal `poll_write`
/// calls on the `Opened` state.
fn poll_lazy_open(
    args: LazyOpenArgs<'_>, cx: &mut Context<'_>, buf: &[u8],
) -> Poll<io::Result<(OpenedStreamParts, usize)>> {
    let LazyOpenArgs {
        port,
        max_frame_size,
        mux,
        pending_data_tx,
        pending_error_tx,
        open_in_progress,
    } = args;
    if open_in_progress.is_none() {
        // If pending senders have been consumed by a previous failed open
        // attempt, the stream is permanently broken: nothing left to
        // register with the workers.
        let (Some(data_tx), Some(error_tx)) = (pending_data_tx.take(), pending_error_tx.take())
        else {
            return Poll::Ready(Err(broken_pipe()));
        };
        let max_payload = (max_frame_size as usize).saturating_sub(8).max(1);
        let n = buf.len().min(max_payload);
        // Clone the first chunk so the future is cancellation-safe: if this
        // poll returns Pending the next poll re-uses the same payload, and
        // if the future is dropped the caller never observes the bytes as
        // committed.
        let first_payload = Bytes::copy_from_slice(&buf[..n]);
        let mux_clone = mux.clone();
        let fut = async move {
            mux_clone
                .realize_portforward_pair(port, first_payload, data_tx, error_tx)
                .await
        };
        *open_in_progress = Some(Box::pin(fut));
    }

    let fut = open_in_progress.as_mut().expect("future just inserted");
    match fut.as_mut().poll(cx) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Ok(parts)) => {
            *open_in_progress = None;
            let max_payload = (max_frame_size as usize).saturating_sub(8).max(1);
            let n = buf.len().min(max_payload);
            Poll::Ready(Ok((parts, n)))
        }
        Poll::Ready(Err(_)) => {
            *open_in_progress = None;
            Poll::Ready(Err(broken_pipe()))
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match &mut this.state {
            StreamState::Unopened {
                data_rx,
                read_buf,
                read_eof,
                ..
            } => poll_read_channel(data_rx, read_buf, read_eof, cx, buf),
            StreamState::Opened {
                data_rx,
                read_buf,
                read_eof,
                ..
            } => poll_read_channel(data_rx, read_buf, read_eof, cx, buf),
            StreamState::Transitioning => unreachable!(),
        }
    }
}

impl AsyncBufRead for Stream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();
        match &mut this.state {
            StreamState::Unopened {
                data_rx,
                read_buf,
                read_eof,
                ..
            } => poll_fill_buf_channel(data_rx, read_buf, read_eof, cx),
            StreamState::Opened {
                data_rx,
                read_buf,
                read_eof,
                ..
            } => poll_fill_buf_channel(data_rx, read_buf, read_eof, cx),
            StreamState::Transitioning => unreachable!(),
        }
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.get_mut();
        match &mut this.state {
            StreamState::Unopened { read_buf, .. } => consume_channel_buf(read_buf, amt),
            StreamState::Opened { read_buf, .. } => consume_channel_buf(read_buf, amt),
            StreamState::Transitioning => unreachable!(),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        // Empty writes are a no-op; never trigger lazy open.
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        // Drive lazy open if needed. We borrow the Unopened state directly,
        // then transition by replacing `state` with the new `Opened` value.
        if matches!(this.state, StreamState::Unopened { .. }) {
            // Extract the fields we need to drive the future without moving
            // the receivers (they stay borrowed by the state).
            let (parts, n_consumed) = match &mut this.state {
                StreamState::Unopened {
                    port,
                    mux,
                    pending_data_tx,
                    pending_error_tx,
                    open_in_progress,
                    max_frame_size,
                    ..
                } => match poll_lazy_open(
                    LazyOpenArgs {
                        port: *port,
                        max_frame_size: *max_frame_size,
                        mux,
                        pending_data_tx,
                        pending_error_tx,
                        open_in_progress,
                    },
                    cx,
                    buf,
                ) {
                    Poll::Ready(Ok(v)) => v,
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => return Poll::Pending,
                },
                _ => unreachable!(),
            };

            // Transition Unopened -> Opened, transferring ownership of the
            // active_pairs slot from the release guard into the StreamGuard.
            let old = std::mem::replace(&mut this.state, StreamState::Transitioning);
            let StreamState::Unopened {
                mux,
                data_rx,
                error_rx,
                max_frame_size,
                read_buf,
                read_eof,
                mut release_guard,
                ..
            } = old
            else {
                unreachable!()
            };
            if let Some(g) = release_guard.as_mut() {
                g.disarm();
            }

            let graceful_shutdown = Arc::new(AtomicBool::new(false));
            let guard = StreamGuard {
                data_id: parts.data_id,
                error_id: parts.error_id,
                mux: mux.clone(),
                ctrl_permit_error: Some(parts.ctrl_permit_error),
                ctrl_permit_data: Some(parts.ctrl_permit_data),
                close_reg_permit_error: Some(parts.close_reg_permit_error),
                close_reg_permit_data: Some(parts.close_reg_permit_data),
                graceful_shutdown: Arc::clone(&graceful_shutdown),
            };
            let write_tx = PollSender::new(mux.cmd_sender());
            this.state = StreamState::Opened {
                data_id: parts.data_id,
                data_rx,
                error_rx,
                mux,
                write_tx,
                send_window: parts.send_window,
                max_frame_size,
                read_buf,
                read_eof,
                graceful_shutdown,
                guard,
            };
            // Drop the disarmed release guard explicitly.
            drop(release_guard);
            return Poll::Ready(Ok(n_consumed));
        }

        match &mut this.state {
            StreamState::Opened {
                data_id,
                write_tx,
                send_window,
                max_frame_size,
                ..
            } => poll_write_via_sender(write_tx, *data_id, send_window, *max_frame_size, cx, buf),
            StreamState::Unopened { .. } => unreachable!("handled above"),
            StreamState::Transitioning => unreachable!(),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match &mut this.state {
            // Unopened: no SPDY stream exists yet. There is nothing on the
            // wire to half-close. Drop will release the local slot when the
            // Stream goes out of scope.
            StreamState::Unopened { .. } => Poll::Ready(Ok(())),
            StreamState::Opened {
                graceful_shutdown,
                mux,
                data_id,
                ..
            } => poll_shutdown_opened(graceful_shutdown, mux, *data_id),
            StreamState::Transitioning => unreachable!(),
        }
    }
}

// ---------------------------------------------------------------------------
// Split halves
// ---------------------------------------------------------------------------

/// State shared between `DataStream` and `ErrorStream` after `split()`.
/// The data half drives lazy open; the error half participates via a single
/// guard reference once the stream is realized.
enum SharedSplitState {
    Unopened(UnopenedShared),
    Opened(OpenedShared),
    /// Used while transferring fields out during the Unopened -> Opened
    /// transition. Should never be observed by user code because the
    /// transition is performed under the parking_lot guard.
    Transitioning,
}

struct UnopenedShared {
    port: u16,
    mux: MuxHandle,
    pending_data_tx: Option<mpsc::Sender<Bytes>>,
    pending_error_tx: Option<mpsc::Sender<Bytes>>,
    open_in_progress: Option<LazyOpenFuture>,
    release_guard: Option<PairReleaseGuard>,
}

struct OpenedShared {
    data_id: u32,
    mux: MuxHandle,
    write_tx: PollSender<MuxCommand>,
    send_window: Arc<SendWindow>,
    graceful_shutdown: Arc<AtomicBool>,
    /// Kept alive for its `Drop` impl, which sends RST_STREAM (or skips on
    /// graceful shutdown) and notifies the workers. Never read directly.
    #[allow(dead_code)]
    guard: StreamGuard,
}

/// Data half of a split SPDY stream: AsyncRead (from pod) + AsyncWrite (to
/// pod). Lazy open fires on the first non-empty write through this half.
pub struct DataStream {
    data_rx: mpsc::Receiver<Bytes>,
    max_frame_size: u32,
    read_buf: Option<Bytes>,
    read_eof: bool,
    shared: Arc<parking_lot::Mutex<SharedSplitState>>,
}

impl Unpin for DataStream {}

impl AsyncRead for DataStream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        poll_read_channel(
            &mut this.data_rx,
            &mut this.read_buf,
            &mut this.read_eof,
            cx,
            buf,
        )
    }
}

impl AsyncBufRead for DataStream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();
        poll_fill_buf_channel(
            &mut this.data_rx,
            &mut this.read_buf,
            &mut this.read_eof,
            cx,
        )
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        consume_channel_buf(&mut self.get_mut().read_buf, amt);
    }
}

impl AsyncWrite for DataStream {
    fn poll_write(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let mut guard = this.shared.lock();
        if let SharedSplitState::Unopened(u) = &mut *guard {
            let res = poll_lazy_open(
                LazyOpenArgs {
                    port: u.port,
                    max_frame_size: this.max_frame_size,
                    mux: &u.mux,
                    pending_data_tx: &mut u.pending_data_tx,
                    pending_error_tx: &mut u.pending_error_tx,
                    open_in_progress: &mut u.open_in_progress,
                },
                cx,
                buf,
            );
            match res {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok((parts, n_consumed))) => {
                    let old = std::mem::replace(&mut *guard, SharedSplitState::Transitioning);
                    let SharedSplitState::Unopened(mut u) = old else {
                        unreachable!()
                    };
                    if let Some(g) = u.release_guard.as_mut() {
                        g.disarm();
                    }
                    let graceful_shutdown = Arc::new(AtomicBool::new(false));
                    let stream_guard = StreamGuard {
                        data_id: parts.data_id,
                        error_id: parts.error_id,
                        mux: u.mux.clone(),
                        ctrl_permit_error: Some(parts.ctrl_permit_error),
                        ctrl_permit_data: Some(parts.ctrl_permit_data),
                        close_reg_permit_error: Some(parts.close_reg_permit_error),
                        close_reg_permit_data: Some(parts.close_reg_permit_data),
                        graceful_shutdown: Arc::clone(&graceful_shutdown),
                    };
                    let write_tx = PollSender::new(u.mux.cmd_sender());
                    *guard = SharedSplitState::Opened(OpenedShared {
                        data_id: parts.data_id,
                        mux: u.mux,
                        write_tx,
                        send_window: parts.send_window,
                        graceful_shutdown,
                        guard: stream_guard,
                    });
                    drop(u.release_guard);
                    return Poll::Ready(Ok(n_consumed));
                }
            }
        }
        match &mut *guard {
            SharedSplitState::Opened(o) => poll_write_via_sender(
                &mut o.write_tx,
                o.data_id,
                &o.send_window,
                this.max_frame_size,
                cx,
                buf,
            ),
            SharedSplitState::Unopened(_) => unreachable!("handled above"),
            SharedSplitState::Transitioning => unreachable!(),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let guard = this.shared.lock();
        match &*guard {
            SharedSplitState::Unopened(_) => Poll::Ready(Ok(())),
            SharedSplitState::Opened(o) => {
                poll_shutdown_opened(&o.graceful_shutdown, &o.mux, o.data_id)
            }
            SharedSplitState::Transitioning => unreachable!(),
        }
    }
}

/// Error half of a split SPDY stream: AsyncRead only (pod error messages).
pub struct ErrorStream {
    error_rx: mpsc::Receiver<Bytes>,
    error_buf: Option<Bytes>,
    error_eof: bool,
    #[allow(dead_code)] // kept alive so the shared open-state and guard outlive both halves
    shared: Arc<parking_lot::Mutex<SharedSplitState>>,
}

impl Unpin for ErrorStream {}

impl AsyncRead for ErrorStream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        poll_read_channel(
            &mut this.error_rx,
            &mut this.error_buf,
            &mut this.error_eof,
            cx,
            buf,
        )
    }
}
