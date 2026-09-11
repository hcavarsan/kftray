use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
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

pub struct PortForwardProcess {
    pub handle: JoinHandle<anyhow::Result<()>>,
    pub direct_forwarder: Option<Arc<PortForwarder>>,
    pub cancellation_token: CancellationToken,
    pub config_id: String,
    pub ws_client_handle: Option<JoinHandle<()>>,
}

impl PortForwardProcess {
    pub fn new(handle: JoinHandle<anyhow::Result<()>>, config_id: String) -> Self {
        Self {
            handle,
            direct_forwarder: None,
            cancellation_token: CancellationToken::new(),
            config_id,
            ws_client_handle: None,
        }
    }

    pub fn with_forwarder_and_token(
        handle: JoinHandle<anyhow::Result<()>>, forwarder: Arc<PortForwarder>, config_id: String,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            handle,
            direct_forwarder: Some(forwarder),
            cancellation_token,
            config_id,
            ws_client_handle: None,
        }
    }

    pub fn set_ws_client_handle(&mut self, ws_handle: JoinHandle<()>) {
        self.ws_client_handle = Some(ws_handle);
    }

    /// Cleanup and abort the port forward process.
    /// Uses timeouts to prevent blocking on shutdown operations.
    pub async fn cleanup_and_abort(self) {
        const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

        tracing::info!("Cancelling port forward for config: {}", self.config_id);

        if let Some(ws_handle) = self.ws_client_handle {
            ws_handle.abort();
            match timeout(SHUTDOWN_TIMEOUT, ws_handle).await {
                Err(_) => tracing::warn!(
                    "WebSocket client shutdown timed out for config: {}",
                    self.config_id
                ),
                Ok(Err(error)) if !error.is_cancelled() => tracing::warn!(
                    "WebSocket client task failed for config {}: {}",
                    self.config_id,
                    error
                ),
                _ => {}
            }
        }

        self.cancellation_token.cancel();

        if let Some(forwarder) = &self.direct_forwarder {
            tracing::info!(
                "Cleaning up forwarder resources for config: {}",
                self.config_id
            );
            if timeout(SHUTDOWN_TIMEOUT, forwarder.shutdown())
                .await
                .is_err()
            {
                tracing::warn!(
                    "Forwarder shutdown timed out for config: {}, forcing abort",
                    self.config_id
                );
            }
        }

        self.handle.abort();
        if timeout(SHUTDOWN_TIMEOUT, self.handle).await.is_err() {
            tracing::warn!(
                "Port-forward task shutdown timed out for config: {}",
                self.config_id
            );
        }
    }

    pub fn cancel(&self) {
        tracing::info!("Cancelling port forward for config: {}", self.config_id);
        self.cancellation_token.cancel();
    }

    pub fn abort(&self) {
        self.handle.abort();
    }

    pub async fn get_current_active_pod(&self) -> Option<String> {
        if let Some(forwarder) = &self.direct_forwarder {
            forwarder.get_current_active_pod().await
        } else {
            None
        }
    }
}

lazy_static! {
    pub static ref CHILD_PROCESSES: DashMap<String, PortForwardProcess> = DashMap::new();
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

    #[instrument(skip(self), fields(config_id = self.config_id))]
    pub async fn cleanup_resources(&self) -> anyhow::Result<()> {
        if let Some(addr) = &self.local_address
            && crate::network_utils::is_custom_loopback_address(addr)
        {
            let _ = crate::network_utils::remove_loopback_address(addr).await;
        }
        Ok(())
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
            kube_portforward::PodReadiness::Running,
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
}
