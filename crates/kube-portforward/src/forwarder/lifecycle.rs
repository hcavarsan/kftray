use std::sync::Arc;
use std::time::Duration;

use quanta::Instant;
use tracing::debug;

use super::pool::{
    PooledSession,
    drain_and_cancel_all,
};
use super::{
    CONNECTION_SLOT_TIMEOUT,
    Forwarder,
    READY_POD_WAIT,
};
use crate::error::Error;
use crate::pod_watch::PodChange;
use crate::recovery::RecoverySignal;
use crate::session::Session;

impl Forwarder {
    pub(super) async fn ensure_session(&self, target_port: u16) -> Result<Arc<Session>, Error> {
        let call_id = self.next_call_id();
        let t_total = Instant::now();
        let t0 = Instant::now();
        let ready = self
            .pod_watcher
            .wait_for_ready_pod(READY_POD_WAIT)
            .await
            .ok_or_else(|| Error::Configuration("no ready pod available".into()))?;
        tracing::info!(
            call_id,
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "ensure_session: ready_pod resolved"
        );
        let pod_uid = ready.uid.clone().unwrap_or_else(|| ready.name.clone());

        // Fast path: check under read lock.
        let needs_drain = {
            let pool = self.sessions.read().await;
            pool.target_pod_uid.as_deref() != Some(pod_uid.as_str())
        };
        if needs_drain {
            let drained: Vec<_> = {
                let mut pool = self.sessions.write().await;
                // Re-check under write lock (another task may have already drained).
                if pool.target_pod_uid.as_deref() == Some(pod_uid.as_str()) {
                    Vec::new()
                } else {
                    pool.target_pod_uid = Some(pod_uid.clone());
                    let drained = pool.entries.drain(..).collect();
                    pool.refresh_snapshot();
                    drained
                }
            };
            for pooled in drained {
                pooled.session.cancellation_token().cancel();
            }
        }

        // Fast path: reuse an existing non-full session.
        if let Some(s) = self
            .try_reuse_session(target_port, &pod_uid, &ready.name)
            .await
        {
            return Ok(s);
        }

        // Coalescing gate: if another caller is already opening a session,
        // wait for it instead of opening a duplicate. Without this, N
        // concurrent callers each create their own session (N × 4 WS
        // connections) when one session can serve them all.
        //
        // CRITICAL: The `Notified` future MUST be created while holding
        // the pool lock (before `drop(pool)`). If created after the lock
        // is dropped, `notify_waiters()` can fire in the gap between
        // `drop(pool)` and `notified()`, causing the notification to be
        // missed and the caller to wait the full timeout.
        {
            let pool = self.sessions.read().await;
            if pool.has_pending_or_available() {
                // Register the waiter BEFORE releasing the lock. Creating
                // the Notified future alone is not enough — registration
                // happens on first poll, not on creation. enable() forces
                // registration so any notify_waiters() that fires between
                // drop(pool) and the .await is captured.
                let notified = self.session_ready.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                drop(pool);
                let _ = tokio::time::timeout(Duration::from_secs(5), notified).await;
                // Re-check: the newly created session should be available now.
                if let Some(s) = self
                    .try_reuse_session(target_port, &pod_uid, &ready.name)
                    .await
                {
                    return Ok(s);
                }
                // Fall through to create a new session if the in-flight one
                // failed or was full.
            }
        }

        self.retire_dead_sessions().await;

        if let Some(s) = self.find_reusable_session() {
            self.maybe_prefetch(&s, target_port, ready.name.clone(), pod_uid)
                .await;
            return Ok(s);
        }

        // RAII slot: decrements opening_count on drop, cancel-safe.
        let _slot = self.reserve_new_slot().await?;

        let permit = tokio::time::timeout(
            CONNECTION_SLOT_TIMEOUT,
            self.portforward_semaphore.acquire(),
        )
        .await;
        let _permit = match permit {
            Ok(Ok(p)) => p,
            Ok(Err(_)) => {
                return Err(Error::Network("connection slot semaphore closed".into()));
            }
            Err(_) => {
                return Err(Error::Network(
                    "timed out waiting for connection slot".into(),
                ));
            }
        };

        let t_open = Instant::now();
        let open_result = self.open_session(&ready.name, target_port).await;
        tracing::info!(
            call_id,
            elapsed_ms = t_open.elapsed().as_millis() as u64,
            outcome = if open_result.is_ok() { "ok" } else { "err" },
            "ensure_session: open_session done"
        );

        let session = Arc::new(open_result?);
        let mut pool = self.sessions.write().await;
        pool.entries.push(PooledSession {
            session: Arc::clone(&session),
            created_at: Instant::now(),
            pod_uid: pod_uid.clone(),
        });
        if pool.target_pod_uid.is_none() {
            pool.target_pod_uid = Some(pod_uid.clone());
        }
        pool.refresh_snapshot();
        drop(pool);

        // Wake all callers waiting at the coalescing gate so they can
        // reuse this session instead of opening more.
        self.session_ready.notify_waiters();

        self.maybe_prefetch(&session, target_port, ready.name, pod_uid)
            .await;
        tracing::info!(
            call_id,
            elapsed_ms = t_total.elapsed().as_millis() as u64,
            "ensure_session: total"
        );
        Ok(session)
    }

    pub(super) async fn open_session(&self, pod_name: &str, port: u16) -> Result<Session, Error> {
        let cb = Arc::clone(&self.recovery_callback);
        self.pf_client
            .session(&*self.namespace, pod_name, port)
            .capacity(self.config.session_capacity)
            .keepalive(self.config.ping_interval, self.config.watchdog_timeout)
            .shutdown_grace(self.config.shutdown_grace)
            .cancellation_token(self.session_cancel.child_token())
            .on_recovery(move |signal: RecoverySignal| (cb)(signal))
            .open()
            .await
    }

    pub(super) async fn spawn_pod_change_reactor(&self) {
        let mut rx = self.pod_watcher.subscribe();
        let sessions = Arc::clone(&self.sessions);
        let cancel = self.cancel.clone();
        let recovery_cb = Arc::clone(&self.recovery_callback);
        self.background_tasks.lock().await.spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    ev = rx.recv() => match ev {
                        Ok(PodChange::Died(name)) => {
                            debug!("forwarder: pod {} died, draining sessions", name);
                            drain_and_cancel_all(&sessions).await;
                            (recovery_cb)(RecoverySignal::ServerClose);
                        }
                        Ok(PodChange::Ready(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        });
    }
}
