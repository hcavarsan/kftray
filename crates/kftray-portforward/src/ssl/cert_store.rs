use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{
    Context,
    Result,
};
use base64::Engine;
use keyring::Entry;
use lazy_static::lazy_static;
use log::{
    debug,
    info,
    warn,
};
use rustls::pki_types::{
    CertificateDer,
    PrivateKeyDer,
    PrivatePkcs8KeyDer,
};
use serde::{
    Deserialize,
    Serialize,
};
use tokio::fs as async_fs;

use super::cert_generator::CertificatePair;

const KFTRAY_SERVICE: &str = "kftray-ssl";
const KFTRAY_SSL_VAULT: &str = "ssl-keys-vault";

lazy_static! {
    pub static ref TEST_SSL_VAULT: Mutex<SslKeyVault> = Mutex::new(SslKeyVault::default());
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SslKeyVault {
    pub ca_private_key: Option<String>,
    pub certificate_keys: HashMap<String, String>,
}

pub struct CertificateStore {
    store_path: PathBuf,
}

impl CertificateStore {
    pub fn new() -> Result<Self> {
        let store_path = if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
            PathBuf::from(config_dir).join("ssl-certs")
        } else {
            dirs::config_dir()
                .context("No config directory found")?
                .join("kftray")
                .join("ssl-certs")
        };

        fs::create_dir_all(&store_path).context("Failed to create SSL certificate directory")?;

        Ok(Self { store_path })
    }

    pub fn with_path(store_path: PathBuf) -> Result<Self> {
        fs::create_dir_all(&store_path).context("Failed to create SSL certificate directory")?;
        Ok(Self { store_path })
    }

    pub async fn store(&self, alias: &str, cert_pair: &CertificatePair) -> Result<()> {
        info!(
            "Storing certificate for alias: {} (certificate on disk, private key in keychain)",
            alias
        );

        let cert_file = self.store_path.join(format!("{}.crt", alias));
        let metadata_file = self.store_path.join(format!("{}.json", alias));

        let cert_pem = self.certificate_to_pem(&cert_pair.certificate)?;
        let key_pem = self.private_key_to_pem(&cert_pair.private_key)?;

        async_fs::write(&cert_file, &cert_pem)
            .await
            .context("Failed to write certificate file")?;

        self.store_private_key_in_keychain(alias, &key_pem)
            .await
            .context("Failed to store private key in keychain")?;

        let metadata = serde_json::json!({
            "domain": cert_pair.domain,
            "local_domain": cert_pair.local_domain,
            "subject_alt_names": cert_pair.subject_alt_names,
        });

        async_fs::write(&metadata_file, serde_json::to_string_pretty(&metadata)?)
            .await
            .context("Failed to write certificate metadata")?;

        #[cfg(test)]
        {
            debug!("Test mode: cert file exists: {}", cert_file.exists());
            debug!(
                "Test mode: private key stored: {}",
                self.private_key_exists_in_keychain(alias).await
            );
        }

        info!(
            "Successfully stored certificate and private key securely for alias: {}",
            alias
        );
        Ok(())
    }

    pub async fn load(&self, alias: &str) -> Result<CertificatePair> {
        debug!(
            "Loading certificate for alias: {} (certificate from disk, private key from keychain)",
            alias
        );

        let cert_file = self.store_path.join(format!("{}.crt", alias));
        let metadata_file = self.store_path.join(format!("{}.json", alias));

        let cert_pem = async_fs::read_to_string(cert_file)
            .await
            .context("Failed to read certificate file")?;
        let key_pem = self
            .retrieve_private_key_from_keychain(alias)
            .await
            .context("Failed to retrieve private key from keychain")?;

        let certificate = self.pem_to_certificate(&cert_pem)?;
        let private_key = self.pem_to_private_key(&key_pem)?;

        let (domain, local_domain, subject_alt_names) = if metadata_file.exists() {
            match async_fs::read_to_string(metadata_file).await {
                Ok(metadata_str) => {
                    match serde_json::from_str::<serde_json::Value>(&metadata_str) {
                        Ok(metadata) => {
                            let domain = metadata["domain"].as_str().unwrap_or(alias).to_string();
                            let local_domain = metadata["local_domain"]
                                .as_str()
                                .unwrap_or(&format!("{}.local", alias))
                                .to_string();
                            let subject_alt_names = metadata["subject_alt_names"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_else(Vec::new);
                            (domain, local_domain, subject_alt_names)
                        }
                        Err(_) => (alias.to_string(), format!("{}.local", alias), vec![]),
                    }
                }
                Err(_) => (alias.to_string(), format!("{}.local", alias), vec![]),
            }
        } else {
            (alias.to_string(), format!("{}.local", alias), vec![])
        };

        debug!(
            "Successfully loaded certificate and private key for alias: {}",
            alias
        );

        Ok(CertificatePair {
            certificate,
            private_key,
            domain,
            local_domain,
            subject_alt_names,
        })
    }

    pub async fn exists(&self, alias: &str) -> bool {
        let cert_file = self.store_path.join(format!("{}.crt", alias));
        let key_exists = self.private_key_exists_in_keychain(alias).await;

        cert_file.exists() && key_exists
    }

    pub async fn is_valid(&self, alias: &str) -> bool {
        if !self.exists(alias).await {
            return false;
        }

        true
    }

    pub fn get_store_path(&self) -> &std::path::Path {
        &self.store_path
    }

    pub async fn remove(&self, alias: &str) -> Result<()> {
        info!("Removing certificate and private key for alias: {}", alias);

        let cert_file = self.store_path.join(format!("{}.crt", alias));
        let metadata_file = self.store_path.join(format!("{}.json", alias));

        if cert_file.exists() {
            async_fs::remove_file(cert_file)
                .await
                .context("Failed to remove certificate file")?;
        }

        if metadata_file.exists() {
            async_fs::remove_file(metadata_file)
                .await
                .context("Failed to remove certificate metadata")?;
        }

        self.remove_private_key_from_keychain(alias)
            .await
            .context("Failed to remove private key from keychain")?;

        info!(
            "Successfully removed certificate and private key for alias: {}",
            alias
        );
        Ok(())
    }

    fn certificate_to_pem(&self, cert: &[CertificateDer]) -> Result<String> {
        let mut pem_data = String::new();
        for der in cert {
            let encoded = base64::engine::general_purpose::STANDARD.encode(der.as_ref());
            pem_data.push_str("-----BEGIN CERTIFICATE-----\n");
            for chunk in encoded.as_bytes().chunks(64) {
                pem_data.push_str(&String::from_utf8_lossy(chunk));
                pem_data.push('\n');
            }
            pem_data.push_str("-----END CERTIFICATE-----\n");
        }
        Ok(pem_data)
    }

    fn private_key_to_pem(&self, key: &PrivateKeyDer) -> Result<String> {
        let key_bytes = match key {
            PrivateKeyDer::Pkcs8(key) => key.secret_pkcs8_der(),
            _ => return Err(anyhow::anyhow!("Unsupported private key format")),
        };

        let encoded = base64::engine::general_purpose::STANDARD.encode(key_bytes);
        let mut pem_data = String::new();
        pem_data.push_str("-----BEGIN PRIVATE KEY-----\n");
        for chunk in encoded.as_bytes().chunks(64) {
            pem_data.push_str(&String::from_utf8_lossy(chunk));
            pem_data.push('\n');
        }
        pem_data.push_str("-----END PRIVATE KEY-----\n");
        Ok(pem_data)
    }

    fn pem_to_certificate(&self, pem: &str) -> Result<Vec<CertificateDer<'static>>> {
        let mut certificates = Vec::new();
        let mut lines = pem.lines();

        while let Some(line) = lines.next() {
            if line == "-----BEGIN CERTIFICATE-----" {
                let mut cert_data = String::new();
                for line in lines.by_ref() {
                    if line == "-----END CERTIFICATE-----" {
                        break;
                    }
                    cert_data.push_str(line);
                }

                let der_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&cert_data)
                    .context("Failed to decode certificate base64")?;
                certificates.push(CertificateDer::from(der_bytes));
            }
        }

        if certificates.is_empty() {
            return Err(anyhow::anyhow!("No certificates found in PEM data"));
        }

        Ok(certificates)
    }

    fn pem_to_private_key(&self, pem: &str) -> Result<PrivateKeyDer<'static>> {
        let mut lines = pem.lines();

        while let Some(line) = lines.next() {
            if line == "-----BEGIN PRIVATE KEY-----" {
                let mut key_data = String::new();
                for line in lines.by_ref() {
                    if line == "-----END PRIVATE KEY-----" {
                        break;
                    }
                    key_data.push_str(line);
                }

                let der_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&key_data)
                    .context("Failed to decode private key base64")?;
                return Ok(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der_bytes)));
            }
        }

        Err(anyhow::anyhow!("No private key found in PEM data"))
    }

    async fn store_private_key_in_keychain(
        &self, alias: &str, private_key_pem: &str,
    ) -> Result<()> {
        debug!("Storing private key in SSL vault for alias: {}", alias);

        let mut vault = self.load_ssl_vault().await?.unwrap_or_default();
        vault
            .certificate_keys
            .insert(alias.to_string(), private_key_pem.to_string());

        #[cfg(test)]
        {
            debug!(
                "Test mode: vault now has {} keys",
                vault.certificate_keys.len()
            );
            debug!(
                "Test mode: vault contains key for alias {}: {}",
                alias,
                vault.certificate_keys.contains_key(alias)
            );
        }

        self.save_ssl_vault(&vault).await
    }

    async fn retrieve_private_key_from_keychain(&self, alias: &str) -> Result<String> {
        debug!("Retrieving private key from SSL vault for alias: {}", alias);

        let vault = self
            .load_ssl_vault()
            .await?
            .ok_or_else(|| anyhow::anyhow!("SSL vault not found"))?;

        vault
            .certificate_keys
            .get(alias)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Private key not found for alias: {}", alias))
    }

    async fn private_key_exists_in_keychain(&self, alias: &str) -> bool {
        if let Ok(Some(vault)) = self.load_ssl_vault().await {
            let exists = vault.certificate_keys.contains_key(alias);
            #[cfg(test)]
            {
                debug!(
                    "Test mode: checking key for alias {}: exists = {}, vault has {} keys",
                    alias,
                    exists,
                    vault.certificate_keys.len()
                );
            }
            exists
        } else {
            #[cfg(test)]
            {
                debug!(
                    "Test mode: no vault found when checking key for alias {}",
                    alias
                );
            }
            false
        }
    }

    pub async fn remove_private_key_from_keychain(&self, alias: &str) -> Result<()> {
        debug!("Removing private key from SSL vault for alias: {}", alias);

        let mut vault = self.load_ssl_vault().await?.unwrap_or_default();
        vault.certificate_keys.remove(alias);
        self.save_ssl_vault(&vault).await
    }

    pub async fn list_all_certificates(&self) -> Result<Vec<String>> {
        let mut certificates = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.store_path)
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
                if self.private_key_exists_in_keychain(alias).await {
                    certificates.push(alias.to_string());
                } else {
                    warn!(
                        "Found certificate file without corresponding keychain entry: {}",
                        alias
                    );
                }
            }
        }

        Ok(certificates)
    }

    async fn load_ssl_vault(&self) -> Result<Option<SslKeyVault>> {
        if std::env::var("KFTRAY_TEST_MODE").is_ok() {
            let vault = TEST_SSL_VAULT.lock().unwrap().clone();
            #[cfg(test)]
            {
                debug!(
                    "Test mode: loading vault with {} keys, {} CA key",
                    vault.certificate_keys.len(),
                    if vault.ca_private_key.is_some() {
                        "has"
                    } else {
                        "no"
                    }
                );
            }
            return Ok(Some(vault));
        }

        let entry = Entry::new(KFTRAY_SERVICE, KFTRAY_SSL_VAULT)
            .context("Failed to create SSL vault keychain entry")?;

        match entry.get_password() {
            Ok(vault_json) => {
                let vault: SslKeyVault =
                    serde_json::from_str(&vault_json).context("Failed to deserialize SSL vault")?;
                Ok(Some(vault))
            }
            Err(_) => Ok(None),
        }
    }

    async fn save_ssl_vault(&self, vault: &SslKeyVault) -> Result<()> {
        if std::env::var("KFTRAY_TEST_MODE").is_ok() {
            *TEST_SSL_VAULT.lock().unwrap() = vault.clone();
            return Ok(());
        }

        let entry = Entry::new(KFTRAY_SERVICE, KFTRAY_SSL_VAULT)
            .context("Failed to create SSL vault keychain entry")?;

        let vault_json = serde_json::to_string(vault).context("Failed to serialize SSL vault")?;

        entry
            .set_password(&vault_json)
            .context("Failed to store SSL vault in keychain")?;

        Ok(())
    }

    pub async fn cleanup_ssl_vault(&self) -> Result<()> {
        info!("Cleaning up SSL vault from keychain");

        if std::env::var("KFTRAY_TEST_MODE").is_ok() {
            info!("Test mode enabled, clearing test vault");
            *TEST_SSL_VAULT.lock().unwrap() = SslKeyVault::default();
            return Ok(());
        }

        let entry = Entry::new(KFTRAY_SERVICE, KFTRAY_SSL_VAULT)
            .context("Failed to create SSL vault keychain entry")?;

        match entry.delete_credential() {
            Ok(_) => info!("Successfully removed SSL vault from keychain"),
            Err(_) => debug!("SSL vault not found in keychain (already removed)"),
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::ssl::cert_generator::CertificateGenerator;

    async fn create_test_store() -> (CertificateStore, TempDir) {
        unsafe {
            std::env::set_var("KFTRAY_TEST_MODE", "1");
        }

        let temp_dir = TempDir::new().unwrap();
        let store = CertificateStore::with_path(temp_dir.path().to_path_buf()).unwrap();
        (store, temp_dir)
    }

    #[tokio::test]
    async fn test_store_and_load_certificate() {
        *TEST_SSL_VAULT.lock().unwrap() = Default::default();

        unsafe {
            std::env::set_var("KFTRAY_SKIP_CA_INSTALL", "1");
            std::env::set_var("KFTRAY_TEST_MODE", "1");
        }
        let (store, temp_dir) = create_test_store().await;
        let generator = CertificateGenerator::for_testing(temp_dir.path());
        let cert_pair = generator.generate_for_alias("test-service").await.unwrap();

        store.store("test-service", &cert_pair).await.unwrap();

        let cert_file = store.store_path.join("test-service.crt");
        let key_exists = store.private_key_exists_in_keychain("test-service").await;
        println!(
            "Debug - cert_file.exists(): {}, key_exists: {}",
            cert_file.exists(),
            key_exists
        );

        assert!(store.exists("test-service").await);

        let loaded_cert = store.load("test-service").await.unwrap();
        assert_eq!(loaded_cert.domain, "test-service.local");
        assert!(!loaded_cert.certificate.is_empty());
    }

    #[tokio::test]
    async fn test_certificate_not_exists() {
        *TEST_SSL_VAULT.lock().unwrap() = Default::default();

        let (store, _temp_dir) = create_test_store().await;
        assert!(!store.exists("non-existent").await);
    }

    #[tokio::test]
    async fn test_remove_certificate() {
        *TEST_SSL_VAULT.lock().unwrap() = Default::default();

        unsafe {
            std::env::set_var("KFTRAY_SKIP_CA_INSTALL", "1");
            std::env::set_var("KFTRAY_TEST_MODE", "1");
        }
        let (store, temp_dir) = create_test_store().await;
        let generator = CertificateGenerator::for_testing(temp_dir.path());
        let cert_pair = generator.generate_for_alias("test-service").await.unwrap();

        store.store("test-service", &cert_pair).await.unwrap();
        assert!(store.exists("test-service").await);

        store.remove("test-service").await.unwrap();
        assert!(!store.exists("test-service").await);
    }
}
