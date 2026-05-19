use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    AtomicU32,
    Ordering,
};
use std::time::Duration;

use bytes::BytesMut;
use tokio::sync::{
    mpsc,
    oneshot,
};
use tokio_util::sync::CancellationToken;

use super::commands::{
    MuxCommand,
    StreamRegistration,
};
use super::window::SendWindow;
use super::{
    FRAME_WORKERS,
    RST_STATUS_FLOW_CONTROL,
    RST_STATUS_REFUSED_STREAM,
};
use crate::codec::{
    Frame,
    SpdyCodec,
};
use crate::error::Error;
use crate::transport::{
    WsFrameReader,
    WsMessage,
};

/// Value-only configuration for the reader task.
#[derive(Clone, Copy)]
pub(super) struct ReaderConfig {
    pub initial_window_size: u32,
    pub idle_timeout: Duration,
    pub ping_timeout: Duration,
    pub configured_max_frame_size: u32,
}

/// Clone-able shared channel handles for the reader.
#[derive(Clone)]
pub(super) struct ReaderChannels {
    pub control_tx: mpsc::Sender<MuxCommand>,
    /// Dedicated channel for WINDOW_UPDATE commands. Never contends with
    /// PING/PONG/RST_STREAM/CloseStream traffic on control_tx.
    pub window_tx: mpsc::Sender<MuxCommand>,
    pub frame_txs: [mpsc::Sender<Frame>; FRAME_WORKERS],
    pub reg_txs: Arc<[mpsc::Sender<StreamRegistration>; FRAME_WORKERS]>,
    pub close_reg_txs: Arc<[mpsc::Sender<StreamRegistration>; FRAME_WORKERS]>,
}

/// Clone-able Arc-wrapped shared state for the reader.
#[derive(Clone)]
pub(super) struct ReaderSharedState {
    pub cancel: CancellationToken,
    pub peer_initial_window: Arc<AtomicU32>,
    pub peer_max_concurrent: Arc<AtomicU32>,
    pub goaway_received: Arc<AtomicBool>,
    pub session_send_window: Arc<SendWindow>,
    pub max_frame_size: Arc<AtomicU32>,
}

/// Owned runtime pieces for the reader task.
pub(super) struct ReaderParts<R: WsFrameReader> {
    pub ws_read: R,
    pub channels: ReaderChannels,
    pub shared: ReaderSharedState,
    pub ping_ready: Option<oneshot::Sender<Result<(), Error>>>,
}

/// Context passed to `route_frame` to avoid long parameter lists.
pub(super) struct ReaderContext<'a> {
    pub config: ReaderConfig,
    pub channels: &'a ReaderChannels,
    pub shared: &'a ReaderSharedState,
    pub waiting_ping_ready: &'a mut Option<oneshot::Sender<Result<(), Error>>>,
    pub session_recv_consumed: &'a mut u64,
    pub codec: &'a mut SpdyCodec,
}

