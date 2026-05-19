use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
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
    RST_STATUS_CANCEL,
    RST_STATUS_FLOW_CONTROL,
    RST_STATUS_INVALID_STREAM,
};
use crate::codec::Frame;
use crate::error::Error;

/// Per-stream state owned exclusively by one frame worker.
pub(super) struct StreamState {
    /// Data sender. `None` after remote FIN (half-close).
    data_tx: Option<mpsc::Sender<Bytes>>,
    send_window: Arc<SendWindow>,
    /// Bytes delivered to the per-stream channel since the last
    /// WINDOW_UPDATE was sent.
    consumed_since_update: u64,
}

/// Owns a shard of streams partitioned by `stream_id % FRAME_WORKERS`.
/// No shared mutable state across workers.
///
/// Drains `close_reg_rx` (close registrations) before `reg_rx` (open
/// registrations) so teardown is never blocked by a burst of opens.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_frame_worker(
    _worker_id: usize, mut frame_rx: mpsc::Receiver<Frame>,
    mut reg_rx: mpsc::Receiver<StreamRegistration>,
    mut close_reg_rx: mpsc::Receiver<StreamRegistration>, control_tx: mpsc::Sender<MuxCommand>,
    window_tx: mpsc::Sender<MuxCommand>, cancel: CancellationToken, initial_window_size: u32,
) {
    let mut streams: HashMap<u32, StreamState> = HashMap::new();
    let mut pending_replies: HashMap<u32, oneshot::Sender<Result<(), Error>>> = HashMap::new();

    loop {
        // Drain close registrations first (non-blocking, priority).
        while let Ok(reg) = close_reg_rx.try_recv() {
            apply_registration(reg, &mut streams, &mut pending_replies, &control_tx);
        }
        // Drain any pending open registrations (non-blocking) to ensure
        // Open registrations are applied before any frames for that stream.
        while let Ok(reg) = reg_rx.try_recv() {
            apply_registration(reg, &mut streams, &mut pending_replies, &control_tx);
        }

        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            // Close registrations have priority over opens.
            reg = close_reg_rx.recv() => match reg {
                Some(r) => apply_registration(r, &mut streams, &mut pending_replies, &control_tx),
                None => break,
            },
            // Open registrations checked before frames.
            reg = reg_rx.recv() => match reg {
                Some(r) => apply_registration(r, &mut streams, &mut pending_replies, &control_tx),
                None => break,
            },
            frame = frame_rx.recv() => match frame {
                Some(f) => dispatch_frame_in_worker(
                    f, &mut streams, &mut pending_replies,
                    &control_tx, &window_tx, initial_window_size,
                ),
                None => break,
            },
        }
    }

    // Shutdown: fail all pending replies, poison all send windows.
    let orphaned_streams = streams.len();
    let orphaned_replies = pending_replies.len();
    if orphaned_streams > 0 || orphaned_replies > 0 {
        tracing::debug!(
            worker_id = _worker_id,
            orphaned_streams,
            orphaned_replies,
            "worker shutting down with in-flight streams"
        );
    }
    for (_, reply_tx) in pending_replies.drain() {
        let _ = reply_tx.send(Err(Error::MuxClosed));
    }
    for (_, state) in streams.drain() {
        state.send_window.close();
    }
}

