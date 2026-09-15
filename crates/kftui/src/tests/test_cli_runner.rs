use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::stream;

use crate::cli::runner::PortForwardRunner;

/// A stop still in flight when `drain_stop_tasks`'s deadline elapses must be
/// dropped immediately, not kept alive until the caller's own later cleanup
/// step (`reconcile_shutdown_cleanup`) runs and this function's locals
/// finally go out of scope: a dropped future releases whatever it holds
/// (e.g. the per-config recovery lock `stop_config` acquires), while one
/// still alive keeps holding it and can make an unrelated reconcile pass for
/// the same id contend for that same lock.
#[tokio::test(start_paused = true)]
async fn drain_stop_tasks_drops_unfinished_futures_when_the_deadline_elapses() {
    let dropped = Arc::new(AtomicBool::new(false));

    struct DropSignal(Arc<AtomicBool>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let signal = DropSignal(dropped.clone());
    let stuck = Box::pin(stream::once(async move {
        // Held for as long as this future is alive; released only when it is
        // dropped, never by returning normally.
        let _signal = signal;
        std::future::pending::<()>().await;
        #[allow(unreachable_code)]
        (1i64, Ok(()))
    }));

    let (results, completed_ids, drained_ok) =
        PortForwardRunner::drain_stop_tasks(stuck, Duration::from_millis(50)).await;

    assert!(!drained_ok, "the deadline must be reported as exceeded");
    assert!(
        results.is_empty(),
        "a never-finishing stop reports no result"
    );
    assert!(completed_ids.is_empty());
    assert!(
        dropped.load(Ordering::SeqCst),
        "a stop future still in flight when the deadline elapses must be dropped \
         immediately, releasing whatever it holds, instead of staying alive until \
         some later cleanup step finishes"
    );
}

/// Companion to the drop test: a stop that finishes comfortably inside the
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
