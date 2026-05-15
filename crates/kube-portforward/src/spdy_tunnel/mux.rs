use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicU32,
    AtomicUsize,
    Ordering,
};
use std::time::Duration;

use bytes::{Buf, Bytes, BytesMut};
use futures::stream::SplitSink;
use futures::{
    SinkExt,
    StreamExt,
};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use tokio::sync::{
    mpsc,
    oneshot,
};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::codec::{
    Frame,
    SpdyCodec,
};
use super::error::Error;
use super::stream::Stream;

const SYN_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Command sent from MuxHandle to the background mux task.
pub(crate) enum MuxCommand {
    OpenStream {
        stream_id: u32,
        headers: Vec<(String, String)>,
        fin: bool,
        data_tx: mpsc::Sender<Bytes>,
        reply_tx: oneshot::Sender<Result<(), Error>>,
    },
    SendData {
        stream_id: u32,
        payload: Bytes,
        fin: bool,
    },
    CloseStream {
        stream_id: u32,
    },
}

/// Cheaply cloneable handle to the mux background task.
#[derive(Clone)]
pub(crate) struct MuxHandle {
    cmd_tx: mpsc::Sender<MuxCommand>,
    active_pairs: Arc<AtomicUsize>,
    next_stream_id: Arc<AtomicU32>,
    next_request_id: Arc<AtomicU32>,
    closed: CancellationToken,
}

impl MuxHandle {
    /// Start the mux background task, send an initial PING to verify the
    /// upstream SPDY connection is established (the API server's TunnelingHandler
    /// needs time to complete the kubelet SPDY handshake before forwarding
    /// frames), and return a handle once the PING response arrives.
    pub(crate) async fn spawn(
        ws: WebSocketStream<TokioIo<Upgraded>>, cancel: CancellationToken,
    ) -> Result<Self, Error> {
        let (cmd_tx, cmd_rx) = mpsc::channel(256);
        let active_pairs = Arc::new(AtomicUsize::new(0));
        let closed = cancel.clone();

        let handle = Self {
            cmd_tx,
            active_pairs: Arc::clone(&active_pairs),
            next_stream_id: Arc::new(AtomicU32::new(1)), // client uses odd IDs starting at 1
            next_request_id: Arc::new(AtomicU32::new(0)),
            closed: closed.clone(),
        };

        let (ping_tx, ping_rx) = oneshot::channel();
        tokio::spawn(async move {
            run_mux_task(ws, cmd_rx, closed, Some(ping_tx)).await;
        });

        // Wait for the initial PING roundtrip to confirm the upstream
        // SPDY connection is alive before returning the handle.
        match tokio::time::timeout(Duration::from_secs(10), ping_rx).await {
            Ok(Ok(Ok(()))) => {
                tracing::debug!("SPDY mux: initial PING succeeded, connection ready");
                Ok(handle)
            }
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => Err(Error::MuxClosed),
            Err(_) => Err(Error::SynReplyTimeout(0)),
        }
    }

    /// Open a port-forward stream pair (error stream + data stream).
    ///
    /// Returns a `Stream` that provides `AsyncRead + AsyncWrite` on the data
    /// half and `AsyncRead` on the error half.
    pub(crate) async fn open_portforward_pair(&self, port: u16) -> Result<Stream, Error> {
        if self.closed.is_cancelled() {
            return Err(Error::MuxClosed);
        }

        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);

        // 1. Open error stream (FIN=true, client closes write side immediately)
        let error_id = self.next_stream_id.fetch_add(2, Ordering::Relaxed);
        let (error_tx, error_rx) = mpsc::channel(64);
        // SPDY/3.1 requires lowercased header names — Go's spdystream
        // rejects frames with non-lowercase names ("header was not lowercased").
        let error_headers = vec![
            ("streamtype".to_string(), "error".to_string()),
            ("port".to_string(), port.to_string()),
            ("requestid".to_string(), request_id.to_string()),
        ];
        self.send_open(error_id, error_headers, true, error_tx)
            .await?;

        // 2. Open data stream (FIN=false)
        let data_id = self.next_stream_id.fetch_add(2, Ordering::Relaxed);
        let (data_tx, data_rx) = mpsc::channel(256);
        let data_headers = vec![
            ("streamtype".to_string(), "data".to_string()),
            ("port".to_string(), port.to_string()),
            ("requestid".to_string(), request_id.to_string()),
        ];
        let reply = self
            .send_open(data_id, data_headers, false, data_tx)
            .await?;

