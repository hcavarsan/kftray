use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use kftray_commons::models::config_model::Config;
use lazy_static::lazy_static;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::kube::listener::{
    ListenerConfig,
    PortForwarder,
    Protocol,
};
use crate::kube::models::{
    PortForward,
    Target,
};

/// Owns the tasks backing a single forward. Dropping it aborts them, so a
/// startup future that is cancelled before the process reaches
/// [`CHILD_PROCESSES`] cannot leave an untracked listener running.
pub struct PortForwardProcess {
    handle: Option<JoinHandle<anyhow::Result<()>>>,
    pub direct_forwarder: Option<Arc<PortForwarder>>,
    pub cancellation_token: CancellationToken,
    pub config_id: String,
    ws_client_handle: Option<JoinHandle<()>>,
    /// Snapshot taken at registration so a stop can still release cluster
    /// resources, loopback addresses and host entries when the configuration
    /// has since been deleted from the database.
    config: Option<Config>,
}

impl PortForwardProcess {
    pub fn new(handle: JoinHandle<anyhow::Result<()>>, config_id: String) -> Self {
        Self {
            handle: Some(handle),
            direct_forwarder: None,
            cancellation_token: CancellationToken::new(),
            config_id,
            ws_client_handle: None,
            config: None,
        }
    }

    pub fn with_forwarder_and_token(
        handle: JoinHandle<anyhow::Result<()>>, forwarder: Arc<PortForwarder>, config_id: String,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            handle: Some(handle),
            direct_forwarder: Some(forwarder),
            cancellation_token,
            config_id,
            ws_client_handle: None,
            config: None,
        }
    }

    pub fn set_ws_client_handle(&mut self, ws_handle: JoinHandle<()>) {
        self.ws_client_handle = Some(ws_handle);
    }

    pub fn set_config(&mut self, config: Config) {
        self.config = Some(config);
    }

    pub fn config(&self) -> Option<&Config> {
        self.config.as_ref()
    }

    /// The API server the forward was established against, when it is known
    /// to this process.
    pub fn destination(&self) -> Option<String> {
        self.direct_forwarder
            .as_ref()
            .map(|forwarder| forwarder.cluster_identity())
    }

    /// Cleanup and abort the port forward process.
    /// Uses timeouts to prevent blocking on shutdown operations.
    pub async fn cleanup_and_abort(&mut self) {
        const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

        self.cancellation_token.cancel();
        let handle = self.handle.take();
        let ws_client_handle = self.ws_client_handle.take();
        let direct_forwarder = self.direct_forwarder.take();

        if let Some(handle) = &handle {
            handle.abort();
        }
        if let Some(handle) = &ws_client_handle {
            handle.abort();
        }

        let tasks = async {
            let forwarding = async {
                if let Some(handle) = handle {
                    let _ = handle.await;
                }
            };
            let websocket = async {
                if let Some(handle) = ws_client_handle {
                    let _ = handle.await;
                }
            };
            tokio::join!(forwarding, websocket);
        };
        // Abort synchronously before the first await: if this future is dropped
        // partway through `shutdown`, the remaining workers, including a
        // handshake stalled outside any cancellation point, would keep their
        // sockets and their `Arc<PortForwarder>` forever.
        if let Some(forwarder) = &direct_forwarder {
            forwarder.abort_workers();
        }
        let shutdown = async {
            if let Some(forwarder) = direct_forwarder {
                forwarder.shutdown().await;
            }
        };
        let (tasks, shutdown) = tokio::join!(
            timeout(SHUTDOWN_TIMEOUT, tasks),
            timeout(SHUTDOWN_TIMEOUT, shutdown)
        );
        if tasks.is_err() || shutdown.is_err() {
            tracing::warn!(
                "Port-forward shutdown timed out for config: {}",
                self.config_id
            );
        }
    }

    pub fn cancel(&self) {
        tracing::info!("Cancelling port forward for config: {}", self.config_id);
        self.cancellation_token.cancel();
        self.abort();
    }

    pub fn abort(&self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
        if let Some(handle) = &self.ws_client_handle {
            handle.abort();
        }
    }

    pub async fn get_current_active_pod(&self) -> Option<String> {
        if let Some(forwarder) = &self.direct_forwarder {
            forwarder.get_current_active_pod().await
        } else {
            None
        }
    }
}

