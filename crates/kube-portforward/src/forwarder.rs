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

use std::sync::Arc;
use std::time::Duration;

use quanta::Instant;
use tokio::sync::{
    Mutex as TokioMutex,
    RwLock as TokioRwLock,
    Semaphore,
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::client::Client;
use crate::error::Error;
use crate::channel::keepalive::{
    RecoveryCallback,
    RecoverySignal,
};
use crate::pod_watch::{
    PodChange,
    PodSelector,
    PodWatcher,
};
use crate::session::Session;
use crate::stream::Stream;

/// Drain all sessions from the pool under a write lock, then cancel each
/// session's cancellation token outside the lock.
async fn drain_and_cancel_all(sessions: &TokioRwLock<SessionPool>) {
    let drained: Vec<PooledSession> = {
        let mut pool = sessions.write().await;
        pool.target_pod_uid = None;
        pool.entries.drain(..).collect()
    };
    for pooled in drained {
        pooled.session.cancellation_token().cancel();
    }
}

const DEFAULT_MAX_SESSIONS: usize = 128;
const DEFAULT_SESSION_CAPACITY: usize = 32;
const DEFAULT_PING: Duration = Duration::from_secs(15);
const DEFAULT_WATCHDOG: Duration = Duration::from_secs(30);
const DEFAULT_DRAIN: Duration = Duration::from_secs(2);
const DEFAULT_PRUNE_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_PRUNE_IDLE_AGE: Duration = Duration::from_secs(60);
const DEFAULT_PREFETCH_THRESHOLD: f32 = 0.75;
const READY_POD_WAIT: Duration = Duration::from_secs(5);
const CONNECTION_SLOT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECTION_SLOT_PERMITS: usize = 50;

#[derive(Clone, Copy)]
struct KeepaliveConfig {
    ping: Duration,
    watchdog: Duration,
}

#[derive(Clone, Copy)]
struct ForwarderConfig {
    max_sessions: usize,
    session_capacity: usize,
    keepalive: KeepaliveConfig,
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
            keepalive: KeepaliveConfig {
                ping: DEFAULT_PING,
                watchdog: DEFAULT_WATCHDOG,
            },
            shutdown_grace: DEFAULT_DRAIN,
            prune_interval: DEFAULT_PRUNE_INTERVAL,
            prune_idle_age: DEFAULT_PRUNE_IDLE_AGE,
            prefetch_threshold: DEFAULT_PREFETCH_THRESHOLD,
        }
    }
}

struct PooledSession {
    session: Arc<Session>,
    created_at: Instant,
    #[allow(dead_code)]
    pod_uid: String,
}

struct SessionPool {
    entries: Vec<PooledSession>,
    target_pod_uid: Option<String>,
    prefetch_in_flight: bool,
    opening_count: usize,
}

impl SessionPool {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            target_pod_uid: None,
            prefetch_in_flight: false,
            opening_count: 0,
        }
    }
}

/// Long-lived port-forward orchestrator. Construct via [`Forwarder::builder`].
pub struct Forwarder {
    pf_client: Arc<Client>,
    namespace: Arc<str>,
    pod_watcher: Arc<PodWatcher>,
    sessions: Arc<TokioRwLock<SessionPool>>,
    config: ForwarderConfig,
    cancel: CancellationToken,
    session_cancel: CancellationToken,
    recovery_callback: RecoveryCallback,
    portforward_semaphore: Arc<Semaphore>,
    background_tasks: Arc<TokioMutex<JoinSet<()>>>,
    call_counter: std::sync::atomic::AtomicU64,
}

impl Forwarder {
    pub fn builder(
        kube_client: kube::Client, cluster_url: http::Uri, namespace: impl Into<String>,
    ) -> ForwarderBuilder {
        ForwarderBuilder {
            kube_client,
            cluster_url,
            namespace: namespace.into(),
            selector: None,
            config: ForwarderConfig::default(),
            cancel: None,
            recovery_callback: None,
        }
    }

