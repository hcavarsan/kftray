//! Generic Kubernetes pod watcher.
//!
//! Tracks pod readiness in a namespace using `kube_runtime::reflector` and
//! resolves a [`PodSelector`] (label expression or pod name) to a currently
//! ready pod. Lifecycle events are broadcast on a [`tokio::sync::broadcast`]
//! channel as [`PodChange`].
//!
//! This module is intentionally kftray-agnostic — it knows nothing about
//! services, workload types, or any higher-level configuration.

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwapOption;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    Api,
    ResourceExt,
};
use kube_runtime::{
    WatchStreamExt,
    reflector::{
        self,
        ReflectHandle,
        Store,
    },
    watcher::{
        self,
        Config as WatcherConfig,
    },
};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{
    debug,
    error,
};

use crate::error::Error;

/// Pod selection strategy.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum PodSelector {
    /// Match a single pod by exact name.
    Name(String),
    /// Match pods by a Kubernetes label selector expression
    /// (e.g. `app=nginx,tier=frontend`).
    Labels { selector: String },
}

/// Pod lifecycle change broadcast to subscribers.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum PodChange {
    /// A pod matching the selector became ready (or replaced a previously
    /// ready pod). Carries the new pod name.
    Ready(String),
    /// The previously ready pod was deleted. Carries the dead pod name.
    Died(String),
}

/// Snapshot of the currently ready pod.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReadyPod {
    pub name: String,
    pub uid: Option<String>,
}

impl ReadyPod {
    /// Create a new `ReadyPod` snapshot.
    pub const fn new(name: String, uid: Option<String>) -> Self {
        Self { name, uid }
    }
}

/// Watches pods in a namespace and tracks the currently ready pod matching
/// a [`PodSelector`]. Owns a background reflector task that is aborted on
/// [`PodWatcher::shutdown`].
pub struct PodWatcher {
    store: Store<Pod>,
    _subscriber: ReflectHandle<Pod>,
    latest_ready: Arc<ArcSwapOption<ReadyPod>>,
    change_tx: broadcast::Sender<PodChange>,
    selector: PodSelector,
    reflector_task: JoinHandle<()>,
    subscriber_task: JoinHandle<()>,
    cancel: CancellationToken,
}

impl Drop for PodWatcher {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.reflector_task.abort();
        self.subscriber_task.abort();
    }
}

impl PodWatcher {
    /// Start a watcher against `namespace` selecting pods by `selector`.
    pub async fn new(
        client: kube::Client, namespace: &str, selector: PodSelector,
    ) -> Result<Self, Error> {
        let label_expr = match &selector {
            PodSelector::Labels { selector } => selector.clone(),
            PodSelector::Name(_) => String::new(),
        };

        let (store, writer) = reflector::store_shared(256);
        let subscriber = writer.subscribe().ok_or_else(|| {
            Error::Configuration("failed to create pod reflector subscriber".into())
        })?;

        let cancel = CancellationToken::new();
        let latest_ready: Arc<ArcSwapOption<ReadyPod>> = Arc::new(ArcSwapOption::const_empty());
        let (change_tx, _) = broadcast::channel(16);

        let pods_api: Api<Pod> = Api::namespaced(client, namespace);
        let watcher_config = if label_expr.is_empty() {
            WatcherConfig::default()
        } else {
            WatcherConfig::default().labels(&label_expr)
        };

        let reflector_cancel = cancel.clone();
        let reflector_latest = Arc::clone(&latest_ready);
        let reflector_change_tx = change_tx.clone();
        let reflector_task = tokio::spawn(async move {
            let stream = watcher::watcher(pods_api, watcher_config)
                .default_backoff()
                .modify(|pod| {
                    pod.managed_fields_mut().clear();
                    pod.annotations_mut().clear();
                    if let Some(status) = &mut pod.status {
                        status.container_statuses = None;
                        status.init_container_statuses = None;
                        status.ephemeral_container_statuses = None;
                    }
                })
                .reflect_shared(writer);

            let mut stream = std::pin::pin!(stream);
            loop {
                tokio::select! {
                    biased;
                    () = reflector_cancel.cancelled() => break,
                    next = stream.next() => match next {
                        Some(Ok(watcher::Event::Delete(pod))) => {
                            let name = pod.name_any();
                            let prev = reflector_latest.rcu(|cur| {
                                if cur.as_deref().is_some_and(|c| c.name == name) {
                                    None
                                } else {
                                    cur.clone()
                                }
                            });
                            if prev.as_deref().is_some_and(|c| c.name == name) {
                                let _ = reflector_change_tx.send(PodChange::Died(name));
                            }
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            error!("pod reflector error: {}", e);
                            tokio::select! {
                                biased;
                                () = reflector_cancel.cancelled() => break,
                                () = tokio::time::sleep(Duration::from_secs(1)) => {}
                            }
                        }
                        None => break,
                    },
                }
            }
        });

        let subscriber_cancel = cancel.clone();
        let subscriber_latest = Arc::clone(&latest_ready);
        let subscriber_change_tx = change_tx.clone();
        let subscriber_selector = selector.clone();
        let subscriber_handle = subscriber.clone();
        let subscriber_task = tokio::spawn(async move {
            let mut stream = std::pin::pin!(subscriber_handle);
            loop {
                tokio::select! {
                    biased;
                    () = subscriber_cancel.cancelled() => break,
                    next = stream.next() => match next {
                        Some(pod) => {
                            update_latest(
                                &subscriber_latest,
                                &pod,
                                &subscriber_selector,
                                &subscriber_change_tx,
                            );
                        }
                        None => break,
                    },
                }
            }
        });

        Ok(Self {
            store,
            _subscriber: subscriber,
            latest_ready,
            change_tx,
            selector,
            reflector_task,
            subscriber_task,
            cancel,
        })
    }

