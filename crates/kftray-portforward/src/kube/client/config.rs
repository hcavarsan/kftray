use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use kftray_commons::config_dir::get_kubeconfig_paths;
use kube::config::{
    Config,
    KubeConfigOptions,
    Kubeconfig,
};
use log::{
    error,
    info,
};
use openssl::base64::{
    decode_block,
    encode_block,
};
use secrecy::ExposeSecret;

use super::utils::{
    convert_pkcs8_to_pkcs1,
    is_pkcs8_key,
};

pub trait ConfigExtClone {
    fn clone_with_invalid_certs(&self, accept_invalid_certs: bool) -> Self;
}

impl ConfigExtClone for Config {
    fn clone_with_invalid_certs(&self, accept_invalid_certs: bool) -> Self {
        let mut config = self.clone();
        config.accept_invalid_certs = accept_invalid_certs;
        config
    }
}

pub fn get_kubeconfig_paths_from_option(kubeconfig: Option<String>) -> Result<Vec<PathBuf>> {
    match kubeconfig {
        Some(path) if path == "default" => {
            info!("Using default kubeconfig paths.");
            get_kubeconfig_paths()
        }
        Some(path) => {
            info!("Using provided kubeconfig paths: {path}");
            Ok(path.split(':').map(PathBuf::from).collect())
        }
        None => {
            info!("No kubeconfig path provided, using default paths.");
            get_kubeconfig_paths()
        }
    }
}