    /// Acquire a stream to the target pod's `target_port`. Waits up to 5s
    /// for a ready pod on first call. Opens new sessions on demand and
    /// retires drained ones.
    pub async fn connect(&self, target_port: u16) -> Result<Stream, Error> {
        for _ in 0..self.config.max_sessions {
            let session = self.ensure_session(target_port).await?;
            match session.connect().await {
                Ok(s) => return Ok(s),
                Err(Error::CapacityExhausted { .. }) => continue,
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

    async fn ensure_session(&self, target_port: u16) -> Result<Arc<Session>, Error> {
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

        {
            let mut pool = self.sessions.write().await;
            if pool.target_pod_uid.as_deref() != Some(pod_uid.as_str()) {
                let drained: Vec<_> = pool.entries.drain(..).collect();
                pool.target_pod_uid = Some(pod_uid.clone());
                drop(pool);
                for pooled in drained {
                    pooled.session.cancellation_token().cancel();
                }
            }
        }

        if let Some(s) = self
            .try_reuse_session(target_port, &pod_uid, &ready.name)
            .await
        {
            return Ok(s);
        }

        self.retire_dead_sessions().await;

        if let Some(s) = self.find_reusable_session().await {
            self.maybe_prefetch(&s, target_port, ready.name.clone(), pod_uid)
                .await;
            return Ok(s);
        }

        self.reserve_new_slot().await?;

        let permit = tokio::time::timeout(
            CONNECTION_SLOT_TIMEOUT,
            self.portforward_semaphore.acquire(),
        )
        .await;
        let _permit = match permit {
            Ok(Ok(p)) => p,
            Ok(Err(_)) => {
                self.sessions.write().await.opening_count -= 1;
                return Err(Error::Network("connection slot semaphore closed".into()));
            }
            Err(_) => {
                self.sessions.write().await.opening_count -= 1;
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

        let mut pool = self.sessions.write().await;
        pool.opening_count -= 1;
        let session = Arc::new(open_result?);
        pool.entries.push(PooledSession {
            session: Arc::clone(&session),
            created_at: Instant::now(),
            pod_uid: pod_uid.clone(),
        });
        if pool.target_pod_uid.is_none() {
            pool.target_pod_uid = Some(pod_uid.clone());
        }
        drop(pool);
        self.maybe_prefetch(&session, target_port, ready.name, pod_uid)
            .await;
        tracing::info!(
            call_id,
            elapsed_ms = t_total.elapsed().as_millis() as u64,
            "ensure_session: total"
        );
        Ok(session)
    }

    /// Snapshot sessions under lock, drop the lock, then check `is_drained()`
    /// on each. Re-acquire to swap in only alive sessions.
    async fn retire_dead_sessions(&self) {
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
            retired_sessions
        };

        for retired_session in retired {
            retired_session.cancellation_token().cancel();
        }
    }

    /// Snapshot non-cancelled session handles under write guard, drop it,
    /// then check `is_full()` on each. Return first non-full session.
    async fn find_reusable_session(&self) -> Option<Arc<Session>> {
        let candidates: Vec<Arc<Session>> = {
            let pool = self.sessions.read().await;
            pool.entries
                .iter()
                .filter(|e| !e.session.cancellation_token().is_cancelled())
                .map(|e| Arc::clone(&e.session))
                .collect()
        };

        for session in candidates {
            if !session.is_full() {
                return Some(session);
            }
        }
        None
    }

    /// Reserve a slot for a new session by incrementing `opening_count`.
    async fn reserve_new_slot(&self) -> Result<(), Error> {
        let mut pool = self.sessions.write().await;
        let projected = pool.entries.len() + pool.opening_count;
        if projected >= self.config.max_sessions {
            return Err(Error::CapacityExhausted {
                in_use: projected,
                capacity: self.config.max_sessions,
            });
        }
        pool.opening_count += 1;
        Ok(())
    }

    fn next_call_id(&self) -> u64 {
        self.call_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Snapshot session handles under read guard, drop it, then check
    /// `is_full()` on each outside the lock.
    async fn try_reuse_session(
        &self, target_port: u16, pod_uid: &str, pod_name: &str,
    ) -> Option<Arc<Session>> {
        let candidates: Vec<Arc<Session>> = {
            let pool = self.sessions.read().await;
            pool.entries
                .iter()
                .filter(|e| !e.session.cancellation_token().is_cancelled())
                .map(|e| Arc::clone(&e.session))
                .collect()
        };

        for session in candidates {
            if !session.is_full() {
                self.maybe_prefetch(
                    &session,
                    target_port,
                    pod_name.to_string(),
                    pod_uid.to_string(),
                )
                .await;
                return Some(session);
            }
        }
        None
    }

    async fn open_session(&self, pod_name: &str, port: u16) -> Result<Session, Error> {
        let cb = Arc::clone(&self.recovery_callback);
        self.pf_client
            .session(&*self.namespace, pod_name, port)
            .capacity(self.config.session_capacity)
            .keepalive(self.config.keepalive.ping, self.config.keepalive.watchdog)
            .shutdown_grace(self.config.shutdown_grace)
            .cancellation_token(self.session_cancel.child_token())
            .on_recovery(move |signal: RecoverySignal| (cb)(signal))
            .open()
            .await
    }

    async fn maybe_prefetch(
        &self, session: &Arc<Session>, target_port: u16, pod_name: String, pod_uid: String,
    ) {
        let capacity = session.capacity();
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
                .keepalive(config.keepalive.ping, config.keepalive.watchdog)
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
                    } else {
                        drop(pool);
                        new.cancellation_token().cancel();
                    }
                }
                Err(e) => debug!("forwarder: prefetch failed: {}", e),
            }
        });
    }

