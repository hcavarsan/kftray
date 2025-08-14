use std::sync::Arc;

use anyhow::Context;
use k8s_openapi::api::core::v1::{
    Pod,
    Service,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::Api;
use serde::{
    Deserialize,
    Serialize,
};
use tokio::sync::Mutex;
use tracing::debug;

use super::connection_pool::PooledPortForwarder;
use super::target_cache::TargetCache;

impl NameSpace {
    pub fn name_any(&self) -> String {
        self.0.clone().unwrap_or_else(|| "default".to_string())
    }
}

impl From<i32> for Port {
    fn from(port: i32) -> Self {
        Self::Number(port)
    }
}

impl From<&str> for Port {
    fn from(port: &str) -> Self {
        Self::Name(port.to_string())
    }
}

impl From<IntOrString> for Port {
    fn from(port: IntOrString) -> Self {
        match port {
            IntOrString::Int(port) => Self::Number(port),
            IntOrString::String(port) => Self::Name(port),
        }
    }
}

impl TargetPod {
    pub fn new(pod_name: String, port_number: i32) -> anyhow::Result<Self> {
        let port_number = u16::try_from(port_number).context("Port not valid")?;

        Ok(Self {
            pod_name,
            port_number,
        })
    }

    #[inline]
    pub fn into_parts(self) -> (String, u16) {
        (self.pod_name, self.port_number)
    }
}
impl Target {
    pub fn new<I: Into<Option<T>>, T: Into<String>, P: Into<Port>>(
        selector: TargetSelector, port: P, namespace: I,
    ) -> Self {
        Self {
            selector,
            port: port.into(),
            namespace: NameSpace(namespace.into().map(Into::into)),
        }
    }

    pub fn find(&self, pod: &Pod, port: Option<Port>) -> anyhow::Result<TargetPod> {
        let port = port.as_ref().unwrap_or(&self.port);
        let pod_name = pod.metadata.name.as_ref().context("Pod Name is None")?;

        let port_number = match port {
            Port::Number(port) => *port,
            Port::Name(name) => {
                pod.spec
                    .as_ref()
                    .context("Pod Spec is None")?
                    .containers
                    .iter()
                    .find_map(|c| {
                        c.ports
                            .as_ref()?
                            .iter()
                            .find(|p| p.name.as_ref() == Some(name))
                    })
                    .context("Port not found")?
                    .container_port
            }
        };

        TargetPod::new(pod_name.clone(), port_number)
    }
}

#[inline]
fn is_pod_ready(pod: &&Pod) -> bool {
    let is_ready = pod
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions
                .iter()
                .any(|c| c.type_ == "Ready" && c.status == "True")
        });

    if tracing::enabled!(tracing::Level::DEBUG) {
        debug!(
            "Pod: {}, is_ready: {}",
            pod.metadata.name.as_deref().unwrap_or("unknown"),
            is_ready
        );
    }

    is_ready
}

#[derive(Serialize, Deserialize)]
pub struct KubeContextInfo {
    pub name: String,
}

#[derive(Serialize)]
pub struct KubeNamespaceInfo {
    pub name: String,
}

#[derive(Serialize)]
pub struct KubeServiceInfo {
    pub name: String,
}

#[derive(Serialize)]
pub struct KubeServicePortInfo {
    pub name: Option<String>,
    pub port: Option<IntOrString>,
}

#[derive(Serialize, Debug)]
pub struct PodInfo {
    pub labels_str: String,
}

