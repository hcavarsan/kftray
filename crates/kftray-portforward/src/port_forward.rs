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

#[cfg(test)]
lazy_static! {
    pub(crate) static ref PROCESS_TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::new(());
}

fn pod_readiness_for(workload_type: &str) -> kube_portforward::PodReadiness {
    if workload_type == "proxy" {
        kube_portforward::PodReadiness::Running
    } else {
        kube_portforward::PodReadiness::Ready
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
        }
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
    pub async fn cleanup_resources(&self) -> anyhow::Result<()> {
        let mut errors: Vec<String> = Vec::new();
        if let Some(addr) = &self.local_address
            && crate::network_utils::is_custom_loopback_address(addr)
            && let Err(error) = crate::network_utils::remove_loopback_address(addr).await
        {
            errors.push(error.to_string());
        }
        // Reported rather than swallowed: this runs when a startup failed or
        // was cancelled after adding an alias, and the caller keeps the config
        // tracked for retry when cleanup did not finish. Run on a blocking
        // thread because the hosts file is written synchronously behind a lock.
        let config_id = self.config_id;
        match tokio::task::spawn_blocking(move || {
            let mut errors = Vec::new();
            if let Err(error) = crate::hostsfile::remove_host_entry(&config_id.to_string()) {
                errors.push(error.to_string());
            }
            if let Err(error) = crate::hostsfile::remove_ssl_host_entry(&config_id.to_string()) {
                errors.push(error.to_string());
            }
            errors
        })
        .await
        {
            Ok(hosts_errors) => errors.extend(hosts_errors),
            Err(error) => errors.push(format!("Hosts cleanup task failed: {error}")),
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(errors.join("; ")))
        }
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
            kube_portforward::PodReadiness::Ready
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
