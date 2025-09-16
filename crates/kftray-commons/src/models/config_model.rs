use serde::{
    Deserialize,
    Deserializer,
    Serialize,
};

fn deserialize_bool_from_anything<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Bool(b) => Ok(Some(b)),
        Value::String(s) => match s.as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Err(D::Error::custom(format!("Invalid boolean string: {}", s))),
        },
        Value::Null => Ok(None),
        _ => Err(D::Error::custom("Expected boolean, string, or null")),
    }
}

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
    #[serde(deserialize_with = "deserialize_bool_from_anything")]
    pub domain_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubeconfig: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_bool_from_anything")]
    pub http_logs_enabled: Option<bool>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_logs_max_file_size: Option<u64>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_logs_retention_days: Option<u64>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_bool_from_anything")]
    pub http_logs_auto_cleanup: Option<bool>,
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
            http_logs_enabled: Some(false),
            http_logs_max_file_size: Some(10 * 1024 * 1024), // 10MB
            http_logs_retention_days: Some(7),
            http_logs_auto_cleanup: Some(true),
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
        assert_eq!(config.http_logs_enabled, Some(false));
        assert_eq!(config.http_logs_max_file_size, Some(10 * 1024 * 1024));
        assert_eq!(config.http_logs_retention_days, Some(7));
        assert_eq!(config.http_logs_auto_cleanup, Some(true));
    }
}
