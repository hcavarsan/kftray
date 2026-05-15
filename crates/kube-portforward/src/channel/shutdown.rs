use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tungstenite::Message;

use crate::error::Error;

pub(crate) async fn shutdown(
    writer_mailbox: mpsc::Sender<Message>, cancel: CancellationToken,
    join_set: &mut JoinSet<Result<(), Error>>, drain_timeout: Duration,
) {
    let _ = tokio::time::timeout(
        Duration::from_millis(100),
        writer_mailbox.send(Message::Close(None)),
    )
    .await;

    cancel.cancel();

    let drain = async {
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Err(e)) => tracing::error!("background task returned error during drain: {e}"),
                Err(e) if e.is_panic() => {
                    tracing::error!("background task panicked during drain: {e}")
                }
                Err(e) if !e.is_cancelled() => {
                    tracing::error!("background task join error during drain: {e}")
                }
                _ => {}
            }
        }
    };

    if tokio::time::timeout(drain_timeout, drain).await.is_err() {
        let pending = join_set.len();
        join_set.abort_all();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Err(e)) => tracing::error!("aborted task returned error: {e}"),
                Err(e) if e.is_panic() => tracing::error!("aborted task panicked: {e}"),
                _ => {}
            }
        }
        tracing::warn!(
            "kube-portforward graceful shutdown timed out; aborted {} pending tasks",
            pending
        );
    }
}
