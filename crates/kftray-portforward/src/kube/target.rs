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

/// Marker substring identifying the "no ready pods" resolution failure, so
/// consumers (e.g. kftray-network-monitor) can classify it without pattern
/// matching on the full formatted message.
pub const NO_READY_PODS_ERROR: &str = "No ready pods available";

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
        TargetSelector::ServiceName(service_name) if service_name.trim().is_empty() => {
            Err(anyhow::anyhow!("Pod configuration has no service name"))
        }
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

/// Resolves the port and reports the pod it was read from.
///
/// The pod identity matters for a named port: the name maps to a number in that
/// pod's spec, and a rollout can map it to a different number. A caller that
/// connects afterwards has to know whether it is still talking to the same pod.
pub async fn resolve_target_port_for_pod(
    forwarder: &Forwarder, pod_api: &Api<Pod>, target: &Target, timeout: Duration,
) -> anyhow::Result<(u16, Option<kube_portforward::ReadyPod>)> {
    match &target.port {
        Port::Number(port) => match u16::try_from(*port) {
            Ok(port) if port > 0 => Ok((port, None)),
            _ => Err(anyhow::anyhow!("Port number {} is out of range", port)),
        },
        Port::Name(port_name) => {
            // A pod can go between being selected as ready and being read: a
            // rollout retires it in that window. Selection is repeated within
            // the same deadline rather than failing the resolution, since a
            // replacement is usually ready by then. Any other error stands.
            let deadline = tokio::time::Instant::now() + timeout;
            let mut attempt: u32 = 0;
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                let pod_name = forwarder
                    .wait_for_ready_pod(remaining)
                    .await
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "{NO_READY_PODS_ERROR} to resolve port name '{}'",
                            port_name
                        )
                    })?;

                let pod = match pod_api.get(&pod_name).await {
                    Ok(pod) => pod,
                    Err(kube::Error::Api(response)) if response.code == 404 => {
                        let backoff = Duration::from_millis(200)
                            .saturating_mul(attempt + 1)
                            .min(Duration::from_secs(2));
                        let time_left =
                            deadline.saturating_duration_since(tokio::time::Instant::now());
                        if time_left <= backoff {
                            return Err(anyhow::anyhow!(
                                "Pod '{}' was retired before its port '{}' could be resolved \
                                 and the deadline ran out while waiting for a replacement",
                                pod_name,
                                port_name
                            ));
                        }
                        log::debug!(
                            "Pod '{pod_name}' went away before its port could be read; \
                             selecting again (attempt {attempt})"
                        );
                        attempt += 1;
                        // Gives the watch a moment to notice the retirement,
                        // so a stale selection is not re-read in a tight loop.
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to fetch pod '{}': {}", pod_name, e));
                    }
                };

                // The UID comes from the pod the number was read from, so a
                // replacement reusing the name is still a different identity.
                let identity = kube_portforward::ReadyPod::new(pod_name, pod.metadata.uid.clone());

                return target
                    .find(&pod, None)
                    .map(|target_pod| (target_pod.port_number, Some(identity)));
            }
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

    #[tokio::test]
    async fn resolve_pod_selector_errors_when_service_name_is_blank() {
        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let target = Target::new(
            TargetSelector::ServiceName("   ".to_string()),
            8080,
            "default",
        );

        let err = resolve_pod_selector(&client, "default", &target)
            .await
            .expect_err("a blank service name must not resolve a selector");
        assert_eq!(err.to_string(), "Pod configuration has no service name");
    }

    fn ready_pod_json(name: &str) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": name,
                "uid": format!("{name}-uid"),
                "resourceVersion": "1",
            },
            "status": {
                "phase": "Running",
                "conditions": [{"type": "Ready", "status": "True"}],
            },
        })
    }

    #[tokio::test]
    async fn resolve_target_port_for_pod_reports_the_retired_pod_when_the_deadline_runs_out() {
        let pod_name = "retired-pod";
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");

        let driver = tokio::spawn({
            let pod_name = pod_name.to_string();
            async move {
                let mut listed_once = false;
                let mut held_watches = Vec::new();
                while let Some((request, send)) = handle.next_request().await {
                    let path = request.uri().path().to_string();
                    if path.ends_with(&format!("/pods/{pod_name}")) {
                        // The pod object itself is gone: every read 404s.
                        let body = serde_json::json!({
                            "kind": "Status",
                            "apiVersion": "v1",
                            "status": "Failure",
                            "message": format!("pods \"{pod_name}\" not found"),
                            "reason": "NotFound",
                            "code": 404,
                        });
                        send.send_response(
                            Response::builder()
                                .status(404)
                                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                                .unwrap(),
                        );
                    } else if !listed_once {
                        // Initial list: report the pod ready so the selector
                        // resolves once, before it is found retired.
                        listed_once = true;
                        let body = serde_json::json!({
                            "apiVersion": "v1",
                            "kind": "PodList",
                            "metadata": { "resourceVersion": "1" },
                            "items": [ready_pod_json(&pod_name)],
                        });
                        send.send_response(
                            Response::builder()
                                .status(200)
                                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                                .unwrap(),
                        );
                    } else {
                        // A follow-up watch on the collection: left open so the
                        // reflector's cached ready pod is never invalidated.
                        held_watches.push(send);
                    }
                }
            }
        });

        let forwarder = kube_portforward::Forwarder::builder(
            client.clone(),
            "http://127.0.0.1:1".parse().unwrap(),
            "default",
        )
        .pod_selector(kube_portforward::PodSelector::Name(pod_name.to_string()))
        .build()
        .await
        .expect("forwarder should build without contacting the apiserver");

        let pod_api: Api<Pod> = Api::namespaced(client, "default");
        let target = Target::new(
            TargetSelector::PodLabel("app=web".to_string()),
            "http",
            "default",
        );

        let result =
            resolve_target_port_for_pod(&forwarder, &pod_api, &target, Duration::from_millis(350))
                .await;

        driver.abort();
        let _ = driver.await;

        let err = result.expect_err("a pod that keeps 404ing must not resolve a port");
        let message = err.to_string();
        assert!(
            message.contains(pod_name),
            "error should name the retired pod: {message}"
        );
        assert!(
            message.contains("deadline"),
            "error should explain the retry deadline ran out: {message}"
        );
        assert!(
            !message.contains("Failed to fetch pod"),
            "must not fall through to the generic fetch-failure path when retiring near the \
             deadline: {message}"
        );
        assert!(
            !message.contains(NO_READY_PODS_ERROR),
            "a pod found retired must report that, not the generic no-ready-pods error: {message}"
        );
    }
}
