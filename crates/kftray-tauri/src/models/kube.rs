use std::collections::HashMap;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::sync::Arc;

use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::Api;
use serde::{
    Deserialize,
    Serialize,
};
use tokio::sync::Mutex;

use crate::kubeforward::vx::{
    Pod,
    Service,
};

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
    pub connection: Arc<Mutex<Option<tokio::net::TcpStream>>>,
}

#[derive(Clone)]
pub enum TargetSelector {
    ServiceName(String),
    PodLabel(String),
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
