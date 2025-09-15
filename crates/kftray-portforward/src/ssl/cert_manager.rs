use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use kftray_commons::models::settings_model::AppSettings;
use log::info;
use log::warn;
use serde::{
    Deserialize,
    Serialize,
};

use super::cert_generator::{
    CertificateGenerator,
    CertificatePair,
};
use super::cert_store::CertificateStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateInfo {
    pub domain: String,
    pub expires_at: Option<String>,
    pub is_valid: bool,
    pub file_path: Option<String>,
}

pub struct CertificateManager {
    generator: CertificateGenerator,
    store: CertificateStore,
    settings: AppSettings,
}

impl CertificateManager {
    pub fn new(settings: &AppSettings) -> Result<Self> {
        Ok(Self {
            generator: CertificateGenerator::new()?,
            store: CertificateStore::new()?,
            settings: settings.clone(),
        })
    }

    pub fn with_components(
        generator: CertificateGenerator, store: CertificateStore, settings: AppSettings,
    ) -> Self {
        Self {
            generator,
            store,
            settings,
        }
    }

    pub async fn ensure_certificate(&self, alias: &str) -> Result<CertificatePair> {
        if self.store.is_valid(alias).await {
            match self.store.load(alias).await {
                Ok(cert_pair) => {
                    info!("Using existing SSL certificate for alias: {}", alias);
                    return Ok(cert_pair);
                }
                Err(e) => {
                    info!("Failed to load existing certificate for {}: {}", alias, e);
                }
            }
        }

        info!("Generating SSL certificate for alias: {}", alias);

        let cert_pair = self
            .generator
            .generate_for_alias_with_validity(alias, self.settings.ssl_cert_validity_days)
            .await
            .context("Failed to generate certificate")?;

        self.store
            .store(alias, &cert_pair)
            .await
            .context("Failed to store certificate")?;

        info!(
            "SSL certificate generated and stored successfully for: {}",
            alias
        );
        Ok(cert_pair)
    }

    pub async fn ensure_global_certificate(&self, domains: &[String]) -> Result<CertificatePair> {
        let cert_name = "global-ssl-cert";
        let domains_hash = self.compute_domains_hash(domains);

        if self.store.is_valid(cert_name).await {
            match self.store.load(cert_name).await {
                Ok(cert_pair) => {
                    let missing_domains: Vec<_> = domains
                        .iter()
                        .filter(|domain| !cert_pair.subject_alt_names.contains(domain))
                        .collect();

                    if missing_domains.is_empty() {
                        info!("Using existing global SSL certificate (all domains covered)");
                        return Ok(cert_pair);
                    } else {
                        info!(
                            "Certificate missing domains: {:?}, regenerating",
                            missing_domains
                        );
                    }
                }
                Err(e) => {
                    info!("Failed to load existing global certificate: {}", e);
                }
            }
        }

        info!(
            "Generating global SSL certificate for domains: {:?}",
            domains
        );

        let cert_pair = self
            .generator
            .generate_for_domains_with_validity(
                domains.to_vec(),
                self.settings.ssl_cert_validity_days,
            )
            .await
            .context("Failed to generate global certificate")?;

        self.store
            .store(cert_name, &cert_pair)
            .await
            .context("Failed to store global certificate")?;

        self.store_domains_hash(cert_name, &domains_hash).await?;

        info!("Successfully generated global SSL certificate");

        Ok(cert_pair)
    }

    fn compute_domains_hash(&self, domains: &[String]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{
            Hash,
            Hasher,
        };

        let mut sorted_domains = domains.to_vec();
        sorted_domains.sort();
        sorted_domains.dedup();

        let mut hasher = DefaultHasher::new();
        for domain in &sorted_domains {
            domain.hash(&mut hasher);
        }
        format!("{:x}", hasher.finish())
    }

    async fn get_stored_domains_hash(&self, cert_name: &str) -> Result<String> {
        let hash_file = self.get_domains_hash_path(cert_name);
        tokio::fs::read_to_string(&hash_file)
            .await
            .map(|s| s.trim().to_string())
            .context("Failed to read stored domains hash")
    }

