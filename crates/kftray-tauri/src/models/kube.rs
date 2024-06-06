use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::Api;
use serde::Serialize;

use crate::kubeforward::vx::{
    Pod,
    Service,
};

#[derive(Serialize)]

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

#[derive(Clone)]
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
}

#[derive(Clone)]

pub enum TargetSelector {
    ServiceName(String),
}

#[derive(Clone)]

pub enum Port {
    Number(i32),
    Name(String),
}

#[derive(Clone)]

pub struct Target {
    pub selector: TargetSelector,
    pub port: Port,
    pub namespace: NameSpace,
}

#[derive(Clone)]

pub struct NameSpace(pub Option<String>);

#[derive(Clone)]

pub struct TargetPod {
    pub pod_name: String,
    pub port_number: u16,
}
/// Pod selection according to impl specific criteria.

pub(crate) trait PodSelection {
    fn select<'p>(&self, pods: &'p [Pod], selector: &str) -> anyhow::Result<&'p Pod>;
}

/// Selects any pod so long as it's ready.

pub(crate) struct AnyReady {}
