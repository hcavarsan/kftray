use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    Pod,
    Service,
};
use kftray_commons::models::config_model::Config;
use kube::api::{
    Api,
    ListParams,
};
use kube::{
    Client,
    ResourceExt,
};
use log::warn;

use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};
use crate::kube::target::{
    service_label_selector,
    service_lookup_error,
};

const POD_NAME_LABEL: &str = "statefulset.kubernetes.io/pod-name";
const HEADLESS_TAG: &str = "headless";

pub async fn headless_pod_configs(source: &Config) -> Result<Vec<Config>, String> {
    let (service_name, port) = source_service(source)?;
    let key = ServiceClientKey::new(source.context.clone(), source.kubeconfig.clone());
    let connection = SHARED_CLIENT_MANAGER
        .get_connection(key)
        .await
        .map_err(|e| e.to_string())?;

    fetch_pod_configs(source, service_name, port, &connection.client).await
}

fn source_service(source: &Config) -> Result<(&str, u16), String> {
    if source.workload_type.as_deref() != Some("service") || source.protocol != "tcp" {
        return Err("Only a TCP service forward can add pod forwards".to_string());
    }
    let service_name = source
        .service
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "The forward has no service name".to_string())?;
    let port = source
        .remote_port
        .filter(|port| *port > 0)
        .ok_or_else(|| "The forward has no remote port".to_string())?;
    Ok((service_name, port))
}

async fn fetch_pod_configs(
    source: &Config, service_name: &str, port: u16, client: &Client,
) -> Result<Vec<Config>, String> {
    let namespace = source.namespace.as_str();
    let service = Api::<Service>::namespaced(client.clone(), namespace)
        .get(service_name)
        .await
        .map_err(|e| service_lookup_error(service_name, &e).to_string())?;

    let spec = service.spec.as_ref();
    if spec.and_then(|spec| spec.cluster_ip.as_deref()) != Some("None") {
        return Err(format!(
            "Service '{service_name}' is not headless, so its pods have no DNS names"
        ));
    }
    let selector = spec
        .and_then(|spec| spec.selector.as_ref())
        .filter(|selector| !selector.is_empty())
        .ok_or_else(|| format!("Service '{service_name}' has no selector"))?;

    let pods = Api::<Pod>::namespaced(client.clone(), namespace)
        .list(&ListParams::default().labels(&service_label_selector(selector)))
        .await
        .map_err(|e| format!("Failed to list the pods of Service '{service_name}': {e}"))?
        .items;

    pod_configs(source, service_name, port, &pods)
}

fn pod_configs(
    source: &Config, service_name: &str, port: u16, pods: &[Pod],
) -> Result<Vec<Config>, String> {
    let mut tags = source.tags.clone();
    tags.insert(HEADLESS_TAG.to_string(), service_name.to_string());

    let configs: Vec<Config> = pods
        .iter()
        .filter_map(|pod| pod_config(source, service_name, port, &tags, pod))
        .collect();

    if configs.is_empty() {
        return Err(format!(
            "No pod of Service '{service_name}' has a DNS name under it and a {POD_NAME_LABEL} \
             label"
        ));
    }
    Ok(configs)
}

fn pod_config(
    source: &Config, service_name: &str, port: u16, tags: &BTreeMap<String, String>, pod: &Pod,
) -> Option<Config> {
    let spec = pod.spec.as_ref()?;
    if spec.subdomain.as_deref() != Some(service_name) {
        return None;
    }
    let hostname = spec.hostname.as_deref().filter(|name| !name.is_empty())?;
    let Some(pod_name) = pod.labels().get(POD_NAME_LABEL) else {
        warn!(
            "Skipping pod '{}' of Service '{service_name}': it has no {POD_NAME_LABEL} label",
            pod.name_any()
        );
        return None;
    };
    let dns_name = format!(
        "{hostname}.{service_name}.{}.svc.cluster.local",
        source.namespace
    );

    Some(Config {
        id: None,
        workload_type: Some("pod".to_string()),
        service: Some(dns_name.clone()),
        target: Some(format!("{POD_NAME_LABEL}={pod_name}")),
        alias: Some(dns_name),
        domain_enabled: Some(true),
        auto_loopback_address: true,
        local_address: None,
        local_port: Some(port),
        remote_port: Some(port),
        remote_address: None,
        tags: tags.clone(),
        ..source.clone()
    })
}