/// Reads WebSocket frames, decodes them through the SPDY codec (decompressor
/// half), and routes stream-keyed frames to the appropriate frame worker.
/// Handles session-level frames (PING, SETTINGS, GOAWAY, session
/// WINDOW_UPDATE) inline.
///
/// Sends control commands (PING responses, WINDOW_UPDATE, CloseStream for
/// rejected streams) to `control_tx` so the writer processes them first.
///
/// Generic over the transport reader via [`WsFrameReader`] to decouple SPDY
/// framing from the concrete WebSocket implementation.
pub(super) async fn run_reader<R: WsFrameReader>(parts: ReaderParts<R>, config: ReaderConfig) {
    let ReaderParts {
        mut ws_read,
        channels,
        shared,
        ping_ready,
    } = parts;

    let mut codec = SpdyCodec::with_max_frame_size(config.configured_max_frame_size);
    let mut frame_buf = BytesMut::with_capacity(16 * 1024);
    let mut waiting_ping_ready = ping_ready;

    // Session-level recv window tracking
    let mut session_recv_consumed: u64 = 0;

    // Idle timeout tracking
    let idle_timeout = config.idle_timeout;
    let ping_timeout = config.ping_timeout;
    let mut last_frame_time = tokio::time::Instant::now();
    let mut idle_ping_sent = false;
    let mut idle_probe_sent_at: Option<tokio::time::Instant> = None;

    loop {
        // Compute timeout for idle detection
        let time_since_last = last_frame_time.elapsed();
        let idle_remaining = idle_timeout.saturating_sub(time_since_last);

        // Check ping timeout if we sent an idle probe
        if let Some(sent_at) = idle_probe_sent_at {
            if sent_at.elapsed() >= ping_timeout {
                tracing::warn!("SPDY reader: ping timeout after {ping_timeout:?}, tearing down");
                break;
            }
        }

        tokio::select! {
            biased;

            _ = shared.cancel.cancelled() => {
                tracing::debug!("SPDY reader: cancelled");
                break;
            }

            // Idle timeout fires: send a probe PING
            _ = tokio::time::sleep(idle_remaining), if !idle_ping_sent && idle_probe_sent_at.is_none() => {
                tracing::debug!("SPDY reader: idle timeout ({idle_timeout:?}), sending probe PING");
                let _ = channels.control_tx.try_send(MuxCommand::EncodePing { id: 0xFFFF_FFFD });
                idle_ping_sent = true;
                idle_probe_sent_at = Some(tokio::time::Instant::now());
            }

            msg = ws_read.read_message() => {
                match msg {
                    Some(Ok(WsMessage::Binary(data))) => {
                        // Any frame received resets idle tracking
                        last_frame_time = tokio::time::Instant::now();
                        idle_ping_sent = false;
                        idle_probe_sent_at = None;

                        tracing::trace!(
                            len = data.len(),
                            "SPDY reader: received WS binary message"
                        );
                        frame_buf.extend_from_slice(&data);

                        let mut should_break = false;
                        while frame_buf.len() >= 8 {
                            match codec.decode_frame(&mut frame_buf) {
                                Ok(Some(frame)) => {
                                    let mut ctx = ReaderContext {
                                        config,
                                        channels: &channels,
                                        shared: &shared,
                                        waiting_ping_ready: &mut waiting_ping_ready,
                                        session_recv_consumed: &mut session_recv_consumed,
                                        codec: &mut codec,
                                    };
                                    if route_frame(frame, &mut ctx).is_err() {
                                        should_break = true;
                                        break;
                                    }
                                }
                                Ok(None) => break,
                                Err(Error::FrameTooLarge { stream_id, size, max }) => {
                                    tracing::warn!(
                                        stream_id,
                                        size,
                                        max,
                                        "SPDY reader: incoming frame too large, RST_STREAM"
                                    );
                                    let _ = channels.control_tx.try_send(MuxCommand::CloseStream {
                                        stream_id,
                                        status: RST_STATUS_FLOW_CONTROL,
                                    });
                                    if frame_buf.len() >= 8 {
                                        let flags_len = u32::from_be_bytes([
                                            frame_buf[4], frame_buf[5],
                                            frame_buf[6], frame_buf[7],
                                        ]);
                                        let payload_len = (flags_len & 0x00FF_FFFF) as usize;
                                        let total = 8 + payload_len;
                                        if frame_buf.len() >= total {
                                            let _ = frame_buf.split_to(total);
                                        } else {
                                            frame_buf.clear();
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("SPDY decode error: {e}");
                                    should_break = true;
                                    break;
                                }
                            }
                        }
                        if should_break {
                            break;
                        }
                    }
                    Some(Ok(WsMessage::Ping(p))) => {
                        last_frame_time = tokio::time::Instant::now();
                        idle_ping_sent = false;
                        idle_probe_sent_at = None;
                        let _ = channels.control_tx.try_send(MuxCommand::SendWsPong { payload: p });
                    }
                    Some(Ok(WsMessage::Close)) | None => {
                        tracing::debug!("SPDY reader: WebSocket closed");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::warn!("SPDY reader: WebSocket error: {e}");
                        break;
                    }
                }
            }
        }
    }

    shared.cancel.cancel();
    // Workers handle their own cleanup (pending_replies + send_windows)
    // when their frame_rx / reg_rx channels close (triggered by cancel
    // or by this function dropping frame_txs).
}

/// Routes a decoded SPDY frame. Handles session-level frames inline and
/// dispatches stream-keyed frames to the appropriate worker. Sends control
/// commands to `control_tx` for priority processing by the writer.
pub(super) fn route_frame(frame: Frame, ctx: &mut ReaderContext<'_>) -> Result<(), Error> {
    match frame {
        Frame::Data {
            stream_id,
            ref payload,
            ..
        } => {
            // Session-level recv window tracking (reader-owned)
            if !payload.is_empty() {
                let payload_len = payload.len() as u64;
                *ctx.session_recv_consumed += payload_len;
                if *ctx.session_recv_consumed >= (ctx.config.initial_window_size / 2) as u64 {
                    let delta = *ctx.session_recv_consumed as u32;
                    // Only reset the counter on successful send. If the channel
                    // is full, the accumulated bytes carry over to the next DATA
                    // frame and we retry then. WINDOW_UPDATE deltas are additive,
                    // so accumulating across attempts is safe. Resetting on
                    // failure causes the peer's view of our recv window to drift
                    // monotonically toward zero, the root cause of the 45s stall.
                    if ctx
                        .channels
                        .window_tx
                        .try_send(MuxCommand::EncodeWindowUpdate {
                            stream_id: 0, // session-level
                            delta,
                        })
                        .is_ok()
                    {
                        *ctx.session_recv_consumed = 0;
                    }
                }
            }
            let worker = (stream_id % FRAME_WORKERS as u32) as usize;
            if ctx.channels.frame_txs[worker].try_send(frame).is_err() {
                tracing::debug!(
                    stream_id,
                    "SPDY reader: worker queue full/closed, dropping frame"
                );
                let _ = ctx.channels.control_tx.try_send(MuxCommand::CloseStream {
                    stream_id,
                    status: RST_STATUS_FLOW_CONTROL,
                });
            }
        }
        Frame::SynReply { stream_id, .. } | Frame::RstStream { stream_id, .. } => {
            let worker = (stream_id % FRAME_WORKERS as u32) as usize;
            if ctx.channels.frame_txs[worker].try_send(frame).is_err() {
                tracing::debug!(
                    stream_id,
                    "SPDY reader: worker queue full/closed, dropping frame"
                );
            }
        }
        Frame::WindowUpdate {
            stream_id,
            delta_window_size,
        } => {
            if stream_id == 0 {
                // Session-level WINDOW_UPDATE: handle inline
                ctx.shared.session_send_window.replenish(delta_window_size);
            } else {
                // Per-stream WINDOW_UPDATE: route to worker
                let worker = (stream_id % FRAME_WORKERS as u32) as usize;
                if ctx.channels.frame_txs[worker]
                    .try_send(Frame::WindowUpdate {
                        stream_id,
                        delta_window_size,
                    })
                    .is_err()
                {
                    tracing::trace!(
                        stream_id,
                        "SPDY reader: worker queue full/closed for WINDOW_UPDATE"
                    );
                }
            }
        }

        Frame::Ping { id } => {
            if let Some(tx) = ctx.waiting_ping_ready.take() {
                tracing::debug!(id, "SPDY reader: initial PING response received");
                let _ = tx.send(Ok(()));
            } else if id % 2 == 0 {
                // Server-initiated PING (even ID): respond via control channel.
                let _ = ctx
                    .channels
                    .control_tx
                    .try_send(MuxCommand::EncodePing { id });
            }
            // else: response to our keepalive ping (odd ID). Idle tracking
            // already reset by the caller since any frame resets idle timer.
        }
        Frame::GoAway {
            last_good_stream_id,
            status,
        } => {
            tracing::warn!(last_good_stream_id, status, "SPDY GOAWAY received");

            // Set flag to stop accepting new streams.
            ctx.shared.goaway_received.store(true, Ordering::Release);

            // Broadcast GoAway to all workers via close_reg channels
            // (GoAway is a teardown event, not an open event).
            for close_reg_tx in ctx.channels.close_reg_txs.iter() {
                let _ = close_reg_tx.try_send(StreamRegistration::GoAway {
                    last_good_stream_id,
                    status,
                });
            }
        }
        Frame::SynStream {
            stream_id,
            headers,
            fin,
        } => {
            // Server-initiated stream: port-forwarding doesn't accept these.
            tracing::debug!(
                stream_id,
                num_headers = headers.len(),
                fin,
                "SPDY server-initiated SynStream rejected (not supported for port-forward)"
            );
            let _ = ctx.channels.control_tx.try_send(MuxCommand::CloseStream {
                stream_id,
                status: RST_STATUS_REFUSED_STREAM,
            });
        }
        Frame::Settings {
            initial_window_size,
            max_concurrent_streams,
            max_frame_size,
        } => {
            // Honor INITIAL_WINDOW_SIZE: broadcast delta to all workers
            if let Some(new_size) = initial_window_size {
                let old_size = ctx
                    .shared
                    .peer_initial_window
                    .swap(new_size, Ordering::AcqRel);
                let delta = new_size as i64 - old_size as i64;
                if delta != 0 {
                    tracing::debug!(
                        old_size,
                        new_size,
                        delta,
                        "SPDY SETTINGS: broadcasting window delta to workers"
                    );
                    for reg_tx in ctx.channels.reg_txs.iter() {
                        let _ = reg_tx.try_send(StreamRegistration::SettingsWindowDelta { delta });
                    }
                }
            }

            // Honor MAX_CONCURRENT_STREAMS
            if let Some(max) = max_concurrent_streams {
                tracing::debug!(
                    max_concurrent_streams = max,
                    "SPDY SETTINGS: peer updated MAX_CONCURRENT_STREAMS"
                );
                ctx.shared.peer_max_concurrent.store(max, Ordering::Release);
            }

            // Honor MAX_FRAME_SIZE
            if let Some(mfs) = max_frame_size {
                tracing::debug!(
                    max_frame_size = mfs,
                    "SPDY SETTINGS: peer updated MAX_FRAME_SIZE"
                );
                ctx.shared.max_frame_size.store(mfs, Ordering::Release);
                ctx.codec.set_max_frame_size(mfs);
            }
        }
        Frame::Unknown => {}
    }
    Ok(())
}