    async fn store_domains_hash(&self, cert_name: &str, domains_hash: &str) -> Result<()> {
        let hash_file = self.get_domains_hash_path(cert_name);
        tokio::fs::write(&hash_file, domains_hash)
            .await
            .context("Failed to write domains hash")
    }

    fn get_domains_hash_path(&self, cert_name: &str) -> std::path::PathBuf {
        self.store
            .get_store_path()
            .join(format!("{}.domains_hash", cert_name))
    }

    pub async fn domains_changed(&self, domains: &[String]) -> bool {
        let cert_name = "global-ssl-cert";
        let current_hash = self.compute_domains_hash(domains);

        match self.get_stored_domains_hash(cert_name).await {
            Ok(stored_hash) => stored_hash != current_hash,
            Err(_) => true,
        }
    }

    pub async fn cleanup_all_ssl_artifacts() -> Result<()> {
        info!("Cleaning up all SSL artifacts due to SSL being disabled");

        if std::env::var("KFTRAY_TEST_MODE").is_ok() {
            info!("Test mode enabled, skipping system cleanup operations");
            return Ok(());
        }

        let mut cleanup_errors = Vec::new();

        if let Err(e) = Self::cleanup_system_ca_certificate().await {
            warn!("Failed to cleanup system CA certificate: {}", e);
            cleanup_errors.push(format!("system CA certificate: {}", e));
        }

        let base_dir = if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
            PathBuf::from(config_dir)
        } else if let Some(config_dir) = dirs::config_dir() {
            config_dir.join("kftray")
        } else {
            warn!("No config directory found for SSL cleanup");
            cleanup_errors.push("No config directory found".to_string());
            PathBuf::new()
        };

        if !base_dir.as_os_str().is_empty() {
            let ssl_certs_dir = base_dir.join("ssl-certs");
            if ssl_certs_dir.exists() {
                match tokio::fs::remove_dir_all(&ssl_certs_dir).await {
                    Ok(_) => info!("Removed SSL certificates directory: {:?}", ssl_certs_dir),
                    Err(e) => {
                        warn!("Failed to remove SSL certificates directory: {}", e);
                        cleanup_errors.push(format!("certificates directory: {}", e));
                    }
                }
            }

            let ssl_ca_dir = base_dir.join("ssl-ca");
            if ssl_ca_dir.exists() {
                match tokio::fs::remove_dir_all(&ssl_ca_dir).await {
                    Ok(_) => info!("Removed SSL CA directory: {:?}", ssl_ca_dir),
                    Err(e) => {
                        warn!("Failed to remove SSL CA directory: {}", e);
                        cleanup_errors.push(format!("CA directory: {}", e));
                    }
                }
            }
        }

        if let Ok(store) = super::cert_store::CertificateStore::new()
            && let Err(e) = Self::cleanup_keychain_entries(&store).await
        {
            warn!("Failed to cleanup keychain entries: {}", e);
            cleanup_errors.push(format!("keychain entries: {}", e));
        }