impl Drop for PortForwardProcess {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
        self.abort();
        let Some(forwarder) = self.direct_forwarder.take() else {
            return;
        };
        // Connections stalled outside a cancellation point, such as a TLS
        // handshake, only release their socket when their task is aborted.
        forwarder.abort_workers();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move { forwarder.shutdown().await });
        }
    }
}

lazy_static! {
    pub static ref CHILD_PROCESSES: DashMap<i64, PortForwardProcess> = DashMap::new();
}

/// Active pod for every registered forward, gathered by a single pass over
/// [`CHILD_PROCESSES`] rather than one registry lookup per configuration id.
/// The forwarder handles are cloned and the registry guard released before
/// any of them is awaited.
pub async fn active_pods() -> std::collections::HashMap<i64, Option<String>> {
    let forwarders: Vec<(i64, Option<Arc<PortForwarder>>)> = CHILD_PROCESSES
        .iter()
        .map(|entry| (*entry.key(), entry.value().direct_forwarder.clone()))
        .collect();

    let mut pods = std::collections::HashMap::with_capacity(forwarders.len());
    for (id, forwarder) in forwarders {
        let pod = match forwarder {
            Some(forwarder) => forwarder.get_current_active_pod().await,
            None => None,
        };
        pods.insert(id, pod);
    }
    pods
}

lazy_static! {
    /// Serializes tests across this crate (and kftui's, which shares the
    /// same process-global registries) that touch `CHILD_PROCESSES` or
    /// other global forwarding state. Compiled unconditionally, mirroring
    /// `kftray_commons::test_utils::test_db`, so a dependent
    /// crate's own `#[cfg(test)]` code can synchronize on it too.
    pub static ref PROCESS_TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::new(());
}

fn pod_readiness_for(workload_type: &str) -> kube_portforward::PodReadiness {
    // Expose's relay is probed the same relay-aware way a proxy's is (see
    // `expose::kubernetes::relay_pod_ready`): the aggregate `Ready` condition
    // waits on every sidecar in the pod, which has nothing to do with the
    // relay actually listening.
    if workload_type == "proxy" || workload_type == "expose" {
        kube_portforward::PodReadiness::Running
    } else {
        kube_portforward::PodReadiness::Ready
    }
}

/// Outcome of [`PortForward::cleanup_resources`]. A privilege-unavailable
/// address release is not retryable by trying again, so it must not keep
/// the configuration recorded as needing cleanup forever; a hosts-file or
/// retryable address failure must.
#[derive(Debug, PartialEq, Eq)]
pub enum CleanupOutcome {
    /// Nothing left to retry. Carries a privilege-unavailable notice, if
    /// any, so the caller can still report it once.
    Settled(Option<String>),
    /// At least one release needs a retry.
    Incomplete(String),
}

/// Combines what cleanup found into one outcome: a privilege-unavailable
/// address release settles (retrying automatically will not help without
/// user action), but any other failure, alone or alongside one, keeps the
/// configuration recorded for a retry.
fn merge_cleanup_outcome(unsatisfiable: Vec<String>, errors: Vec<String>) -> CleanupOutcome {
    if errors.is_empty() {
        if unsatisfiable.is_empty() {
            CleanupOutcome::Settled(None)
        } else {
            CleanupOutcome::Settled(Some(unsatisfiable.join("; ")))
        }
    } else {
        let mut incomplete = unsatisfiable;
        incomplete.extend(errors);
        CleanupOutcome::Incomplete(incomplete.join("; "))
    }
}

impl PortForward {
    pub fn new(
        target: Target, local_port: impl Into<Option<u16>>,
        local_address: impl Into<Option<String>>, context_name: Option<String>,
        kubeconfig: Option<String>, config_id: i64, workload_type: String,
    ) -> Self {
        Self {
            target,
            local_port: local_port.into(),
            local_address: local_address.into(),
            context_name,
            kubeconfig,
            config_id,
            workload_type,
            expected_destination: None,
        }
    }