pub struct PortForwardConfig {
    pub local_port: Option<u16>,
    pub local_address: Option<String>,
    pub context_name: Option<String>,
    pub kubeconfig: Option<String>,
    pub config_id: i64,
    pub workload_type: String,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct PortForward {
    pub target: Target,
    pub local_port: Option<u16>,
    pub local_address: Option<String>,
    pub pod_api: Api<Pod>,
    pub svc_api: Api<Service>,
    pub client: kube::Client,
    pub context_name: Option<String>,
    pub config_id: i64,
    pub workload_type: String,
    pub connection: Arc<Mutex<Option<tokio::net::TcpStream>>>,
    pub target_cache: Arc<TargetCache>,
    pub connection_pool: Option<Arc<PooledPortForwarder>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TargetSelector {
    ServiceName(String),
    PodLabel(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Port {
    Number(i32),
    Name(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub selector: TargetSelector,
    pub port: Port,
    pub namespace: NameSpace,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NameSpace(pub Option<String>);

#[derive(Clone, Debug, PartialEq)]
pub struct TargetPod {
    pub pod_name: String,
    pub port_number: u16,
}

pub trait PodSelection {
    fn select<'p>(&self, pods: &'p [Pod], selector: &str) -> anyhow::Result<&'p Pod>;
}

pub struct AnyReady {}

impl PodSelection for AnyReady {
    fn select<'p>(&self, pods: &'p [Pod], selector: &str) -> anyhow::Result<&'p Pod> {
        let pod = pods.iter().find(is_pod_ready).context(anyhow::anyhow!(
            "No ready pods found matching the selector '{}'",
            selector
        ))?;

        Ok(pod)
    }
}

#[cfg(test)]
mod tests {
    use k8s_openapi::api::core::v1::{
        Container,
        ContainerPort,
        PodCondition,
        PodSpec,
        PodStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    use super::*;

    fn create_test_pod(name: &str, with_ports: bool, ready: bool) -> Pod {
        let mut pod = Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "test-container".to_string(),
                    ports: if with_ports {
                        Some(vec![
                            ContainerPort {
                                name: Some("http".to_string()),
                                container_port: 8080,
                                ..Default::default()
                            },
                            ContainerPort {
                                name: Some("grpc".to_string()),
                                container_port: 9090,
                                ..Default::default()
                            },
                        ])
                    } else {
                        None
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: None,
        };

        if ready {
            pod.status = Some(PodStatus {
                conditions: Some(vec![PodCondition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            });
        }

        pod
    }

    #[test]
    fn test_namespace_name_any() {
        let ns1 = NameSpace(Some("test-namespace".to_string()));
        let ns2 = NameSpace(None);

        assert_eq!(ns1.name_any(), "test-namespace");
        assert_eq!(ns2.name_any(), "default");
    }

    #[test]
    fn test_port_conversions() {
        let port1: Port = 8080.into();
        assert_eq!(port1, Port::Number(8080));

        let port2: Port = "http".into();
        assert_eq!(port2, Port::Name("http".to_string()));

        let port3: Port = IntOrString::Int(9090).into();
        assert_eq!(port3, Port::Number(9090));

        let port4: Port = IntOrString::String("grpc".to_string()).into();
        assert_eq!(port4, Port::Name("grpc".to_string()));
    }

    #[test]
    fn test_target_pod_new() {
        let result = TargetPod::new("test-pod".to_string(), 8080);
        assert!(result.is_ok());
        let pod = result.unwrap();
        assert_eq!(pod.pod_name, "test-pod");
        assert_eq!(pod.port_number, 8080);

        let result = TargetPod::new("test-pod".to_string(), 70000);
        assert!(result.is_err());
    }

    #[test]
    fn test_target_pod_into_parts() {
        let pod = TargetPod {
            pod_name: "test-pod".to_string(),
            port_number: 8080,
        };

        let (name, port) = pod.into_parts();
        assert_eq!(name, "test-pod");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_target_new() {
        let target = Target::new(
            TargetSelector::ServiceName("svc1".to_string()),
            8080,
            "default",
        );

        assert_eq!(
            target.selector,
            TargetSelector::ServiceName("svc1".to_string())
        );
        assert_eq!(target.port, Port::Number(8080));
        assert_eq!(target.namespace, NameSpace(Some("default".to_string())));

        let target_no_ns = Target::new::<Option<String>, String, &str>(
            TargetSelector::PodLabel("app=web".to_string()),
            "http",
            None,
        );

        assert_eq!(
            target_no_ns.selector,
            TargetSelector::PodLabel("app=web".to_string())
        );
        assert_eq!(target_no_ns.port, Port::Name("http".to_string()));
        assert_eq!(target_no_ns.namespace, NameSpace(None));
    }

    #[test]
    fn test_target_find() {
        let pod = create_test_pod("test-pod", true, true);

        let target1 = Target::new(
            TargetSelector::PodLabel("app=web".to_string()),
            8080,
            "default",
        );

        let result = target1.find(&pod, None);
        assert!(result.is_ok());
        let target_pod = result.unwrap();
        assert_eq!(target_pod.pod_name, "test-pod");
        assert_eq!(target_pod.port_number, 8080);

        let target2 = Target::new(
            TargetSelector::PodLabel("app=web".to_string()),
            "http",
            "default",
        );

        let result = target2.find(&pod, None);
        assert!(result.is_ok());
        let target_pod = result.unwrap();
        assert_eq!(target_pod.pod_name, "test-pod");
        assert_eq!(target_pod.port_number, 8080);

        let target3 = Target::new(
            TargetSelector::PodLabel("app=web".to_string()),
            "grpc",
            "default",
        );

        let result = target3.find(&pod, None);
        assert!(result.is_ok());
        let target_pod = result.unwrap();
        assert_eq!(target_pod.pod_name, "test-pod");
        assert_eq!(target_pod.port_number, 9090);

        let target4 = Target::new(
            TargetSelector::PodLabel("app=web".to_string()),
            "http",
            "default",
        );

        let result = target4.find(&pod, Some(Port::Number(5000)));
        assert!(result.is_ok());
        let target_pod = result.unwrap();
        assert_eq!(target_pod.pod_name, "test-pod");
        assert_eq!(target_pod.port_number, 5000);

        let target5 = Target::new(
            TargetSelector::PodLabel("app=web".to_string()),
            "nonexistent",
            "default",
        );

        let result = target5.find(&pod, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_pod_ready() {
        let ready_pod = create_test_pod("ready-pod", true, true);
        let not_ready_pod = create_test_pod("not-ready-pod", true, false);

        assert!(is_pod_ready(&&ready_pod));
        assert!(!is_pod_ready(&&not_ready_pod));
    }

    #[test]
    fn test_any_ready_selection() {
        let ready_pod = create_test_pod("ready-pod", true, true);
        let not_ready_pod = create_test_pod("not-ready-pod", true, false);

        let selector = AnyReady {};

        let pods = vec![ready_pod.clone()];
        let result = selector.select(&pods, "app=web");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().metadata.name, Some("ready-pod".to_string()));

        let pods = vec![not_ready_pod.clone()];
        let result = selector.select(&pods, "app=web");
        assert!(result.is_err());

        let pods = vec![not_ready_pod, ready_pod.clone()];
        let result = selector.select(&pods, "app=web");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().metadata.name, Some("ready-pod".to_string()));

        let pods: Vec<Pod> = vec![];
        let result = selector.select(&pods, "app=web");
        assert!(result.is_err());
    }
}
