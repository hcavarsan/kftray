use std::sync::Arc;
use std::time::Duration;

use quanta::Instant;
use tokio::sync::RwLock as TokioRwLock;
use tracing::debug;

use super::Forwarder;
use super::pool::{
    PooledSession,
    SessionPool,
};
use crate::recovery::RecoverySignal;
use crate::session::Session;

impl Forwarder {
    pub(super) async fn maybe_prefetch(
        &self, session: &Arc<Session>, target_port: u16, pod_name: String, pod_uid: String,
    ) {
        // Replenish spare streams if below low watermark (non-blocking best-effort).
        // `replenish_spare_streams` internally guards against concurrent runs
        // via a CAS, so spawning multiple tasks is safe but wasteful.
        if session.needs_replenish() && !session.is_full() {
            let session_clone = Arc::clone(session);
            self.background_tasks.lock().await.spawn(async move {
                session_clone.replenish_spare_streams().await;
            });
        }

        // Use operating capacity (scheduling cap) for prefetch threshold,
        // not the hard cap. At pool=6 × operating_max=64 this triggers
        // at ~230 active pairs (0.60 × 384).
        let capacity = session.operating_capacity();
        if capacity == 0 {
            return;
        }
        let in_use = session.in_use();
        if (in_use as f32) < (capacity as f32) * self.config.prefetch_threshold {
            return;
        }

        {
            let mut pool = self.sessions.write().await;
            if pool.prefetch_in_flight
                || pool.entries.len() >= self.config.max_sessions
                || pool.target_pod_uid.as_deref() != Some(pod_uid.as_str())
            {
                return;
            }
            pool.prefetch_in_flight = true;
        }

        let pf_client = Arc::clone(&self.pf_client);
        let sessions = Arc::clone(&self.sessions);
        let namespace = Arc::clone(&self.namespace);
        let session_cancel = self.session_cancel.clone();
        let config = self.config;
        let recovery_cb = Arc::clone(&self.recovery_callback);

        self.background_tasks.lock().await.spawn(async move {
            debug!("forwarder: prefetching session for pod {}", pod_name);
            let cb = Arc::clone(&recovery_cb);
            let result = pf_client
                .session(&*namespace, &pod_name, target_port)
                .capacity(config.session_capacity)
                .keepalive(config.ping_interval, config.watchdog_timeout)
                .shutdown_grace(config.shutdown_grace)
                .cancellation_token(session_cancel.child_token())
                .on_recovery(move |signal: RecoverySignal| (cb)(signal))
                .open()
                .await;

            let mut pool = sessions.write().await;
            pool.prefetch_in_flight = false;
            match result {
                Ok(new) => {
                    let ok = pool.entries.len() < config.max_sessions
                        && pool.target_pod_uid.as_deref() == Some(pod_uid.as_str());
                    if ok {
                        pool.entries.push(PooledSession {
                            session: Arc::new(new),
                            created_at: Instant::now(),
                            pod_uid: pod_uid.clone(),
                        });
                        pool.refresh_snapshot();
                    } else {
                        drop(pool);
                        new.cancellation_token().cancel();
                    }
                }
                Err(e) => debug!("forwarder: prefetch failed: {}", e),
            }
        });
    }

    pub(super) async fn spawn_prune(&self) {
        let sessions = Arc::clone(&self.sessions);
        let cancel = self.cancel.clone();
        let interval_dur = self.config.prune_interval;
        let idle_age = self.config.prune_idle_age;
        self.background_tasks.lock().await.spawn(async move {
            let mut interval = tokio::time::interval(interval_dur);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    _ = interval.tick() => {
                        prune_once(&sessions, idle_age).await;
                    }
                }
            }
        });
    }
}

/// Snapshot session handles + metadata under write guard, drop it, then
/// check `in_use()` on each outside the lock. Re-acquire to remove idle
/// entries.
async fn prune_once(sessions: &Arc<TokioRwLock<SessionPool>>, idle_age: Duration) {
    let snapshots: Vec<(Arc<Session>, Instant, bool)> = {
        let pool = sessions.read().await;
        if pool.entries.is_empty() {
            return;
        }
        pool.entries
            .iter()
            .map(|e| {
                (
                    Arc::clone(&e.session),
                    e.created_at,
                    e.session.cancellation_token().is_cancelled(),
                )
            })
            .collect()
    };

    let total = snapshots.len();
    let mut idle_indices = Vec::new();
    for (i, (session, created_at, cancelled)) in snapshots.iter().enumerate() {
        let aged_idle = session.in_use() == 0 && created_at.elapsed() > idle_age;
        if *cancelled || aged_idle {
            idle_indices.push(i);
        }
    }

    let to_prune = if total > 1 {
        idle_indices.len().min(total - 1)
    } else {
        0
    };
    if to_prune == 0 {
        return;
    }

    let mut pool = sessions.write().await;
    // Re-check: pool may have changed while we were checking in_use()
    if pool.entries.len() != total {
        return;
    }
    let prune_set: std::collections::HashSet<usize> =
        idle_indices.into_iter().take(to_prune).collect();
    let mut dropped = Vec::with_capacity(to_prune);
    let mut kept = Vec::with_capacity(total - to_prune);
    for (i, entry) in pool.entries.drain(..).enumerate() {
        if prune_set.contains(&i) {
            dropped.push(entry);
        } else {
            kept.push(entry);
        }
    }
    pool.entries = kept;
    pool.refresh_snapshot();
    drop(pool);
    for dropped_session in dropped {
        dropped_session.session.cancellation_token().cancel();
    }
}
