use futures::SinkExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tungstenite::Message;

use crate::error::Error;

const BATCH_CAP: usize = 32;

pub(crate) async fn writer_task<S>(
    mut sink: S, mut rx: mpsc::Receiver<Message>, cancel: CancellationToken,
) -> Result<(), Error>
where
    S: SinkExt<Message, Error = tungstenite::Error> + Unpin + Send,
{
    let mut batch: Vec<Message> = Vec::with_capacity(BATCH_CAP);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let _ = sink.close().await;
                return Ok(());
            }
            n = rx.recv_many(&mut batch, BATCH_CAP) => {
                if n == 0 {
                    let _ = sink.close().await;
                    return Ok(());
                }
                for msg in batch.drain(..) {
                    if let Err(e) = sink.feed(msg).await {
                        tracing::warn!(error = %e, "WebSocket sink feed error, closing writer");
                        let _ = sink.close().await;
                        return Ok(());
                    }
                }
                if let Err(e) = sink.flush().await {
                    tracing::warn!(error = %e, "WebSocket sink flush error, closing writer");
                    let _ = sink.close().await;
                    return Ok(());
                }
            }
        }
    }
}