        // 3. Wait for SYN_REPLY on data stream (server acknowledges)
        match tokio::time::timeout(SYN_REPLY_TIMEOUT, reply).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => return Err(e),
            Ok(Err(_)) => return Err(Error::MuxClosed),
            Err(_) => return Err(Error::SynReplyTimeout(data_id)),
        }

        self.active_pairs.fetch_add(1, Ordering::Relaxed);

        Ok(Stream::new(
            data_id,
            error_id,
            data_rx,
            error_rx,
            self.clone(),
        ))
    }

    /// Send an OpenStream command and return the reply receiver.
    async fn send_open(
        &self, stream_id: u32, headers: Vec<(String, String)>, fin: bool,
        data_tx: mpsc::Sender<Bytes>,
    ) -> Result<oneshot::Receiver<Result<(), Error>>, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.cmd_tx
            .send(MuxCommand::OpenStream {
                stream_id,
                headers,
                fin,
                data_tx,
                reply_tx,
            })
            .await
            .map_err(|_| Error::MuxClosed)?;

        Ok(reply_rx)
    }

    /// Non-blocking send for use in poll_write (cannot await in poll context).
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

    /// Close a stream (sends RST_STREAM).
    pub(crate) fn close_stream(&self, stream_id: u32) {
        let _ = self.cmd_tx.try_send(MuxCommand::CloseStream { stream_id });
    }

    /// Decrement the active pair count.
    pub(crate) fn release_pair(&self) {
        self.active_pairs.fetch_sub(1, Ordering::Relaxed);
    }

    pub(crate) fn active_pairs(&self) -> usize {
        self.active_pairs.load(Ordering::Relaxed)
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.is_cancelled()
    }

    /// Clone the command sender for use by `PollSender` in stream writes.
    pub(crate) fn cmd_sender(&self) -> mpsc::Sender<MuxCommand> {
        self.cmd_tx.clone()
    }
}

