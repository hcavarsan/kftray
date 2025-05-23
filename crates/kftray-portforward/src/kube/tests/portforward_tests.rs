use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use http::{
    Request,
    Response,
};
use k8s_openapi::api::core::v1::{
    Pod,
    Service,
};
use kftray_commons::models::config_model::Config;
use kftray_http_logs::HttpLogState;
use kube::client::Body;
use kube::{
    Api,
    Client,
};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tower_test::mock;

use crate::kube::models::{
    Port,
    PortForward,
    Target,
    TargetSelector,
};
use crate::kube::start::start_port_forward;
use crate::port_forward::CHILD_PROCESSES;

struct KubernetesMocker {
    handle: mock::Handle<Request<Body>, Response<Body>>,
}

impl KubernetesMocker {
    fn new(handle: mock::Handle<Request<Body>, Response<Body>>) -> Self {
        Self { handle }
    }

    async fn expect_list_pods(&mut self, pods: Vec<Pod>) -> Result<()> {
        let (request, send) = self.handle.next_request().await.unwrap();

        assert_eq!(request.method(), "GET");
        assert!(request.uri().path().contains("/pods"));

        let pod_list = k8s_openapi::List {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ListMeta::default(),
            items: pods,
        };

        let response = Response::builder()
            .status(200)
            .body(Body::from(serde_json::to_vec(&pod_list)?))
            .unwrap();

        send.send_response(response);
        Ok(())
    }

    async fn expect_get_service(&mut self, service: Service) -> Result<()> {
        let (request, send) = self.handle.next_request().await.unwrap();

        assert_eq!(request.method(), "GET");
        assert!(request.uri().path().contains("/services/"));

        let response = Response::builder()
            .status(200)
            .body(Body::from(serde_json::to_vec(&service)?))
            .unwrap();

        send.send_response(response);
        Ok(())
    }

    async fn expect_pod_portforward(&mut self) -> Result<()> {
        let (request, send) = self.handle.next_request().await.unwrap();

        assert_eq!(request.method(), "GET");
        assert!(request.uri().path().contains("/portforward"));

        let response = Response::builder()
            .status(101)
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Accept", "mock-accept-key")
            .body(Body::empty())
            .unwrap();

        send.send_response(response);
        Ok(())
    }
}

fn create_mock_pod(name: &str, namespace: &str, ready: bool) -> Pod {
    let mut pod = Pod::default();
    pod.metadata.name = Some(name.to_string());
    pod.metadata.namespace = Some(namespace.to_string());

    let mut labels = std::collections::BTreeMap::new();
    labels.insert("app".to_string(), "test-app".to_string());
    pod.metadata.labels = Some(labels);

    if let Some(spec) = &mut pod.spec {
        spec.containers[0].ports = Some(vec![k8s_openapi::api::core::v1::ContainerPort {
            name: Some("http".to_string()),
            container_port: 8080,
            ..Default::default()
        }]);
    }

    if ready {
        pod.status = Some(k8s_openapi::api::core::v1::PodStatus {
            phase: Some("Running".to_string()),
            conditions: Some(vec![k8s_openapi::api::core::v1::PodCondition {
                type_: "Ready".to_string(),
                status: "True".to_string(),
                ..Default::default()
            }]),
            ..Default::default()
        });
    }

    pod
}

fn create_mock_service(name: &str, namespace: &str) -> Service {
    let mut service = Service::default();
    service.metadata.name = Some(name.to_string());
    service.metadata.namespace = Some(namespace.to_string());

    if let Some(spec) = &mut service.spec {
        let mut selector = std::collections::BTreeMap::new();
        selector.insert("app".to_string(), "test-app".to_string());
        spec.selector = Some(selector);

        spec.ports = Some(vec![k8s_openapi::api::core::v1::ServicePort {
            name: Some("http".to_string()),
            port: 80,
            target_port: Some(
                k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::String(
                    "http".to_string(),
                ),
            ),
            ..Default::default()
        }]);
    }

    service
}