pub fn merge_kubeconfigs(paths: &[PathBuf]) -> Result<(Kubeconfig, Vec<String>, Vec<String>)> {
    let mut errors = Vec::new();
    let mut all_contexts = Vec::new();
    let mut merged_kubeconfig = Kubeconfig::default();

    for path in paths {
        info!("Attempting to read kubeconfig from path: {path:?}");
        match Kubeconfig::read_from(path) {
            Ok(kubeconfig) => {
                info!("Successfully read kubeconfig from {path:?}");
                let contexts = crate::kube::operations::list_contexts(&kubeconfig);
                all_contexts.extend(contexts.clone());
                info!("Available contexts in {path:?}: {contexts:?}");
                match merged_kubeconfig.clone().merge(kubeconfig) {
                    Ok(merged) => merged_kubeconfig = merged,
                    Err(e) => {
                        let error_msg = format!("Failed to merge kubeconfig from {path:?}: {e}");
                        error!("{error_msg}");
                        errors.push(error_msg);
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to read kubeconfig from {path:?}: {e}");
                error!("{error_msg}");
                errors.push(error_msg);
            }
        }
    }

    Ok((merged_kubeconfig, all_contexts, errors))
}

pub async fn create_config_with_context(
    kubeconfig: &Kubeconfig, context_name: &str,
) -> Result<Config> {
    info!("Creating configuration for context: {context_name}");
    let mut kubeconfig = kubeconfig.clone();

    for auth_info in &mut kubeconfig.auth_infos {
        if let Some(ref mut auth_info_data) = auth_info.auth_info
            && let Some(client_key_data) = &auth_info_data.client_key_data
        {
            let decoded_key = decode_block(client_key_data.expose_secret())
                .context("Failed to decode client key data")?;

            if is_pkcs8_key(&decoded_key) {
                let converted_key = convert_pkcs8_to_pkcs1(&decoded_key)
                    .context("Failed to convert PKCS#8 key to PKCS#1")?;
                let encoded_key = encode_block(&converted_key);
                auth_info_data.client_key_data = Some(encoded_key.into());
            }
        }
    }

    let context_to_use = if context_name == "@current" {
        kubeconfig.current_context.clone()
    } else {
        Some(context_name.to_owned())
    };

    Config::from_custom_kubeconfig(
        kubeconfig,
        &KubeConfigOptions {
            context: context_to_use,
            ..Default::default()
        },
    )
    .await
    .context("Failed to create configuration from kubeconfig")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_kubeconfig_paths_from_option() {
        use std::env;
        use std::fs;
        use std::sync::Mutex;

        use tempfile::TempDir;

        lazy_static::lazy_static! {
            static ref ENV_LOCK: Mutex<()> = Mutex::new(());
        }
        let _env_guard = ENV_LOCK.lock().unwrap();

        let original_kubeconfig = env::var("KUBECONFIG").ok();
        let original_home = env::var("HOME").ok();

        let temp_dir = TempDir::new().unwrap();

        let explicit_paths =
            get_kubeconfig_paths_from_option(Some("/path1:/path2".to_string())).unwrap();
        assert_eq!(explicit_paths.len(), 2);
        assert_eq!(explicit_paths[0], std::path::Path::new("/path1"));
        assert_eq!(explicit_paths[1], std::path::Path::new("/path2"));

        let mock_kubeconfig_path = temp_dir.path().join("mock_kubeconfig");
        fs::write(&mock_kubeconfig_path, "mock kubeconfig content").unwrap();

        unsafe { env::set_var("KUBECONFIG", mock_kubeconfig_path.to_str().unwrap()) };

        let default_path_result = get_kubeconfig_paths_from_option(Some("default".to_string()));
        assert!(default_path_result.is_ok());
        let default_path = default_path_result.unwrap();
        assert!(!default_path.is_empty());
        assert_eq!(default_path[0], mock_kubeconfig_path);

        let none_path_result = get_kubeconfig_paths_from_option(None);
        assert!(none_path_result.is_ok());
        let none_path = none_path_result.unwrap();
        assert!(!none_path.is_empty());
        assert_eq!(none_path[0], mock_kubeconfig_path);

        let fake_home = temp_dir.path().join("fake_home");
        fs::create_dir_all(fake_home.join(".kube")).unwrap();
        let fake_kubeconfig = fake_home.join(".kube").join("config");
        fs::write(&fake_kubeconfig, "home dir kubeconfig content").unwrap();

        unsafe { env::remove_var("KUBECONFIG") };

        unsafe { env::set_var("HOME", fake_home.to_str().unwrap()) };

        let home_fallback_result = get_kubeconfig_paths_from_option(None);
        assert!(home_fallback_result.is_ok());
        let home_fallback_path = home_fallback_result.unwrap();
        assert!(!home_fallback_path.is_empty());
        assert_eq!(home_fallback_path[0], fake_kubeconfig);

        let nonexistent_dir = temp_dir.path().join("nonexistent");

        unsafe { env::set_var("HOME", nonexistent_dir.to_str().unwrap()) };

        unsafe {
            env::set_var(
                "KUBECONFIG",
                temp_dir.path().join("nonexistent_file").to_str().unwrap(),
            )
        };

        let error_result = get_kubeconfig_paths_from_option(None);
        assert!(error_result.is_err());

        match original_kubeconfig {
            Some(val) => unsafe { env::set_var("KUBECONFIG", val) },
            None => unsafe { env::remove_var("KUBECONFIG") },
        }

        match original_home {
            Some(val) => unsafe { env::set_var("HOME", val) },
            None => unsafe { env::remove_var("HOME") },
        }
    }

    #[test]
    fn test_merge_kubeconfigs_empty() {
        let (_config, contexts, errors) = merge_kubeconfigs(&[]).unwrap();
        assert!(contexts.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_merge_kubeconfigs_with_invalid_path() {
        let paths = vec![PathBuf::from("/invalid/path/that/should/not/exist")];
        let (_config, contexts, errors) = merge_kubeconfigs(&paths).unwrap();
        assert!(contexts.is_empty());
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_list_contexts() {
        let kubeconfig = Kubeconfig {
            contexts: vec![
                kube::config::NamedContext {
                    name: "context1".to_string(),
                    context: Some(kube::config::Context::default()),
                },
                kube::config::NamedContext {
                    name: "context2".to_string(),
                    context: Some(kube::config::Context::default()),
                },
            ],
            ..Default::default()
        };

        let contexts = crate::kube::operations::list_contexts(&kubeconfig);
        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0], "context1");
        assert_eq!(contexts[1], "context2");
    }

    #[test]
    fn test_config_ext_clone() {
        let mut config = Config::new("https://example.com".parse().unwrap());
        config.accept_invalid_certs = false;

        let cloned_config = config.clone_with_invalid_certs(true);
        assert!(cloned_config.accept_invalid_certs);

        let cloned_config_false = config.clone_with_invalid_certs(false);
        assert!(!cloned_config_false.accept_invalid_certs);
    }

    #[tokio::test]
    async fn test_create_config_with_context() {
        let mut kubeconfig = Kubeconfig::default();
        let context_name = "test-context";

        let named_context = kube::config::NamedContext {
            name: context_name.to_string(),
            context: Some(kube::config::Context::default()),
        };
        kubeconfig.contexts = vec![named_context];

        let result = create_config_with_context(&kubeconfig, context_name).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_config_ext_clone_preserves_other_fields() {
        let mut config = Config::new("https://example.com".parse().unwrap());
        config.accept_invalid_certs = false;
        config.default_namespace = "test-namespace".to_string();
        config.read_timeout = Some(std::time::Duration::from_secs(30));

        let cloned_config = config.clone_with_invalid_certs(true);
        assert!(cloned_config.accept_invalid_certs);
        assert_eq!(
            cloned_config.default_namespace,
            "test-namespace".to_string()
        );
        assert_eq!(
            cloned_config.read_timeout,
            Some(std::time::Duration::from_secs(30))
        );
        assert_eq!(cloned_config.cluster_url, config.cluster_url);
    }

    #[tokio::test]
    async fn test_create_config_with_insecure_skip_tls_verify_true() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let kubeconfig_path = temp_dir.path().join("kubeconfig");

        let kubeconfig_content = r#"
apiVersion: v1
kind: Config
clusters:
- name: test-cluster
  cluster:
    server: https://test-server.com
    insecure-skip-tls-verify: true
contexts:
- name: test-context
  context:
    cluster: test-cluster
    user: test-user
current-context: test-context
users:
- name: test-user
  user:
    token: test-token
"#;

        std::fs::write(&kubeconfig_path, kubeconfig_content).unwrap();

        let kubeconfig = Kubeconfig::read_from(&kubeconfig_path).unwrap();
        let config = create_config_with_context(&kubeconfig, "test-context")
            .await
            .unwrap();

        assert!(config.accept_invalid_certs);
    }

    #[tokio::test]
    async fn test_create_config_with_insecure_skip_tls_verify_false() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let kubeconfig_path = temp_dir.path().join("kubeconfig");

        let kubeconfig_content = r#"
apiVersion: v1
kind: Config
clusters:
- name: test-cluster
  cluster:
    server: https://test-server.com
    insecure-skip-tls-verify: false
contexts:
- name: test-context
  context:
    cluster: test-cluster
    user: test-user
current-context: test-context
users:
- name: test-user
  user:
    token: test-token
"#;

        std::fs::write(&kubeconfig_path, kubeconfig_content).unwrap();

        let kubeconfig = Kubeconfig::read_from(&kubeconfig_path).unwrap();
        let config = create_config_with_context(&kubeconfig, "test-context")
            .await
            .unwrap();

        assert!(!config.accept_invalid_certs);
    }

    #[tokio::test]
    async fn test_create_config_without_insecure_skip_tls_verify() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let kubeconfig_path = temp_dir.path().join("kubeconfig");

        let kubeconfig_content = r#"
apiVersion: v1
kind: Config
clusters:
- name: test-cluster
  cluster:
    server: https://test-server.com
contexts:
- name: test-context
  context:
    cluster: test-cluster
    user: test-user
current-context: test-context
users:
- name: test-user
  user:
    token: test-token
"#;

        std::fs::write(&kubeconfig_path, kubeconfig_content).unwrap();

        let kubeconfig = Kubeconfig::read_from(&kubeconfig_path).unwrap();
        let config = create_config_with_context(&kubeconfig, "test-context")
            .await
            .unwrap();

        assert!(!config.accept_invalid_certs);
    }

    #[tokio::test]
    async fn test_merge_kubeconfigs_with_insecure_skip_tls() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let kubeconfig_path = temp_dir.path().join("kubeconfig");

        let kubeconfig_content = r#"
apiVersion: v1
kind: Config
clusters:
- name: secure-cluster
  cluster:
    server: https://secure-server.com
    insecure-skip-tls-verify: false
- name: insecure-cluster
  cluster:
    server: https://insecure-server.com
    insecure-skip-tls-verify: true
contexts:
- name: secure-context
  context:
    cluster: secure-cluster
    user: test-user
- name: insecure-context
  context:
    cluster: insecure-cluster
    user: test-user
users:
- name: test-user
  user:
    token: test-token
"#;

        std::fs::write(&kubeconfig_path, kubeconfig_content).unwrap();

        let (merged_kubeconfig, contexts, errors) = merge_kubeconfigs(&[kubeconfig_path]).unwrap();

        assert_eq!(contexts.len(), 2);
        assert!(contexts.contains(&"secure-context".to_string()));
        assert!(contexts.contains(&"insecure-context".to_string()));
        assert!(errors.is_empty());

        let insecure_cluster = merged_kubeconfig
            .clusters
            .iter()
            .find(|c| c.name == "insecure-cluster")
            .unwrap();
        assert_eq!(
            insecure_cluster
                .cluster
                .as_ref()
                .unwrap()
                .insecure_skip_tls_verify,
            Some(true)
        );

        let secure_cluster = merged_kubeconfig
            .clusters
            .iter()
            .find(|c| c.name == "secure-cluster")
            .unwrap();
        assert_eq!(
            secure_cluster
                .cluster
                .as_ref()
                .unwrap()
                .insecure_skip_tls_verify,
            Some(false)
        );
    }
}
