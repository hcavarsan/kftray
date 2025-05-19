use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use futures::TryStreamExt;
use kftray_http_logs::HttpLogState;
use kube::{
    api::Api,
    Client,
};
use lazy_static::lazy_static;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::{
    net::{
        TcpListener,
        TcpStream,
    },
    task::JoinHandle,
};
use tokio_stream::wrappers::TcpListenerStream;
use tracing::{
    error,
    info,
    instrument,
    trace,
};

use crate::kube::client::create_client_with_specific_context;
use crate::kube::models::{
    PortForward,
    Target,
};
use crate::kube::pod_finder::TargetPodFinder;
use crate::kube::tcp_forwarder::TcpForwarder;
use crate::kube::udp_forwarder::UdpForwarder;

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
            create_client_with_specific_context(kubeconfig, Some(context_name)).await?
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

    #[instrument(skip(self), fields(config_id = self.config_id))]
    pub async fn cleanup_resources(&self) -> anyhow::Result<()> {
        if let Some(addr) = &self.local_address {
            if addr != "127.0.0.1" {
                info!("Cleaning up loopback address: {}", addr);
                if let Err(e) = crate::network_utils::remove_loopback_address(addr).await {
                    error!("Failed to remove loopback address {}: {}", addr, e);
                }
            }
        }

        Ok(())
    }

    #[instrument(skip(self, http_log_state), fields(config_id = self.config_id))]
    pub async fn port_forward_tcp(
        self, http_log_state: Arc<HttpLogState>,
    ) -> anyhow::Result<(u16, tokio::task::JoinHandle<()>)> {
        let local_addr = self
            .local_address
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());

        if let Err(e) = crate::network_utils::ensure_loopback_address(&local_addr).await {
            error!(
                "Failed to configure loopback address {}: {:?}",
                local_addr, e
            );
            return Err(anyhow::anyhow!(
                "Failed to configure loopback address: {}",
                e
            ));
        }

        let addr = format!("{}:{}", local_addr, self.local_port())
            .parse::<SocketAddr>()
            .expect("Invalid local address");

        info!("Binding TCP listener");
        let bind = TcpListener::bind(addr).await?;
        let port = bind.local_addr()?.port();
        info!(%port, "Bound TCP listener");

        let server = {
            let cancel_notifier = CANCEL_NOTIFIER.clone();
            let http_log_state = http_log_state.clone();
            TcpListenerStream::new(bind).try_for_each(move |client_conn: TcpStream| {
                let pf = self.clone();
                let client_conn = Arc::new(Mutex::new(client_conn));
                let http_log_state = http_log_state.clone();
                let cancel_notifier = cancel_notifier.clone();
                async move {
                    info!("Handling new TCP connection");
                    if let Ok(peer_addr) = client_conn.lock().await.peer_addr() {
                        trace!(%peer_addr, "new connection");
                    }

                    let conn = client_conn.lock().await;
                    conn.set_nodelay(true).map_err(|e| {
                        error!(error = %e, "Failed to set nodelay");
                        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                    })?;
                    drop(conn);

                    info!("Finding target pod");
                    let target = pf.finder().find(&pf.target).await.map_err(|e| {
                        error!(error = %e, "Failed to find target pod");
                        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                    })?;
                    info!(pod_name = %target.pod_name, pod_port = %target.port_number, "Found target pod");

                    let (pod_name, pod_port) = target.into_parts();
                    info!(%pod_name, %pod_port, "Initiating portforward API call");
                    let mut port_forwarder = pf
                        .pod_api
                        .portforward(&pod_name, &[pod_port])
                        .await
                        .map_err(|e| {
                            error!(error = %e, "Portforward API call failed");
                            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                        })?;
                    info!("Portforward API call successful");

                    info!("Taking stream from port_forwarder");
                    let upstream_conn = port_forwarder.take_stream(pod_port).ok_or_else(|| {
                        error!("Failed to take stream for port {}", pod_port);
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "port not found in forwarder".to_string(),
                        )
                    })?;
                    info!("Successfully took stream");

                    let mut forwarder = TcpForwarder::new(pf.config_id, pf.workload_type.clone());

                    if let Err(e) = forwarder.initialize_logger(&http_log_state, port).await {
                        error!("Failed to initialize HTTP logger: {:?}", e);
                    }

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
        info!("TCP Port forwarder server setup complete");
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

        if let Err(e) = crate::network_utils::ensure_loopback_address(&local_addr).await {
            error!(
                "Failed to configure loopback address {}: {:?}",
                local_addr, e
            );
            return Err(anyhow::anyhow!(
                "Failed to configure loopback address: {}",
                e
            ));
        }

        let local_port = self.local_port();

        UdpForwarder::bind_and_forward(local_addr, local_port, upstream_conn).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use http::{
        Request,
        Response,
        StatusCode,
    };
    use k8s_openapi::api::core::v1::{
        Pod,
        PodCondition,
        PodSpec,
        PodStatus,
        Service,
        ServicePort,
        ServiceSpec,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::List;
    use kube::client::Body;
    use tokio::time::Duration;
    use tower_test::mock;
    use tracing_subscriber;

    use super::*;
    use crate::kube::models::{
        NameSpace,
        Port,
        TargetSelector,
    };

    fn mock_pod(
        name: &str, namespace: &str, labels: Option<BTreeMap<String, String>>, ready: bool,
    ) -> Pod {
        let status = if ready {
            Some(PodStatus {
                phase: Some("Running".to_string()),
                conditions: Some(vec![PodCondition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    last_probe_time: None,
                    last_transition_time: None,
                    message: None,
                    observed_generation: None,
                    reason: None,
                }]),
                ..Default::default()
            })
        } else {
            Some(PodStatus {
                phase: Some("Pending".to_string()),
                ..Default::default()
            })
        };
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                labels,
                ..Default::default()
            },
            spec: Some(PodSpec {
                ..Default::default()
            }),
            status,
        }
    }

    fn mock_service(
        name: &str, namespace: &str, selector: Option<BTreeMap<String, String>>,
    ) -> Service {
        Service {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                selector,
                ports: Some(vec![ServicePort {
                    port: 80,
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_child_processes_map() {
        {
            let mut processes = CHILD_PROCESSES.lock().unwrap();
            assert_eq!(processes.len(), 0);

            processes.insert("test-key".to_string(), dummy_handle());
            assert_eq!(processes.len(), 1);
            assert!(processes.contains_key("test-key"));

            processes.remove("test-key");
            assert_eq!(processes.len(), 0);
        }
    }

    #[allow(clippy::async_yields_async)]
    fn dummy_handle() -> JoinHandle<()> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async { tokio::spawn(async {}) })
    }

    #[tokio::test]
    async fn test_cancel_notifier() {
        let notifier = CANCEL_NOTIFIER.clone();
        let task = tokio::spawn(async move {
            tokio::select! {
                _ = notifier.notified() => true,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => false,
            }
        });

        CANCEL_NOTIFIER.notify_one();

        assert!(task.await.unwrap());
    }

    #[tokio::test]
    async fn test_local_port_and_address() {
        let target = Target {
            selector: TargetSelector::ServiceName("test-service".to_string()),
            port: Port::Number(8080),
            namespace: NameSpace(Some("default".to_string())),
        };

        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = Client::new(mock_service, "default");

        let pf_with_port = PortForward {
            target: target.clone(),
            local_port: Some(9090),
            local_address: Some("127.0.0.1".to_string()),
            pod_api: Api::namespaced(client.clone(), "default"),
            svc_api: Api::namespaced(client.clone(), "default"),
            context_name: None,
            config_id: 1,
            workload_type: "pod".to_string(),
            connection: Arc::new(Mutex::new(None)),
        };

        assert_eq!(pf_with_port.local_port(), 9090);
        assert_eq!(pf_with_port.local_address(), Some("127.0.0.1".to_string()));

        let pf_defaults = PortForward {
            target: target.clone(),
            local_port: Some(0),
            local_address: None,
            pod_api: Api::namespaced(client.clone(), "default"),
            svc_api: Api::namespaced(client.clone(), "default"),
            context_name: None,
            config_id: 2,
            workload_type: "pod".to_string(),
            connection: Arc::new(Mutex::new(None)),
        };

        assert_eq!(pf_defaults.local_port(), 0);
        assert_eq!(pf_defaults.local_address(), None);
    }

    #[tokio::test]
    async fn test_port_forward_new_context_propagation() {
        let target = Target {
            selector: TargetSelector::ServiceName("test-service".to_string()),
            port: Port::Number(8080),
            namespace: NameSpace(Some("default".to_string())),
        };
        let context_name = Some("my-kube-context".to_string());
        let _kubeconfig = Some("/path/to/config".to_string());

        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = Client::new(mock_service, "default");

        let pf = PortForward {
            target,
            local_port: Some(12345),
            local_address: None,
            pod_api: Api::namespaced(client.clone(), "default"),
            svc_api: Api::namespaced(client.clone(), "default"),
            context_name: context_name.clone(),
            config_id: 1,
            workload_type: "service".to_string(),
            connection: Arc::new(Mutex::new(None)),
        };

        assert_eq!(pf.context_name, context_name);
    }

    fn setup_mock_pf_for_api_test() -> (
        PortForward,
        Client,
        mock::Handle<Request<Body>, Response<Body>>,
    ) {
        let target = Target {
            selector: TargetSelector::ServiceName("test-svc".to_string()),
            port: Port::Number(80),
            namespace: NameSpace(Some("test-ns".to_string())),
        };

        let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = Client::new(mock_service, "test-ns");

        let pf = PortForward {
            target,
            local_port: Some(0),
            local_address: None,
            pod_api: Api::namespaced(client.clone(), "test-ns"),
            svc_api: Api::namespaced(client.clone(), "test-ns"),
            context_name: None,
            config_id: 1,
            workload_type: "service".to_string(),
            connection: Arc::new(Mutex::new(None)),
        };
        (pf, client, handle)
    }

    async fn mock_kube_api_calls(handle: &mut mock::Handle<Request<Body>, Response<Body>>) {
        info!("Mock server: Expecting GET Service");
        let (request, send) = handle
            .next_request()
            .await
            .expect("Service GET request expected");
        assert_eq!(request.method(), "GET");
        assert_eq!(
            request.uri().path(),
            "/api/v1/namespaces/test-ns/services/test-svc"
        );
        let svc = mock_service(
            "test-svc",
            "test-ns",
            Some(
                [("app".to_string(), "my-app".to_string())]
                    .into_iter()
                    .collect(),
            ),
        );
        let response = Response::builder()
            .body(Body::from(serde_json::to_vec(&svc).unwrap()))
            .unwrap();
        info!("Mock server: Received GET Service, sending response");
        send.send_response(response);

        info!("Mock server: Expecting LIST Pods");
        let (request, send) = handle
            .next_request()
            .await
            .expect("Pod LIST request expected");
        assert_eq!(request.method(), "GET");
        assert_eq!(request.uri().path(), "/api/v1/namespaces/test-ns/pods");
        assert_eq!(
            request.uri().query().unwrap(),
            "&labelSelector=app%3Dmy-app"
        );
        let pod = mock_pod(
            "test-pod-123",
            "test-ns",
            Some(
                [("app".to_string(), "my-app".to_string())]
                    .into_iter()
                    .collect(),
            ),
            true,
        );
        let pod_list: List<Pod> = List {
            items: vec![pod],
            ..Default::default()
        };
        let response = Response::builder()
            .body(Body::from(serde_json::to_vec(&pod_list).unwrap()))
            .unwrap();
        info!("Mock server: Received LIST Pods, sending response");
        send.send_response(response);

        info!("Mock server: Expecting GET Portforward");
        let (request, send) = handle
            .next_request()
            .await
            .expect("Portforward request expected");
        assert_eq!(request.method(), "GET");
        assert!(request.uri().path().ends_with("/portforward"));
        assert!(request.headers().contains_key(http::header::UPGRADE));
        assert!(request.headers().contains_key(http::header::CONNECTION));
        assert!(request
            .headers()
            .contains_key(http::header::SEC_WEBSOCKET_KEY));
        assert!(request
            .headers()
            .contains_key(http::header::SEC_WEBSOCKET_VERSION));

        let response = Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header(http::header::UPGRADE, "websocket")
            .header(http::header::CONNECTION, "Upgrade")
            .header(http::header::SEC_WEBSOCKET_ACCEPT, "dummy_accept_key")
            .body(Body::empty())
            .unwrap();
        info!("Mock server: Received GET Portforward, sending response");
        send.send_response(response);
        info!("Mock server: All expected requests handled");
    }

    #[tokio::test]
    async fn test_port_forward_tcp_api_calls() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        info!("Starting test_port_forward_tcp_api_calls");

        let (pf_base, _client, _handle) = setup_mock_pf_for_api_test();
        let http_log_state = Arc::new(HttpLogState::new());

        let (mock_service_test, mut handle_test) = mock::pair::<Request<Body>, Response<Body>>();
        let client_test = Client::new(mock_service_test, "test-ns");

        let target_clone = pf_base.target.clone();

        let pf_test = PortForward {
            target: target_clone,
            local_port: Some(0),
            local_address: Some("127.0.0.1".to_string()),
            pod_api: Api::namespaced(client_test.clone(), "test-ns"),
            svc_api: Api::namespaced(client_test.clone(), "test-ns"),
            context_name: None,
            config_id: 1,
            workload_type: "service".to_string(),
            connection: Arc::new(Mutex::new(None)),
        };

        info!("Spawning mock server task");
        let mock_server_task = tokio::spawn(async move {
            mock_kube_api_calls(&mut handle_test).await;
        });

        info!("Calling port_forward_tcp");
        let pf_result = pf_test.port_forward_tcp(http_log_state).await;
        assert!(
            pf_result.is_ok(),
            "port_forward_tcp failed to start listener"
        );
        let (bound_port, server_task_handle) = pf_result.unwrap();
        assert_ne!(bound_port, 0, "Listener did not bind to a dynamic port");

        info!("Simulating client connection");
        let connect_addr = format!("127.0.0.1:{}", bound_port);
        let connect_task = tokio::spawn(async move {
            match TcpStream::connect(&connect_addr).await {
                Ok(stream) => {
                    drop(stream);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        });

        let connect_result = tokio::time::timeout(Duration::from_secs(1), connect_task).await;
        assert!(
            connect_result.is_ok(),
            "Client connection attempt timed out"
        );
        assert!(
            connect_result.unwrap().is_ok(),
            "Client connection attempt failed"
        );
        info!("Client connection simulated");

        info!("Waiting for mock server task");
        let mock_server_result =
            tokio::time::timeout(Duration::from_secs(5), mock_server_task).await;
        assert!(mock_server_result.is_ok(), "Mock server task timed out");
        assert!(
            mock_server_result.unwrap().is_ok(),
            "Mock server task failed"
        );
        info!("Mock server task completed");

        server_task_handle.abort();
        info!("Finished test_port_forward_tcp_api_calls");
    }

    #[tokio::test]
    async fn test_port_forward_udp_api_calls() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        info!("Starting test_port_forward_udp_api_calls");

        let (pf, _client, mut handle) = setup_mock_pf_for_api_test();

        info!("Spawning mock server task");
        let mock_server = tokio::spawn(async move {
            mock_kube_api_calls(&mut handle).await;
        });

        info!("Calling port_forward_udp");
        let result = tokio::time::timeout(Duration::from_secs(5), pf.port_forward_udp()).await;
        info!("port_forward_udp call returned");

        info!("Waiting for mock server task");
        let mock_server_result = mock_server.await;
        assert!(
            mock_server_result.is_ok(),
            "Mock server did not handle all requests"
        );

        assert!(result.is_ok(), "port_forward_udp timed out unexpectedly");
        assert!(
            result.unwrap().is_err(),
            "port_forward_udp succeeded unexpectedly, expected error after API calls"
        );
        info!("Finished test_port_forward_udp_api_calls");
    }
}
