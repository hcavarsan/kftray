//! Long-lived port-forward orchestrator.
//!
//! [`Forwarder`] sits above [`Session`] and owns:
//!
//! - a [`PodWatcher`] tracking the currently ready pod,
//! - a bounded pool of [`Session`]s drained-on-pod-change,
//! - prune and prefetch background tasks.
//!
//! Callers get a [`Stream`] from [`Forwarder::connect`] without thinking
//! about pod identity, capacity, or recreation after pod rollover.

mod builder;
mod lifecycle;
mod pool;
mod prefetch;

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
pub use builder::ForwarderBuilder;
use pool::{
    SessionPool,
    drain_and_cancel_all,
};
use tokio::sync::{
    Mutex as TokioMutex,
    RwLock as TokioRwLock,
    Semaphore,
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::client::Client;
use crate::error::Error;
use crate::pod_watch::PodWatcher;
use crate::recovery::RecoveryCallback;
use crate::session::Session;
use crate::stream::Stream;

const DEFAULT_MAX_SESSIONS: usize = 128;
const DEFAULT_SESSION_CAPACITY: usize = 32;
const DEFAULT_PING: Duration = Duration::from_secs(15);
const DEFAULT_WATCHDOG: Duration = Duration::from_secs(30);
const DEFAULT_DRAIN: Duration = Duration::from_secs(2);
const DEFAULT_PRUNE_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_PRUNE_IDLE_AGE: Duration = Duration::from_secs(60);
const DEFAULT_PREFETCH_THRESHOLD: f32 = 0.60;
const READY_POD_WAIT: Duration = Duration::from_secs(5);
const CONNECTION_SLOT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECTION_SLOT_PERMITS: usize = 50;

#[derive(Clone, Copy)]
struct ForwarderConfig {
    max_sessions: usize,
    session_capacity: usize,
    ping_interval: Duration,
    watchdog_timeout: Duration,
    shutdown_grace: Duration,
    prune_interval: Duration,
    prune_idle_age: Duration,
    prefetch_threshold: f32,
}

impl Default for ForwarderConfig {
    fn default() -> Self {
        Self {
            max_sessions: DEFAULT_MAX_SESSIONS,
            session_capacity: DEFAULT_SESSION_CAPACITY,
            ping_interval: DEFAULT_PING,
            watchdog_timeout: DEFAULT_WATCHDOG,
            shutdown_grace: DEFAULT_DRAIN,
            prune_interval: DEFAULT_PRUNE_INTERVAL,
            prune_idle_age: DEFAULT_PRUNE_IDLE_AGE,
            prefetch_threshold: DEFAULT_PREFETCH_THRESHOLD,
        }
    }
}

/// Long-lived port-forward orchestrator. Construct via [`Forwarder::builder`].
pub struct Forwarder {
    pf_client: Arc<Client>,
    namespace: Arc<str>,
    pod_watcher: Arc<PodWatcher>,
    sessions: Arc<TokioRwLock<SessionPool>>,
    /// Lock-free snapshot of live sessions, shared with
    /// `SessionPool::snapshot`. Updated atomically by `refresh_snapshot`
    /// after every pool mutation. Reads (`find_reusable_session`,
    /// `try_reuse_session`) load this without acquiring any lock.
    session_snap: Arc<ArcSwap<Vec<Arc<Session>>>>,
    config: ForwarderConfig,
    cancel: CancellationToken,
    session_cancel: CancellationToken,
    recovery_callback: RecoveryCallback,
    portforward_semaphore: Arc<Semaphore>,
    background_tasks: Arc<TokioMutex<JoinSet<()>>>,
    call_counter: std::sync::atomic::AtomicU64,
    /// Notified when a new session finishes opening. Concurrent callers
    /// waiting for a session wake up and try to reuse the newly created one
    /// instead of each opening their own.
    session_ready: Arc<tokio::sync::Notify>,
}

impl Forwarder {
    /// Acquire a stream to the target pod's `target_port`. Waits up to 5s
    /// for a ready pod on first call. Opens new sessions on demand and
    /// retires drained ones.
    pub async fn connect(&self, target_port: u16) -> Result<Stream, Error> {
        for _ in 0..self.config.max_sessions {
            let session = self.ensure_session(target_port).await?;
            match session.connect().await {
                Ok(s) => return Ok(s),
                // Capacity exhaustion on this session means the pool is saturated;
                // ensure_session() will pick a different one on the next iteration.
                Err(Error::CapacityExhausted { .. }) => {}
                Err(e) => return Err(e),
            }
        }
        Err(Error::CapacityExhausted {
            in_use: 0,
            capacity: self.config.max_sessions,
        })
    }

    /// Cancellation token tripped by [`Forwarder::shutdown`].
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn ready_pod(&self) -> Option<String> {
        self.pod_watcher.ready_pod().map(|p| p.name)
    }

    /// Cancel background tasks, drain all sessions.
    pub async fn shutdown(self) -> Result<(), Error> {
        self.pod_watcher.shutdown();
        self.cancel.cancel();
        self.session_cancel.cancel();
        drain_and_cancel_all(&self.sessions).await;
        let mut set = self.background_tasks.lock().await;
        set.abort_all();
        while let Some(result) = set.join_next().await {
            match result {
                Err(e) if e.is_panic() => {
                    tracing::warn!("background task panicked during shutdown: {e}");
                }
                Err(e) if !e.is_cancelled() => {
                    tracing::warn!("background task failed during shutdown: {e}");
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_sane() {
        let c = ForwarderConfig::default();
        assert_eq!(c.max_sessions, 128);
        assert_eq!(c.session_capacity, 32);
        assert!(c.prefetch_threshold > 0.0 && c.prefetch_threshold < 1.0);
    }
}
