use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use anyhow::{
    anyhow,
    Result,
};
use futures::StreamExt;
use k8s_openapi::api::core::v1::{
    Pod,
    Service,
};
use kube::{
    Api,
    Client,
    ResourceExt,
};
use kube_runtime::{
    reflector::{
        self,
        ReflectHandle,
        Store,
    },
    watcher::{
        self,
        Config as WatcherConfig,
    },
    WatchStreamExt,
};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{
    debug,
    error,
    info,
};

use crate::kube::models::{
    Port,
    Target,
    TargetPod,
    TargetSelector,
};

pub struct PodWatcher {
    store: Store<Pod>,
    subscriber: ReflectHandle<Pod>,
    latest_ready_pod: Arc<RwLock<Option<TargetPod>>>,
    _reflector_task: JoinHandle<()>,
    _subscriber_task: JoinHandle<()>,
    cancellation_token: CancellationToken,
    pod_change_tx: tokio::sync::broadcast::Sender<String>,
    target: Target,
}

impl Drop for PodWatcher {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
        self._reflector_task.abort();
        self._subscriber_task.abort();
        debug!("PodWatcher background tasks aborted");
    }
}

impl PodWatcher {
    pub async fn new(client: Client, target: Target) -> Result<Self> {
        let namespace = target.namespace.name_any();
        let label_selector = Self::resolve_label_selector(&client, &namespace, &target).await?;

        info!(
            "Starting pod watcher for namespace '{}' with labels: {}",
            namespace, label_selector
        );

        let (store, writer) = reflector::store_shared(256);
        let subscriber = writer
            .subscribe()
            .ok_or_else(|| anyhow!("Failed to create subscriber"))?;

        let cancellation_token = CancellationToken::new();
        let latest_ready_pod = Arc::new(RwLock::new(None));
        let latest_pod_clone = latest_ready_pod.clone();
        let target_clone = target.clone();
        let namespace_clone = namespace.clone();

        let (pod_change_tx, _) = tokio::sync::broadcast::channel(16);
        let pod_change_tx_clone = pod_change_tx.clone();

        let pods_api: Api<Pod> = Api::namespaced(client, &namespace);
        let watcher_config = WatcherConfig::default().labels(&label_selector);

        let token_clone = cancellation_token.clone();
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
                .reflect_shared(writer)
                .applied_objects();

            let mut stream = std::pin::pin!(stream);

            loop {
                tokio::select! {
                    _ = token_clone.cancelled() => {
                        debug!("Pod reflector task cancelled");
                        break;
                    }
                    result = stream.next() => {
                        match result {
                            Some(Ok(pod)) => {
                                debug!("Pod reflected: {}", pod.name_any());
                            }
                            Some(Err(e)) => {
                                error!("Reflector error: {}", e);
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                            None => {
                                debug!("Pod reflector stream ended");
                                break;
                            }
                        }
                    }
                }
            }
        });