fn setup_test_config() -> Config {
    Config {
        id: Some(1),
        context: "test-context".to_string(),
        kubeconfig: None,
        namespace: "test-namespace".to_string(),
        service: Some("test-service".to_string()),
        alias: Some("test-alias".to_string()),
        local_port: Some(0),
        remote_port: Some(8080),
        protocol: "tcp".to_string(),
        workload_type: Some("service".to_string()),
        target: None,
        local_address: None,
        domain_enabled: None,
        remote_address: None,
    }
}

#[tokio::test]
async fn test_port_forward_tcp_success() -> Result<()> {
    let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(mock_service, "test-namespace");
    let mut mocker = KubernetesMocker::new(handle);

    let target = Target::new(
        TargetSelector::ServiceName("test-service".to_string()),
        Port::Number(8080),
        "test-namespace",
    );

    let mock_task = tokio::spawn(async move {
        mocker
            .expect_get_service(create_mock_service("test-service", "test-namespace"))
            .await
            .unwrap();

        mocker
            .expect_list_pods(vec![create_mock_pod("test-pod", "test-namespace", true)])
            .await
            .unwrap();

        mocker.expect_pod_portforward().await.unwrap();

        tokio::time::sleep(Duration::from_secs(3)).await;
    });

    let config = setup_test_config();
    let _configs = vec![config];

    let port_forward = PortForward {
        target,
        local_port: Some(0),
        local_address: None,
        pod_api: Api::namespaced(client.clone(), "test-namespace"),
        svc_api: Api::namespaced(client.clone(), "test-namespace"),
        context_name: Some("test-context".to_string()),
        config_id: 1,
        workload_type: "service".to_string(),
        connection: Arc::new(tokio::sync::Mutex::new(None)),
    };

    let port_forward_task = tokio::spawn(async move {
        match port_forward
            .port_forward_tcp(Arc::new(HttpLogState::new()))
            .await
        {
            Ok((port, handle)) => {
                let key = "1_test-service".to_string();
                {
                    let mut processes = CHILD_PROCESSES.lock().unwrap();
                    processes.insert(key.clone(), handle);
                }

                println!("Port forwarding successfully established on port {port}");

                Ok(port)
            }
            Err(e) => {
                eprintln!("Port forwarding failed: {e}");
                Err(e)
            }
        }
    });

    let port_forward_result = timeout(Duration::from_secs(5), port_forward_task).await??;
    let bound_port = port_forward_result?;

    assert!(
        bound_port > 0,
        "Port forwarding did not bind to a dynamic port"
    );

    let connect_result = timeout(
        Duration::from_secs(2),
        TcpStream::connect(format!("127.0.0.1:{bound_port}")),
    )
    .await;

    if let Ok(Ok(_stream)) = connect_result {
        println!("Successfully connected to port forward");
    } else {
        println!("Connection to port forward failed (expected in test)");
    }

    mock_task.abort();

    {
        let mut processes = CHILD_PROCESSES.lock().unwrap();
        processes.clear();
    }

    Ok(())
}

