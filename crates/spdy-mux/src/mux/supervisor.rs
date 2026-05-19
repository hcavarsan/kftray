use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Watches all task handles (1 writer + 1 reader + 5 workers = 7 total).
/// When any task exits, cancels the session.
///
/// # Panic recovery
///
/// When any task panics (`JoinError::is_panic()`):
/// - Logs the panic with `error!`
/// - Marks session poisoned (cancel token)
/// - Does NOT restart. A panic kills the session permanently. Restart at the
///   Forwarder layer.
pub(super) async fn supervise(
    writer_handle: JoinHandle<&'static str>, reader_handle: JoinHandle<&'static str>,
    worker_handles: Vec<JoinHandle<&'static str>>, cancel: CancellationToken,
) {
    let mut tasks = tokio::task::JoinSet::new();

    tasks.spawn(async move {
        match writer_handle.await {
            Ok(name) => name,
            Err(e) => {
                if e.is_panic() {
                    tracing::error!("SPDY writer panicked — session poisoned, no restart: {e}");
                }
                "writer"
            }
        }
    });
    tasks.spawn(async move {
        match reader_handle.await {
            Ok(name) => name,
            Err(e) => {
                if e.is_panic() {
                    tracing::error!("SPDY reader panicked — session poisoned, no restart: {e}");
                }
                "reader"
            }
        }
    });
    for (i, wh) in worker_handles.into_iter().enumerate() {
        tasks.spawn(async move {
            match wh.await {
                Ok(name) => name,
                Err(e) => {
                    if e.is_panic() {
                        tracing::error!(
                            worker_id = i,
                            "SPDY frame worker panicked — session poisoned, no restart: {e}"
                        );
                    }
                    "worker"
                }
            }
        });
    }

    if let Some(result) = tasks.join_next().await {
        match result {
            Ok(name) => tracing::debug!("SPDY supervisor: {name} exited first"),
            Err(e) => tracing::debug!("SPDY supervisor: task join error: {e}"),
        }
    }

    cancel.cancel();
}
