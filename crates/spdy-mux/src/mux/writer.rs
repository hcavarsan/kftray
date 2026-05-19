use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::commands::{
    MuxCommand,
    StreamRegistration,
};
use super::{
    FRAME_WORKERS,
    GOAWAY_STATUS_OK,
    WRITE_BATCH_CAP,
};
use crate::codec::SpdyCodec;
use crate::error::Error;
use crate::transport::WsFrameWriter;

/// Collapse the repeated "timeout + write_binary + warn on error" pattern
/// (Family C) into a single macro invocation.  On success the macro increments
/// `$bytes_counter` by `$len`; on feed-error or timeout it logs and
/// early-returns `(true, $bytes_counter)`.
macro_rules! write_with_timeout {
    ($writer:expr, $payload:expr, $timeout:expr, $bytes_counter:ident += $len:expr, $err_msg:literal, $timeout_msg:literal) => {
        match tokio::time::timeout($timeout, $writer.write_binary(Bytes::from($payload))).await {
            Ok(Ok(())) => {
                $bytes_counter += $len;
            }
            Ok(Err(e)) => {
                tracing::warn!($err_msg, e);
                return (true, $bytes_counter);
            }
            Err(_) => {
                tracing::warn!($timeout_msg, $timeout);
                return (true, $bytes_counter);
            }
        }
    };
}

/// Value-only configuration for the writer task.
#[derive(Clone, Copy)]
pub(super) struct WriterConfig {
    pub initial_window_size: u32,
    pub max_concurrent_streams: u32,
    pub ping_interval: Duration,
    pub write_timeout: Duration,
    pub max_frame_size: u32,
}

/// Owned runtime pieces for the writer task.
pub(super) struct WriterParts<W: WsFrameWriter> {
    pub writer: W,
    pub cmd_rx: mpsc::Receiver<MuxCommand>,
    pub control_rx: mpsc::Receiver<MuxCommand>,
    /// Dedicated channel for WINDOW_UPDATE commands. Drained with highest
    /// priority to prevent flow-control stalls under contention.
    pub window_rx: mpsc::Receiver<MuxCommand>,
    pub close_reg_txs: Arc<[mpsc::Sender<StreamRegistration>; FRAME_WORKERS]>,
    pub cancel: CancellationToken,
}

/// Encode a non-inline `MuxCommand` into a binary payload (SPDY frame bytes)
/// for a WebSocket binary frame.
///
/// `OpenStreamPairAndWrite`, `SendWsPong`, and `CloseStream` are handled
/// inline in `run_writer` because they require multiple writes or async
/// operations that cannot be returned as a single `Bytes`.
pub(super) fn encode_command(cmd: MuxCommand, codec: &SpdyCodec) -> Result<Bytes, Error> {
    match cmd {
        MuxCommand::OpenStreamPairAndWrite { .. } => {
            unreachable!("OpenStreamPairAndWrite must be handled inline in run_writer")
        }
        MuxCommand::SendWsPong { .. } => {
            unreachable!("SendWsPong must be handled inline in run_writer")
        }
        MuxCommand::SendData {
            stream_id,
            payload,
            fin,
        } => {
            let frame_bytes = codec.encode_data(stream_id, &payload, fin);
            Ok(Bytes::from(frame_bytes))
        }
        MuxCommand::SendRawFrame { frame } => Ok(frame),
        MuxCommand::CloseStream { .. } => {
            unreachable!("CloseStream must be handled inline in run_writer")
        }
        MuxCommand::EncodePing { id } => {
            let frame_bytes = codec.encode_ping(id);
            Ok(Bytes::from(frame_bytes))
        }
        MuxCommand::EncodeWindowUpdate { stream_id, delta } => {
            let frame_bytes = codec.encode_window_update(stream_id, delta);
            Ok(Bytes::from(frame_bytes))
        }
        MuxCommand::GoAway {
            last_good_stream_id,
        } => {
            let frame_bytes = codec.encode_goaway(last_good_stream_id, GOAWAY_STATUS_OK);
            tracing::info!(
                last_good_stream_id,
                "SPDY writer: sending GOAWAY (graceful shutdown)"
            );
            Ok(Bytes::from(frame_bytes))
        }
    }
}

