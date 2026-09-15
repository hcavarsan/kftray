use std::time::Duration;

use futures::stream;

use crate::cli::runner::PortForwardRunner;

/// A stop still in flight when `drain_stop_tasks`'s deadline elapses must
/// keep running to completion, detached, rather than being aborted: only the
/// future joining it inside the drained stream is dropped, not the spawned
/// task doing the actual stop work (releasing a relay, an address claim,
/// etc). `stop_all_port_forwards` relies on this by wrapping every stop in
/// `tokio::spawn` before feeding it to `drain_stop_tasks`.
#[tokio::test(start_paused = true)]
async fn drain_stop_tasks_detaches_unfinished_spawned_stops_when_the_deadline_elapses() {
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    let handle = tokio::spawn(async move {
        release_rx.await.expect("release must be sent");
        done_tx.send(()).expect("receiver must still be listening");
        (1i64, Ok::<(), String>(()))
    });

    let tasks = Box::pin(stream::once(async move {
        match handle.await {
            Ok(item) => item,
            Err(error) => (1i64, Err(error.to_string())),
        }
    }));

    let (results, completed_ids, drained_ok) =
        PortForwardRunner::drain_stop_tasks(tasks, Duration::from_millis(50)).await;

    assert!(!drained_ok, "the deadline must be reported as exceeded");
    assert!(
        results.is_empty(),
        "an unfinished stop reports no result to this caller"
    );
    assert!(completed_ids.is_empty());

    release_tx
        .send(())
        .expect("the spawned stop must still be alive, not aborted, after the deadline");

    tokio::time::timeout(Duration::from_secs(1), done_rx)
        .await
        .expect("must not time out waiting on a still-running detached stop")
        .expect("the detached stop must run to completion and report back");
}

/// Companion to the detach test: a stop that finishes comfortably inside the
/// deadline must still report its result and id normally.
#[tokio::test(start_paused = true)]
async fn drain_stop_tasks_reports_results_that_finish_in_time() {
    let tasks = Box::pin(stream::once(async { (7i64, Ok::<(), String>(())) }));

    let (results, completed_ids, drained_ok) =
        PortForwardRunner::drain_stop_tasks(tasks, Duration::from_secs(5)).await;

    assert!(drained_ok);
    assert_eq!(results, vec![(7, Ok(()))]);
    assert!(completed_ids.contains(&7));
}
