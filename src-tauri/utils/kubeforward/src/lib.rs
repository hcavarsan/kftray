pub mod kubecontext;
pub mod pod_selection;
pub mod port_forward;
pub mod proxy;
pub(crate) use k8s_openapi::api::core::v1 as vx;

use anyhow::Context;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::ResourceExt;
use vx::Pod;

#[derive(Clone)]
pub enum TargetSelector {
    ServiceName(String),
}

#[derive(Clone)]
pub enum Port {
    Number(i32),
    Name(String),
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

#[derive(Clone)]
pub struct Target {
    selector: TargetSelector,
    port: Port,
    namespace: NameSpace,
}

#[derive(Clone)]
pub(crate) struct NameSpace(Option<String>);
impl NameSpace {
    /// Returns the configured namespace or the default.
    pub(crate) fn name_any(&self) -> String {
        let default = "default".to_string();
        self.0.clone().unwrap_or(default)
    }
}

#[derive(Clone)]
pub(crate) struct TargetPod {
    pod_name: String,
    port_number: u16,
}
impl TargetPod {
    fn new(pod_name: String, port_number: i32) -> anyhow::Result<Self> {
        let port_number = u16::try_from(port_number).context("Port not valid")?;
        Ok(Self {
            pod_name,
            port_number,
        })
    }
    pub(crate) fn into_parts(self) -> (String, u16) {
        (self.pod_name, self.port_number)
    }
}

impl Target {
    pub fn new<I: Into<Option<T>>, T: Into<String>, P: Into<Port>>(
        selector: TargetSelector,
        port: P,
        namespace: I,
    ) -> Self {
        Self {
            selector,
            port: port.into(),
            namespace: NameSpace(namespace.into().map(Into::into)),
        }
    }

    pub fn with_selector(mut self, selector: TargetSelector) -> Self {
        self.selector = selector;
        self
    }

    pub fn with_port<P: Into<Port>>(mut self, port: P) -> Self {
        self.port = port.into();
        self
    }
    pub fn with_namespace<I: Into<Option<T>>, T: Into<String>>(mut self, namespace: I) -> Self {
        self.namespace = NameSpace(namespace.into().map(Into::into));
        self
    }

    pub(crate) fn find(&self, pod: &Pod, port: Option<Port>) -> anyhow::Result<TargetPod> {
        let port = match &port {
            None => &self.port,
            Some(port) => port,
        };

        TargetPod::new(
            pod.name_any(),
            match port {
                Port::Number(port) => *port,
                Port::Name(name) => {
                    let spec = pod.spec.as_ref().context("Pod Spec is None")?;
                    let containers = &spec.containers;
                    let mut ports = containers.iter().filter_map(|c| c.ports.as_ref()).flatten();
                    let port = ports.find(|p| p.name.as_ref() == Some(name));
                    port.context("Port not found")?.container_port
                }
            },
        )
    }
}