    /// Requires the connection this forward resolves to reach `destination`.
    pub fn expecting_destination(mut self, destination: Option<String>) -> Self {
        self.expected_destination = destination;
        self
    }

    pub fn local_port(&self) -> u16 {
        self.local_port.unwrap_or(0)
    }

    pub fn local_address(&self) -> Option<String> {
        self.local_address.clone()
    }

    /// Releases everything `start_config` registered outside the process: the
    /// custom loopback address and the domain alias in the hosts file.
    #[instrument(skip(self), fields(config_id = self.config_id))]
    /// Releases what a startup acquired locally. `config` is the row the
    /// startup ran with: it names the aliases the hosts-file cleanup has to
    /// verify gone.
    pub async fn cleanup_resources(
        &self, config: Option<&kftray_commons::models::config_model::Config>,
        mode: kftray_commons::utils::db_mode::DatabaseMode,
    ) -> CleanupOutcome {
        let mut errors: Vec<String> = Vec::new();
        let mut unsatisfiable: Vec<String> = Vec::new();
        // Routed through the ownership-safe release: two configurations of one
        // service can share an address, and removing it directly would take the
        // alias from under the other forward.
        if let Some(addr) = &self.local_address
            && crate::network_utils::is_custom_loopback_address(addr)
            && let Err(error) =
                crate::kube::stop::release_address_with_fallback(addr, Some(self.config_id), mode)
                    .await
        {
            if error.is_unsatisfiable() {
                unsatisfiable.push(error.to_string());
            } else {
                errors.push(error.to_string());
            }
        }
        // Reported rather than swallowed: this runs when a startup failed or
        // was cancelled after adding an alias, and the caller keeps the config
        // tracked for retry when cleanup did not finish.
        let config_id = self.config_id;
        let snapshot = config.cloned();
        let in_use = crate::kube::stop::forwarding_configs(mode).await;
        match crate::hostsfile::remove_config_host_entries(
            config_id,
            snapshot.as_ref(),
            &in_use,
            mode,
        )
        .await
        {
            Ok(()) => {}
            Err(error) => errors.push(error.to_string()),
        }
        merge_cleanup_outcome(unsatisfiable, errors)
    }

    #[instrument(skip(self, tls_acceptor), fields(config_id = self.config_id))]
    pub async fn port_forward_tcp(
        self, tls_acceptor: Option<tokio_rustls::TlsAcceptor>,
    ) -> anyhow::Result<(u16, PortForwardProcess)> {
        let local_addr = self
            .local_address
            .as_deref()
            .unwrap_or("127.0.0.1")
            .to_owned();

        let namespace = self.target.namespace.name_any();

        let direct_forwarder = PortForwarder::new(
            &namespace,
            self.target.clone(),
            self.context_name.clone(),
            self.kubeconfig.clone(),
            pod_readiness_for(&self.workload_type),
            self.expected_destination.as_deref(),
        )
        .await?;
        let direct_forwarder = Arc::new(direct_forwarder);

        let listener_config = ListenerConfig {
            local_address: local_addr,
            local_port: self.local_port(),
            protocol: Protocol::Tcp,
            tls_acceptor,
        };

        let forwarder_clone = direct_forwarder.clone();
        let cancellation_token = CancellationToken::new();
        let (port, handle) = match direct_forwarder
            .start_listener(
                listener_config,
                self.config_id,
                self.workload_type.clone(),
                cancellation_token.clone(),
            )
            .await
        {
            Ok(result) => result,
            Err(e) => {
                forwarder_clone.shutdown().await;
                return Err(e);
            }
        };

        let process = PortForwardProcess::with_forwarder_and_token(
            handle,
            forwarder_clone,
            self.config_id.to_string(),
            cancellation_token,
        );
        Ok((port, process))
    }

