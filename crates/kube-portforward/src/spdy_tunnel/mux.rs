use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicU32,
    AtomicUsize,
    Ordering,
};
use std::time::Duration;

use bytes::Bytes;
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
enum MuxCommand {
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
    /// Start the mux background task and return a handle.
    pub(crate) fn spawn(ws: WebSocketStream<TokioIo<Upgraded>>, cancel: CancellationToken) -> Self {
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

        let active_pairs_task = Arc::clone(&active_pairs);
        tokio::spawn(async move {
            run_mux_task(ws, cmd_rx, closed, active_pairs_task).await;
        });

        handle
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
        let error_headers = vec![
            ("streamType".to_string(), "error".to_string()),
            ("port".to_string(), port.to_string()),
            ("requestID".to_string(), request_id.to_string()),
        ];
        self.send_open(error_id, error_headers, true, error_tx)
            .await?;

        // 2. Open data stream (FIN=false)
        let data_id = self.next_stream_id.fetch_add(2, Ordering::Relaxed);
        let (data_tx, data_rx) = mpsc::channel(256);
        let data_headers = vec![
            ("streamType".to_string(), "data".to_string()),
            ("port".to_string(), port.to_string()),
            ("requestID".to_string(), request_id.to_string()),
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
}

/// Background task that owns the WebSocket and routes frames.
async fn run_mux_task(
    ws: WebSocketStream<TokioIo<Upgraded>>, mut cmd_rx: mpsc::Receiver<MuxCommand>,
    cancel: CancellationToken, _active_pairs: Arc<AtomicUsize>,
) {
    let (mut ws_write, mut ws_read) = ws.split();
    let mut codec = SpdyCodec::new();
    let mut streams: HashMap<u32, mpsc::Sender<Bytes>> = HashMap::new();
    let mut pending_replies: HashMap<u32, oneshot::Sender<Result<(), Error>>> = HashMap::new();

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

            msg = ws_read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        match codec.decode_frame(&data) {
                            Ok(Frame::Data { stream_id, payload, fin }) => {
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
                            Ok(Frame::SynReply { stream_id, headers, fin }) => {
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
                            Ok(Frame::RstStream { stream_id, status }) => {
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
                            Ok(Frame::Ping { id }) => {
                                // Respond with PONG (same ID)
                                let pong = codec.encode_ping(id);
                                if let Err(e) = ws_write.send(Message::Binary(pong.into())).await {
                                    tracing::warn!("failed to send SPDY PING response: {e}");
                                }
                            }
                            Ok(Frame::SynStream { stream_id, headers, fin }) => {
                                tracing::debug!(
                                    stream_id,
                                    num_headers = headers.len(),
                                    fin,
                                    "SPDY server-initiated SynStream (ignored for port-forward)"
                                );
                            }
                            Ok(Frame::Unknown) => {}
                            Err(e) => {
                                tracing::warn!("SPDY decode error: {e}");
                            }
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