/// Dispatches a decoded SPDY frame within a worker. Handles only stream-keyed
/// frames. Entirely synchronous and never blocks.
pub(super) fn dispatch_frame_in_worker(
    frame: Frame, streams: &mut HashMap<u32, StreamState>,
    pending_replies: &mut HashMap<u32, oneshot::Sender<Result<(), Error>>>,
    cmd_tx: &mpsc::Sender<MuxCommand>, window_tx: &mpsc::Sender<MuxCommand>,
    initial_window_size: u32,
) {
    match frame {
        Frame::Data {
            stream_id,
            payload,
            fin,
        } => {
            if !payload.is_empty() {
                let payload_len = payload.len() as u64;

                // Session-level recv window is tracked by the reader (not here).
                let mut remove = false;

                if let Some(state) = streams.get_mut(&stream_id) {
                    // Only send data if the data_tx is still open (not half-closed)
                    if let Some(ref data_tx) = state.data_tx {
                        match data_tx.try_send(payload) {
                            Ok(()) => {
                                // Track consumed bytes since last per-stream WINDOW_UPDATE.
                                state.consumed_since_update += payload_len;
                                if state.consumed_since_update >= (initial_window_size / 2) as u64 {
                                    let delta = state.consumed_since_update as u32;
                                    // Only reset on success; if the channel is full,
                                    // accumulate and retry on the next DATA frame.
                                    if window_tx
                                        .try_send(MuxCommand::EncodeWindowUpdate {
                                            stream_id,
                                            delta,
                                        })
                                        .is_ok()
                                    {
                                        state.consumed_since_update = 0;
                                    }
                                }
                            }
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                // On buffer-full: do NOT send RST_STREAM. Stop draining frame_rx
                                // instead; the reader backs up, TCP
                                // applies backpressure to the peer, the peer slows down. The frame
                                // is "lost" for this delivery
                                // attempt, but the per-stream recv window is not replenished
                                // (no WINDOW_UPDATE), so the peer naturally pauses sending to this
                                // stream.
                                tracing::debug!(
                                    stream_id,
                                    "SPDY stream buffer full, applying backpressure (not RST)"
                                );
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                remove = true;
                            }
                        }
                    }
                    // If data_tx is None (half-closed), silently ignore. The
                    // stream entry is kept for WINDOW_UPDATE processing.
                } else {
                    // TRACE level: routinely fires during connection teardown when
                    // late DATA frames arrive for streams we've already closed.
                    // The RST_STREAM response is correct per SPDY spec; only the
                    // log noise is undesirable at DEBUG.
                    tracing::trace!(
                        stream_id,
                        "SPDY DATA for unknown stream, sending RST_STREAM"
                    );
                    let _ = cmd_tx.try_send(MuxCommand::CloseStream {
                        stream_id,
                        status: RST_STATUS_INVALID_STREAM,
                    });
                }

                if remove {
                    streams.remove(&stream_id);
                    let _ = cmd_tx.try_send(MuxCommand::CloseStream {
                        stream_id,
                        status: RST_STATUS_FLOW_CONTROL,
                    });
                    return;
                }
            }

            // Half-close: remote FIN means peer is done sending. Close our data Sender so
            // the consumer reads EOF, but keep the stream entry alive for
            // outgoing WINDOW_UPDATE processing. Full cleanup happens when
            // StreamGuard fires Close.
            if fin {
                if let Some(state) = streams.get_mut(&stream_id) {
                    tracing::debug!(stream_id, "SPDY DATA FIN received, half-closing read side");
                    state.data_tx = None;
                }
            }
        }
        Frame::SynReply {
            stream_id,
            headers,
            fin,
        } => {
            tracing::debug!(
                stream_id,
                num_headers = headers.len(),
                fin,
                "SPDY SYN_REPLY received"
            );
            if let Some(reply_tx) = pending_replies.remove(&stream_id) {
                let _ = reply_tx.send(Ok(()));
            } else if !streams.contains_key(&stream_id) {
                let _ = cmd_tx.try_send(MuxCommand::CloseStream {
                    stream_id,
                    status: RST_STATUS_INVALID_STREAM,
                });
            }
            // Half-close on SYN_REPLY with FIN
            if fin {
                if let Some(state) = streams.get_mut(&stream_id) {
                    state.data_tx = None;
                }
            }
        }
        Frame::RstStream { stream_id, status } => {
            // Poison the send window so any in-progress poll_write on this
            // stream returns BrokenPipe immediately. Critical for the
            // no-SYN_REPLY-wait path: if the server rejects the stream
            // via RST_STREAM, the caller learns through the poisoned window,
            // not through an awaited oneshot.
            tracing::debug!(stream_id, status, "SPDY RST_STREAM received");
            if let Some(state) = streams.remove(&stream_id) {
                state.send_window.close();
            }
            if let Some(reply_tx) = pending_replies.remove(&stream_id) {
                let _ = reply_tx.send(Err(Error::StreamReset(stream_id, status)));
            }
        }
        Frame::WindowUpdate {
            stream_id,
            delta_window_size,
        } => {
            // Only per-stream WINDOW_UPDATEs reach workers (stream_id == 0
            // is handled by the reader directly).
            if let Some(state) = streams.get(&stream_id) {
                state.send_window.replenish(delta_window_size);
            }
            // else: stream already closed, harmless.
        }
        // Other frame types should not reach workers.
        _ => {}
    }
}

pub(super) fn apply_registration(
    reg: StreamRegistration, streams: &mut HashMap<u32, StreamState>,
    pending_replies: &mut HashMap<u32, oneshot::Sender<Result<(), Error>>>,
    cmd_tx: &mpsc::Sender<MuxCommand>,
) {
    match reg {
        StreamRegistration::Open {
            stream_id,
            data_tx,
            reply_tx,
            send_window,
        } => {
            streams.insert(
                stream_id,
                StreamState {
                    data_tx: Some(data_tx),
                    send_window,
                    consumed_since_update: 0,
                },
            );
            pending_replies.insert(stream_id, reply_tx);
        }
        StreamRegistration::Close { stream_id } => {
            if let Some(state) = streams.remove(&stream_id) {
                state.send_window.close();
            }
            pending_replies.remove(&stream_id);
        }
        StreamRegistration::SettingsWindowDelta { delta } => {
            for state in streams.values() {
                state.send_window.apply_delta(delta);
            }
        }
        StreamRegistration::GoAway {
            last_good_stream_id,
            status,
        } => {
            // Clean up streams with id > last_good_stream_id in this shard.
            let bad_ids: Vec<u32> = streams
                .keys()
                .filter(|&&id| id > last_good_stream_id)
                .copied()
                .collect();

            for id in bad_ids {
                if let Some(state) = streams.remove(&id) {
                    state.send_window.close();
                }
                if let Some(reply_tx) = pending_replies.remove(&id) {
                    let _ = reply_tx.send(Err(Error::GoAway {
                        last_good_stream_id,
                        status,
                    }));
                }
                let _ = cmd_tx.try_send(MuxCommand::CloseStream {
                    stream_id: id,
                    status: RST_STATUS_CANCEL,
                });
            }
        }
    }
}
