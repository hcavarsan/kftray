use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct Config {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_port: Option<u16>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default)]
    pub workload_type: Option<String>,
    #[serde(default)]
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_address: Option<String>,
    #[serde(default)]
    pub auto_loopback_address: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubeconfig: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            id: None,
            service: Some("default-service".to_string()),
            namespace: "default-namespace".to_string(),
            local_port: Some(0),
            remote_port: Some(0),
            context: Some("current-context".to_string()),
            workload_type: Some("default-workload".to_string()),
            protocol: "protocol".to_string(),
            remote_address: Some("default-remote-address".to_string()),
            local_address: Some("127.0.0.1".to_string()),
            auto_loopback_address: false,
            domain_enabled: Some(false),
            alias: Some("default-alias".to_string()),
            kubeconfig: Some("default".to_string()),
            target: Some("default-target".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();

        assert_eq!(config.id, None);
        assert_eq!(config.service, Some("default-service".to_string()));
        assert_eq!(config.namespace, "default-namespace".to_string());
        assert_eq!(config.local_port, Some(0));
        assert_eq!(config.remote_port, Some(0));
        assert_eq!(config.context, Some("current-context".to_string()));
        assert_eq!(config.workload_type, Some("default-workload".to_string()));
        assert_eq!(config.protocol, "protocol".to_string());
        assert_eq!(
            config.remote_address,
            Some("default-remote-address".to_string())
        );
        assert_eq!(config.local_address, Some("127.0.0.1".to_string()));
        assert!(!config.auto_loopback_address);
        assert_eq!(config.domain_enabled, Some(false));
        assert_eq!(config.alias, Some("default-alias".to_string()));
        assert_eq!(config.kubeconfig, Some("default".to_string()));
        assert_eq!(config.target, Some("default-target".to_string()));
    }
}