    /// Returns the currently ready pod, if any. Single scan of the store.
    pub fn ready_pod(&self) -> Option<ReadyPod> {
        let cached = self.latest_ready.load_full();
        let mut first_ready: Option<ReadyPod> = None;

        for pod in self.store.state() {
            if !is_pod_ready(&pod, &self.selector) {
                continue;
            }
            // If the cached pod is still ready, return it directly
            if let Some(ref c) = cached {
                if pod.name_any() == c.name {
                    return Some((**c).clone());
                }
            }
            // Track first ready pod as fallback
            if first_ready.is_none() {
                first_ready = Some(ReadyPod {
                    name: pod.name_any(),
                    uid: pod.metadata.uid.clone(),
                });
            }
        }

        match first_ready {
            Some(ready) => {
                self.latest_ready.store(Some(Arc::new(ready.clone())));
                Some(ready)
            }
            None => {
                self.latest_ready.store(None);
                None
            }
        }
    }

    /// Wait until a ready pod is available or `timeout` elapses.
    /// Wakes on `PodChange` events instead of polling.
    pub async fn wait_for_ready_pod(&self, timeout: Duration) -> Option<ReadyPod> {
        if let Some(pod) = self.ready_pod() {
            return Some(pod);
        }

        let mut rx = self.subscribe();
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                biased;
                () = &mut deadline => return None,
                ev = rx.recv() => {
                    match ev {
                        Ok(PodChange::Ready(_)) => {
                            if let Some(pod) = self.ready_pod() {
                                return Some(pod);
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => return None,
                        _ => {}
                    }
                }
            }
        }
    }

    /// Subscribe to pod lifecycle events. Each new subscriber sees only
    /// events emitted after the subscription.
    pub fn subscribe(&self) -> broadcast::Receiver<PodChange> {
        self.change_tx.subscribe()
    }

    /// Cancel background tasks. Idempotent.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

fn update_latest(
    latest: &Arc<ArcSwapOption<ReadyPod>>, pod: &Pod, selector: &PodSelector,
    change_tx: &broadcast::Sender<PodChange>,
) {
    if !is_pod_ready(pod, selector) {
        return;
    }

    let name = pod.name_any();

    // Check whether the pod actually changed before allocating
    let prev = latest.load();
    let changed = match prev.as_deref() {
        Some(cur) => cur.name != name,
        None => true,
    };

    if changed {
        let ready = Arc::new(ReadyPod {
            name: name.clone(),
            uid: pod.metadata.uid.clone(),
        });
        latest.store(Some(ready));
        debug!("pod_watch: ready pod changed to {}", name);
        let _ = change_tx.send(PodChange::Ready(name));
    }
}

fn matches_selector(pod: &Pod, selector: &PodSelector) -> bool {
    match selector {
        PodSelector::Name(name) => pod.name_any() == *name,
        PodSelector::Labels { .. } => true,
    }
}

fn is_pod_ready(pod: &Pod, selector: &PodSelector) -> bool {
    if !matches_selector(pod, selector) {
        return false;
    }
    let Some(status) = pod.status.as_ref() else {
        return false;
    };
    let running = status.phase.as_deref() == Some("Running");
    if !running {
        return false;
    }
    status
        .conditions
        .as_ref()
        .map(|cs| cs.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use k8s_openapi::api::core::v1::{
        PodCondition,
        PodStatus,
    };
    use kube::api::ObjectMeta;

    use super::*;

    fn mk_pod(name: &str, ready: bool, running: bool) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some(if running { "Running" } else { "Pending" }.to_string()),
                conditions: Some(vec![PodCondition {
                    type_: "Ready".into(),
                    status: if ready { "True" } else { "False" }.into(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn ready_running_pod_is_ready_for_labels_selector() {
        let pod = mk_pod("p1", true, true);
        assert!(is_pod_ready(
            &pod,
            &PodSelector::Labels {
                selector: "app=x".into()
            }
        ));
    }

    #[test]
    fn not_running_pod_is_not_ready() {
        let pod = mk_pod("p1", true, false);
        assert!(!is_pod_ready(
            &pod,
            &PodSelector::Labels {
                selector: String::new()
            }
        ));
    }

    #[test]
    fn name_selector_filters_by_name() {
        let pod = mk_pod("p1", true, true);
        assert!(is_pod_ready(&pod, &PodSelector::Name("p1".into())));
        assert!(!is_pod_ready(&pod, &PodSelector::Name("p2".into())));
    }

    #[test]
    fn ready_condition_must_be_true() {
        let pod = mk_pod("p1", false, true);
        assert!(!is_pod_ready(
            &pod,
            &PodSelector::Labels {
                selector: String::new()
            }
        ));
    }
}
