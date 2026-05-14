/// ServiceClientKey identifies a unique kube client by context + kubeconfig
/// path. The client cache itself lives in
/// [`crate::registry::PortForwardRegistry`].

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ServiceClientKey {
    pub context_name: Option<String>,
    pub kubeconfig_path: Option<String>,
}

impl ServiceClientKey {
    pub fn new(context_name: Option<String>, kubeconfig_path: Option<String>) -> Self {
        Self {
            context_name,
            kubeconfig_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_client_key() {
        let key1 = ServiceClientKey::new(
            Some("context1".to_string()),
            Some("/path/to/config".to_string()),
        );

        let key2 = ServiceClientKey::new(
            Some("context1".to_string()),
            Some("/path/to/config".to_string()),
        );

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_service_client_key_context_reuse() {
        let key_config1 = ServiceClientKey::new(
            Some("prod-cluster".to_string()),
            Some("/home/user/.kube/config".to_string()),
        );

        let key_config2 = ServiceClientKey::new(
            Some("prod-cluster".to_string()),
            Some("/home/user/.kube/config".to_string()),
        );

        assert_eq!(key_config1, key_config2);
    }
}
