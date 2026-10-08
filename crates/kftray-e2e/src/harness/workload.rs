use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{
    Context,
    Result,
};
use k8s_openapi::api::apps::v1::{
    Deployment,
    DeploymentSpec,
};
use k8s_openapi::api::core::v1::{
    Container,
    ContainerPort,
    PodSpec,
    PodTemplateSpec,
    Service,
    ServicePort,
    ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{
    LabelSelector,
    ObjectMeta,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::Client;
use kube::api::{
    Api,
    PostParams,
};
use kube_runtime::wait::await_condition;
use kube_runtime::wait::conditions::is_deployment_completed;
use tokio::time::timeout;

pub const NAMESPACE: &str = "e2e";

const HTTP_ECHO_IMAGE: &str = "hashicorp/http-echo:1.0.0";

#[derive(Clone, Copy, Debug)]
pub enum Workload {
    EchoHttp,
}

impl Workload {
    pub fn all() -> &'static [Workload] {
        &[Self::EchoHttp]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::EchoHttp => "echo-http",
        }
    }

    pub fn port(self) -> u16 {
        match self {
            Self::EchoHttp => 5678,
        }
    }

    pub fn selector(self) -> String {
        format!("app={}", self.name())
    }

    fn objects(self, namespace: &str) -> (Deployment, Service) {
        match self {
            Self::EchoHttp => (
                deployment(
                    self,
                    namespace,
                    Container {
                        name: self.name().to_owned(),
                        image: Some(HTTP_ECHO_IMAGE.to_owned()),
                        args: Some(vec![
                            "-text=kftray-e2e".to_owned(),
                            format!("-listen=:{}", self.port()),
                        ]),
                        ports: Some(vec![ContainerPort {
                            container_port: self.port().into(),
                            protocol: Some("TCP".to_owned()),
                            ..ContainerPort::default()
                        }]),
                        ..Container::default()
                    },
                ),
                service(self, namespace, "TCP"),
            ),
        }
    }

    pub async fn apply(self, client: &Client, namespace: &str) -> Result<()> {
        let (deployment, service) = self.objects(namespace);
        Api::<Deployment>::namespaced(client.clone(), namespace)
            .create(&PostParams::default(), &deployment)
            .await
            .with_context(|| format!("create deployment {}", self.name()))?;
        Api::<Service>::namespaced(client.clone(), namespace)
            .create(&PostParams::default(), &service)
            .await
            .with_context(|| format!("create service {}", self.name()))?;
        Ok(())
    }

    pub async fn wait_ready(self, client: &Client, namespace: &str, limit: Duration) -> Result<()> {
        let api = Api::<Deployment>::namespaced(client.clone(), namespace);
        timeout(
            limit,
            await_condition(api, self.name(), is_deployment_completed()),
        )
        .await
        .with_context(|| format!("{} did not become ready within {limit:?}", self.name()))?
        .with_context(|| format!("watch deployment {}", self.name()))?;
        Ok(())
    }
}

fn labels(workload: Workload) -> BTreeMap<String, String> {
    BTreeMap::from([("app".to_owned(), workload.name().to_owned())])
}

fn deployment(workload: Workload, namespace: &str, container: Container) -> Deployment {
    Deployment {
        metadata: ObjectMeta {
            name: Some(workload.name().to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels(workload)),
            ..ObjectMeta::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(labels(workload)),
                ..LabelSelector::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels(workload)),
                    ..ObjectMeta::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![container],
                    ..PodSpec::default()
                }),
            },
            ..DeploymentSpec::default()
        }),
        ..Deployment::default()
    }
}

fn service(workload: Workload, namespace: &str, protocol: &str) -> Service {
    Service {
        metadata: ObjectMeta {
            name: Some(workload.name().to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels(workload)),
            ..ObjectMeta::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels(workload)),
            ports: Some(vec![ServicePort {
                name: Some(protocol.to_ascii_lowercase()),
                port: workload.port().into(),
                target_port: Some(IntOrString::Int(workload.port().into())),
                protocol: Some(protocol.to_owned()),
                ..ServicePort::default()
            }]),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    }
}