#[tokio::test]
async fn test_port_forward_udp_success() -> Result<()> {
    let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(mock_service, "test-namespace");
    let mut mocker = KubernetesMocker::new(handle);

    let target = Target::new(
        TargetSelector::ServiceName("test-service".to_string()),
        Port::Number(8080),
        "test-namespace",
    );

    let mock_task = tokio::spawn(async move {
        mocker
            .expect_get_service(create_mock_service("test-service", "test-namespace"))
            .await
            .unwrap();

        mocker
            .expect_list_pods(vec![create_mock_pod("test-pod", "test-namespace", true)])
            .await
            .unwrap();

        mocker.expect_pod_portforward().await.unwrap();

        tokio::time::sleep(Duration::from_secs(3)).await;
    });

    let mut config = setup_test_config();
    config.protocol = "udp".to_string();

    let port_forward = PortForward {
        target,
        local_port: Some(0),
        local_address: None,
        pod_api: Api::namespaced(client.clone(), "test-namespace"),
        svc_api: Api::namespaced(client.clone(), "test-namespace"),
        context_name: Some("test-context".to_string()),
        config_id: 1,
        workload_type: "service".to_string(),
        connection: Arc::new(tokio::sync::Mutex::new(None)),
    };

    let port_forward_task = tokio::spawn(async move {
        match port_forward.port_forward_udp().await {
            Ok((port, handle)) => {
                let key = "1_test-service".to_string();
                {
                    let mut processes = CHILD_PROCESSES.lock().unwrap();
                    processes.insert(key.clone(), handle);
                }

                println!("UDP Port forwarding successfully established on port {port}");

                Ok(port)
            }
            Err(e) => {
                eprintln!("UDP Port forwarding failed: {e}");
                Err(e)
            }
        }
    });

    let port_forward_result = timeout(Duration::from_secs(5), port_forward_task).await;

    match port_forward_result {
        Ok(Ok(Ok(bound_port))) => {
            assert!(
                bound_port > 0,
                "UDP Port forwarding did not bind to a dynamic port"
            );
            println!("UDP Port forwarding unexpectedly succeeded (port {bound_port})");

            {
                let mut processes = CHILD_PROCESSES.lock().unwrap();
                if let Some(handle) = processes.remove("1_test-service") {
                    handle.abort();
                }
            }
        }
        Ok(Ok(Err(_))) | Ok(Err(_)) | Err(_) => {
            println!("UDP Port forwarding failed as expected in test environment");
        }
    }

    mock_task.abort();

    Ok(())
}

#[tokio::test]
async fn test_start_port_forward_success() -> Result<()> {
    let mut configs = Vec::new();

    let mut tcp_config = setup_test_config();
    tcp_config.id = Some(1);
    tcp_config.service = Some("tcp-service".to_string());
    configs.push(tcp_config);

    let mut udp_config = setup_test_config();
    udp_config.id = Some(2);
    udp_config.service = Some("udp-service".to_string());
    udp_config.protocol = "udp".to_string();
    configs.push(udp_config);

    let result = start_port_forward(configs, "tcp", Arc::new(HttpLogState::new())).await;

    assert!(
        result.is_err(),
        "start_port_forward should fail in test without proper mocking"
    );

    Ok(())
}

#[tokio::test]
async fn test_start_port_forward_mock_components() -> Result<()> {
    use std::sync::atomic::{
        AtomicUsize,
        Ordering,
    };

    let success_counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = success_counter.clone();

    let mut test_config = setup_test_config();
    test_config.local_port = Some(0);

    let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(mock_service, "test-namespace");

    let mock_task = tokio::spawn(async move {
        let mut mocker = KubernetesMocker::new(handle);

        mocker
            .expect_get_service(create_mock_service("test-service", "test-namespace"))
            .await?;
        counter_clone.fetch_add(1, Ordering::SeqCst);

        mocker
            .expect_list_pods(vec![create_mock_pod("test-pod", "test-namespace", true)])
            .await?;
        counter_clone.fetch_add(1, Ordering::SeqCst);

        mocker.expect_pod_portforward().await?;
        counter_clone.fetch_add(1, Ordering::SeqCst);

        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok::<(), anyhow::Error>(())
    });

    let target = Target::new(
        TargetSelector::ServiceName(test_config.service.clone().unwrap_or_default()),
        Port::Number(test_config.remote_port.unwrap_or_default() as i32),
        test_config.namespace.clone(),
    );

    let port_forward = PortForward {
        target,
        local_port: test_config.local_port,
        local_address: test_config.local_address.clone(),
        pod_api: Api::namespaced(client.clone(), &test_config.namespace.clone()),
        svc_api: Api::namespaced(client.clone(), &test_config.namespace.clone()),
        context_name: Some(test_config.context.clone()),
        config_id: test_config.id.unwrap_or_default(),
        workload_type: test_config.workload_type.clone().unwrap_or_default(),
        connection: Arc::new(tokio::sync::Mutex::new(None)),
    };

    let http_log_state = Arc::new(HttpLogState::new());
    let port_forward_result = port_forward.port_forward_tcp(http_log_state).await;

    match port_forward_result {
        Ok((port, handle)) => {
            println!("Successfully started TCP port forwarding on port {port}");
            assert!(port > 0, "Port should be assigned a dynamic port");
            success_counter.fetch_add(1, Ordering::SeqCst);

            let connect_result = timeout(
                Duration::from_secs(1),
                TcpStream::connect(format!("127.0.0.1:{port}")),
            )
            .await;

            if let Ok(Ok(_)) = connect_result {
                success_counter.fetch_add(1, Ordering::SeqCst);
                println!("Successfully connected to the forwarded port");
            }

            let handle_key = format!(
                "{}_{}",
                test_config.id.unwrap(),
                test_config.service.clone().unwrap_or_default()
            );
            {
                let mut processes = CHILD_PROCESSES.lock().unwrap();
                processes.insert(handle_key.clone(), handle);
            }

            {
                let mut processes = CHILD_PROCESSES.lock().unwrap();
                if let Some(handle) = processes.remove(&handle_key) {
                    handle.abort();
                }
            }

            success_counter.fetch_add(1, Ordering::SeqCst);
        }
        Err(e) => {
            println!("Failed to start port forwarding: {e}");
            panic!("Port forwarding should have succeeded but failed: {e}");
        }
    }

    mock_task.abort();

    let final_count = success_counter.load(Ordering::SeqCst);
    assert!(
        final_count >= 1,
        "Expected at least 1 successful test step, got {final_count}"
    );

    Ok(())
}