/// Background task that owns the WebSocket and routes frames.
async fn run_mux_task(
    ws: WebSocketStream<TokioIo<Upgraded>>, mut cmd_rx: mpsc::Receiver<MuxCommand>,
    cancel: CancellationToken, ping_ready: Option<oneshot::Sender<Result<(), Error>>>,
) {
    let (mut ws_write, mut ws_read) = ws.split();
    let mut codec = SpdyCodec::new();
    let mut streams: HashMap<u32, mpsc::Sender<Bytes>> = HashMap::new();
    let mut pending_replies: HashMap<u32, oneshot::Sender<Result<(), Error>>> = HashMap::new();
    let mut frame_buf = BytesMut::with_capacity(16 * 1024);
    let mut waiting_ping_ready = ping_ready;

    // Send initial PING to confirm upstream SPDY connection is alive.
    // The API server's TunnelingHandler needs to complete the kubelet
    // SPDY handshake before it can forward frames.
    {
        let ping_frame = codec.encode_ping(1);
        tracing::debug!("SPDY mux: sending initial PING");
        if let Err(e) = ws_write.send(Message::Binary(ping_frame.into())).await {
            tracing::warn!("SPDY mux: failed to send initial PING: {e}");
            if let Some(tx) = waiting_ping_ready.take() {
                let _ = tx.send(Err(Error::WebSocket(e)));
            }
            cancel.cancel();
            return;
        }
    }

    // Keepalive PING: client uses odd IDs; 1 was the initial PING.
    let mut ping_interval = tokio::time::interval(Duration::from_secs(10));
    ping_interval.tick().await; // skip the immediate first tick
    let mut ping_id: u32 = 3;

    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                tracing::debug!("SPDY mux task cancelled");
                break;
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(cmd) => {
                        if let Err(e) = handle_command(
                            cmd,
                            &mut ws_write,
                            &mut codec,
                            &mut streams,
                            &mut pending_replies,
                        ).await {
                            tracing::warn!("SPDY mux command error: {e}");
                            cancel.cancel();
                            break;
                        }
                    }
                    None => {
                        tracing::debug!("SPDY mux command channel closed");
                        cancel.cancel();
                        break;
                    }
                }
            }

            _ = ping_interval.tick() => {
                let ping = codec.encode_ping(ping_id);
                ping_id = ping_id.wrapping_add(2);
                if let Err(e) = ws_write.send(Message::Binary(ping.into())).await {
                    tracing::warn!("SPDY mux: keepalive PING failed: {e}");
                    cancel.cancel();
                    break;
                }
            }

            msg = ws_read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        tracing::trace!(
                            len = data.len(),
                            hex = %format_hex_preview(&data, 64),
                            "SPDY mux: received WS binary message"
                        );
                        frame_buf.extend_from_slice(&data);

                        // Parse all complete frames from the accumulated buffer
                        let mut should_break = false;
                        while frame_buf.len() >= 8 {
                            match codec.decode_frame(&frame_buf) {
                                Ok(Some((frame, consumed))) => {
                                    frame_buf.advance(consumed);
                                    match frame {
                                        Frame::Data { stream_id, payload, fin } => {
                                            if let Some(tx) = streams.get(&stream_id) {
                                                if !payload.is_empty() {
                                                    let _ = tx.send(payload).await;
                                                }
                                            } else {
                                                tracing::debug!(stream_id, "SPDY DATA for unknown stream (dropped)");
                                            }
                                            if fin {
                                                streams.remove(&stream_id);
                                            }
                                        }
                                        Frame::SynReply { stream_id, headers, fin } => {
                                            tracing::debug!(
                                                stream_id,
                                                num_headers = headers.len(),
                                                fin,
                                                "SPDY SYN_REPLY received"
                                            );
                                            if let Some(reply_tx) = pending_replies.remove(&stream_id) {
                                                let _ = reply_tx.send(Ok(()));
                                            }
                                        }
                                        Frame::RstStream { stream_id, status } => {
                                            let was_tracked = streams.remove(&stream_id).is_some();
                                            if let Some(reply_tx) = pending_replies.remove(&stream_id) {
                                                let _ = reply_tx.send(Err(Error::StreamReset(stream_id, status)));
                                            } else if !was_tracked {
                                                tracing::debug!(
                                                    stream_id,
                                                    status,
                                                    "SPDY RST_STREAM for unknown stream: {}",
                                                    Error::StreamNotFound(stream_id)
                                                );
                                            }
                                        }
                                        Frame::Ping { id } => {
                                            if let Some(tx) = waiting_ping_ready.take() {
                                                tracing::debug!(id, "SPDY mux: initial PING response received");
                                                let _ = tx.send(Ok(()));
                                            } else if id % 2 == 0 {
                                                // Server-initiated PING (even ID) — respond
                                                let pong = codec.encode_ping(id);
                                                if let Err(e) = ws_write.send(Message::Binary(pong.into())).await {
                                                    tracing::warn!("failed to send SPDY PING response: {e}");
                                                }
                                            }
                                            // else: response to our keepalive PING (odd ID), ignore
                                        }
                                        Frame::GoAway { last_good_stream_id, status } => {
                                            tracing::warn!(last_good_stream_id, status, "SPDY GOAWAY received");
                                            cancel.cancel();
                                            should_break = true;
                                            break;
                                        }
                                        Frame::SynStream { stream_id, headers, fin } => {
                                            tracing::debug!(
                                                stream_id,
                                                num_headers = headers.len(),
                                                fin,
                                                "SPDY server-initiated SynStream (ignored for port-forward)"
                                            );
                                        }
                                        Frame::Unknown => {}
                                    }
                                }
                                Ok(None) => break, // need more data
                                Err(e) => {
                                    tracing::warn!("SPDY decode error: {e}");
                                    cancel.cancel();
                                    should_break = true;
                                    break;
                                }
                            }
                        }
                        if should_break {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if let Err(e) = ws_write.send(Message::Pong(p)).await {
                            tracing::warn!("failed to send WS pong: {e}");
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::debug!("SPDY WebSocket closed");
                        cancel.cancel();
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::warn!("SPDY WebSocket error: {e}");
                        cancel.cancel();
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    // Clean up: fail all pending replies
    for (_, reply_tx) in pending_replies.drain() {
        let _ = reply_tx.send(Err(Error::MuxClosed));
    }
}

async fn handle_command(
    cmd: MuxCommand, ws_write: &mut SplitSink<WebSocketStream<TokioIo<Upgraded>>, Message>,
    codec: &mut SpdyCodec, streams: &mut HashMap<u32, mpsc::Sender<Bytes>>,
    pending_replies: &mut HashMap<u32, oneshot::Sender<Result<(), Error>>>,
) -> Result<(), Error> {
    match cmd {
        MuxCommand::OpenStream {
            stream_id,
            headers,
            fin,
            data_tx,
            reply_tx,
        } => {
            let frame_bytes = codec.encode_syn_stream(stream_id, &headers, fin)?;
            tracing::debug!(
                stream_id,
                ?headers,
                fin,
                len = frame_bytes.len(),
                hex = %format_hex_preview(&frame_bytes, 64),
                "SPDY mux: sending SYN_STREAM"
            );
            ws_write
                .send(Message::Binary(frame_bytes.into()))
                .await
                .map_err(Error::WebSocket)?;
            streams.insert(stream_id, data_tx);
            pending_replies.insert(stream_id, reply_tx);
        }
        MuxCommand::SendData {
            stream_id,
            payload,
            fin,
        } => {
            let frame_bytes = codec.encode_data(stream_id, &payload, fin);
            ws_write
                .send(Message::Binary(frame_bytes.into()))
                .await
                .map_err(Error::WebSocket)?;
            if fin {
                streams.remove(&stream_id);
            }
        }
        MuxCommand::CloseStream { stream_id } => {
            let frame_bytes = codec.encode_rst_stream(stream_id, 5); // CANCEL status
            ws_write
                .send(Message::Binary(frame_bytes.into()))
                .await
                .map_err(Error::WebSocket)?;
            streams.remove(&stream_id);
        }
    }
    Ok(())
}

/// Format a byte slice as hex for trace logging, truncated to `max_bytes`.
fn format_hex_preview(data: &[u8], max_bytes: usize) -> String {
    use std::fmt::Write;
    let limit = data.len().min(max_bytes);
    let mut s = String::with_capacity(limit * 3);
    for (i, b) in data[..limit].iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{b:02x}");
    }
    if data.len() > max_bytes {
        let _ = write!(s, "... ({} more bytes)", data.len() - max_bytes);
    }
    s
}
