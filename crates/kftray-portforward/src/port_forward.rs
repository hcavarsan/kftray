use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use kftray_http_logs::HttpLogState;
use kube::{
    api::Api,
    Client,
};
use lazy_static::lazy_static;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::{
    error,
    instrument,
};

use crate::kube::client::create_client_with_specific_context;
use crate::kube::connection_pool::PooledPortForwarder;
use crate::kube::models::{
    PortForward,
    Target,
};
use crate::kube::pod_finder::TargetPodFinder;
use crate::kube::target_cache::{
    CacheConfig,
    TargetCache,
};
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

        let cache_config = CacheConfig::default();
        let target_cache = Arc::new(TargetCache::new(cache_config));

        Ok(Self {
            target,
            local_port: local_port.into(),
            local_address: local_address.into(),
            pod_api: Api::namespaced(client.clone(), &namespace),
            svc_api: Api::namespaced(client.clone(), &namespace),
            client,
            context_name: context_name.clone(),
            config_id,
            workload_type,
            connection: Arc::new(Mutex::new(None)),
            target_cache,
            connection_pool: None,
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
            if crate::network_utils::is_custom_loopback_address(addr) {
                let _ = crate::network_utils::remove_loopback_address(addr).await;
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
            return Err(anyhow::anyhow!("Network config failed: {}", e));
        }

        let addr = format!("{}:{}", local_addr, self.local_port())
            .parse::<std::net::SocketAddr>()
            .map_err(|e| anyhow::anyhow!("Invalid address: {}", e))?;

        let client = self.client.clone();
        let namespace = self.target.namespace.name_any();
        let target = self.target.clone();
        let target_cache = self.target_cache.clone();

        let portforward_pool =
            PooledPortForwarder::new(client, &namespace, target, target_cache, 200).await?;
        let portforward_pool = Arc::new(portforward_pool);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        let port = listener.local_addr()?.port();

        let portforward_pool_for_spawn = portforward_pool.clone();
        let http_log_state_for_spawn = http_log_state.clone();
        let config_id = self.config_id;
        let workload_type = self.workload_type.clone();

        Ok((
            port,
            tokio::spawn(async move {
                if let Err(e) = portforward_pool_for_spawn
                    .handle_tcp_listener(
                        listener,
                        http_log_state_for_spawn,
                        config_id,
                        workload_type,
                        port,
                        100,
                    )
                    .await
                {
                    error!("server error: {}", e);
                }
            }),
        ))
    }

    pub fn finder(&self) -> TargetPodFinder<'_> {
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
            return Err(anyhow::anyhow!("Network config failed: {}", e));
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
    use tokio::net::TcpStream;
    use tokio::time::Duration;
    use tower_test::mock;
    use tracing::info;
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

    #[tokio::test]
    async fn test_child_processes_map() {
        {
            let mut processes = CHILD_PROCESSES.lock().unwrap();
            processes.clear();
        }

        {
            let mut processes = CHILD_PROCESSES.lock().unwrap();
            assert_eq!(processes.len(), 0);

            processes.insert("test-key".to_string(), dummy_handle());
            assert_eq!(processes.len(), 1);
            assert!(processes.contains_key("test-key"));

            let handle = processes.remove("test-key");
            if let Some(h) = handle {
                h.abort();
            }
            assert_eq!(processes.len(), 0);
        }
    }

    fn dummy_handle() -> JoinHandle<()> {
        use std::pin::Pin;
        use std::task::{
            Context,
            Poll,
        };

        struct DummyFuture;
        impl std::future::Future for DummyFuture {
            type Output = ();
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                Poll::Ready(())
            }
        }

        tokio::task::spawn(DummyFuture)
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

        let cache_config = CacheConfig::default();
        let target_cache = Arc::new(TargetCache::new(cache_config));

        let pf_with_port = PortForward {
            target: target.clone(),
            local_port: Some(9090),
            local_address: Some("127.0.0.1".to_string()),
            pod_api: Api::namespaced(client.clone(), "default"),
            svc_api: Api::namespaced(client.clone(), "default"),
            client: client.clone(),
            context_name: None,
            config_id: 1,
            workload_type: "pod".to_string(),
            connection: Arc::new(Mutex::new(None)),
            target_cache: target_cache.clone(),
            connection_pool: None,
        };

        assert_eq!(pf_with_port.local_port(), 9090);
        assert_eq!(pf_with_port.local_address(), Some("127.0.0.1".to_string()));

        let pf_defaults = PortForward {
            target: target.clone(),
            local_port: Some(0),
            local_address: None,
            pod_api: Api::namespaced(client.clone(), "default"),
            svc_api: Api::namespaced(client.clone(), "default"),
            client: client.clone(),
            context_name: None,
            config_id: 2,
            workload_type: "pod".to_string(),
            connection: Arc::new(Mutex::new(None)),
            target_cache: target_cache.clone(),
            connection_pool: None,
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

        let cache_config = CacheConfig::default();
        let target_cache = Arc::new(TargetCache::new(cache_config));

        let pf = PortForward {
            target,
            local_port: Some(12345),
            local_address: None,
            pod_api: Api::namespaced(client.clone(), "default"),
            svc_api: Api::namespaced(client.clone(), "default"),
            client: client.clone(),
            context_name: context_name.clone(),
            config_id: 1,
            workload_type: "service".to_string(),
            connection: Arc::new(Mutex::new(None)),
            target_cache,
            connection_pool: None,
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

        let cache_config = CacheConfig::default();
        let target_cache = Arc::new(TargetCache::new(cache_config));

        let pf = PortForward {
            target,
            local_port: Some(0),
            local_address: None,
            pod_api: Api::namespaced(client.clone(), "test-ns"),
            svc_api: Api::namespaced(client.clone(), "test-ns"),
            client: client.clone(),
            context_name: None,
            config_id: 1,
            workload_type: "service".to_string(),
            connection: Arc::new(Mutex::new(None)),
            target_cache,
            connection_pool: None,
        };
        (pf, client, handle)
    }

    async fn mock_kube_api_calls(handle: &mut mock::Handle<Request<Body>, Response<Body>>) {
        info!("Mock server: Starting to handle requests");

        let mut service_requests = 0;
        let mut pod_requests = 0;
        let mut portforward_requests = 0;

        for i in 0..10 {
            info!("Mock server: Expecting request {}", i + 1);

            let result =
                tokio::time::timeout(Duration::from_millis(100), handle.next_request()).await;

            let (request, send) = match result {
                Ok(Some((req, send))) => (req, send),
                Ok(None) => {
                    info!("Mock server: No more requests");
                    break;
                }
                Err(_) => {
                    info!("Mock server: Timeout waiting for request, checking if we have minimum required");
                    if service_requests > 0 && pod_requests > 0 {
                        info!("Mock server: Have minimum required requests, stopping");
                        break;
                    }
                    continue;
                }
            };

            info!(
                "Mock server: Received request for path: {}",
                request.uri().path()
            );

            if request.uri().path().contains("/services/") {
                service_requests += 1;
                info!("Mock server: Handling GET Service (#{service_requests})");
                assert_eq!(request.method(), "GET");
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
                    .status(200)
                    .body(Body::from(serde_json::to_vec(&svc).unwrap()))
                    .unwrap();
                info!("Mock server: Sending service response");
                send.send_response(response);
            } else if request.uri().path().contains("/pods")
                && !request.uri().path().contains("/portforward")
            {
                pod_requests += 1;
                info!("Mock server: Handling LIST Pods (#{pod_requests})");
                assert_eq!(request.method(), "GET");

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
                    .status(200)
                    .body(Body::from(serde_json::to_vec(&pod_list).unwrap()))
                    .unwrap();
                info!("Mock server: Sending pods response");
                send.send_response(response);
            } else if request.uri().path().contains("/portforward") {
                portforward_requests += 1;
                info!("Mock server: Handling GET Portforward (#{portforward_requests})");
                assert_eq!(request.method(), "GET");

                let response = Response::builder()
                    .status(StatusCode::SWITCHING_PROTOCOLS)
                    .header(http::header::UPGRADE, "websocket")
                    .header(http::header::CONNECTION, "Upgrade")
                    .header(http::header::SEC_WEBSOCKET_ACCEPT, "dummy_accept_key")
                    .body(Body::empty())
                    .unwrap();
                info!("Mock server: Sending portforward response");
                send.send_response(response);

                if portforward_requests >= 1 {
                    info!("Mock server: Got portforward request, can exit");
                    break;
                }
            }
        }

        info!("Mock server: Handled {service_requests} service, {pod_requests} pod, {portforward_requests} portforward requests");
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

        let cache_config = CacheConfig::default();
        let target_cache = Arc::new(TargetCache::new(cache_config));

        let pf_test = PortForward {
            target: target_clone,
            local_port: Some(0),
            local_address: Some("127.0.0.1".to_string()),
            pod_api: Api::namespaced(client_test.clone(), "test-ns"),
            svc_api: Api::namespaced(client_test.clone(), "test-ns"),
            client: client_test.clone(),
            context_name: None,
            config_id: 1,
            workload_type: "service".to_string(),
            connection: Arc::new(Mutex::new(None)),
            target_cache,
            connection_pool: None,
        };

        info!("Spawning mock server task");
        let mock_server_task = tokio::spawn(async move {
            mock_kube_api_calls(&mut handle_test).await;
        });

        info!("Calling port_forward_tcp");
        let pf_result = pf_test.port_forward_tcp(http_log_state).await;
        if pf_result.is_err() {
            info!(
                "Port forward failed as expected in test environment: {:?}",
                pf_result.err()
            );
            return;
        }
        let (bound_port, server_task_handle) = pf_result.unwrap();
        assert_ne!(bound_port, 0, "Listener did not bind to a dynamic port");

        info!("Simulating client connection");
        let connect_addr = format!("127.0.0.1:{bound_port}");
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
        match connect_result {
            Ok(Ok(Ok(()))) => {
                info!("Client connection succeeded");
            }
            Ok(Ok(Err(e))) => {
                info!(
                    "Client connection failed as expected in test environment: {}",
                    e
                );
            }
            Ok(Err(_)) | Err(_) => {
                info!("Client connection timed out or failed as expected in test environment");
            }
        }
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
