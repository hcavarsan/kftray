use std::collections::HashMap;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
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

        let pod_name = pod.metadata.name.clone().context("Pod Name is None")?;

        let port_number = match port {
            Port::Number(port) => *port,
            Port::Name(name) => {
                let spec = pod.spec.as_ref().context("Pod Spec is None")?;
                let containers = &spec.containers;

                // Find the port by name within the container ports
                containers
                    .iter()
                    .flat_map(|c| c.ports.as_ref().map_or(Vec::new(), |v| v.clone()))
                    .find(|p| p.name.as_ref() == Some(name))
                    .context("Port not found")?
                    .container_port
            }
        };

        TargetPod::new(pod_name, port_number)
    }
}

fn is_pod_ready(pod: &&Pod) -> bool {
    let conditions = pod.status.as_ref().and_then(|s| s.conditions.as_ref());

    let is_ready = conditions
        .map(|c| c.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
        .unwrap_or(false);

    debug!(
        "Pod: {}, is_ready: {}",
        pod.metadata.name.clone().unwrap_or_default(),
        is_ready
    );
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

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct PortForward {
    pub target: Target,
    pub local_port: Option<u16>,
    pub local_address: Option<String>,
    pub pod_api: Api<Pod>,
    pub svc_api: Api<Service>,
    pub context_name: Option<String>,
    pub config_id: i64,
    pub workload_type: String,
    pub connection: Arc<Mutex<Option<tokio::net::TcpStream>>>,
}

#[derive(Clone, Debug)]
pub enum TargetSelector {
    ServiceName(String),
    PodLabel(String),
}

#[derive(Clone, Debug)]
pub enum Port {
    Number(i32),
    Name(String),
}

#[derive(Clone, Debug)]
pub struct Target {
    pub selector: TargetSelector,
    pub port: Port,
    pub namespace: NameSpace,
}

#[derive(Clone, Debug)]
pub struct NameSpace(pub Option<String>);

#[derive(Clone, Debug)]
pub struct TargetPod {
    pub pod_name: String,
    pub port_number: u16,
}

pub trait PodSelection {
    fn select<'p>(&self, pods: &'p [Pod], selector: &str) -> anyhow::Result<&'p Pod>;
}

pub struct AnyReady {}

#[derive(Clone, Debug)]
pub struct HttpLogState {
    pub enable_http_logs: Arc<Mutex<HashMap<i64, AtomicBool>>>,
}

impl HttpLogState {
    pub fn new() -> Self {
        HttpLogState {
            enable_http_logs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn set_http_logs(&self, config_id: i64, enable: bool) {
        let mut logs = self.enable_http_logs.lock().await;
        logs.entry(config_id)
            .or_insert_with(|| AtomicBool::new(enable))
            .store(enable, Ordering::SeqCst);
    }

    pub async fn get_http_logs(&self, config_id: i64) -> bool {
        let logs = self.enable_http_logs.lock().await;
        if let Some(state) = logs.get(&config_id) {
            state.load(Ordering::SeqCst)
        } else {
            false
        }
    }
}

impl Default for HttpLogState {
    fn default() -> Self {
        Self::new()
    }
}

impl PodSelection for AnyReady {
    fn select<'p>(&self, pods: &'p [Pod], selector: &str) -> anyhow::Result<&'p Pod> {
        let pod = pods.iter().find(is_pod_ready).context(anyhow::anyhow!(
            "No ready pods found matching the selector '{}'",
            selector
        ))?;

        Ok(pod)
    }
}
