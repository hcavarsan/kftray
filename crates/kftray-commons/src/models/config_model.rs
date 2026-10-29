use std::collections::BTreeMap;

use serde::{
    Deserialize,
    Deserializer,
    Serialize,
};

use super::ssh_provider::{
    SshProvider,
    WORKLOAD_SSH_PORT_FORWARD,
};

fn deserialize_bool_from_anything<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    use std::fmt;

    use serde::de::{
        self,
        Visitor,
    };

    struct BoolOrStringVisitor;

    impl<'de> Visitor<'de> for BoolOrStringVisitor {
        type Value = Option<bool>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a boolean, string 'true'/'false', or null")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match value {
                "true" => Ok(Some(true)),
                "false" => Ok(Some(false)),
                _ => Err(E::custom(format!("Invalid boolean string: {}", value))),
            }
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_any(BoolOrStringVisitor)
}

fn is_false(value: &bool) -> bool {
    !value
}

fn is_empty_string(value: &str) -> bool {
    value.is_empty()
}

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct Config {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "is_empty_string")]
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
    #[serde(skip_serializing_if = "is_empty_string")]
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_provider: Option<SshProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_address: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "is_false")]
    pub auto_loopback_address: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default)]
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
    // Expose-specific fields
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure_type: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure_protocol: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_bool_from_anything")]
    pub cert_manager_enabled: Option<bool>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_issuer: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_issuer_kind: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress_class: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress_annotations: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tags: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            id: None,
            service: Some("default-service".to_string()),
            namespace: "default-namespace".to_string(),
            local_port: Some(0),
            remote_port: Some(0),
            context: Some(Self::DEFAULT_CONTEXT.to_string()),
            workload_type: Some("default-workload".to_string()),
            protocol: "protocol".to_string(),
            remote_address: Some(Self::DEFAULT_REMOTE_ADDRESS.to_string()),
            ssh_provider: None,
            local_address: Some("127.0.0.1".to_string()),
            auto_loopback_address: false,
            domain_enabled: Some(false),
            alias: Some("default-alias".to_string()),
            kubeconfig: Some("default".to_string()),
            target: Some("default-target".to_string()),
            http_logs_enabled: Some(false),
            http_logs_max_file_size: Some(10 * 1024 * 1024),
            http_logs_retention_days: Some(7),
            http_logs_auto_cleanup: Some(true),
            exposure_type: None,
            exposure_protocol: None,
            cert_manager_enabled: None,
            cert_issuer: None,
            cert_issuer_kind: None,
            ingress_class: None,
            ingress_annotations: None,
            tags: Default::default(),
        }
    }
}

impl Config {
    pub const DEFAULT_CONTEXT: &'static str = "current-context";
    pub const DEFAULT_REMOTE_ADDRESS: &'static str = "default-remote-address";

    /// A Cluster Only expose set to `tcp` carries raw bytes. Every other
    /// expose carries HTTP, and a public one always goes through an Ingress.
    pub fn exposes_tcp(&self) -> bool {
        self.exposure_type.as_deref() != Some("public")
            && self.exposure_protocol.as_deref() == Some("tcp")
    }

    /// An ssh-port-forward config needs no Service, so the alias is enough
    /// for a hosts entry.
    pub fn domain_alias_eligible(&self) -> bool {
        self.domain_enabled == Some(true)
            && self.alias.as_deref().is_some_and(|a| !a.is_empty())
            && (self.service.is_some()
                || self.workload_type.as_deref() == Some(WORKLOAD_SSH_PORT_FORWARD))
    }

    /// Started by `start_port_forward(.., "tcp")` without a relay pod.
    pub fn forwards_directly(&self) -> bool {
        let workload_type = self.workload_type.as_deref();
        matches!(
            workload_type,
            Some("expose") | Some(WORKLOAD_SSH_PORT_FORWARD)
        ) || (matches!(workload_type, None | Some("service" | "pod")) && self.protocol == "tcp")
    }

    pub fn prepare_for_export(mut self) -> Self {
        self.id = None;

        if Self::is_placeholder(&self.protocol) {
            self.protocol = String::new();
        }

        if self.target.as_deref().is_some_and(Self::is_placeholder) {
            self.target = None;
        }

        if self
            .remote_address
            .as_deref()
            .is_some_and(Self::is_placeholder)
        {
            self.remote_address = None;
        }

        if self.alias.as_deref().is_some_and(Self::is_placeholder) {
            self.alias = None;
        }

        if self.local_address.as_deref() == Some("127.0.0.1") {
            self.local_address = None;
        }

        if self.kubeconfig.as_deref() == Some("default") {
            self.kubeconfig = None;
        }

        if self.domain_enabled == Some(false) {
            self.domain_enabled = None;
        }

        if self.http_logs_enabled != Some(true) {
            self.http_logs_enabled = None;
        }

        if self.http_logs_max_file_size == Some(10 * 1024 * 1024) {
            self.http_logs_max_file_size = None;
        }

        if self.http_logs_retention_days == Some(7) {
            self.http_logs_retention_days = None;
        }

        if self.http_logs_auto_cleanup == Some(true) {
            self.http_logs_auto_cleanup = None;
        }

        match self.workload_type.as_deref() {
            Some("service") => {
                self.target = None;
                self.remote_address = None;
            }
            Some("pod") => {
                self.service = None;
                self.remote_address = None;
            }
            Some("proxy") => {
                self.service = None;
                self.target = None;
            }
            Some("expose") => {
                self.service = None;
                self.target = None;
                self.remote_address = None;
                self.remote_port = None;
                if !self.exposes_tcp() {
                    self.exposure_protocol = None;
                }
                if self.exposure_type.as_deref() == Some("cluster") {
                    self.cert_manager_enabled = None;
                    self.cert_issuer = None;
                    self.cert_issuer_kind = None;
                    self.ingress_class = None;
                    self.ingress_annotations = None;
                }
            }
            Some(WORKLOAD_SSH_PORT_FORWARD) => {
                self.service = None;
                self.target = None;
                self.context = None;
                self.kubeconfig = None;
                self.namespace = String::new();
            }
            _ => {}
        }

        self
    }