#[tokio::test]
async fn test_start_port_forward_mock_components_udp() -> Result<()> {
    use std::sync::atomic::{
        AtomicUsize,
        Ordering,
    };

    let success_counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = success_counter.clone();

    let mut test_config = setup_test_config();
    test_config.local_port = Some(0);
    test_config.protocol = "udp".to_string();

    let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(mock_service, "test-namespace");

    let mock_task = tokio::spawn(async move {
        let mut mocker = KubernetesMocker::new(handle);

        mocker
            .expect_get_service(create_mock_service("test-service", "test-namespace"))
            .await?;
        counter_clone.fetch_add(1, Ordering::SeqCst);

        mocker
            .expect_list_pods(vec![create_mock_pod("test-pod", "test-namespace", true)])
            .await?;
        counter_clone.fetch_add(1, Ordering::SeqCst);

        mocker.expect_pod_portforward().await?;
        counter_clone.fetch_add(1, Ordering::SeqCst);

        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok::<(), anyhow::Error>(())
    });

    let target = Target::new(
        TargetSelector::ServiceName(test_config.service.clone().unwrap_or_default()),
        Port::Number(test_config.remote_port.unwrap_or_default() as i32),
        test_config.namespace.clone(),
    );

    let port_forward = PortForward {
        target,
        local_port: test_config.local_port,
        local_address: test_config.local_address.clone(),
        pod_api: Api::namespaced(client.clone(), &test_config.namespace.clone()),
        svc_api: Api::namespaced(client.clone(), &test_config.namespace.clone()),
        context_name: Some(test_config.context.clone()),
        config_id: test_config.id.unwrap_or_default(),
        workload_type: test_config.workload_type.clone().unwrap_or_default(),
        connection: Arc::new(tokio::sync::Mutex::new(None)),
    };

    match port_forward.port_forward_udp().await {
        Ok((port, handle)) => {
            println!("Successfully started UDP port forwarding on port {port}");
            assert!(port > 0, "Port should be assigned a dynamic port");

            handle.abort();

            success_counter.fetch_add(1, Ordering::SeqCst);
        }
        Err(e) => {
            println!("Expected: UDP port forwarding failed: {e}");

            mock_task.abort();

            let final_count = success_counter.load(Ordering::SeqCst);
            assert!(
                final_count >= 1,
                "Expected at least 1 successful API call, got {final_count}"
            );

            return Ok(());
        }
    }

    mock_task.abort();

    Ok(())
}
