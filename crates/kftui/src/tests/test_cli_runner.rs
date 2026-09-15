use std::time::Duration;

use crate::cli::runner::PortForwardRunner;

/// A stop still in flight when `drain_stop_tasks`'s deadline elapses must
/// keep running to completion, detached, rather than being aborted: only the
/// future joining its `JoinHandle` is dropped, not the spawned task doing
/// the actual stop work (releasing a relay, an address claim, etc).
/// `stop_all_port_forwards` relies on this by spawning every stop with
/// `tokio::spawn` before handing the handles to `drain_stop_tasks`.
#[tokio::test(start_paused = true)]
async fn drain_stop_tasks_detaches_unfinished_spawned_stops_when_the_deadline_elapses() {
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    let handle = tokio::spawn(async move {
        release_rx.await.expect("release must be sent");
        done_tx.send(()).expect("receiver must still be listening");
        Ok::<(), String>(())
    });

    let (results, completed_ids, drained_ok) =
        PortForwardRunner::drain_stop_tasks(vec![(1, handle)], Duration::from_millis(50)).await;

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
    let handle = tokio::spawn(async { Ok::<(), String>(()) });

    let (results, completed_ids, drained_ok) =
        PortForwardRunner::drain_stop_tasks(vec![(7, handle)], Duration::from_secs(5)).await;

    assert!(drained_ok);
    assert_eq!(results, vec![(7, Ok(()))]);
    assert!(completed_ids.contains(&7));
}

/// Every handle passed to `drain_stop_tasks` was already spawned by the
/// caller before the call, so a slow sibling timing out must not discard a
/// result a faster stop already reported: the two run concurrently on the
/// runtime from the moment they were spawned, not one after another gated by
/// how `drain_stop_tasks` happens to await them.
#[tokio::test(start_paused = true)]
async fn drain_stop_tasks_preserves_a_finished_result_when_a_later_sibling_times_out() {
    let fast_handle = tokio::spawn(async { Ok::<(), String>(()) });

    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let slow_handle = tokio::spawn(async move {
        release_rx.await.expect("release must be sent");
        Ok::<(), String>(())
    });

    let (results, completed_ids, drained_ok) = PortForwardRunner::drain_stop_tasks(
        vec![(1, fast_handle), (2, slow_handle)],
        Duration::from_millis(50),
    )
    .await;

    assert!(!drained_ok, "the slow stop must be reported as unfinished");
    assert_eq!(
        results,
        vec![(1, Ok(()))],
        "the fast stop's result must survive its slower sibling timing out"
    );
    assert!(completed_ids.contains(&1));
    assert!(
        !completed_ids.contains(&2),
        "the slow stop must not be reported as completed"
    );

    let _ = release_tx.send(());
}