/// Encodes and flushes commands to the WebSocket. Owns the WebSocket writer
/// and the SPDY codec (compressor half). Stream IDs are allocated by the
/// caller under the per-handle open sequencer.
///
/// Drains `control_rx` (CloseStream, GoAway, PING, PONG, WINDOW_UPDATE)
/// before `cmd_rx` (data channel) on every iteration via `biased` select.
///
/// Generic over the transport writer via [`WsFrameWriter`] to decouple SPDY
/// framing from the concrete WebSocket implementation.
pub(super) async fn run_writer<W: WsFrameWriter>(parts: WriterParts<W>, config: WriterConfig) {
    let WriterParts {
        mut writer,
        mut cmd_rx,
        mut control_rx,
        mut window_rx,
        close_reg_txs,
        cancel,
    } = parts;

    let mut codec = SpdyCodec::with_max_frame_size(config.max_frame_size);

    // Send initial PING (ID 1) before entering the main loop.
    {
        let ping_frame = codec.encode_ping(1);
        tracing::debug!("SPDY writer: sending initial PING");
        let write_timeout = config.write_timeout;
        match tokio::time::timeout(write_timeout, writer.write_binary(Bytes::from(ping_frame)))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("SPDY writer: initial PING failed: {e}");
                cancel.cancel();
                return;
            }
            Err(_) => {
                tracing::warn!("SPDY writer: initial PING timed out after {write_timeout:?}");
                cancel.cancel();
                return;
            }
        }
        match tokio::time::timeout(write_timeout, writer.flush()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("SPDY writer: initial PING flush failed: {e}");
                cancel.cancel();
                return;
            }
            Err(_) => {
                tracing::warn!("SPDY writer: initial PING flush timed out after {write_timeout:?}");
                cancel.cancel();
                return;
            }
        }
    }

    // Send our SETTINGS frame right after initial PING.
    {
        let settings_entries = vec![
            (7, config.initial_window_size),    // SETTINGS_INITIAL_WINDOW_SIZE
            (4, config.max_concurrent_streams), // SETTINGS_MAX_CONCURRENT_STREAMS
        ];
        let settings_frame = codec.encode_settings(&settings_entries);
        tracing::debug!(
            initial_window_size = config.initial_window_size,
            max_concurrent_streams = config.max_concurrent_streams,
            "SPDY writer: sending our SETTINGS"
        );
        let write_timeout = config.write_timeout;
        match tokio::time::timeout(
            write_timeout,
            writer.write_binary(Bytes::from(settings_frame)),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("SPDY writer: SETTINGS send failed: {e}");
                cancel.cancel();
                return;
            }
            Err(_) => {
                tracing::warn!("SPDY writer: SETTINGS send timed out after {write_timeout:?}");
                cancel.cancel();
                return;
            }
        }
        match tokio::time::timeout(write_timeout, writer.flush()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("SPDY writer: SETTINGS flush failed: {e}");
                cancel.cancel();
                return;
            }
            Err(_) => {
                tracing::warn!("SPDY writer: SETTINGS flush timed out after {write_timeout:?}");
                cancel.cancel();
                return;
            }
        }
    }

    let mut ping_interval = tokio::time::interval(config.ping_interval);
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping_interval.tick().await; // skip immediate first tick

    // Writer stall watchdog: track time of last successful flush. If progress
    // stalls past 2× write_timeout while commands are queued, kill the handle.
    let mut last_writer_progress = tokio::time::Instant::now();
    let mut stall_check = tokio::time::interval(Duration::from_secs(5));
    stall_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    stall_check.tick().await; // skip immediate first tick

    let write_timeout = config.write_timeout;
    let mut ping_id: u32 = 3;
    let mut had_error = false;
    // Close notifications deferred until after flush. Decouples the wire
    // write path from the worker's close_reg_tx drain rate: the batch
    // encodes RST_STREAM frames at full speed, flushes them, THEN notifies
    // the workers about closed streams. This is an OPTIMIZATION path;
    // the correctness path is the pre-reserved permits in StreamGuard::drop.
    let mut pending_closes: Vec<u32> = Vec::new();

    /// Max commands to opportunistically coalesce per flush cycle
    /// (1 from recv + OPPORTUNISTIC_CAP from try_recv). Larger batches
    /// amortize flush/syscall cost but add latency for the last frame
    /// in the batch. At 64 total, worst-case added latency per frame
    /// is ~2ms (64 frames × ~30μs encode each), acceptable for the
    /// reduction in flush overhead under sustained load.
    const OPPORTUNISTIC_CAP: usize = WRITE_BATCH_CAP - 1;

    let mut bytes_since_flush: usize = 0;
    let mut batch_start: Option<tokio::time::Instant> = None;

    loop {
        tokio::select! {
            biased;

            () = cancel.cancelled() => break,

            // HIGHEST PRIORITY: drain WINDOW_UPDATE channel.
            // Flow-control frames must never be delayed. A stalled
            // WINDOW_UPDATE causes the peer to stop sending, deadlocking
            // the entire session.
            win = window_rx.recv() => {
                let Some(cmd) = win else {
                    tracing::debug!("SPDY writer: window channel closed");
                    break;
                };
                let (err, n) = process_writer_command(
                    cmd, &mut writer, &mut codec, &mut pending_closes, write_timeout,
                ).await;
                bytes_since_flush += n;
                if batch_start.is_none() && n > 0 { batch_start = Some(tokio::time::Instant::now()); }
                if err { had_error = true; }
                // Drain all pending WINDOW_UPDATEs -- they're tiny and critical.
                if !had_error {
                    while let Ok(c) = window_rx.try_recv() {
                        let (err, n) = process_writer_command(
                            c, &mut writer, &mut codec, &mut pending_closes, write_timeout,
                        ).await;
                        bytes_since_flush += n;
                        if err {
                            had_error = true;
                            break;
                        }
                    }
                }
            }

            // Priority: drain control channel (one + opportunistic).
            ctrl = control_rx.recv() => {
                let Some(cmd) = ctrl else {
                    tracing::debug!("SPDY writer: control channel closed");
                    break;
                };
                let (err, n) = process_writer_command(
                    cmd, &mut writer, &mut codec, &mut pending_closes, write_timeout,
                ).await;
                bytes_since_flush += n;
                if batch_start.is_none() && n > 0 { batch_start = Some(tokio::time::Instant::now()); }
                if err { had_error = true; }
                // Opportunistically drain a few more control commands.
                if !had_error {
                    for _ in 0..OPPORTUNISTIC_CAP {
                        match control_rx.try_recv() {
                            Ok(c) => {
                                let (err, n) = process_writer_command(
                                    c, &mut writer, &mut codec, &mut pending_closes, write_timeout,
                                ).await;
                                bytes_since_flush += n;
                                if err {
                                    had_error = true;
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
            }

            // Data channel: take one, then opportunistically grab a few more.
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    tracing::debug!("SPDY writer: command channel closed");
                    break;
                };
                // Always drain pending controls first (priority).
                while let Ok(c) = control_rx.try_recv() {
                    let (err, n) = process_writer_command(
                        c, &mut writer, &mut codec, &mut pending_closes, write_timeout,
                    ).await;
                    bytes_since_flush += n;
                    if batch_start.is_none() && n > 0 { batch_start = Some(tokio::time::Instant::now()); }
                    if err {
                        had_error = true;
                        break;
                    }
                }
                if !had_error {
                    let (err, n) = process_writer_command(
                        cmd, &mut writer, &mut codec, &mut pending_closes, write_timeout,
                    ).await;
                    bytes_since_flush += n;
                    if batch_start.is_none() && n > 0 { batch_start = Some(tokio::time::Instant::now()); }
                    if err { had_error = true; }
                }
                // Opportunistically drain a few more data commands.
                if !had_error {
                    for _ in 0..OPPORTUNISTIC_CAP {
                        match cmd_rx.try_recv() {
                            Ok(c) => {
                                let (err, n) = process_writer_command(
                                    c, &mut writer, &mut codec, &mut pending_closes, write_timeout,
                                ).await;
                                bytes_since_flush += n;
                                if err {
                                    had_error = true;
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
            }

            _ = ping_interval.tick() => {
                let ping = codec.encode_ping(ping_id);
                ping_id = ping_id.wrapping_add(2);
                match tokio::time::timeout(write_timeout, writer.write_binary(Bytes::from(ping))).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        tracing::warn!("SPDY writer: keepalive PING feed failed: {e}");
                        had_error = true;
                    }
                    Err(_) => {
                        tracing::warn!("SPDY writer: keepalive PING timed out after {write_timeout:?}");
                        had_error = true;
                    }
                }
            }

            // Writer stall watchdog.
            _ = stall_check.tick() => {
                let stall_threshold = write_timeout.saturating_mul(2);
                let queued = control_rx.len() + cmd_rx.len();
                if last_writer_progress.elapsed() > stall_threshold && queued > 0 {
                    tracing::warn!(
                        stalled_for = ?last_writer_progress.elapsed(),
                        queued_commands = queued,
                        "SPDY writer: stall watchdog triggered, killing handle"
                    );
                    cancel.cancel();
                    break;
                }
                // Stall check doesn't write anything, skip flush.
                continue;
            }
        }

        if had_error {
            break;
        }

        // End-of-batch flush. Each select branch processes up to
        // WRITE_BATCH_CAP commands (1 from recv + OPPORTUNISTIC_CAP from
        // try_recv) before reaching here, coalescing up to 64 frames per
        // flush.
        if bytes_since_flush > 0 {
            match tokio::time::timeout(write_timeout, writer.flush()).await {
                Ok(Ok(())) => {
                    last_writer_progress = tokio::time::Instant::now();
                }
                Ok(Err(e)) => {
                    tracing::warn!("SPDY writer: flush error: {e}");
                    break;
                }
                Err(_) => {
                    tracing::warn!(
                        "SPDY writer: flush timed out after {write_timeout:?}, peer stalled"
                    );
                    break;
                }
            }
            bytes_since_flush = 0;
            batch_start = None;
        }

        // Drain deferred close notifications to the workers via close_reg
        // channels. This is an OPTIMIZATION; the correctness path is the
        // pre-reserved permits in StreamGuard::drop.
        //
        // CRITICAL: use `try_send`, not `send().await`. If a worker's
        // close_reg_rx is momentarily full, blocking the writer here would
        // freeze the entire data plane. On `Full`, requeue so the next
        // cycle tries again. On `Closed`, the worker is gone.
        let mut requeued: Vec<u32> = Vec::new();
        for stream_id in std::mem::take(&mut pending_closes) {
            match close_reg_txs[(stream_id % FRAME_WORKERS as u32) as usize]
                .try_send(StreamRegistration::Close { stream_id })
            {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    requeued.push(stream_id);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    // Worker gone, session is dying.
                    had_error = true;
                    break;
                }
            }
        }
        pending_closes.extend(requeued);
        if had_error {
            break;
        }
    }

    cancel.cancel();
    let _ = writer.close().await;
}

/// Process a single writer command: encode and write to the WebSocket.
/// Returns `(had_error, bytes_written)`. `had_error` is true if the writer
/// should break.
pub(super) async fn process_writer_command<W: WsFrameWriter>(
    cmd: MuxCommand, writer: &mut W, codec: &mut SpdyCodec, pending_closes: &mut Vec<u32>,
    write_timeout: Duration,
) -> (bool, usize) {
    let mut bytes_written: usize = 0;

    // OpenStreamPairAndWrite: encode both SYN_STREAM frames, the empty
    // DATA+FIN that half-closes the error stream, and the first DATA
    // frame on the data stream inline. All four frames are emitted as one
    // atomic batch so the wire sees:
    //
    //   SYN_STREAM(error_id, fin=false)
    //   SYN_STREAM(data_id,  fin=false)
    //   DATA(error_id, empty, fin=true)
    //   DATA(data_id,  first_payload, fin=false)
    //
    // in monotonic ID order. Header content is whatever the caller built;
    // the codec just encodes the (key, value) list with the standard
    // SPDY/3.1 zlib dictionary.
    //
    // The "error stream half-closed at open" pattern (empty DATA+FIN
    // right after the SYN_STREAMs) is a common SPDY/3.1 idiom for
    // "I will never write on this stream, but I want to read from it".
    // Setting fin=true on the SYN_STREAM itself is allowed by the spec
    // but some peers (notably Kubernetes kubelet) reject it; this
    // implementation always uses the explicit DATA+FIN form so it works
    // against the widest set of peers.
    if let MuxCommand::OpenStreamPairAndWrite {
        error_id,
        data_id,
        error_headers,
        data_headers,
        first_payload,
    } = cmd
    {
        // SYN_STREAM(error, fin=false). The half-close arrives as an
        // explicit empty DATA+FIN below.
        match codec.encode_syn_stream(error_id, &error_headers, false) {
            Ok(frame_bytes) => {
                let len = frame_bytes.len();
                tracing::debug!(
                    stream_id = error_id,
                    fin = false,
                    len,
                    "SPDY writer: sending SYN_STREAM (error)"
                );
                write_with_timeout!(
                    writer,
                    frame_bytes,
                    write_timeout,
                    bytes_written += len,
                    "SPDY writer: SYN_STREAM (error) feed error: {}",
                    "SPDY writer: SYN_STREAM (error) timed out after {:?}"
                );
            }
            Err(e) => {
                tracing::warn!("SPDY writer: SYN_STREAM (error) encode error: {e}");
                return (true, bytes_written);
            }
        }
        match codec.encode_syn_stream(data_id, &data_headers, false) {
            Ok(frame_bytes) => {
                let len = frame_bytes.len();
                tracing::debug!(
                    stream_id = data_id,
                    fin = false,
                    len,
                    "SPDY writer: sending SYN_STREAM (data)"
                );
                write_with_timeout!(
                    writer,
                    frame_bytes,
                    write_timeout,
                    bytes_written += len,
                    "SPDY writer: SYN_STREAM (data) feed error: {}",
                    "SPDY writer: SYN_STREAM (data) timed out after {:?}"
                );
            }
            Err(e) => {
                tracing::warn!("SPDY writer: SYN_STREAM (data) encode error: {e}");
                return (true, bytes_written);
            }
        }
        // DATA(error_id, empty, fin=true). Tells the peer we will never
        // write on the error stream, while leaving the read direction open
        // so the peer can send us error messages.
        {
            let frame_bytes = codec.encode_data(error_id, &[], true);
            let len = frame_bytes.len();
            tracing::debug!(
                stream_id = error_id,
                fin = true,
                len,
                "SPDY writer: half-closing error stream with empty DATA+FIN"
            );
            write_with_timeout!(
                writer,
                frame_bytes,
                write_timeout,
                bytes_written += len,
                "SPDY writer: error-stream DATA+FIN feed error: {}",
                "SPDY writer: error-stream DATA+FIN timed out after {:?}"
            );
        }
        // Emit the first DATA frame on the data stream. The send window
        // was debited at the lazy-open call site (see `Stream::poll_write`
        // unopened branch in `stream.rs`); we encode the frame as-is.
        if !first_payload.is_empty() {
            let frame_bytes = codec.encode_data(data_id, &first_payload, false);
            let len = frame_bytes.len();
            tracing::debug!(
                stream_id = data_id,
                fin = false,
                len,
                "SPDY writer: sending first DATA after lazy open"
            );
            write_with_timeout!(
                writer,
                frame_bytes,
                write_timeout,
                bytes_written += len,
                "SPDY writer: lazy-open first DATA feed error: {}",
                "SPDY writer: lazy-open first DATA timed out after {:?}"
            );
        }
        return (false, bytes_written);
    }

    // SendWsPong requires write_pong: handle inline.
    if let MuxCommand::SendWsPong { payload } = cmd {
        match tokio::time::timeout(write_timeout, writer.write_pong(payload)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("SPDY writer: pong feed error: {e}");
                return (true, 0);
            }
            Err(_) => {
                tracing::warn!("SPDY writer: pong timed out after {write_timeout:?}");
                return (true, 0);
            }
        }
        return (false, 0);
    }

    // CloseStream: encode RST_STREAM now, defer worker notification
    // until after the flush.
    if let MuxCommand::CloseStream { stream_id, status } = cmd {
        let rst_bytes = codec.encode_rst_stream(stream_id, status);
        let len = rst_bytes.len();
        write_with_timeout!(
            writer,
            rst_bytes,
            write_timeout,
            bytes_written += len,
            "SPDY writer: RST_STREAM feed error: {}",
            "SPDY writer: RST_STREAM timed out after {:?}"
        );
        pending_closes.push(stream_id);
        return (false, bytes_written);
    }

    match encode_command(cmd, codec) {
        Ok(payload) => {
            let len = payload.len();
            write_with_timeout!(
                writer,
                payload,
                write_timeout,
                bytes_written += len,
                "SPDY writer: feed error: {}",
                "SPDY writer: write_binary timed out after {:?}"
            );
            (false, bytes_written)
        }
        Err(e) => {
            tracing::warn!("SPDY writer: encode error: {e}");
            (true, 0)
        }
    }
}
