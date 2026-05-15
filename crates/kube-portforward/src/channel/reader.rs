use futures::StreamExt;
use futures::stream::SplitStream;
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_tungstenite::WebSocketStream;
use tokio_util::sync::CancellationToken;
use tungstenite::Message;

use super::frame;
use super::keepalive::{
    KeepaliveHandle,
    RecoveryCallback,
    RecoverySignal,
};
use super::routing::Router;
use crate::error::Error;
use crate::subprotocol::Subprotocol;

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_reader(
    subprotocol: Subprotocol, stream: SplitStream<WebSocketStream<TokioIo<Upgraded>>>,
    router: Router, writer_mailbox: mpsc::Sender<Message>, cancel: CancellationToken,
    keepalive: KeepaliveHandle, recovery_callback: RecoveryCallback,
    join_set: &mut JoinSet<Result<(), Error>>,
) {
    join_set.spawn(async move {
        let mut stream = stream;
        keepalive.arm();
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(()),
                maybe_msg = stream.next() => {
                    match maybe_msg {
                        None => return Ok(()),
                        Some(Err(e)) => {
                            tracing::warn!(?subprotocol, "reader: WS stream error: {}", e);
                            // Note: tungstenite::Error is flattened to String because
                            // RecoverySignal::NetworkError carries String. Changing this
                            // would require RecoverySignal to carry a boxed error source.
                            recovery_callback(RecoverySignal::NetworkError(e.to_string()));
                            cancel.cancel();
                            return Ok(());
                        }
                        Some(Ok(Message::Binary(payload))) => {
                            if subprotocol == Subprotocol::V5
                                && let Some(closed_channel) = frame::parse_close_signal(&payload)
                            {
                                tracing::debug!("v5 reader: server CLOSE on channel {}", closed_channel);
                                router.remove(closed_channel);
                                continue;
                            }
                            keepalive.note_pong();
                            match frame::split_channel_byte(payload) {
                                Ok((channel, body)) => {
                                    router.dispatch(channel, body).await;
                                }
                                Err(e) => {
                                    tracing::warn!(?subprotocol, "reader: invalid frame: {}", e);
                                }
                            }
                        }
                        Some(Ok(Message::Ping(p))) => {
                            tokio::select! {
                                _ = writer_mailbox.send(Message::Pong(p)) => {}
                                _ = cancel.cancelled() => return Ok(()),
                            }
                        }
                        Some(Ok(Message::Pong(_))) => {
                            keepalive.note_pong();
                        }
                        Some(Ok(Message::Close(_))) => {
                            tracing::debug!(?subprotocol, "reader: server sent Close");
                            recovery_callback(RecoverySignal::ServerClose);
                            cancel.cancel();
                            return Ok(());
                        }
                        Some(Ok(Message::Text(_))) => {
                            tracing::warn!(?subprotocol, "reader: unexpected Text frame; ignoring");
                        }
                        Some(Ok(Message::Frame(_))) => {}
                    }
                }
            }
        }
    });
}