        if cleanup_errors.is_empty() {
            info!("Successfully cleaned up all SSL artifacts");
            Ok(())
        } else {
            warn!(
                "SSL cleanup completed with some errors: {:?}",
                cleanup_errors
            );

            Ok(())
        }
    }

    async fn cleanup_keychain_entries(store: &super::cert_store::CertificateStore) -> Result<()> {
        info!("Cleaning up SSL vault from keychain");

        if let Err(e) = store.cleanup_ssl_vault().await {
            warn!("Failed to cleanup SSL vault: {}", e);
            return Err(e);
        }

        info!("Successfully cleaned up SSL vault from keychain");
        Ok(())
    }

    async fn cleanup_system_ca_certificate() -> Result<()> {
        if std::env::var("KFTRAY_SKIP_CA_INSTALL").is_ok() {
            info!("Skipping system CA removal due to KFTRAY_SKIP_CA_INSTALL environment variable");
            return Ok(());
        }

        let ca_cert_path = if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
            PathBuf::from(config_dir)
                .join("ssl-ca")
                .join("kftray-ca.crt")
        } else {
            dirs::config_dir()
                .context("No config directory found")?
                .join("kftray")
                .join("ssl-ca")
                .join("kftray-ca.crt")
        };

        if !ca_cert_path.exists() {
            info!("No CA certificate file found, skipping system trust store removal");
            return Ok(());
        }

        match tokio::fs::read_to_string(&ca_cert_path).await {
            Ok(ca_pem) => {
                if let Ok(ca_der_vec) = pem::parse(&ca_pem) {
                    let ca_cert_der = ca_der_vec.contents();

                    match super::platform::remove_ca_certificate(ca_cert_der).await {
                        Ok(_) => {
                            info!(
                                "Successfully removed kftray CA certificate from system trust store"
                            );
                        }
                        Err(e) => {
                            warn!(
                                "Failed to remove CA certificate from system trust store: {}",
                                e
                            );
                        }
                    }
                } else {
                    warn!("Failed to parse CA certificate PEM for removal");
                }
            }
            Err(e) => {
                warn!("Failed to read CA certificate file for removal: {}", e);
            }
        }

        Ok(())
    }

    pub async fn regenerate_global_certificate(
        &self, domains: &[String],
    ) -> Result<CertificatePair> {
        let cert_name = "global-ssl-cert";
        let domains_hash = self.compute_domains_hash(domains);

        info!(
            "Force regenerating global SSL certificate for domains: {:?}",
            domains
        );

        if let Err(e) = self.store.remove(cert_name).await {
            warn!("Failed to remove existing certificate: {}", e);
        }

        let hash_file = self.get_domains_hash_path(cert_name);
        if let Err(e) = tokio::fs::remove_file(&hash_file).await {
            warn!("Failed to remove existing domains hash file: {}", e);
        }

        let cert_pair = self
            .generator
            .generate_for_domains_with_validity(
                domains.to_vec(),
                self.settings.ssl_cert_validity_days,
            )
            .await
            .context("Failed to generate global certificate")?;

        self.store
            .store(cert_name, &cert_pair)
            .await
            .context("Failed to store global certificate")?;

        self.store_domains_hash(cert_name, &domains_hash).await?;

        info!("Successfully regenerated global SSL certificate");

        Ok(cert_pair)
    }

    pub async fn regenerate_certificate(&self, alias: &str) -> Result<CertificatePair> {
        info!("Regenerating SSL certificate for alias: {}", alias);

        let cert_pair = self
            .generator
            .generate_for_alias_with_validity(alias, self.settings.ssl_cert_validity_days)
            .await
            .context("Failed to regenerate certificate")?;

        self.store
            .store(alias, &cert_pair)
            .await
            .context("Failed to store regenerated certificate")?;

        info!("SSL certificate regenerated successfully for: {}", alias);
        Ok(cert_pair)
    }

    pub async fn get_certificate_info(&self, alias: &str) -> Result<CertificateInfo> {
        let is_valid = self.store.is_valid(alias).await;
        let exists = self.store.exists(alias).await;

        if !exists {
            let expected_domain = if alias.contains('.') {
                alias.to_string()
            } else {
                format!("{}.local", alias)
            };

            return Ok(CertificateInfo {
                domain: expected_domain,
                expires_at: None,
                is_valid: false,
                file_path: None,
            });
        }

        let domain = if let Ok(cert_pair) = self.store.load(alias).await {
            cert_pair.domain
        } else {
            // Fallback to expected domain format
            if alias.contains('.') {
                alias.to_string()
            } else {
                format!("{}.local", alias)
            }
        };

        Ok(CertificateInfo {
            domain,
            expires_at: None,
            is_valid,
            file_path: Some(format!("~/.config/kftray/ssl-certs/{}.crt", alias)),
        })
    }

    pub async fn list_all_certificates(&self) -> Result<Vec<CertificateInfo>> {
        let store_path = if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
            PathBuf::from(config_dir).join("ssl-certs")
        } else {
            dirs::config_dir()
                .context("No config directory found")?
                .join("kftray")
                .join("ssl-certs")
        };

        if !store_path.exists() {
            return Ok(Vec::new());
        }

        let mut certificates = Vec::new();
        let mut entries = tokio::fs::read_dir(store_path)
            .await
            .context("Failed to read SSL certificates directory")?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .context("Failed to read directory entry")?
        {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            if file_name_str.ends_with(".crt") {
                let alias = file_name_str.trim_end_matches(".crt");
                let cert_info = self.get_certificate_info(alias).await?;
                certificates.push(cert_info);
            }
        }

        Ok(certificates)
    }

    pub async fn remove_certificate(&self, alias: &str) -> Result<()> {
        self.store
            .remove(alias)
            .await
            .context("Failed to remove certificate")?;

        info!("SSL certificate removed for alias: {}", alias);
        Ok(())
    }

    pub async fn certificate_exists(&self, alias: &str) -> bool {
        self.store.exists(alias).await
    }

    pub async fn is_certificate_valid(&self, alias: &str) -> bool {
        self.store.is_valid(alias).await
    }

    pub async fn ensure_ca_installed_and_trusted(&self) -> Result<()> {
        if std::env::var("KFTRAY_SKIP_CA_INSTALL").is_ok() {
            info!("Skipping CA installation due to KFTRAY_SKIP_CA_INSTALL environment variable");
            return Ok(());
        }

        static CA_INSTALL_ATTEMPTED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        if CA_INSTALL_ATTEMPTED.load(std::sync::atomic::Ordering::Relaxed) {
            info!("CA installation already attempted this session, skipping redundant check");
            return Ok(());
        }

        info!("Ensuring CA certificate is installed and trusted system-wide");

        if let Ok(Some(_ca_info)) = self.generator.get_ca_info().await {
            info!("CA certificate file exists, verifying system installation status");

            let ca_cert_path = if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
                PathBuf::from(config_dir)
                    .join("ssl-ca")
                    .join("kftray-ca.crt")
            } else {
                dirs::config_dir()
                    .context("No config directory found")?
                    .join("kftray")
                    .join("ssl-ca")
                    .join("kftray-ca.crt")
            };

            if ca_cert_path.exists()
                && let Ok(ca_pem) = tokio::fs::read_to_string(&ca_cert_path).await
                && let Ok(ca_der_vec) = pem::parse(&ca_pem)
            {
                let ca_cert_der = ca_der_vec.contents();

                match super::platform::is_ca_installed(ca_cert_der).await {
                    Ok(true) => {
                        info!("CA certificate already installed in system trust store");
                        CA_INSTALL_ATTEMPTED.store(true, std::sync::atomic::Ordering::Relaxed);
                        return Ok(());
                    }
                    Ok(false) => {
                        info!("CA file exists but not installed in system trust store");
                    }
                    Err(e) => {
                        warn!("Failed to check CA installation status: {}", e);
                    }
                }
            }

            info!("CA certificate exists but needs system installation");

            if let Ok(ca_pem) = tokio::fs::read_to_string(&ca_cert_path).await {
                if let Ok(ca_der_vec) = pem::parse(&ca_pem) {
                    let ca_cert_der = ca_der_vec.contents();

                    match super::platform::install_ca_certificate(ca_cert_der, &ca_pem).await {
                        Ok(_) => {
                            info!(
                                "Successfully installed existing CA certificate to system trust store"
                            );
                        }
                        Err(e) => {
                            warn!("Failed to install existing CA certificate: {}", e);
                            return Err(e);
                        }
                    }
                } else {
                    return Err(anyhow::anyhow!("Failed to parse existing CA certificate"));
                }
            } else {
                return Err(anyhow::anyhow!("Failed to read existing CA certificate"));
            }
        } else {
            info!("No CA certificate found, creating and installing new one");

            let _temp_cert = self
                .generator
                .generate_for_alias("_ca_install_temp")
                .await?;
            let _ = self.store.remove("_ca_install_temp").await;

            let ca_cert_path = if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
                PathBuf::from(config_dir)
                    .join("ssl-ca")
                    .join("kftray-ca.crt")
            } else {
                dirs::config_dir()
                    .context("No config directory found")?
                    .join("kftray")
                    .join("ssl-ca")
                    .join("kftray-ca.crt")
            };

            if let Ok(ca_pem) = tokio::fs::read_to_string(&ca_cert_path).await {
                if let Ok(ca_der_vec) = pem::parse(&ca_pem) {
                    let ca_cert_der = ca_der_vec.contents();

                    match super::platform::install_ca_certificate(ca_cert_der, &ca_pem).await {
                        Ok(_) => {
                            info!(
                                "Successfully installed new CA certificate to system trust store"
                            );
                        }
                        Err(e) => {
                            warn!("Failed to install new CA certificate: {}", e);
                            return Err(e);
                        }
                    }
                } else {
                    return Err(anyhow::anyhow!("Failed to parse new CA certificate"));
                }
            } else {
                return Err(anyhow::anyhow!("Failed to read new CA certificate"));
            }
        }

        CA_INSTALL_ATTEMPTED.store(true, std::sync::atomic::Ordering::Relaxed);

        info!("CA certificate installation and trust completed");
        Ok(())
    }

    pub async fn ensure_ca_installed(&self) -> Result<()> {
        if let Ok(Some(_ca_info)) = self.generator.get_ca_info().await {
            info!("CA certificate already installed, skipping installation");
            return Ok(());
        }

        info!("CA certificate not found, installing...");

        let _temp_cert = self
            .generator
            .generate_for_alias("_ca_install_temp")
            .await?;

        let _ = self.store.remove("_ca_install_temp").await;

        info!("CA certificate installation completed");
        Ok(())
    }

    pub async fn ensure_wildcard_certificate(&self) -> Result<CertificatePair> {
        let wildcard_alias = "*.local";

        if self.store.is_valid(wildcard_alias).await {
            match self.store.load(wildcard_alias).await {
                Ok(cert_pair) => {
                    info!("Using existing *.local wildcard certificate");
                    return Ok(cert_pair);
                }
                Err(e) => {
                    info!("Failed to load existing wildcard certificate: {}", e);
                }
            }
        }

        info!("Generating *.local wildcard certificate and installing CA");

        let cert_pair = self
            .generator
            .generate_wildcard_certificate("local", self.settings.ssl_cert_validity_days)
            .await
            .context("Failed to generate wildcard certificate")?;

        self.store
            .store(wildcard_alias, &cert_pair)
            .await
            .context("Failed to store wildcard certificate")?;

        info!("Successfully created *.local wildcard certificate and installed CA");
        Ok(cert_pair)
    }

    pub async fn ensure_global_certificate_for_all_configs(&self) -> Result<CertificatePair> {
        let domains = Self::collect_all_domains_from_configs().await?;
        self.ensure_global_certificate(&domains).await
    }

    pub async fn collect_all_domains_from_configs() -> Result<Vec<String>> {
        let mut domains = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "::1".to_string(),
        ];

        match kftray_commons::utils::config::get_configs().await {
            Ok(configs) => {
                let config_count = configs.len();
                for config in configs {
                    if let Some(local_address) = &config.local_address
                        && !local_address.is_empty()
                        && !domains.contains(local_address)
                    {
                        domains.push(local_address.clone());
                    }

                    if let Some(alias) = &config.alias {
                        if !alias.is_empty() && !domains.contains(alias) {
                            domains.push(alias.clone());
                        }
                        let local_domain = format!("{}.local", alias);
                        if !domains.contains(&local_domain) {
                            domains.push(local_domain);
                        }
                    }
                }
                info!(
                    "Collected domains from {} configs: {:?}",
                    config_count, domains
                );
            }
            Err(e) => {
                warn!(
                    "Failed to get configs for domain collection, using defaults: {}",
                    e
                );
            }
        }

        domains.sort();
        domains.dedup();

        Ok(domains)
    }

    pub async fn regenerate_certificate_for_all_configs(&self) -> Result<CertificatePair> {
        let domains = Self::collect_all_domains_from_configs().await?;
        self.regenerate_global_certificate(&domains).await
    }

    pub async fn load_global_certificate(&self) -> Result<CertificatePair> {
        let cert_name = "global-ssl-cert";

        if !self.store.exists(cert_name).await {
            return Err(anyhow::anyhow!(
                "Global SSL certificate does not exist. SSL must be enabled in settings first."
            ));
        }

        if !self.store.is_valid(cert_name).await {
            info!("Existing global certificate is invalid or expired, regenerating");
            return self.ensure_global_certificate_for_all_configs().await;
        }

        match self.store.load(cert_name).await {
            Ok(cert_pair) => {
                let current_domains = Self::collect_all_domains_from_configs()
                    .await
                    .unwrap_or_else(|_| {
                        warn!("Failed to collect domains, using existing certificate");
                        vec![]
                    });

                if !current_domains.is_empty() {
                    let missing_domains: Vec<_> = current_domains
                        .iter()
                        .filter(|domain| !cert_pair.subject_alt_names.contains(domain))
                        .collect();

                    if !missing_domains.is_empty() {
                        info!(
                            "Certificate missing domains: {:?}, regenerating",
                            missing_domains
                        );
                        info!(
                            "Current certificate domains: {:?}",
                            cert_pair.subject_alt_names
                        );
                        info!("Required domains: {:?}", current_domains);

                        if let Err(e) = self.store.remove(cert_name).await {
                            warn!("Failed to remove corrupted certificate: {}", e);
                        }

                        return self.ensure_global_certificate_for_all_configs().await;
                    }
                }

                info!("Successfully loaded existing global SSL certificate");
                Ok(cert_pair)
            }
            Err(e) => {
                warn!("Failed to load global certificate, regenerating: {}", e);
                self.ensure_global_certificate_for_all_configs().await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn create_test_manager() -> (CertificateManager, TempDir) {
        unsafe {
            std::env::set_var("KFTRAY_SKIP_CA_INSTALL", "1");
            std::env::set_var("KFTRAY_TEST_MODE", "1");
        }

        let temp_dir = TempDir::new().unwrap();

        let settings = kftray_commons::models::settings_model::AppSettings {
            ssl_enabled: true,
            ..Default::default()
        };
        let generator = CertificateGenerator::for_testing(temp_dir.path());
        let store = super::CertificateStore::with_path(temp_dir.path().to_path_buf()).unwrap();

        let manager = CertificateManager::with_components(generator, store, settings);

        (manager, temp_dir)
    }

    #[tokio::test]
    async fn test_ensure_certificate_generation() {
        let (manager, _temp_dir) = create_test_manager().await;

        let cert_pair = manager.ensure_certificate("test-service").await.unwrap();
        assert_eq!(cert_pair.domain, "test-service.local");
        assert!(!cert_pair.certificate.is_empty());
    }

    #[tokio::test]
    async fn test_regenerate_certificate() {
        let (manager, _temp_dir) = create_test_manager().await;

        let cert_pair1 = manager.ensure_certificate("test-service").await.unwrap();
        let cert_pair2 = manager
            .regenerate_certificate("test-service")
            .await
            .unwrap();

        assert_eq!(cert_pair1.domain, cert_pair2.domain);

        assert_ne!(cert_pair1.certificate, cert_pair2.certificate);
    }

    #[tokio::test]
    async fn test_certificate_info() {
        let (manager, _temp_dir) = create_test_manager().await;

        let info = manager.get_certificate_info("non-existent").await.unwrap();
        assert!(!info.is_valid);
        assert!(info.file_path.is_none());

        manager.ensure_certificate("test-service").await.unwrap();
        let info = manager.get_certificate_info("test-service").await.unwrap();
        assert_eq!(info.domain, "test-service.local");
    }

    #[tokio::test]
    async fn test_certificate_existence() {
        use crate::ssl::cert_store::TEST_SSL_VAULT;
        *TEST_SSL_VAULT.lock().unwrap() = Default::default();

        let (manager, _temp_dir) = create_test_manager().await;

        assert!(!manager.certificate_exists("test-service").await);

        manager.ensure_certificate("test-service").await.unwrap();

        assert!(manager.certificate_exists("test-service").await);
    }

    #[tokio::test]
    async fn test_remove_certificate() {
        use crate::ssl::cert_store::TEST_SSL_VAULT;
        *TEST_SSL_VAULT.lock().unwrap() = Default::default();

        let (manager, _temp_dir) = create_test_manager().await;

        manager.ensure_certificate("test-service").await.unwrap();
        assert!(manager.certificate_exists("test-service").await);

        manager.remove_certificate("test-service").await.unwrap();
        assert!(!manager.certificate_exists("test-service").await);
    }

    #[tokio::test]
    async fn test_cleanup_ssl_artifacts() {
        unsafe {
            std::env::set_var("KFTRAY_SKIP_CA_INSTALL", "1");
        }

        let cleanup_result = CertificateManager::cleanup_all_ssl_artifacts().await;
        assert!(cleanup_result.is_ok());

        let cleanup_result2 = CertificateManager::cleanup_all_ssl_artifacts().await;
        assert!(cleanup_result2.is_ok());
    }

    #[tokio::test]
    async fn test_domains_change_detection() {
        let (_manager, _temp_dir) = create_test_manager().await;
        let manager = &_manager;

        let domains1 = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        let domains2 = vec!["localhost".to_string(), "test.local".to_string()];
        let domains3 = vec!["127.0.0.1".to_string(), "localhost".to_string()];

        assert!(manager.domains_changed(&domains1).await);

        manager.ensure_global_certificate(&domains1).await.unwrap();

        assert!(!manager.domains_changed(&domains1).await);

        assert!(!manager.domains_changed(&domains3).await);

        assert!(manager.domains_changed(&domains2).await);
    }

    #[tokio::test]
    async fn test_load_global_certificate() {
        use crate::ssl::cert_store::TEST_SSL_VAULT;
        *TEST_SSL_VAULT.lock().unwrap() = Default::default();

        let (_manager, _temp_dir) = create_test_manager().await;
        let manager = &_manager;

        let result = manager.store.load("global-ssl-cert").await;
        assert!(result.is_err());

        let domains = vec!["localhost".to_string(), "test.local".to_string()];
        let generated_cert = manager.ensure_global_certificate(&domains).await.unwrap();

        let loaded_cert = manager.store.load("global-ssl-cert").await.unwrap();

        assert_eq!(
            generated_cert.certificate.len(),
            loaded_cert.certificate.len()
        );

        assert!(
            generated_cert
                .subject_alt_names
                .contains(&"localhost".to_string())
        );
        assert!(
            loaded_cert
                .subject_alt_names
                .contains(&"localhost".to_string())
        );
        assert_eq!(
            generated_cert.subject_alt_names.len(),
            loaded_cert.subject_alt_names.len()
        );

        let loaded_cert2 = manager.store.load("global-ssl-cert").await.unwrap();
        assert_eq!(loaded_cert.certificate, loaded_cert2.certificate);
    }

    #[tokio::test]
    async fn test_ensure_global_certificate_for_all_configs() {
        use crate::ssl::cert_store::TEST_SSL_VAULT;
        *TEST_SSL_VAULT.lock().unwrap() = Default::default();

        let (_manager, _temp_dir) = create_test_manager().await;
        let manager = &_manager;

        let cert_pair = manager
            .ensure_global_certificate_for_all_configs()
            .await
            .unwrap();

        assert!(
            cert_pair
                .subject_alt_names
                .contains(&"localhost".to_string())
        );
        assert!(
            cert_pair
                .subject_alt_names
                .contains(&"127.0.0.1".to_string())
        );
        assert!(cert_pair.subject_alt_names.contains(&"::1".to_string()));

        let loaded_cert = manager.load_global_certificate().await.unwrap();
        assert_eq!(cert_pair.certificate.len(), loaded_cert.certificate.len());
    }
}