    async fn spawn_prune(&self) {
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

    async fn spawn_pod_change_reactor(&self) {
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
    drop(pool);
    for dropped_session in dropped {
        dropped_session.session.cancellation_token().cancel();
    }
}

/// Builder for [`Forwarder`].
pub struct ForwarderBuilder {
    kube_client: kube::Client,
    cluster_url: http::Uri,
    namespace: String,
    selector: Option<PodSelector>,
    config: ForwarderConfig,
    cancel: Option<CancellationToken>,
    recovery_callback: Option<RecoveryCallback>,
}

impl ForwarderBuilder {
    pub fn pod_selector(mut self, sel: PodSelector) -> Self {
        self.selector = Some(sel);
        self
    }

    pub fn max_sessions(mut self, n: usize) -> Self {
        self.config.max_sessions = n;
        self
    }

    pub fn session_capacity(mut self, n: usize) -> Self {
        self.config.session_capacity = n;
        self
    }

    pub fn keepalive(mut self, ping: Duration, watchdog: Duration) -> Self {
        self.config.keepalive = KeepaliveConfig { ping, watchdog };
        self
    }

    pub fn shutdown_grace(mut self, drain: Duration) -> Self {
        self.config.shutdown_grace = drain;
        self
    }

    pub fn prune(mut self, interval: Duration, idle_age: Duration) -> Self {
        self.config.prune_interval = interval;
        self.config.prune_idle_age = idle_age;
        self
    }

    pub fn prefetch_threshold(mut self, ratio: f32) -> Self {
        self.config.prefetch_threshold = ratio.clamp(0.0, 1.0);
        self
    }

    pub fn cancellation_token(mut self, t: CancellationToken) -> Self {
        self.cancel = Some(t);
        self
    }

    pub fn on_recovery<F>(mut self, cb: F) -> Self
    where
        F: Fn(RecoverySignal) + Send + Sync + 'static,
    {
        self.recovery_callback = Some(Arc::new(cb));
        self
    }

    pub async fn build(self) -> Result<Forwarder, Error> {
        if self.config.max_sessions == 0 {
            return Err(Error::Configuration("max_sessions must be > 0".into()));
        }
        if self.config.session_capacity == 0 {
            return Err(Error::Configuration("session_capacity must be > 0".into()));
        }
        let selector = self
            .selector
            .ok_or_else(|| Error::Configuration("pod_selector is required".into()))?;
        let pod_watcher =
            Arc::new(PodWatcher::new(self.kube_client.clone(), &self.namespace, selector).await?);
        let pf_client = Arc::new(Client::new(self.kube_client, self.cluster_url));
        let cancel = self.cancel.unwrap_or_default();
        let recovery_callback: RecoveryCallback =
            self.recovery_callback.unwrap_or_else(|| Arc::new(|_| {}));

        let forwarder = Forwarder {
            pf_client,
            namespace: Arc::from(self.namespace),
            pod_watcher,
            sessions: Arc::new(TokioRwLock::new(SessionPool::new())),
            config: self.config,
            cancel,
            session_cancel: CancellationToken::new(),
            recovery_callback,
            portforward_semaphore: Arc::new(Semaphore::new(CONNECTION_SLOT_PERMITS)),
            background_tasks: Arc::new(TokioMutex::new(JoinSet::new())),
            call_counter: std::sync::atomic::AtomicU64::new(0),
        };
        forwarder.spawn_prune().await;
        forwarder.spawn_pod_change_reactor().await;
        Ok(forwarder)
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
