use serde::{
    Deserialize,
    Serialize,
};

pub const WORKLOAD_SSH_PORT_FORWARD: &str = "ssh-port-forward";

const DEFAULT_INTERFACE: &str = "nic0";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SshProvider {
    Known(SshProviderKind),
    /// From a newer kftray, kept as it came.
    Unknown(serde_json::Value),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SshProviderKind {
    GcpBastion(GcpBastionProvider),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GcpBastionProvider {
    pub project: String,
    pub zone: String,
    pub instance: String,
    #[serde(
        default = "default_interface",
        skip_serializing_if = "is_default_interface"
    )]
    pub interface: String,
}

fn default_interface() -> String {
    DEFAULT_INTERFACE.to_string()
}

fn is_default_interface(interface: &str) -> bool {
    interface == DEFAULT_INTERFACE
}

impl SshProviderKind {
    pub const TYPE_NAMES: &'static [&'static str] = &["gcp-bastion"];

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::GcpBastion(_) => "gcp-bastion",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::GcpBastion(_) => "GCP Bastion (IAP)",
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::GcpBastion(p) => p.validate(),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::GcpBastion(p) => format!("{} ({}/{})", p.instance, p.project, p.zone),
        }
    }
}

impl GcpBastionProvider {
    fn validate(&self) -> Result<(), String> {
        if !is_valid_project(&self.project) {
            return Err(format!(
                "GCP project \"{}\" is not valid (example: my-project-123)",
                self.project
            ));
        }
        if !is_valid_zone(&self.zone) {
            return Err(format!(
                "GCP zone \"{}\" is not valid (example: us-central1-b)",
                self.zone
            ));
        }
        if !is_valid_instance(&self.instance) {
            return Err(format!(
                "GCP instance \"{}\" is not valid (example: jumper)",
                self.instance
            ));
        }
        if !is_valid_interface(&self.interface) {
            return Err(format!(
                "GCP network interface \"{}\" is not valid (example: nic0)",
                self.interface
            ));
        }
        Ok(())
    }
}

fn is_valid_project(project: &str) -> bool {
    let len = project.chars().count();
    (1..=100).contains(&len)
        && project.starts_with(|c: char| c.is_ascii_lowercase())
        && project
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | ':' | '-'))
}

/// `<letters>-<letters><digits>-<letter>`, e.g. `us-central1-b`.
fn is_valid_zone(zone: &str) -> bool {
    let mut parts = zone.split('-');
    let (Some(area), Some(region), Some(suffix), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let letters = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase());
    let region_letters = region.trim_end_matches(|c: char| c.is_ascii_digit());
    letters(area)
        && letters(region_letters)
        && region.len() > region_letters.len()
        && suffix.len() == 1
        && letters(suffix)
}