#[cfg(test)]
mod tests {
    use http::{
        Request,
        Response,
    };
    use k8s_openapi::api::core::v1::{
        PodSpec,
        ServicePort,
        ServiceSpec,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
    use kftray_commons::utils::config::insert_new_configs_with_mode;
    use kftray_commons::utils::db_mode::DatabaseMode;
    use kube::client::Body;
    use tower_test::mock;

    use super::*;

    fn source() -> Config {
        Config {
            id: Some(7),
            service: Some("mongo".to_string()),
            namespace: "db".to_string(),
            local_port: Some(37017),
            remote_port: Some(27017),
            context: Some("dev".to_string()),
            workload_type: Some("service".to_string()),
            protocol: "tcp".to_string(),
            alias: Some("mongo".to_string()),
            local_address: Some("127.0.0.1".to_string()),
            tags: [("team".to_string(), "data".to_string())].into(),
            ..Config::default()
        }
    }

    fn headless_service() -> Service {
        Service {
            metadata: ObjectMeta {
                name: Some("mongo".to_string()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                cluster_ip: Some("None".to_string()),
                selector: Some([("app".to_string(), "mongo".to_string())].into()),
                ports: Some(vec![ServicePort {
                    port: 80,
                    target_port: Some(IntOrString::Int(27017)),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn stateful_pod(name: &str, subdomain: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                labels: Some(
                    [
                        ("app".to_string(), "mongo".to_string()),
                        (POD_NAME_LABEL.to_string(), name.to_string()),
                    ]
                    .into(),
                ),
                ..Default::default()
            },
            spec: Some(PodSpec {
                hostname: Some(name.to_string()),
                subdomain: Some(subdomain.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn replica_set(count: usize) -> Vec<Pod> {
        (0..count)
            .map(|index| stateful_pod(&format!("mongo-{index}"), "mongo"))
            .collect()
    }

    fn ok(value: &impl serde::Serialize) -> Response<Body> {
        Response::builder()
            .status(200)
            .body(Body::from(serde_json::to_vec(value).unwrap()))
            .unwrap()
    }

    #[test]
    fn each_pod_gets_a_forward_under_its_cluster_dns_name() {
        let configs = pod_configs(&source(), "mongo", 27017, &replica_set(3)).unwrap();

        let aliases: Vec<_> = configs
            .iter()
            .map(|c| c.alias.as_deref().unwrap())
            .collect();
        assert_eq!(
            aliases,
            [
                "mongo-0.mongo.db.svc.cluster.local",
                "mongo-1.mongo.db.svc.cluster.local",
                "mongo-2.mongo.db.svc.cluster.local",
            ]
        );
        let config = &configs[1];
        assert_eq!(config.workload_type.as_deref(), Some("pod"));
        assert_eq!(
            config.target.as_deref(),
            Some("statefulset.kubernetes.io/pod-name=mongo-1")
        );
        assert_eq!(config.domain_enabled, Some(true));
        assert!(config.auto_loopback_address);
        assert_eq!(config.local_address, None);
        assert_eq!(config.context.as_deref(), Some("dev"));
        assert_eq!(config.id, None);
        assert_eq!(
            config.tags,
            [
                ("headless".to_string(), "mongo".to_string()),
                ("team".to_string(), "data".to_string()),
            ]
            .into()
        );
    }

    #[test]
    fn each_pod_asks_the_helper_for_its_own_address() {
        let configs = pod_configs(&source(), "mongo", 27017, &replica_set(3)).unwrap();

        let address_keys: Vec<_> = configs.iter().map(|c| c.service.as_deref()).collect();
        assert_eq!(
            address_keys,
            [
                Some("mongo-0.mongo.db.svc.cluster.local"),
                Some("mongo-1.mongo.db.svc.cluster.local"),
                Some("mongo-2.mongo.db.svc.cluster.local"),
            ]
        );
    }

    #[test]
    fn pods_with_no_dns_name_under_the_service_are_left_out() {
        let mut no_hostname = stateful_pod("mongo-2", "mongo");
        no_hostname.spec.as_mut().unwrap().hostname = None;
        let mut no_pod_name_label = stateful_pod("mongo-3", "mongo");
        no_pod_name_label
            .metadata
            .labels
            .as_mut()
            .unwrap()
            .remove(POD_NAME_LABEL);
        let pods = vec![
            stateful_pod("mongo-0", "mongo"),
            stateful_pod("mongo-1", "mongo-arbiter"),
            no_hostname,
            no_pod_name_label,
        ];

        let configs = pod_configs(&source(), "mongo", 27017, &pods).unwrap();

        assert_eq!(configs.len(), 1);
        assert_eq!(
            configs[0].alias.as_deref(),
            Some("mongo-0.mongo.db.svc.cluster.local")
        );
    }

    #[test]
    fn a_service_with_no_named_pods_is_an_error() {
        let error = pod_configs(
            &source(),
            "mongo",
            27017,
            &[stateful_pod("mongo-0", "other")],
        )
        .unwrap_err();

        assert_eq!(
            error,
            "No pod of Service 'mongo' has a DNS name under it and a \
             statefulset.kubernetes.io/pod-name label"
        );
    }

    #[test]
    fn only_a_tcp_service_forward_names_a_service_to_expand() {
        let pod = Config {
            workload_type: Some("pod".to_string()),
            ..source()
        };
        let udp = Config {
            protocol: "udp".to_string(),
            ..source()
        };

        assert_eq!(source_service(&source()), Ok(("mongo", 27017)));
        for config in [pod, udp] {
            assert_eq!(
                source_service(&config),
                Err("Only a TCP service forward can add pod forwards".to_string())
            );
        }
    }

    #[tokio::test]
    async fn the_forwards_pod_port_carries_over_to_every_pod() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = Client::new(mock_service, "default");
        let server = tokio::spawn(async move {
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.uri().path(), "/api/v1/namespaces/db/services/mongo");
            send.send_response(ok(&headless_service()));

            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.uri().path(), "/api/v1/namespaces/db/pods");
            assert_eq!(request.uri().query(), Some("&labelSelector=app%3Dmongo"));
            send.send_response(ok(&k8s_openapi::List::<Pod> {
                items: replica_set(2),
                metadata: Default::default(),
            }));
        });

        let source = source();
        let (service_name, port) = source_service(&source).unwrap();
        let configs = fetch_pod_configs(&source, service_name, port, &client)
            .await
            .unwrap();

        server.await.unwrap();
        let ports: Vec<_> = configs
            .iter()
            .map(|c| (c.local_port, c.remote_port))
            .collect();
        assert_eq!(ports, [(Some(27017), Some(27017)); 2]);
    }

    #[tokio::test]
    async fn a_service_with_a_cluster_ip_is_refused() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = Client::new(mock_service, "default");
        let server = tokio::spawn(async move {
            let (_, send) = handle.next_request().await.unwrap();
            let mut service = headless_service();
            service.spec.as_mut().unwrap().cluster_ip = Some("10.43.0.12".to_string());
            send.send_response(ok(&service));
        });

        let error = fetch_pod_configs(&source(), "mongo", 27017, &client)
            .await
            .unwrap_err();

        server.await.unwrap();
        assert_eq!(
            error,
            "Service 'mongo' is not headless, so its pods have no DNS names"
        );
    }

    #[tokio::test]
    async fn running_it_again_after_a_scale_up_saves_only_the_new_pods() {
        let _db = kftray_commons::test_utils::test_db().await;
        let mode = DatabaseMode::Memory;

        let first = pod_configs(&source(), "mongo", 27017, &replica_set(2)).unwrap();
        let first_ids = insert_new_configs_with_mode(first, mode).await.unwrap();
        assert_eq!(first_ids.len(), 2);

        let again = pod_configs(&source(), "mongo", 27017, &replica_set(2)).unwrap();
        let again_ids = insert_new_configs_with_mode(again, mode).await.unwrap();
        assert!(again_ids.is_empty());

        let scaled = pod_configs(&source(), "mongo", 27017, &replica_set(3)).unwrap();
        let scaled_ids = insert_new_configs_with_mode(scaled, mode).await.unwrap();
        assert_eq!(scaled_ids.len(), 1);
    }
}
