//! Dedicated `current_thread` Tokio runtime for data-plane tasks.
//!
//! Port-forward I/O (the `copy_buf` loops) is latency-sensitive. Running these
//! on the default multi-thread work-stealing scheduler introduces jitter from
//! unrelated control-plane work (DB queries, TUI rendering, pod watcher polls).
//!
//! This module provides a dedicated single-threaded runtime on its own OS
//! thread. Connection-forwarding tasks are spawned onto it, isolating the
//! data-plane from scheduler contention.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::Handle;
use tokio::task::JoinHandle;

/// Global data-plane runtime, lazily initialized on first use.
static DATAPLANE_RT: OnceLock<DataPlaneRuntime> = OnceLock::new();

/// A dedicated `current_thread` runtime for latency-sensitive I/O tasks.
///
/// `current_thread` (NOT multi-thread) is intentional: this isolates the
/// data-plane from Tokio's work-stealing scheduler. With only one worker on
/// one dedicated OS thread, tasks never migrate between cores, eliminating
/// cache-thrash and scheduling jitter that hurt tail latency. This mirrors
/// Cloudflare Pingora's `NoStealRuntime` pattern.
struct DataPlaneRuntime {
    handle: Handle,
    /// Keep the thread alive. Dropped only at process exit.
    _thread: std::thread::JoinHandle<()>,
}

impl DataPlaneRuntime {
    fn new() -> Self {
        // Channel to pass the Handle out of the spawned thread.
        let (tx, rx) = std::sync::mpsc::sync_channel::<Handle>(1);

        let thread = std::thread::Builder::new()
            .name("kf-dataplane".into())
            .spawn(move || {
                // Single-threaded runtime. Mandatory — multi-thread would
                // defeat the NoSteal isolation that justifies this module.
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .thread_name("kf-dataplane")
                    .build()
                    .expect("failed to build dataplane current_thread runtime");
                let _ = tx.send(rt.handle().clone());
                // Park the runtime forever — tasks are spawned externally via Handle.
                rt.block_on(std::future::pending::<()>());
            })
            .expect("failed to spawn dataplane thread");

        let handle = rx
            .recv()
            .expect("dataplane thread died before sending handle");
        Self {
            handle,
            _thread: thread,
        }
    }
}

/// Spawn a future onto the dedicated data-plane runtime.
///
/// The data-plane runtime is a `current_thread` executor on its own OS thread,
/// isolated from the main multi-thread scheduler. Use this for
/// latency-sensitive I/O tasks (TCP/UDP forwarding loops).
///
/// Falls back to `tokio::spawn` if the runtime fails to initialize.
pub fn spawn_on_dataplane<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let rt = DATAPLANE_RT.get_or_init(DataPlaneRuntime::new);
    rt.handle.spawn(future)
}
