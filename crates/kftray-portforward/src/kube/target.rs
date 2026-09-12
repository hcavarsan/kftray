use std::time::Duration;

use k8s_openapi::api::core::v1::{
    Pod,
    Service,
};
use kube::api::Api;
use kube_portforward::{
    Forwarder,
    PodSelector,
};

use crate::kube::models::{
    Port,
    Target,
    TargetSelector,
};

pub async fn resolve_pod_selector(
    client: &kube::Client, namespace: &str, target: &Target,
) -> anyhow::Result<PodSelector> {
    match &target.selector {
        // An empty Kubernetes label selector matches every pod in the
        // namespace, so a pod configuration without a target would forward to
        // an unrelated workload instead of reporting itself as invalid.
        TargetSelector::PodLabel(label_selector) if label_selector.trim().is_empty() => {
            Err(anyhow::anyhow!("Pod configuration has no label selector"))
        }
        TargetSelector::PodLabel(label_selector) => Ok(PodSelector::Labels {
            selector: label_selector.clone(),
        }),
        TargetSelector::ServiceName(service_name) => {
            let service_api: Api<Service> = Api::namespaced(client.clone(), namespace);
            let service = service_api
                .get(service_name)
                .await
                .map_err(|e| anyhow::anyhow!("Service '{}' not found: {}", service_name, e))?;

            let selector = service
                .spec
                .as_ref()
                .and_then(|spec| spec.selector.as_ref())
                .filter(|selector| !selector.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Service '{}' has no selector", service_name))?;

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

            Ok(PodSelector::Labels {
                selector: label_selector,
            })
        }
    }
}

pub async fn resolve_target_port(
    forwarder: &Forwarder, pod_api: &Api<Pod>, target: &Target, timeout: Duration,
) -> anyhow::Result<u16> {
    match &target.port {
        Port::Number(port) => match u16::try_from(*port) {
            Ok(port) if port > 0 => Ok(port),
            _ => Err(anyhow::anyhow!("Port number {} is out of range", port)),
        },
        Port::Name(port_name) => {
            let pod_name = forwarder.wait_for_ready_pod(timeout).await.ok_or_else(|| {
                anyhow::anyhow!(
                    "No ready pods available to resolve port name '{}'",
                    port_name
                )
            })?;

            let pod = pod_api
                .get(&pod_name)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch pod '{}': {}", pod_name, e))?;

            target
                .find(&pod, None)
                .map(|target_pod| target_pod.port_number)
        }
    }
}

#[cfg(test)]
mod tests {
    use http::{
        Request,
        Response,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::client::Body;
    use tower_test::mock;

    use super::*;

    #[tokio::test]
    async fn resolve_pod_selector_passes_through_pod_label() {
        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let target = Target::new(
            TargetSelector::PodLabel("app=foo,tier=web".to_string()),
            8080,
            "default",
        );

        let selector = resolve_pod_selector(&client, "default", &target)
            .await
            .unwrap();

        match selector {
            PodSelector::Labels { selector } => assert_eq!(selector, "app=foo,tier=web"),
            other => panic!("expected PodSelector::Labels, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_pod_selector_looks_up_service_selector() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let target = Target::new(
            TargetSelector::ServiceName("my-svc".to_string()),
            8080,
            "default",
        );

        let server = tokio::spawn(async move {
            let (request, send) = handle.next_request().await.unwrap();
            assert!(request.uri().path().contains("/services/my-svc"));

            let mut selector = std::collections::BTreeMap::new();
            selector.insert("app".to_string(), "foo".to_string());
            selector.insert("tier".to_string(), "web".to_string());

            let service = Service {
                metadata: ObjectMeta {
                    name: Some("my-svc".to_string()),
                    namespace: Some("default".to_string()),
                    ..Default::default()
                },
                spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
                    selector: Some(selector),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let response = Response::builder()
                .status(200)
                .body(Body::from(serde_json::to_vec(&service).unwrap()))
                .unwrap();
            send.send_response(response);
        });

        let selector = resolve_pod_selector(&client, "default", &target)
            .await
            .unwrap();
        server.await.unwrap();

        match selector {
            PodSelector::Labels { selector } => assert_eq!(selector, "app=foo,tier=web"),
            other => panic!("expected PodSelector::Labels, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_pod_selector_errors_when_service_has_no_selector() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let target = Target::new(
            TargetSelector::ServiceName("headless-svc".to_string()),
            8080,
            "default",
        );

        let server = tokio::spawn(async move {
            let (_request, send) = handle.next_request().await.unwrap();

            let service = Service {
                metadata: ObjectMeta {
                    name: Some("headless-svc".to_string()),
                    namespace: Some("default".to_string()),
                    ..Default::default()
                },
                spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
                    selector: None,
                    ..Default::default()
                }),
                ..Default::default()
            };

            let response = Response::builder()
                .status(200)
                .body(Body::from(serde_json::to_vec(&service).unwrap()))
                .unwrap();
            send.send_response(response);
        });

        let result = resolve_pod_selector(&client, "default", &target).await;
        server.await.unwrap();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resolve_pod_selector_errors_when_service_selector_is_empty() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let target = Target::new(
            TargetSelector::ServiceName("empty-selector-svc".to_string()),
            8080,
            "default",
        );

        let server = tokio::spawn(async move {
            let (_request, send) = handle.next_request().await.unwrap();

            let service = Service {
                metadata: ObjectMeta {
                    name: Some("empty-selector-svc".to_string()),
                    namespace: Some("default".to_string()),
                    ..Default::default()
                },
                spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
                    selector: Some(std::collections::BTreeMap::new()),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let response = Response::builder()
                .status(200)
                .body(Body::from(serde_json::to_vec(&service).unwrap()))
                .unwrap();
            send.send_response(response);
        });

        let result = resolve_pod_selector(&client, "default", &target).await;
        server.await.unwrap();

        assert!(
            result.is_err(),
            "an empty selector must not fall through to a namespace-wide PodSelector::Labels"
        );
    }
}