    fn is_placeholder(s: &str) -> bool {
        matches!(
            s,
            "default-target"
                | Self::DEFAULT_REMOTE_ADDRESS
                | "default-alias"
                | "default-workload"
                | Self::DEFAULT_CONTEXT
                | "protocol"
                | ""
        )
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

    #[test]
    fn test_deserialize_config_without_id() {
        let test_json = r#"{
            "service": "import-test-service",
            "namespace": "import-test-namespace",
            "local_port": 5000,
            "workload_type": "service",
            "protocol": "tcp",
            "context": "test-context"
        }"#;

        let result = serde_json::from_str::<Config>(test_json);
        println!("Deserialization result: {:?}", result);

        match result {
            Ok(config) => {
                assert_eq!(config.id, None);
                assert_eq!(config.service, Some("import-test-service".to_string()));
                assert_eq!(config.namespace, "import-test-namespace".to_string());
            }
            Err(e) => {
                panic!("Failed to deserialize config: {}", e);
            }
        }
    }

    #[test]
    fn test_deserialize_config_array() {
        let test_json = r#"[{
            "service": "import-test-service",
            "namespace": "import-test-namespace",
            "local_port": 5000,
            "workload_type": "service",
            "protocol": "tcp",
            "context": "test-context"
        }]"#;

        let result = serde_json::from_str::<Vec<Config>>(test_json);
        println!("Array deserialization result: {:?}", result);

        match result {
            Ok(configs) => {
                assert_eq!(configs.len(), 1);
                assert_eq!(configs[0].id, None);
                assert_eq!(configs[0].service, Some("import-test-service".to_string()));
            }
            Err(e) => {
                panic!("Failed to deserialize config array: {}", e);
            }
        }
    }

    fn ssh_config() -> Config {
        Config {
            workload_type: Some(WORKLOAD_SSH_PORT_FORWARD.to_string()),
            protocol: "tcp".to_string(),
            service: None,
            target: None,
            context: None,
            kubeconfig: None,
            namespace: String::new(),
            remote_address: Some("proxy.internal".to_string()),
            remote_port: Some(80),
            ssh_provider: Some(
                serde_json::from_value(serde_json::json!({
                    "type": "gcp-bastion",
                    "project": "p",
                    "zone": "us-central1-b",
                    "instance": "jumper",
                    "interface": "nic0"
                }))
                .unwrap(),
            ),
            ..Config::default()
        }
    }

    #[test]
    fn forwards_directly_should_match_workload_and_protocol() {
        let cases = [
            (None, "tcp", true),
            (None, "udp", false),
            (Some("service"), "tcp", true),
            (Some("service"), "udp", false),
            (Some("pod"), "tcp", true),
            (Some("pod"), "udp", false),
            (Some("expose"), "tcp", true),
            (Some("expose"), "udp", true),
            (Some("proxy"), "tcp", false),
            (Some("proxy"), "udp", false),
            (Some(WORKLOAD_SSH_PORT_FORWARD), "tcp", true),
        ];
        for (workload_type, protocol, expected) in cases {
            let config = Config {
                workload_type: workload_type.map(str::to_string),
                protocol: protocol.to_string(),
                ..Config::default()
            };
            assert_eq!(
                config.forwards_directly(),
                expected,
                "{workload_type:?} {protocol}"
            );
        }
    }

    #[test]
    fn domain_alias_eligible_should_need_alias_and_service_or_ssh() {
        let service = Config {
            domain_enabled: Some(true),
            alias: Some("a".to_string()),
            service: Some("svc".to_string()),
            ..Config::default()
        };
        assert!(service.domain_alias_eligible());
        assert!(
            !Config {
                domain_enabled: Some(false),
                ..service.clone()
            }
            .domain_alias_eligible()
        );
        assert!(
            !Config {
                service: None,
                ..service.clone()
            }
            .domain_alias_eligible()
        );

        let ssh = Config {
            domain_enabled: Some(true),
            alias: Some("jumper".to_string()),
            ..ssh_config()
        };
        assert!(ssh.domain_alias_eligible());
        assert!(
            !Config {
                alias: Some(String::new()),
                ..ssh.clone()
            }
            .domain_alias_eligible()
        );
        assert!(!Config { alias: None, ..ssh }.domain_alias_eligible());
    }

    #[test]
    fn prepare_for_export_should_strip_kube_keys_and_default_interface_for_ssh() {
        let exported = Config {
            service: Some("svc".to_string()),
            target: Some("t".to_string()),
            context: Some("ctx".to_string()),
            kubeconfig: Some("/k".to_string()),
            namespace: "ns".to_string(),
            ..ssh_config()
        }
        .prepare_for_export();
        let value = serde_json::to_value(&exported).unwrap();
        for key in ["namespace", "context", "kubeconfig", "service", "target"] {
            assert!(value.get(key).is_none(), "{key} should be stripped");
        }
        assert!(value["ssh_provider"].get("interface").is_none());
        assert_eq!(value["remote_address"], "proxy.internal");
    }
}
