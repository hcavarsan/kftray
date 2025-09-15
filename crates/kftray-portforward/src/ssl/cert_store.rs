use std::fs;
use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use base64::Engine;
use rustls::pki_types::{
    CertificateDer,
    PrivateKeyDer,
    PrivatePkcs8KeyDer,
};
use tokio::fs as async_fs;

use super::cert_generator::CertificatePair;

pub struct CertificateStore {
    store_path: PathBuf,
}

impl CertificateStore {
    pub fn new() -> Result<Self> {
        let store_path = dirs::config_dir()
            .context("No config directory found")?
            .join("kftray")
            .join("ssl-certs");

        fs::create_dir_all(&store_path).context("Failed to create SSL certificate directory")?;

        Ok(Self { store_path })
    }

    pub fn with_path(store_path: PathBuf) -> Result<Self> {
        fs::create_dir_all(&store_path).context("Failed to create SSL certificate directory")?;
        Ok(Self { store_path })
    }

    pub async fn store(&self, alias: &str, cert_pair: &CertificatePair) -> Result<()> {
        let cert_file = self.store_path.join(format!("{}.crt", alias));
        let key_file = self.store_path.join(format!("{}.key", alias));
        let metadata_file = self.store_path.join(format!("{}.json", alias));

        let cert_pem = self.certificate_to_pem(&cert_pair.certificate)?;
        let key_pem = self.private_key_to_pem(&cert_pair.private_key)?;

        async_fs::write(cert_file, cert_pem)
            .await
            .context("Failed to write certificate file")?;

        async_fs::write(key_file, key_pem)
            .await
            .context("Failed to write private key file")?;

        let metadata = serde_json::json!({
            "domain": cert_pair.domain,
            "local_domain": cert_pair.local_domain,
            "subject_alt_names": cert_pair.subject_alt_names,
        });

        async_fs::write(metadata_file, serde_json::to_string_pretty(&metadata)?)
            .await
            .context("Failed to write certificate metadata")?;

        Ok(())
    }

    pub async fn load(&self, alias: &str) -> Result<CertificatePair> {
        let cert_file = self.store_path.join(format!("{}.crt", alias));
        let key_file = self.store_path.join(format!("{}.key", alias));
        let metadata_file = self.store_path.join(format!("{}.json", alias));

        let cert_pem = async_fs::read_to_string(cert_file)
            .await
            .context("Failed to read certificate file")?;
        let key_pem = async_fs::read_to_string(key_file)
            .await
            .context("Failed to read private key file")?;

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
        let key_file = self.store_path.join(format!("{}.key", alias));

        cert_file.exists() && key_file.exists()
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
        let cert_file = self.store_path.join(format!("{}.crt", alias));
        let key_file = self.store_path.join(format!("{}.key", alias));
        let metadata_file = self.store_path.join(format!("{}.json", alias));

        if cert_file.exists() {
            async_fs::remove_file(cert_file)
                .await
                .context("Failed to remove certificate file")?;
        }

        if key_file.exists() {
            async_fs::remove_file(key_file)
                .await
                .context("Failed to remove private key file")?;
        }

        if metadata_file.exists() {
            async_fs::remove_file(metadata_file)
                .await
                .context("Failed to remove certificate metadata")?;
        }

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
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::ssl::cert_generator::CertificateGenerator;

    async fn create_test_store() -> (CertificateStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = CertificateStore {
            store_path: temp_dir.path().to_path_buf(),
        };
        (store, temp_dir)
    }

    #[tokio::test]
    async fn test_store_and_load_certificate() {
        unsafe {
            std::env::set_var("KFTRAY_SKIP_CA_INSTALL", "1");
        }
        let (store, temp_dir) = create_test_store().await;
        let generator = CertificateGenerator::for_testing(temp_dir.path());
        let cert_pair = generator.generate_for_alias("test-service").await.unwrap();

        store.store("test-service", &cert_pair).await.unwrap();
        assert!(store.exists("test-service").await);

        let loaded_cert = store.load("test-service").await.unwrap();
        assert_eq!(loaded_cert.domain, "test-service.local");
        assert!(!loaded_cert.certificate.is_empty());
    }

    #[tokio::test]
    async fn test_certificate_not_exists() {
        let (store, _temp_dir) = create_test_store().await;
        assert!(!store.exists("non-existent").await);
    }

    #[tokio::test]
    async fn test_remove_certificate() {
        unsafe {
            std::env::set_var("KFTRAY_SKIP_CA_INSTALL", "1");
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