        let subscriber_clone = subscriber.clone();
        let token_clone2 = cancellation_token.clone();
        let subscriber_task = tokio::spawn(async move {
            let filtered_stream = subscriber_clone.filter_map(move |pod| {
                let namespace = namespace_clone.clone();
                async move {
                    if pod.namespace().as_deref() != Some(&namespace) {
                        return None;
                    }
                    Some(pod)
                }
            });

            let mut stream = std::pin::pin!(filtered_stream);

            loop {
                tokio::select! {
                    _ = token_clone2.cancelled() => {
                        debug!("Pod subscriber task cancelled");
                        break;
                    }
                    pod = stream.next() => {
                        match pod {
                            Some(pod) => {
                                Self::update_latest_pod(&latest_pod_clone, &pod, &target_clone, &pod_change_tx_clone).await;
                            }
                            None => {
                                debug!("Pod subscriber stream ended");
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            store,
            subscriber,
            latest_ready_pod,
            _reflector_task: reflector_task,
            _subscriber_task: subscriber_task,
            cancellation_token,
            pod_change_tx,
            target,
        })
    }

    async fn update_latest_pod(
        latest_ready_pod: &Arc<RwLock<Option<TargetPod>>>, pod: &Pod, target: &Target,
        pod_change_tx: &tokio::sync::broadcast::Sender<String>,
    ) {
        if Self::is_pod_ready(pod) {
            if let Ok(port_number) = Self::extract_port_from_pod(pod, &target.port) {
                let target_pod = TargetPod {
                    pod_name: pod.name_any(),
                    port_number,
                };

                let mut latest = latest_ready_pod.write().await;
                let pod_changed = match latest.as_ref() {
                    Some(current) => current.pod_name != target_pod.pod_name,
                    None => true,
                };

                if pod_changed {
                    debug!(
                        "Pod changed to: {}, connections should reconnect",
                        pod.name_any()
                    );
                    let _ = pod_change_tx.send(target_pod.pod_name.clone());
                }

                *latest = Some(target_pod);
                debug!("Updated latest ready pod: {}", pod.name_any());
            }
        }
    }

    pub async fn get_ready_pod(&self) -> Option<TargetPod> {
        if let Some(cached) = self.latest_ready_pod.read().await.clone() {
            for pod in self.store.state() {
                if pod.name_any() == cached.pod_name && Self::is_pod_ready(&pod) {
                    return Some(cached);
                }
            }
        }

        for pod in self.store.state() {
            if Self::is_pod_ready(&pod) {
                if let Ok(port_number) = Self::extract_port_from_pod(&pod, &self.target.port) {
                    let target_pod = TargetPod {
                        pod_name: pod.name_any(),
                        port_number,
                    };

                    let mut latest = self.latest_ready_pod.write().await;
                    *latest = Some(target_pod.clone());
                    debug!("Found ready pod: {}", pod.name_any());
                    return Some(target_pod);
                }
            }
        }

        let mut latest = self.latest_ready_pod.write().await;
        *latest = None;
        None
    }

    pub async fn has_running_pods(&self) -> bool {
        let store = self.store.clone();
        for pod in store.state() {
            if let Some(status) = &pod.status {
                if status.phase.as_deref() == Some("Running") {
                    return true;
                }
            }
        }
        false
    }

    pub async fn wait_for_ready_pod(&self, timeout: Duration) -> Option<TargetPod> {
        let start = Instant::now();

        while start.elapsed() < timeout {
            if let Some(pod) = self.get_ready_pod().await {
                return Some(pod);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        None
    }

    pub fn get_store(&self) -> Store<Pod> {
        self.store.clone()
    }

    pub fn create_subscriber(&self) -> ReflectHandle<Pod> {
        self.subscriber.clone()
    }

    pub fn shutdown(&self) {
        self.cancellation_token.cancel();
    }

    pub fn subscribe_pod_changes(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.pod_change_tx.subscribe()
    }

    async fn resolve_label_selector(
        client: &Client, namespace: &str, target: &Target,
    ) -> Result<String> {
        match &target.selector {
            TargetSelector::PodLabel(label_selector) => Ok(label_selector.clone()),
            TargetSelector::ServiceName(service_name) => {
                let service_api: Api<Service> = Api::namespaced(client.clone(), namespace);
                let service = service_api
                    .get(service_name)
                    .await
                    .map_err(|e| anyhow!("Service '{}' not found: {}", service_name, e))?;

                let selector = service
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.selector.as_ref())
                    .ok_or_else(|| anyhow!("Service '{}' has no selector", service_name))?;

                let mut label_selector = String::with_capacity(selector.len() * 20);
                let mut first = true;
                for (k, v) in selector {
                    if !first {
                        label_selector.push(',');
                    }
                    label_selector.push_str(k);
                    label_selector.push('=');
                    label_selector.push_str(v);
                    first = false;
                }

                Ok(label_selector)
            }
        }
    }

    fn is_pod_ready(pod: &Pod) -> bool {
        pod.status
            .as_ref()
            .map(|status| {
                let is_running = status.phase.as_deref() == Some("Running");

                if let Some(name) = &pod.metadata.name {
                    if name.starts_with("kftray-forward-") && is_running {
                        return true;
                    }
                }

                let is_ready = status
                    .conditions
                    .as_ref()
                    .map(|conditions| {
                        conditions
                            .iter()
                            .any(|c| c.type_ == "Ready" && c.status == "True")
                    })
                    .unwrap_or(false);

                is_running && is_ready
            })
            .unwrap_or(false)
    }

    fn extract_port_from_pod(pod: &Pod, target_port: &Port) -> Result<u16> {
        match target_port {
            Port::Number(num) => (*num)
                .try_into()
                .map_err(|_| anyhow!("Invalid port number: {}", num)),
            Port::Name(port_name) => pod
                .spec
                .as_ref()
                .and_then(|spec| {
                    spec.containers
                        .iter()
                        .filter_map(|container| container.ports.as_ref())
                        .flatten()
                        .find(|p| p.name.as_deref() == Some(port_name))
                        .map(|p| p.container_port as u16)
                })
                .ok_or_else(|| anyhow!("Port '{}' not found in pod", port_name)),
        }
    }
}
