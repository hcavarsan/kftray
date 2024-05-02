// kubeforward/mod.rs

pub mod kubecontext;
pub mod pod_selection;
pub mod port_forward;
pub mod proxy;

// Re-export k8s_openapi::api::core::v1 as vx for use within this module and potentially outside
use anyhow::Context;
pub use k8s_openapi::api::core::v1 as vx;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::ResourceExt;
use vx::Pod;

use crate::models::kube::{NameSpace, Port, Target, TargetPod, TargetSelector};

impl NameSpace {
    /// Returns the configured namespace or the default.
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

    pub fn into_parts(self) -> (String, u16) {
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

    pub fn find(&self, pod: &Pod, port: Option<Port>) -> anyhow::Result<TargetPod> {
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