fn is_valid_instance(instance: &str) -> bool {
    let len = instance.chars().count();
    (1..=63).contains(&len)
        && instance.starts_with(|c: char| c.is_ascii_lowercase())
        && !instance.ends_with('-')
        && instance
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_valid_interface(interface: &str) -> bool {
    interface
        .strip_prefix("nic")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

impl SshProvider {
    pub fn type_name(&self) -> &str {
        match self {
            Self::Known(kind) => kind.type_name(),
            Self::Unknown(value) => value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown"),
        }
    }

    pub fn kind(&self) -> Result<&SshProviderKind, String> {
        let value = match self {
            Self::Known(kind) => return Ok(kind),
            Self::Unknown(value) => value,
        };
        if !value.is_object() {
            return Err("ssh_provider must be an object with a \"type\" field".to_string());
        }
        let type_name = self.type_name();
        if SshProviderKind::TYPE_NAMES.contains(&type_name) {
            // The untagged enum swallowed the real serde error; recover it.
            let error = serde_json::from_value::<SshProviderKind>(value.clone())
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            return Err(format!("Invalid ssh_provider \"{type_name}\": {error}"));
        }
        Err(format!(
            "The SSH provider \"{type_name}\" is not supported by this version of kftray"
        ))
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Known(kind) => kind.summary(),
            Self::Unknown(_) => self.type_name().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn gcp(project: &str, zone: &str, instance: &str, interface: &str) -> SshProviderKind {
        SshProviderKind::GcpBastion(GcpBastionProvider {
            project: project.into(),
            zone: zone.into(),
            instance: instance.into(),
            interface: interface.into(),
        })
    }

    #[test]
    fn known_provider_should_parse_with_default_interface_when_omitted() {
        let provider: SshProvider = serde_json::from_value(
            json!({"type": "gcp-bastion", "project": "p-1", "zone": "us-central1-b", "instance": "jumper"}),
        )
        .unwrap();
        assert_eq!(
            provider,
            SshProvider::Known(gcp("p-1", "us-central1-b", "jumper", "nic0"))
        );
    }

    #[test]
    fn serialize_should_skip_interface_when_default() {
        let value =
            serde_json::to_value(SshProvider::Known(gcp("p", "us-central1-b", "j", "nic0")))
                .unwrap();
        assert!(value.get("interface").is_none());
        let value =
            serde_json::to_value(SshProvider::Known(gcp("p", "us-central1-b", "j", "nic1")))
                .unwrap();
        assert_eq!(value["interface"], "nic1");
    }

    #[test]
    fn unknown_type_should_round_trip_and_be_unsupported() {
        let raw = json!({"type": "aws-ssm", "target": "i-1"});
        let provider: SshProvider = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(provider.type_name(), "aws-ssm");
        assert_eq!(provider.summary(), "aws-ssm");
        assert_eq!(serde_json::to_value(&provider).unwrap(), raw);
        assert_eq!(
            provider.kind().unwrap_err(),
            "The SSH provider \"aws-ssm\" is not supported by this version of kftray"
        );
    }

    #[test]
    fn kind_should_report_serde_error_when_known_type_is_malformed() {
        let provider: SshProvider =
            serde_json::from_value(json!({"type": "gcp-bastion", "project": "p", "instance": "j"}))
                .unwrap();
        let error = provider.kind().unwrap_err();
        assert!(
            error.starts_with("Invalid ssh_provider \"gcp-bastion\": "),
            "{error}"
        );
        assert!(error.contains("missing field `zone`"), "{error}");
    }

    #[test]
    fn kind_should_reject_non_object_and_untyped_values() {
        let scalar = SshProvider::Unknown(json!("gcp-bastion"));
        assert_eq!(
            scalar.kind().unwrap_err(),
            "ssh_provider must be an object with a \"type\" field"
        );
        let untyped = SshProvider::Unknown(json!({"project": "p"}));
        assert_eq!(untyped.type_name(), "unknown");
        assert!(untyped.kind().unwrap_err().contains("\"unknown\""));
    }

    #[test]
    fn summary_should_name_instance_project_and_zone() {
        assert_eq!(
            gcp("dta-core-production", "us-central1-b", "jumper", "nic0").summary(),
            "jumper (dta-core-production/us-central1-b)"
        );
        assert_eq!(gcp("p", "z", "j", "nic0").type_name(), "gcp-bastion");
        assert_eq!(gcp("p", "z", "j", "nic0").label(), "GCP Bastion (IAP)");
    }

    #[test]
    fn validate_should_accept_real_values() {
        gcp("dta-core-production", "us-central1-b", "jumper", "nic0")
            .validate()
            .unwrap();
        gcp(
            "example.com:proj-1",
            "northamerica-northeast1-a",
            "a1-b2",
            "nic12",
        )
        .validate()
        .unwrap();
    }

    #[test]
    fn validate_should_name_the_invalid_field() {
        let cases = [
            (
                gcp("1abc", "us-central1-b", "j", "nic0"),
                "GCP project \"1abc\"",
            ),
            (gcp("", "us-central1-b", "j", "nic0"), "GCP project \"\""),
            (gcp("P", "us-central1-b", "j", "nic0"), "GCP project \"P\""),
            (
                gcp("p", "us-central1", "j", "nic0"),
                "GCP zone \"us-central1\"",
            ),
            (
                gcp("p", "us-central-b", "j", "nic0"),
                "GCP zone \"us-central-b\"",
            ),
            (
                gcp("p", "us-central1-bb", "j", "nic0"),
                "GCP zone \"us-central1-bb\"",
            ),
            (gcp("p", "us-central1-b-c", "j", "nic0"), "GCP zone"),
            (
                gcp("p", "us-central1-b", "j-", "nic0"),
                "GCP instance \"j-\"",
            ),
            (
                gcp("p", "us-central1-b", "9j", "nic0"),
                "GCP instance \"9j\"",
            ),
            (gcp("p", "us-central1-b", "J", "nic0"), "GCP instance \"J\""),
            (
                gcp("p", "us-central1-b", &"a".repeat(64), "nic0"),
                "GCP instance",
            ),
            (
                gcp("p", "us-central1-b", "j", "eth0"),
                "GCP network interface \"eth0\"",
            ),
            (
                gcp("p", "us-central1-b", "j", "nic"),
                "GCP network interface \"nic\"",
            ),
        ];
        for (kind, expected) in cases {
            let error = kind.validate().unwrap_err();
            assert!(
                error.starts_with(expected),
                "{error} should start with {expected}"
            );
        }
    }

    #[test]
    fn validate_should_count_chars_not_bytes() {
        let long = "é".repeat(60);
        assert!(gcp(&long, "us-central1-b", "j", "nic0").validate().is_err());
    }
}
