use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    AtomicU32,
    AtomicUsize,
    Ordering,
};

use bytes::Bytes;
use tokio::sync::{
    Mutex as TokioMutex,
    mpsc,
    oneshot,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::commands::{
    MuxCommand,
    OpenState,
    StreamRegistration,
};
use super::config::MuxConfig;
use super::reader::{
    ReaderChannels,
    ReaderConfig,
    ReaderParts,
    ReaderSharedState,
};
use super::supervisor::supervise;
use super::window::SendWindow;
use super::worker::run_frame_worker;
use super::writer::{
    WriterConfig,
    WriterParts,
    run_writer,
};
use super::{
    FRAME_WORKERS,
    MAX_STREAM_ID,
};
use crate::codec::Frame;
use crate::error::Error;
use crate::stream::{
    OpenedStreamParts,
    Stream,
};
use crate::transport::{
    WsFrameReader,
    WsFrameWriter,
};

/// Cheaply cloneable handle to the mux background tasks.
///
/// # Transport break contract
///
/// When the WebSocket closes or errors, all streams get `BrokenPipe`. No
/// transparent reconnection happens at this layer; the Forwarder above
/// opens a new session.
#[derive(Clone)]
pub(crate) struct MuxHandle {
    cmd_tx: mpsc::Sender<MuxCommand>,
    /// Priority control channel for the writer. CloseStream, GoAway,
    /// PING, PONG, and WINDOW_UPDATE go here. The writer drains this
    /// channel BEFORE the data cmd_tx on every iteration.
    control_tx: mpsc::Sender<MuxCommand>,
    /// Per-worker open-registration channels. Caller selects the correct
    /// worker by `stream_id % FRAME_WORKERS`. Each worker owns its
    /// shard of streams and pending_replies (no shared maps).
    reg_txs: Arc<[mpsc::Sender<StreamRegistration>; FRAME_WORKERS]>,
    /// Per-worker close-registration channels. Separate from open
    /// registrations so teardown is never blocked by a burst of opens.
    close_reg_txs: Arc<[mpsc::Sender<StreamRegistration>; FRAME_WORKERS]>,
    active_pairs: Arc<AtomicUsize>,
    /// Per-handle open sequencer. Serializes stream-ID allocation +
    /// worker registration + SYN_STREAM enqueue to guarantee that
    /// SYN_STREAM frames appear on the wire in monotonically-increasing
    /// stream-ID order, as required by SPDY/3.1.
    ///
    /// The critical section is bounded by 3 local mpsc sends
    /// (2× reg_tx + 1× cmd_tx). All channels are bounded with sufficient
    /// capacity to make blocking under healthy operation rare. When the
    /// worker or writer is dying, this mutex is released as the sends
    /// fail with channel-closed errors.
    open_seq: Arc<TokioMutex<OpenState>>,
    /// Peer's initial window size. Updated by SETTINGS frames.
    peer_initial_window: Arc<AtomicU32>,
    /// Peer's MAX_CONCURRENT_STREAMS from SETTINGS. 0 = unlimited.
    peer_max_concurrent: Arc<AtomicU32>,
    /// Our configured max_concurrent_streams (protocol hard cap).
    local_max_concurrent: u32,
    /// Our configured operating_max_streams (scheduling cap).
    local_operating_max: u32,
    /// Whether GOAWAY was received. No new streams accepted after this.
    goaway_received: Arc<AtomicBool>,
    /// Per-stream data channel buffer size.
    stream_data_buffer: usize,
    /// Per-stream error channel buffer size.
    stream_error_buffer: usize,
    /// Max frame size for outgoing DATA (from config, may be updated by peer
    /// SETTINGS).
    max_frame_size: Arc<AtomicU32>,
    closed: CancellationToken,
}

impl MuxHandle {
    /// Select the open-registration channel for a given stream ID.
    fn reg_tx_for(&self, stream_id: u32) -> &mpsc::Sender<StreamRegistration> {
        &self.reg_txs[(stream_id % FRAME_WORKERS as u32) as usize]
    }

    /// Select the close-registration channel for a given stream ID.
    fn close_reg_tx_for(&self, stream_id: u32) -> &mpsc::Sender<StreamRegistration> {
        &self.close_reg_txs[(stream_id % FRAME_WORKERS as u32) as usize]
    }

    /// Start the mux background tasks and verify the connection with an
    /// initial PING roundtrip.
    ///
    /// Generic over the WebSocket transport. The caller provides pre-split
    /// writer and reader halves wrapped in the [`WsFrameWriter`] /
    /// [`WsFrameReader`] trait adapters.
    pub(crate) async fn spawn<W, R>(
        ws_write: W, ws_read: R, cancel: CancellationToken, config: MuxConfig,
    ) -> Result<Self, Error>
    where
        W: WsFrameWriter + 'static,
        R: WsFrameReader + 'static,
    {
        let (cmd_tx, cmd_rx) = mpsc::channel(config.cmd_buffer_size);
        let (control_tx, control_rx) = mpsc::channel(config.control_buffer_size);
        let (window_tx, window_rx) = mpsc::channel(config.window_buffer_size);
        let active_pairs = Arc::new(AtomicUsize::new(0));
        let peer_initial_window = Arc::new(AtomicU32::new(config.initial_window_size));
        let peer_max_concurrent = Arc::new(AtomicU32::new(0)); // 0 = unlimited
        let goaway_received = Arc::new(AtomicBool::new(false));
        // Session send window is kept for the reader (it replenishes when the
        // peer sends session-level WINDOW_UPDATE with stream_id=0).
        // poll_write_via_sender does NOT enforce it for outbound writes: the
        // kubelet apiserver peer never sends session-level WINDOW_UPDATE, so
        // enforcing it would deadlock once the initial window drains.
        // Per-stream windows still provide backpressure.
        let session_send_window = Arc::new(SendWindow::new(config.initial_window_size));
        let max_frame_size = Arc::new(AtomicU32::new(config.max_frame_size));
        let closed = cancel.clone();

        let mut reg_txs_vec: Vec<mpsc::Sender<StreamRegistration>> =
            Vec::with_capacity(FRAME_WORKERS);
        let mut reg_rxs: Vec<mpsc::Receiver<StreamRegistration>> =
            Vec::with_capacity(FRAME_WORKERS);
        let mut close_reg_txs_vec: Vec<mpsc::Sender<StreamRegistration>> =
            Vec::with_capacity(FRAME_WORKERS);
        let mut close_reg_rxs: Vec<mpsc::Receiver<StreamRegistration>> =
            Vec::with_capacity(FRAME_WORKERS);
        let mut frame_txs_vec: Vec<mpsc::Sender<Frame>> = Vec::with_capacity(FRAME_WORKERS);
        let mut frame_rxs: Vec<mpsc::Receiver<Frame>> = Vec::with_capacity(FRAME_WORKERS);
        for _ in 0..FRAME_WORKERS {
            let (rtx, rrx) = mpsc::channel(config.reg_buffer_size);
            reg_txs_vec.push(rtx);
            reg_rxs.push(rrx);
            let (crtx, crrx) = mpsc::channel(config.close_reg_buffer_size);
            close_reg_txs_vec.push(crtx);
            close_reg_rxs.push(crrx);
            let (ftx, frx) = mpsc::channel(config.worker_queue_size);
            frame_txs_vec.push(ftx);
            frame_rxs.push(frx);
        }
        let reg_txs: Arc<[mpsc::Sender<StreamRegistration>; FRAME_WORKERS]> =
            Arc::new(reg_txs_vec.try_into().expect("FRAME_WORKERS channels"));
        let close_reg_txs: Arc<[mpsc::Sender<StreamRegistration>; FRAME_WORKERS]> = Arc::new(
            close_reg_txs_vec
                .try_into()
                .expect("FRAME_WORKERS channels"),
        );
        let frame_txs: [mpsc::Sender<Frame>; FRAME_WORKERS] =
            frame_txs_vec.try_into().expect("FRAME_WORKERS channels");

        let handle = Self {
            cmd_tx,
            control_tx,
            reg_txs: Arc::clone(&reg_txs),
            close_reg_txs: Arc::clone(&close_reg_txs),
            active_pairs: Arc::clone(&active_pairs),
            open_seq: Arc::new(TokioMutex::new(OpenState { next_stream_id: 1 })),
            peer_initial_window: Arc::clone(&peer_initial_window),
            peer_max_concurrent: Arc::clone(&peer_max_concurrent),
            local_max_concurrent: config.max_concurrent_streams,
            local_operating_max: config.operating_max_streams,
            goaway_received: Arc::clone(&goaway_received),
            stream_data_buffer: config.stream_data_buffer,
            stream_error_buffer: config.stream_error_buffer,
            max_frame_size: Arc::clone(&max_frame_size),
            closed: closed.clone(),
        };

        let (ping_tx, ping_rx) = oneshot::channel();

        let writer_config = WriterConfig {
            initial_window_size: config.initial_window_size,
            max_concurrent_streams: config.max_concurrent_streams,
            ping_interval: config.ping_interval,
            write_timeout: config.write_timeout,
            max_frame_size: config.max_frame_size,
        };
        let writer_parts = WriterParts {
            writer: ws_write,
            cmd_rx,
            control_rx,
            window_rx,
            close_reg_txs: Arc::clone(&close_reg_txs),
            cancel: closed.clone(),
        };
        let writer_handle: JoinHandle<&'static str> = tokio::spawn(async move {
            run_writer(writer_parts, writer_config).await;
            "writer"
        });

        let mut worker_handles: Vec<JoinHandle<&'static str>> = Vec::with_capacity(FRAME_WORKERS);
        let mut reg_rxs_iter = reg_rxs.into_iter();
        let mut close_reg_rxs_iter = close_reg_rxs.into_iter();
        let mut frame_rxs_iter = frame_rxs.into_iter();
        for worker_id in 0..FRAME_WORKERS {
            let w_control_tx = handle.control_tx.clone();
            let w_window_tx = window_tx.clone();
            let w_cancel = closed.clone();
            let w_initial_window = config.initial_window_size;
            let w_reg_rx = reg_rxs_iter.next().expect("reg_rx per worker");
            let w_close_reg_rx = close_reg_rxs_iter.next().expect("close_reg_rx per worker");
            let w_frame_rx = frame_rxs_iter.next().expect("frame_rx per worker");
            let wh: JoinHandle<&'static str> = tokio::spawn(async move {
                run_frame_worker(
                    worker_id,
                    w_frame_rx,
                    w_reg_rx,
                    w_close_reg_rx,
                    w_control_tx,
                    w_window_tx,
                    w_cancel,
                    w_initial_window,
                )
                .await;
                "worker"
            });
            worker_handles.push(wh);
        }

        let reader_config = ReaderConfig {
            initial_window_size: config.initial_window_size,
            idle_timeout: config.idle_timeout,
            ping_timeout: config.ping_timeout,
            configured_max_frame_size: config.max_frame_size,
        };
        let reader_channels = ReaderChannels {
            control_tx: handle.control_tx.clone(),
            window_tx,
            frame_txs,
            reg_txs: Arc::clone(&reg_txs),
            close_reg_txs: Arc::clone(&close_reg_txs),
        };
        let reader_shared = ReaderSharedState {
            cancel: closed.clone(),
            peer_initial_window,
            peer_max_concurrent,
            goaway_received,
            session_send_window: Arc::clone(&session_send_window),
            max_frame_size,
        };
        let reader_parts = ReaderParts {
            ws_read,
            channels: reader_channels,
            shared: reader_shared,
            ping_ready: Some(ping_tx),
        };
        let reader_handle: JoinHandle<&'static str> = tokio::spawn(async move {
            super::reader::run_reader(reader_parts, reader_config).await;
            "reader"
        });

        let s_cancel = closed.clone();
        tokio::spawn(async move {
            supervise(writer_handle, reader_handle, worker_handles, s_cancel).await;
        });

        // Wait for initial PING roundtrip.
        match tokio::time::timeout(config.ping_timeout, ping_rx).await {
            Ok(Ok(Ok(()))) => {
                tracing::debug!("SPDY mux: initial PING succeeded, connection ready");
                Ok(handle)
            }
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => Err(Error::MuxClosed),
            Err(_) => Err(Error::SynReplyTimeout(0)),
        }
    }

    /// Open a port-forward stream pair lazily.
    ///
    /// Reserve a paired stream (error + data) and return a lazy `Stream`
    /// handle. Caller-supplied headers go on the wire when the consumer
    /// actually writes its first byte.
    ///
    /// # Lazy open contract
    ///
    /// This call reserves a slot against `active_pairs` and pre-creates
    /// the per-stream `data_rx` / `error_rx` channels, but it does **not**:
    ///
    /// - allocate SPDY stream IDs
    /// - register the streams with frame workers
    /// - send any `SYN_STREAM` frame to the wire
    ///
    /// All of that happens later, on the first non-empty `poll_write` of
    /// the returned `Stream`, via [`MuxHandle::realize_stream_pair`].
    ///
    /// # Why lazy
    ///
    /// Some SPDY/3.1 peers create an upstream connection the moment they
    /// see a `SYN_STREAM` (Kubernetes kubelet is the motivating example:
    /// it dials the target pod TCP port immediately). Fast-closing
    /// servers then close that idle connection within milliseconds, so
    /// any pre-opened spare stream is dead before the consumer can use
    /// it. Lazy open emits `SYN_STREAM` and the first `DATA` atomically,
    /// at the exact moment the consumer has something to send.
    ///
    /// # Headers
    ///
    /// The `error_headers` and `data_headers` lists are passed to the
    /// codec verbatim and become the SYN_STREAM header block for the
    /// respective stream. The multiplexer does not interpret keys or
    /// values.
    ///
    /// # Backpressure
    ///
    /// Checks `active_pairs` against `operating_max_streams` first
    /// (scheduling cap), then against `max_concurrent_streams` (protocol
    /// hard cap). If at cap, returns `Error::CapacityExhausted` immediately.
    /// Failures during the later realization step surface as I/O errors
    /// on `poll_write`.
    pub(crate) async fn open_stream_pair(
        &self, error_headers: Vec<(String, String)>, data_headers: Vec<(String, String)>,
    ) -> Result<Stream, Error> {
        if self.closed.is_cancelled() {
            return Err(Error::MuxClosed);
        }
        if self.goaway_received.load(Ordering::Acquire) {
            return Err(Error::MuxClosed);
        }

        // Reserve capacity: check operating cap first, then hard cap.
        // Atomic CAS handles concurrent reservations without a mutex.
        let prev = self.active_pairs.fetch_add(1, Ordering::AcqRel);
        let operating_limit = self.operating_limit();
        if (prev as u32) >= operating_limit {
            self.active_pairs.fetch_sub(1, Ordering::AcqRel);
            return Err(Error::CapacityExhausted {
                in_use: prev,
                limit: operating_limit,
            });
        }
        let hard_limit = self.hard_limit();
        if (prev as u32) >= hard_limit {
            self.active_pairs.fetch_sub(1, Ordering::AcqRel);
            return Err(Error::CapacityExhausted {
                in_use: prev,
                limit: hard_limit,
            });
        }

        // Pre-create the per-stream channels. The senders are stashed in
        // the Stream and handed to the workers at realize time so any
        // pre-arrival DATA / FIN / RST frames addressed to this stream
        // (after `realize_portforward_pair` registers it) reach the
        // user-facing receivers.
        let (error_tx, error_rx) = mpsc::channel(self.stream_error_buffer);
        let (data_tx, data_rx) = mpsc::channel(self.stream_data_buffer);

        let max_frame = self.max_frame_size.load(Ordering::Acquire);

        Ok(Stream::new_unopened(crate::stream::UnopenedStreamParts {
            error_headers,
            data_headers,
            mux: self.clone(),
            data_rx,
            error_rx,
            pending_data_tx: data_tx,
            pending_error_tx: error_tx,
            max_frame_size: max_frame,
        }))
    }

    /// Realize a lazily-opened stream pair on the wire.
    ///
    /// Called from `Stream::poll_write` on the first non-empty write.
    /// Allocates stream IDs, registers both streams with their workers,
    /// reserves drop permits, and enqueues `OpenStreamPairAndWrite` which
    /// emits `SYN_STREAM(error)`, `SYN_STREAM(data)`, and the first
    /// `DATA(data, first_payload)` atomically in monotonic ID order.
    ///
    /// # Ordering guarantee
    ///
    /// Allocation, worker registration, drop-permit reservation, and writer
    /// enqueue are all serialized under the per-handle `open_seq` mutex.
    /// Because the writer command itself contains both SYNs plus the first
    /// payload, the wire never sees frames for a stream whose `SYN_STREAM`
    /// has not yet been emitted, and IDs are strictly monotonically
    /// increasing on the wire.
    ///
    /// # Backpressure
    ///
    /// All channel sends use `try_send` / `try_reserve_owned` so we never
    /// `.await` while holding `open_seq`. Backpressure surfaces as
    /// `CapacityExhausted` / `MuxClosed` and propagates to the caller's
    /// `poll_write` as `BrokenPipe`.
    pub(crate) async fn realize_stream_pair(
        &self, error_headers: Vec<(String, String)>, data_headers: Vec<(String, String)>,
        first_payload: Bytes, pending_data_tx: mpsc::Sender<Bytes>,
        pending_error_tx: mpsc::Sender<Bytes>,
    ) -> Result<OpenedStreamParts, Error> {
        if self.closed.is_cancelled() {
            return Err(Error::MuxClosed);
        }
        if self.goaway_received.load(Ordering::Acquire) {
            return Err(Error::MuxClosed);
        }

        // Acquire sequencer for the duration of:
        //   - stream ID allocation
        //   - worker registration (2× reg_tx.try_send)
        //   - writer enqueue (cmd_tx.try_send(OpenPortForwardAndWrite))
        //   - permit reservation (4× try_reserve_owned)
        let mut seq = self.open_seq.lock().await;

        // Re-check after acquiring (peer could have sent GOAWAY meanwhile).
        if self.closed.is_cancelled() {
            return Err(Error::MuxClosed);
        }
        if self.goaway_received.load(Ordering::Acquire) {
            return Err(Error::MuxClosed);
        }

        // Allocate IDs. SPDY/3.1 requires client streams to use odd IDs;
        // the sequencer burns two per allocation, so a pair advances by 4.
        let error_id = seq.next_stream_id;
        let data_id = seq.next_stream_id.wrapping_add(2);
        // Detect ID space exhaustion BEFORE advancing.
        if data_id > MAX_STREAM_ID || error_id > MAX_STREAM_ID {
            // Send GOAWAY through control channel (fire-and-forget).
            let _ = self.control_tx.try_send(MuxCommand::GoAway {
                last_good_stream_id: error_id.wrapping_sub(2),
            });
            return Err(Error::MuxClosed);
        }
        seq.next_stream_id = data_id.wrapping_add(2);

        let error_send_window = Arc::new(SendWindow::new(
            self.peer_initial_window.load(Ordering::Acquire),
        ));
        let (error_reply_tx, _error_reply_rx) = oneshot::channel();
        let data_send_window = Arc::new(SendWindow::new(
            self.peer_initial_window.load(Ordering::Acquire),
        ));
        let (data_reply_tx, _data_reply_rx) = oneshot::channel();

        // All sends below use `try_send`, not `send().await`, while
        // holding `open_seq`. See the rationale in the original
        // `open_stream_pair` body: awaiting a bounded-channel send
        // under the mutex is a deadlock vector.

        // Register error stream with its partition's worker.
        match self
            .reg_tx_for(error_id)
            .try_send(StreamRegistration::Open {
                stream_id: error_id,
                data_tx: pending_error_tx,
                reply_tx: error_reply_tx,
                send_window: Arc::clone(&error_send_window),
            }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(Error::CapacityExhausted {
                    in_use: self.active_pairs.load(Ordering::Relaxed),
                    limit: self.operating_limit(),
                });
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(Error::MuxClosed);
            }
        }

        // Register data stream with its (possibly different) worker.
        match self.reg_tx_for(data_id).try_send(StreamRegistration::Open {
            stream_id: data_id,
            data_tx: pending_data_tx,
            reply_tx: data_reply_tx,
            send_window: Arc::clone(&data_send_window),
        }) {
            Ok(()) => {}
            Err(e) => {
                // error_id is already registered; best-effort cleanup.
                let _ = self
                    .close_reg_tx_for(error_id)
                    .try_send(StreamRegistration::Close {
                        stream_id: error_id,
                    });
                return Err(match e {
                    mpsc::error::TrySendError::Full(_) => Error::CapacityExhausted {
                        in_use: self.active_pairs.load(Ordering::Relaxed),
                        limit: self.operating_limit(),
                    },
                    mpsc::error::TrySendError::Closed(_) => Error::MuxClosed,
                });
            }
        }

        // Eagerly debit the data send window for first_payload. The wire
        // command we're about to enqueue will emit the payload as part of
        // the atomic open+write batch, so flow-control accounting must
        // happen here, not in the writer.
        if !first_payload.is_empty() && !data_send_window.consume(first_payload.len()) {
            // Should never happen on a freshly-created window with the
            // peer's initial window size, but the API allows poisoning.
            let _ = self
                .close_reg_tx_for(error_id)
                .try_send(StreamRegistration::Close {
                    stream_id: error_id,
                });
            let _ = self
                .close_reg_tx_for(data_id)
                .try_send(StreamRegistration::Close { stream_id: data_id });
            return Err(Error::MuxClosed);
        }

        // Enqueue the atomic open+first-write command.
        match self.cmd_tx.try_send(MuxCommand::OpenStreamPairAndWrite {
            error_id,
            data_id,
            error_headers,
            data_headers,
            first_payload,
        }) {
            Ok(()) => {}
            Err(e) => {
                // Best-effort cleanup: tell workers to forget both streams.
                let _ = self
                    .close_reg_tx_for(error_id)
                    .try_send(StreamRegistration::Close {
                        stream_id: error_id,
                    });
                let _ = self
                    .close_reg_tx_for(data_id)
                    .try_send(StreamRegistration::Close { stream_id: data_id });
                return Err(match e {
                    mpsc::error::TrySendError::Full(_) => Error::CapacityExhausted {
                        in_use: self.active_pairs.load(Ordering::Relaxed),
                        limit: self.operating_limit(),
                    },
                    mpsc::error::TrySendError::Closed(_) => Error::MuxClosed,
                });
            }
        }

        // Pre-reserve 2 control-channel permits and 2 close-reg permits so
        // `StreamGuard::drop` can deliver `CloseStream` and worker-close
        // notifications synchronously via `OwnedPermit::send`. On failure
        // we must best-effort unregister both streams from their workers,
        // otherwise the workers hold a registration forever (the open
        // command has already been queued to the writer). The Stream
        // remains in `Unopened` state on the caller side and its
        // `release_guard` decrements `active_pairs` on drop.
        let cleanup_registration = |this: &Self| {
            let _ = this
                .close_reg_tx_for(error_id)
                .try_send(StreamRegistration::Close {
                    stream_id: error_id,
                });
            let _ = this
                .close_reg_tx_for(data_id)
                .try_send(StreamRegistration::Close { stream_id: data_id });
        };
        let Ok(ctrl_permit_error) = self.control_tx.clone().try_reserve_owned() else {
            cleanup_registration(self);
            return Err(Error::MuxClosed);
        };
        let Ok(ctrl_permit_data) = self.control_tx.clone().try_reserve_owned() else {
            cleanup_registration(self);
            return Err(Error::MuxClosed);
        };
        let Ok(close_reg_permit_error) =
            self.close_reg_tx_for(error_id).clone().try_reserve_owned()
        else {
            cleanup_registration(self);
            return Err(Error::MuxClosed);
        };
        let Ok(close_reg_permit_data) = self.close_reg_tx_for(data_id).clone().try_reserve_owned()
        else {
            cleanup_registration(self);
            return Err(Error::MuxClosed);
        };

        drop(seq); // release sequencer

        Ok(OpenedStreamParts {
            data_id,
            error_id,
            send_window: data_send_window,
            ctrl_permit_error,
            ctrl_permit_data,
            close_reg_permit_error,
            close_reg_permit_data,
        })
    }

    /// Operating concurrent stream pair limit: min(local_operating, peer).
    /// Used by the open path for scheduling backpressure.
    fn operating_limit(&self) -> u32 {
        let peer_limit = self.peer_max_concurrent.load(Ordering::Acquire);
        if peer_limit > 0 {
            self.local_operating_max.min(peer_limit)
        } else {
            self.local_operating_max
        }
    }

    /// Hard concurrent stream pair limit: min(local_max, peer).
    /// Used for SETTINGS-bound comparisons and protocol violation checks.
    fn hard_limit(&self) -> u32 {
        let peer_limit = self.peer_max_concurrent.load(Ordering::Acquire);
        if peer_limit > 0 {
            self.local_max_concurrent.min(peer_limit)
        } else {
            self.local_max_concurrent
        }
    }

    /// Remaining operating capacity (pairs that can still be opened).
    pub(crate) fn operating_capacity(&self) -> usize {
        let limit = self.operating_limit() as usize;
        let active = self.active_pairs.load(Ordering::Relaxed);
        limit.saturating_sub(active)
    }

    pub(crate) fn send_data_nonblocking(
        &self, stream_id: u32, payload: Bytes, fin: bool,
    ) -> Result<(), Error> {
        self.cmd_tx
            .try_send(MuxCommand::SendData {
                stream_id,
                payload,
                fin,
            })
            .map_err(|_| Error::MuxClosed)
    }

    pub(crate) fn release_pair(&self) {
        self.active_pairs.fetch_sub(1, Ordering::Relaxed);
    }

    pub(crate) fn active_pairs(&self) -> usize {
        self.active_pairs.load(Ordering::Relaxed)
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.is_cancelled()
    }

    pub(crate) fn cmd_sender(&self) -> mpsc::Sender<MuxCommand> {
        self.cmd_tx.clone()
    }

    pub(crate) fn max_concurrent(&self) -> u32 {
        self.hard_limit()
    }
}
