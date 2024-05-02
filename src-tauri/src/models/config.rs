use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct Config {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    pub namespace: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub context: String,
    pub workload_type: String,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_enabled: Option<bool>,
    #[serde(default = "default_kubeconfig_path")]
    pub kubeconfig: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            id: None,
            service: Some("default-service".to_string()),
            namespace: "default-namespace".to_string(),
            local_port: 1234,
            remote_port: 5678,
            context: "default-context".to_string(),
            workload_type: "default-workload".to_string(),
            protocol: "tcp".to_string(),
            remote_address: Some("default-remote-address".to_string()),
            local_address: Some("127.0.0.1".to_string()),
            domain_enabled: Some(false),
            alias: Some("default-alias".to_string()),
            kubeconfig: default_kubeconfig_path(),
        }
    }
}

fn default_kubeconfig_path() -> Option<String> {
    dirs::home_dir().map(|path| {
        let mut kubeconfig_path = path;
        kubeconfig_path.push(".kube");
        kubeconfig_path.push("config");
        kubeconfig_path.to_str().unwrap_or("").to_string()
    })
}
