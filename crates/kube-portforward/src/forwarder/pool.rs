use std::sync::Arc;
use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use arc_swap::ArcSwap;
use quanta::Instant;
use tokio::sync::RwLock as TokioRwLock;

use super::Forwarder;
use crate::session::Session;

pub(super) struct PooledSession {
    pub(super) session: Arc<Session>,
    pub(super) created_at: Instant,
    #[expect(
        dead_code,
        reason = "stored for diagnostic logging and future pod-affinity checks"
    )]
    pub(super) pod_uid: String,
}

pub(super) struct SessionPool {
    pub(super) entries: Vec<PooledSession>,
    pub(super) target_pod_uid: Option<String>,
    pub(super) prefetch_in_flight: bool,
    /// Number of sessions currently being opened. Exposed as `Arc<AtomicUsize>`
    /// so [`OpeningSlot`] can decrement it on drop without needing the
    /// `RwLock`, providing cancel-safety for in-flight session opens.
    pub(super) opening_count: Arc<AtomicUsize>,
    /// Lock-free snapshot of live sessions for fast-path reads.
    /// Updated atomically after every mutation to `entries` via
    /// [`refresh_snapshot`]. Readers (`find_reusable_session`,
    /// `try_reuse_session`) load this without acquiring any lock.
    pub(super) snapshot: Arc<ArcSwap<Vec<Arc<Session>>>>,
}

impl SessionPool {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            target_pod_uid: None,
            prefetch_in_flight: false,
            opening_count: Arc::new(AtomicUsize::new(0)),
            snapshot: Arc::new(ArcSwap::from_pointee(Vec::new())),
        }
    }

    /// Rebuild the lock-free snapshot from current entries.
    /// MUST be called after every mutation to `entries` so concurrent
    /// readers see a consistent view.
    pub(super) fn refresh_snapshot(&self) {
        let sessions: Vec<Arc<Session>> = self
            .entries
            .iter()
            .map(|e| Arc::clone(&e.session))
            .collect();
        self.snapshot.store(Arc::new(sessions));
    }

    /// True when there's already a session being created. Concurrent callers
    /// should wait for it instead of opening yet another session.
    pub(super) fn has_pending_or_available(&self) -> bool {
        self.opening_count.load(Ordering::Relaxed) > 0
            || self
                .entries
                .iter()
                .any(|e| !e.session.cancellation_token().is_cancelled())
    }
}

/// RAII slot reservation for an in-flight session open.
///
/// Incrementing `opening_count` is paired with decrement-on-drop so callers
/// that get cancelled mid-await (e.g. dropped future, timeout) cannot leak
/// slots. The slot is held for the duration of the open attempt; on drop
/// (success OR cancellation OR error) the counter is decremented.
#[must_use = "OpeningSlot decrements opening_count on drop; bind to a variable"]
pub(super) struct OpeningSlot {
    counter: Arc<AtomicUsize>,
}

impl OpeningSlot {
    pub(super) fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::AcqRel);
        Self { counter }
    }
}

impl Drop for OpeningSlot {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Drain all sessions from the pool under a write lock, then cancel each
/// session's cancellation token outside the lock.
pub(super) async fn drain_and_cancel_all(sessions: &TokioRwLock<SessionPool>) {
    let drained: Vec<PooledSession> = {
        let mut pool = sessions.write().await;
        pool.target_pod_uid = None;
        let drained = pool.entries.drain(..).collect();
        pool.refresh_snapshot();
        drained
    };
    for pooled in drained {
        pooled.session.cancellation_token().cancel();
    }
}

impl Forwarder {
    /// Snapshot sessions under lock, drop the lock, then check `is_drained()`
    /// on each. Re-acquire to swap in only alive sessions.
    pub(super) async fn retire_dead_sessions(&self) {
        let snapshots: Vec<(Arc<Session>, bool)> = {
            let pool = self.sessions.read().await;
            pool.entries
                .iter()
                .map(|e| {
                    (
                        Arc::clone(&e.session),
                        e.session.cancellation_token().is_cancelled(),
                    )
                })
                .collect()
        };

        let mut dead_indices = Vec::new();
        for (i, (session, cancelled)) in snapshots.iter().enumerate() {
            if *cancelled || session.is_drained() {
                dead_indices.push(i);
            }
        }

        if dead_indices.is_empty() {
            return;
        }

        let retired: Vec<Arc<Session>> = {
            let mut write = self.sessions.write().await;
            let mut retired_sessions = Vec::new();
            let mut alive = Vec::with_capacity(write.entries.len());
            for (i, entry) in write.entries.drain(..).enumerate() {
                if dead_indices.contains(&i) {
                    retired_sessions.push(Arc::clone(&entry.session));
                } else {
                    alive.push(entry);
                }
            }
            write.entries = alive;
            if write.entries.is_empty() {
                write.target_pod_uid = None;
            }
            write.refresh_snapshot();
            retired_sessions
        };

        for retired_session in retired {
            retired_session.cancellation_token().cancel();
        }
    }

    /// Lock-free fast path: scan the snapshot (lock-free atomic load).
    /// The hot queue serves as a recency hint via `refresh_snapshot`, but
    /// reads use the snapshot directly to avoid pop-then-push-back races
    /// that can drop valid sessions when the queue is full.
    pub(super) fn find_reusable_session(&self) -> Option<Arc<Session>> {
        let snap = self.session_snap.load();
        for session in snap.iter() {
            if !session.cancellation_token().is_cancelled() && !session.is_full() {
                return Some(Arc::clone(session));
            }
        }
        None
    }

    /// Lock-free fast path with prefetch trigger.
    pub(super) async fn try_reuse_session(
        &self, target_port: u16, pod_uid: &str, pod_name: &str,
    ) -> Option<Arc<Session>> {
        let snap = self.session_snap.load();
        for session in snap.iter() {
            if !session.cancellation_token().is_cancelled() && !session.is_full() {
                let chosen = Arc::clone(session);
                // Release the snapshot guard before awaiting.
                drop(snap);
                self.maybe_prefetch(
                    &chosen,
                    target_port,
                    pod_name.to_string(),
                    pod_uid.to_string(),
                )
                .await;
                return Some(chosen);
            }
        }
        None
    }

    /// Reserve a slot for a new session, returning an RAII guard that
    /// decrements `opening_count` on drop. This guarantees the slot is
    /// released even if the caller's future is cancelled mid-await.
    pub(super) async fn reserve_new_slot(&self) -> Result<OpeningSlot, crate::error::Error> {
        let pool = self.sessions.write().await;
        let projected = pool.entries.len() + pool.opening_count.load(Ordering::Relaxed);
        if projected >= self.config.max_sessions {
            return Err(crate::error::Error::CapacityExhausted {
                in_use: projected,
                capacity: self.config.max_sessions,
            });
        }
        Ok(OpeningSlot::new(Arc::clone(&pool.opening_count)))
    }

    pub(super) fn next_call_id(&self) -> u64 {
        self.call_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}
