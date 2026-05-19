use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{
    Mutex as TokioMutex,
    RwLock as TokioRwLock,
    Semaphore,
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::pool::SessionPool;
use super::{
    CONNECTION_SLOT_PERMITS,
    Forwarder,
    ForwarderConfig,
};
use crate::client::Client;
use crate::error::Error;
use crate::pod_watch::{
    PodSelector,
    PodWatcher,
};
use crate::recovery::{
    RecoveryCallback,
    RecoverySignal,
};

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
        self.config.ping_interval = ping;
        self.config.watchdog_timeout = watchdog;
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

        let pool = SessionPool::new();
        let session_snap = Arc::clone(&pool.snapshot);
        let forwarder = Forwarder {
            pf_client,
            namespace: Arc::from(self.namespace),
            pod_watcher,
            sessions: Arc::new(TokioRwLock::new(pool)),
            session_snap,
            config: self.config,
            cancel,
            session_cancel: CancellationToken::new(),
            recovery_callback,
            portforward_semaphore: Arc::new(Semaphore::new(CONNECTION_SLOT_PERMITS)),
            background_tasks: Arc::new(TokioMutex::new(JoinSet::new())),
            call_counter: std::sync::atomic::AtomicU64::new(0),
            session_ready: Arc::new(tokio::sync::Notify::new()),
        };
        forwarder.spawn_prune().await;
        forwarder.spawn_pod_change_reactor().await;
        Ok(forwarder)
    }
}
