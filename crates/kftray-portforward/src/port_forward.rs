use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use futures::TryStreamExt;
use kube::{
    api::Api,
    Client,
};
use lazy_static::lazy_static;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::{
    net::TcpListener,
    task::JoinHandle,
};
use tokio_stream::wrappers::TcpListenerStream;
use tracing::{
    error,
    trace,
};

use crate::models::kube::HttpLogState;
use crate::models::kube::{
    PortForward,
    Target,
};
use crate::pod_finder::TargetPodFinder;
use crate::tcp_forwarder::TcpForwarder;
use crate::udp_forwarder::UdpForwarder;

lazy_static! {
    pub static ref CHILD_PROCESSES: Arc<StdMutex<HashMap<String, JoinHandle<()>>>> =
        Arc::new(StdMutex::new(HashMap::new()));
    pub static ref CANCEL_NOTIFIER: Arc<Notify> = Arc::new(Notify::new());
}

impl PortForward {
    pub async fn new(
        target: Target, local_port: impl Into<Option<u16>>,
        local_address: impl Into<Option<String>>, context_name: Option<String>,
        kubeconfig: Option<String>, config_id: i64, workload_type: String,
    ) -> anyhow::Result<Self> {
        let (client, _, _) = if let Some(ref context_name) = context_name {
            crate::client::create_client_with_specific_context(kubeconfig, Some(context_name))
                .await?
        } else {
            (Some(Client::try_default().await?), None, Vec::new())
        };

        let client = client.ok_or_else(|| {
            anyhow::anyhow!(
                "Client not created for context '{}'",
                context_name.clone().unwrap_or_default()
            )
        })?;
        let namespace = target.namespace.name_any();

        Ok(Self {
            target,
            local_port: local_port.into(),
            local_address: local_address.into(),
            pod_api: Api::namespaced(client.clone(), &namespace),
            svc_api: Api::namespaced(client, &namespace),
            context_name: context_name.clone(),
            config_id,
            workload_type,
            connection: Arc::new(Mutex::new(None)),
        })
    }

    pub fn local_port(&self) -> u16 {
        self.local_port.unwrap_or(0)
    }

    pub fn local_address(&self) -> Option<String> {
        self.local_address.clone()
    }

    pub async fn port_forward_tcp(
        self, http_log_state: Arc<HttpLogState>,
    ) -> anyhow::Result<(u16, tokio::task::JoinHandle<()>)> {
        let local_addr = self
            .local_address
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());

        let addr = format!("{}:{}", local_addr, self.local_port())
            .parse::<SocketAddr>()
            .expect("Invalid local address");

        let bind = TcpListener::bind(addr).await?;
        let port = bind.local_addr()?.port();

        trace!(port, "Bound to local address and port");

        let server = {
            let cancel_notifier = CANCEL_NOTIFIER.clone();
            let http_log_state = http_log_state.clone();
            TcpListenerStream::new(bind).try_for_each(move |client_conn| {
                let pf = self.clone();
                let client_conn = Arc::new(Mutex::new(client_conn));
                let http_log_state = http_log_state.clone();
                let cancel_notifier = cancel_notifier.clone();
                async move {
                    if let Ok(peer_addr) = client_conn.lock().await.peer_addr() {
                        trace!(%peer_addr, "new connection");
                    }

                    let conn = client_conn.lock().await;
                    conn.set_nodelay(true).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                    })?;
                    drop(conn);

                    let target = pf.finder().find(&pf.target).await.map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                    })?;

                    let (pod_name, pod_port) = target.into_parts();
                    let mut port_forwarder = pf
                        .pod_api
                        .portforward(&pod_name, &[pod_port])
                        .await
                        .map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                    })?;

                    let upstream_conn = port_forwarder.take_stream(pod_port).ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "port not found in forwarder".to_string(),
                        )
                    })?;

                    let forwarder = TcpForwarder::new(pf.config_id, pf.workload_type.clone());

                    tokio::spawn(async move {
                        if let Err(e) = forwarder
                            .forward_connection(
                                client_conn,
                                upstream_conn,
                                http_log_state,
                                cancel_notifier,
                                port,
                            )
                            .await
                        {
                            error!(
                                error = e.as_ref() as &dyn std::error::Error,
                                "failed to forward connection"
                            );
                        }
                    });

                    Ok(())
                }
            })
        };

        Ok((
            port,
            tokio::spawn(async {
                if let Err(e) = server.await {
                    error!(error = &e as &dyn std::error::Error, "server error");
                }
            }),
        ))
    }

    pub fn finder(&self) -> TargetPodFinder {
        TargetPodFinder {
            pod_api: &self.pod_api,
            svc_api: &self.svc_api,
        }
    }

    pub async fn port_forward_udp(self) -> anyhow::Result<(u16, JoinHandle<()>)> {
        let target = self.finder().find(&self.target).await?;
        let (pod_name, pod_port) = target.into_parts();

        let mut port_forwarder = self.pod_api.portforward(&pod_name, &[pod_port]).await?;
        let upstream_conn = port_forwarder
            .take_stream(pod_port)
            .ok_or_else(|| anyhow::anyhow!("port not found in forwarder"))?;

        let local_addr = self
            .local_address
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let local_port = self.local_port();

        UdpForwarder::bind_and_forward(local_addr, local_port, upstream_conn).await
    }
}
