use anyhow::Result;
use kube::api::{
    Api,
    ListParams,
};
use tracing::debug;

use crate::kube::models::{
    AnyReady,
    PodSelection,
    Target,
    TargetPod,
    TargetSelector,
};
pub struct TargetPodFinder<'a> {
    pub pod_api: &'a Api<k8s_openapi::api::core::v1::Pod>,
    pub svc_api: &'a Api<k8s_openapi::api::core::v1::Service>,
}

impl TargetPodFinder<'_> {
    pub(crate) async fn find(&self, target: &Target) -> Result<TargetPod> {
        let ready_pod = AnyReady {};

        match &target.selector {
            TargetSelector::ServiceName(name) => {
                self.find_pod_by_service_name(name, &ready_pod, target)
                    .await
            }
            TargetSelector::PodLabel(label) => {
                self.find_pod_by_label(label, &ready_pod, target).await
            }
        }
    }

    async fn find_pod_by_service_name(
        &self, name: &str, ready_pod: &AnyReady, target: &Target,
    ) -> Result<TargetPod> {
        match self.svc_api.get(name).await {
            Ok(service) => {
                if let Some(selector) = service.spec.and_then(|spec| spec.selector) {
                    let label_selector_str = selector
                        .iter()
                        .map(|(key, value)| format!("{}={}", key, value))
                        .collect::<Vec<_>>()
                        .join(",");

                    let pods = self
                        .pod_api
                        .list(&ListParams::default().labels(&label_selector_str))
                        .await?;

                    let pod = ready_pod.select(&pods.items, &label_selector_str)?;
                    target.find(pod, None)
                } else {
                    Err(anyhow::anyhow!("No selector found for service '{}'", name))
                }
            }
            Err(kube::Error::Api(kube::error::ErrorResponse { code: 404, .. })) => {
                let label_selector_str = format!("app={}", name);

                debug!(
                    "Using service name as label selector: {}",
                    label_selector_str
                );

                let pods = self
                    .pod_api
                    .list(&ListParams::default().labels(&label_selector_str))
                    .await?;

                let pod = ready_pod.select(&pods.items, &label_selector_str)?;
                target.find(pod, None)
            }
            Err(e) => Err(anyhow::anyhow!("Error finding service '{}': {}", name, e)),
        }
    }

    async fn find_pod_by_label(
        &self, label: &str, ready_pod: &AnyReady, target: &Target,
    ) -> Result<TargetPod> {
        let label_selector_str = label.to_string();
        let pods = self
            .pod_api
            .list(&ListParams::default().labels(&label_selector_str))
            .await?;

        let pod = ready_pod.select(&pods.items, &label_selector_str)?;

        target.find(pod, None)
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
        ServiceSpec,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::List;
    use kube::client::Body;
    use tower_test::mock;

    use super::*;
    use crate::kube::models::{
        NameSpace,
        Port,
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

    #[tokio::test]
    async fn test_find_pod_by_service_name() {
        let pod = mock_pod(
            "test-pod",
            "test-ns",
            Some(
                [("app".to_string(), "my-app".to_string())]
                    .into_iter()
                    .collect(),
            ),
            true,
        );

        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();

        tokio::spawn(async move {
            if let Some((request, send)) = handle.next_request().await {
                assert_eq!(
                    request.uri().path(),
                    "/api/v1/namespaces/test-ns/services/test-svc"
                );

                let selector = Some(
                    [("app".to_string(), "my-app".to_string())]
                        .into_iter()
                        .collect(),
                );
                let svc = Service {
                    metadata: ObjectMeta {
                        name: Some("test-svc".to_string()),
                        namespace: Some("test-ns".to_string()),
                        ..Default::default()
                    },
                    spec: Some(ServiceSpec {
                        selector,
                        ports: None,
                        ..Default::default()
                    }),
                    ..Default::default()
                };

                let response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from(serde_json::to_vec(&svc).unwrap()))
                    .unwrap();

                send.send_response(response);
            }

            if let Some((request, send)) = handle.next_request().await {
                assert_eq!(request.uri().path(), "/api/v1/namespaces/test-ns/pods");

                let pod_list = List {
                    items: vec![pod],
                    ..Default::default()
                };

                let response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from(serde_json::to_vec(&pod_list).unwrap()))
                    .unwrap();

                send.send_response(response);
            }
        });

        let client = kube::Client::new(mock_service, "test-ns");
        let pod_api = Api::namespaced(client.clone(), "test-ns");
        let svc_api = Api::namespaced(client.clone(), "test-ns");

        let finder = TargetPodFinder {
            pod_api: &pod_api,
            svc_api: &svc_api,
        };

        let target = Target {
            selector: TargetSelector::ServiceName("test-svc".to_string()),
            port: Port::Number(80),
            namespace: NameSpace(Some("test-ns".to_string())),
        };

        let result = finder.find(&target).await;
        assert!(result.is_ok());

        let target_pod = result.unwrap();
        assert_eq!(target_pod.pod_name, "test-pod");
        assert_eq!(target_pod.port_number, 80);
    }

    #[tokio::test]
    async fn test_find_pod_by_label() {
        let pod = mock_pod(
            "test-pod",
            "test-ns",
            Some(
                [("app".to_string(), "my-app".to_string())]
                    .into_iter()
                    .collect(),
            ),
            true,
        );

        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();

        tokio::spawn(async move {
            if let Some((request, send)) = handle.next_request().await {
                assert_eq!(request.uri().path(), "/api/v1/namespaces/test-ns/pods");

                let pod_list = List {
                    items: vec![pod],
                    ..Default::default()
                };

                let response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from(serde_json::to_vec(&pod_list).unwrap()))
                    .unwrap();

                send.send_response(response);
            }
        });

        let client = kube::Client::new(mock_service, "test-ns");
        let pod_api = Api::namespaced(client.clone(), "test-ns");
        let svc_api = Api::namespaced(client.clone(), "test-ns");

        let finder = TargetPodFinder {
            pod_api: &pod_api,
            svc_api: &svc_api,
        };

        let target = Target {
            selector: TargetSelector::PodLabel("app=my-app".to_string()),
            port: Port::Number(80),
            namespace: NameSpace(Some("test-ns".to_string())),
        };

        let result = finder.find(&target).await;
        assert!(result.is_ok());

        let target_pod = result.unwrap();
        assert_eq!(target_pod.pod_name, "test-pod");
        assert_eq!(target_pod.port_number, 80);
    }

    #[tokio::test]
    async fn test_find_pod_by_service_name_not_found() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();

        tokio::spawn(async move {
            if let Some((request, send)) = handle.next_request().await {
                assert_eq!(
                    request.uri().path(),
                    "/api/v1/namespaces/test-ns/services/nonexistent-svc"
                );

                let response = Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap();

                send.send_response(response);
            }

            if let Some((request, send)) = handle.next_request().await {
                assert_eq!(request.uri().path(), "/api/v1/namespaces/test-ns/pods");

                let pod_list = List::<Pod> {
                    items: vec![],
                    ..Default::default()
                };

                let response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from(serde_json::to_vec(&pod_list).unwrap()))
                    .unwrap();

                send.send_response(response);
            }
        });

        let client = kube::Client::new(mock_service, "test-ns");
        let pod_api = Api::namespaced(client.clone(), "test-ns");
        let svc_api = Api::namespaced(client.clone(), "test-ns");

        let finder = TargetPodFinder {
            pod_api: &pod_api,
            svc_api: &svc_api,
        };

        let target = Target {
            selector: TargetSelector::ServiceName("nonexistent-svc".to_string()),
            port: Port::Number(80),
            namespace: NameSpace(Some("test-ns".to_string())),
        };

        let result = finder.find(&target).await;
        assert!(result.is_err());
    }
}