    pub async fn port_forward_udp(self) -> anyhow::Result<(u16, PortForwardProcess)> {
        let local_addr = self
            .local_address
            .as_deref()
            .unwrap_or("127.0.0.1")
            .to_owned();

        let namespace = self.target.namespace.name_any();

        let direct_forwarder = PortForwarder::new(
            &namespace,
            self.target.clone(),
            self.context_name.clone(),
            self.kubeconfig.clone(),
            pod_readiness_for(&self.workload_type),
            self.expected_destination.as_deref(),
        )
        .await?;
        let direct_forwarder = Arc::new(direct_forwarder);

        let listener_config = ListenerConfig {
            local_address: local_addr,
            local_port: self.local_port(),
            protocol: Protocol::Udp,
            tls_acceptor: None,
        };

        let forwarder_clone = direct_forwarder.clone();
        let cancellation_token = CancellationToken::new();
        let (port, handle) = match direct_forwarder
            .start_listener(
                listener_config,
                self.config_id,
                self.workload_type.clone(),
                cancellation_token.clone(),
            )
            .await
        {
            Ok(result) => result,
            Err(e) => {
                forwarder_clone.shutdown().await;
                return Err(e);
            }
        };

        let process = PortForwardProcess::with_forwarder_and_token(
            handle,
            forwarder_clone,
            self.config_id.to_string(),
            cancellation_token,
        );
        Ok((port, process))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privilege_only_failure_settles_cleanup() {
        let outcome = merge_cleanup_outcome(
            vec!["needs elevated privileges that are not available right now".to_owned()],
            Vec::new(),
        );
        assert_eq!(
            outcome,
            CleanupOutcome::Settled(Some(
                "needs elevated privileges that are not available right now".to_owned()
            ))
        );
    }

    #[test]
    fn a_retryable_failure_alongside_a_privilege_notice_stays_incomplete() {
        let outcome = merge_cleanup_outcome(
            vec!["needs elevated privileges that are not available right now".to_owned()],
            vec!["hosts write failed".to_owned()],
        );
        assert_eq!(
            outcome,
            CleanupOutcome::Incomplete(
                "needs elevated privileges that are not available right now; hosts write failed"
                    .to_owned()
            )
        );
    }

    #[test]
    fn nothing_left_settles_with_no_notice() {
        assert_eq!(
            merge_cleanup_outcome(Vec::new(), Vec::new()),
            CleanupOutcome::Settled(None)
        );
    }

    fn dummy_handle() -> JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async { Ok(()) })
    }

    #[tokio::test]
    async fn test_process_cancellation() {
        let process = PortForwardProcess::new(dummy_handle(), "test-process".to_string());
        let cancellation_token = process.cancellation_token.clone();

        let task = tokio::spawn(async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => true,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => false,
            }
        });

        process.cancel();

        assert!(task.await.unwrap());
    }

    #[test]
    fn test_pod_readiness_for_workload_type() {
        assert_eq!(
            pod_readiness_for("proxy"),
            kube_portforward::PodReadiness::Running
        );
        assert_eq!(
            pod_readiness_for("service"),
            kube_portforward::PodReadiness::Ready
        );
        assert_eq!(
            pod_readiness_for("pod"),
            kube_portforward::PodReadiness::Ready
        );
        assert_eq!(
            pod_readiness_for("expose"),
            kube_portforward::PodReadiness::Running
        );
    }

    #[tokio::test]
    async fn cleanup_finishes_owned_tasks_before_returning() {
        let handle = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let forwarding_task = handle.abort_handle();
        let websocket = tokio::spawn(std::future::pending::<()>());
        let websocket_task = websocket.abort_handle();
        let mut process = PortForwardProcess::new(handle, "cleanup".to_owned());
        process.set_ws_client_handle(websocket);

        process.cleanup_and_abort().await;

        assert!(forwarding_task.is_finished());
        assert!(websocket_task.is_finished());
    }

    #[tokio::test]
    async fn dropping_an_unregistered_process_aborts_its_tasks() {
        let handle = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let forwarding_task = handle.abort_handle();
        let websocket = tokio::spawn(std::future::pending::<()>());
        let websocket_task = websocket.abort_handle();
        let mut process = PortForwardProcess::new(handle, "dropped".to_owned());
        process.set_ws_client_handle(websocket);
        let cancellation = process.cancellation_token.clone();

        drop(process);
        tokio::task::yield_now().await;

        assert!(cancellation.is_cancelled());
        assert!(forwarding_task.is_finished());
        assert!(websocket_task.is_finished());
    }
}
