use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use keyring::Entry;
use log::{
    info,
    warn,
};
use pem;
use rcgen::{
    BasicConstraints,
    Certificate,
    CertificateParams,
    DnType,
    IsCa,
    Issuer,
    KeyPair,
};
use rustls::pki_types::{
    CertificateDer,
    PrivateKeyDer,
    PrivatePkcs8KeyDer,
};
use time::{
    Duration,
    OffsetDateTime,
};
use tokio::fs;

use super::cert_store::{
    SslKeyVault,
    TEST_SSL_VAULT,
};

const KFTRAY_SERVICE: &str = "kftray-ssl";
const KFTRAY_SSL_VAULT: &str = "ssl-keys-vault";

#[derive(Debug)]
pub struct CertificatePair {
    pub certificate: Vec<CertificateDer<'static>>,
    pub private_key: PrivateKeyDer<'static>,
    pub domain: String,
    pub local_domain: String,
    pub subject_alt_names: Vec<String>,
}

pub struct CertificateGenerator {
    ca_cert_path: PathBuf,
}

impl CertificateGenerator {
    pub fn new() -> Result<Self> {
        let cert_dir = if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
            PathBuf::from(config_dir).join("ssl-ca")
        } else {
            dirs::config_dir()
                .context("No config directory found")?
                .join("kftray")
                .join("ssl-ca")
        };

        let ca_cert_path = cert_dir.join("kftray-ca.crt");

        Ok(Self { ca_cert_path })
    }

    pub fn with_paths(ca_cert_path: PathBuf, _ca_key_path: PathBuf) -> Self {
        Self { ca_cert_path }
    }

    pub fn for_testing(temp_dir: &std::path::Path) -> Self {
        let ca_cert_path = temp_dir.join("test-ca.crt");
        Self { ca_cert_path }
    }

    async fn get_or_create_ca(&self) -> Result<(Certificate, KeyPair)> {
        if self.ca_cert_path.exists() && self.ca_key_exists_in_vault().await {
            match self.load_existing_ca().await {
                Ok(ca) => {
                    info!(
                        "Using existing kftray CA certificate from: {}",
                        self.ca_cert_path.display()
                    );

                    info!("Existing CA certificate loaded");

                    return Ok(ca);
                }
                Err(e) => {
                    warn!(
                        "Failed to load existing CA certificate, creating new one: {}",
                        e
                    );
                }
            }
        }

        info!("Creating new kftray CA certificate");
        let ca = self.create_new_ca().await?;
        self.save_ca(&ca).await?;

        info!("New CA certificate created and saved");

        Ok(ca)
    }

    fn build_ca_params() -> Result<CertificateParams> {
        let subject_alt_names = vec!["kftray-ca".to_string()];
        let mut ca_params =
            CertificateParams::new(subject_alt_names).context("Failed to create CA params")?;

        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "kftray Local CA");
        ca_params
            .distinguished_name
            .push(DnType::OrganizationName, "kftray");
        ca_params
            .distinguished_name
            .push(DnType::OrganizationalUnitName, "SSL Certificate Authority");

        let now = OffsetDateTime::now_utc();
        ca_params.not_before = now - Duration::minutes(1);
        ca_params.not_after = now + Duration::days(365 * 10);

        Ok(ca_params)
    }

    async fn create_new_ca(&self) -> Result<(Certificate, KeyPair)> {
        let ca_params = Self::build_ca_params()?;
        let ca_key_pair = KeyPair::generate()?;

        let ca_cert = ca_params
            .self_signed(&ca_key_pair)
            .context("Failed to create self-signed CA certificate")?;

        info!("Created new kftray CA certificate (valid for 10 years)");
        Ok((ca_cert, ca_key_pair))
    }

    async fn load_existing_ca(&self) -> Result<(Certificate, KeyPair)> {
        let _cert_pem = fs::read_to_string(&self.ca_cert_path)
            .await
            .context("Failed to read CA certificate PEM file")?;
        let key_pem = self
            .retrieve_ca_private_key_from_vault()
            .await
            .context("Failed to retrieve CA private key from keychain")?;

        let ca_key_pair =
            KeyPair::from_pem(&key_pem).context("Failed to create key pair from PEM")?;

        let ca_params = Self::build_ca_params()?;
        let ca_cert = ca_params
            .self_signed(&ca_key_pair)
            .context("Failed to recreate CA certificate from existing key")?;

        info!("Successfully loaded existing CA key from keychain and recreated certificate");
        Ok((ca_cert, ca_key_pair))
    }

    async fn save_ca(&self, ca: &(Certificate, KeyPair)) -> Result<()> {
        if let Some(parent) = self.ca_cert_path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create CA directory")?;
        }

        let cert_pem = ca.0.pem();
        fs::write(&self.ca_cert_path, cert_pem)
            .await
            .context("Failed to write CA certificate PEM file")?;

        let key_pem = ca.1.serialize_pem();
        self.store_ca_private_key_in_vault(&key_pem)
            .await
            .context("Failed to store CA private key in keychain")?;

        info!(
            "CA certificate saved to: {} and private key stored securely in keychain",
            self.ca_cert_path.display()
        );
        Ok(())
    }

    pub async fn generate_for_alias(&self, alias: &str) -> Result<CertificatePair> {
        self.generate_for_alias_with_validity(alias, 365).await
    }

    pub async fn generate_for_alias_with_validity(
        &self, alias: &str, validity_days: u16,
    ) -> Result<CertificatePair> {
        super::ensure_crypto_provider_installed();

        let (ca_cert, ca_key_pair) = self.get_or_create_ca().await?;

        let (domain, local_domain) = if alias.contains('.') {
            if alias.ends_with(".local") {
                (alias.to_string(), alias.to_string())
            } else {
                (alias.to_string(), format!("{}.local", alias))
            }
        } else {
            let local_domain = format!("{}.local", alias);
            (local_domain.clone(), local_domain)
        };

        let mut san_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        let mut seen_domains = HashSet::new();

        if seen_domains.insert(domain.clone()) {
            san_names.push(domain.clone());
        }
        if domain != local_domain && seen_domains.insert(local_domain.clone()) {
            san_names.push(local_domain.clone());
        }

        let mut cert_params = CertificateParams::new(vec![
            domain.clone(),
            local_domain.clone(),
            "localhost".to_string(),
        ])
        .context("Failed to create certificate params")?;

        cert_params
            .subject_alt_names
            .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
                std::net::Ipv4Addr::new(127, 0, 0, 1),
            )));

        cert_params
            .distinguished_name
            .push(DnType::CommonName, &domain);
        cert_params
            .distinguished_name
            .push(DnType::OrganizationName, "kftray");
        cert_params
            .distinguished_name
            .push(DnType::LocalityName, "Local Development");

        let now = OffsetDateTime::now_utc();
        cert_params.not_before = now - Duration::minutes(1);
        cert_params.not_after = now + Duration::days(validity_days as i64);

        let key_pair = KeyPair::generate()?;

        let ca_params = Self::build_ca_params()?;
        let ca_issuer = Issuer::from_params(&ca_params, &ca_key_pair);
        let cert = cert_params
            .signed_by(&key_pair, &ca_issuer)
            .context("Failed to sign certificate")?;

        let cert_der = cert.der();
        let ca_cert_der = ca_cert.der();

        let private_key = PrivatePkcs8KeyDer::from(key_pair.serialize_der())
            .clone_key()
            .into();

        info!(
            "Generated SSL certificate for {} (valid for {} days)",
            alias, validity_days
        );

        Ok(CertificatePair {
            certificate: vec![cert_der.clone(), ca_cert_der.clone()],
            private_key,
            domain,
            local_domain,
            subject_alt_names: san_names,
        })
    }

    pub async fn generate_for_domains_with_validity(
        &self, domains: Vec<String>, validity_days: u16,
    ) -> Result<CertificatePair> {
        if domains.is_empty() {
            return Err(anyhow::anyhow!("At least one domain must be provided"));
        }

        super::ensure_crypto_provider_installed();

        let (ca_cert, ca_key_pair) = self.get_or_create_ca().await?;

        let primary_domain = domains.first().unwrap().clone();
        let local_domain = if primary_domain.ends_with(".local") {
            primary_domain.clone()
        } else {
            format!("{}.local", primary_domain)
        };

        let mut san_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        let mut seen_domains = std::collections::HashSet::new();

        let mut cert_params = CertificateParams::new(vec!["localhost".to_string()])
            .context("Failed to create certificate params")?;

        cert_params
            .subject_alt_names
            .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
                std::net::Ipv4Addr::new(127, 0, 0, 1),
            )));

        for domain in &domains {
            if seen_domains.insert(domain.clone()) {
                cert_params.subject_alt_names.push(rcgen::SanType::DnsName(
                    domain.as_str().try_into().expect("Invalid domain"),
                ));
                san_names.push(domain.clone());

                if !domain.ends_with(".local") {
                    let local_domain = format!("{}.local", domain);
                    if seen_domains.insert(local_domain.clone()) {
                        cert_params.subject_alt_names.push(rcgen::SanType::DnsName(
                            local_domain.as_str().try_into().expect("Invalid domain"),
                        ));
                        san_names.push(local_domain);
                    }
                }
            }
        }

        cert_params
            .distinguished_name
            .push(DnType::CommonName, &primary_domain);
        cert_params
            .distinguished_name
            .push(DnType::OrganizationName, "kftray");
        cert_params
            .distinguished_name
            .push(DnType::LocalityName, "Local Development");

        let now = OffsetDateTime::now_utc();
        cert_params.not_before = now - Duration::minutes(1);
        cert_params.not_after = now + Duration::days(validity_days as i64);

        let key_pair = KeyPair::generate()?;

        let ca_params = Self::build_ca_params()?;
        let ca_issuer = Issuer::from_params(&ca_params, &ca_key_pair);
        let cert = cert_params
            .signed_by(&key_pair, &ca_issuer)
            .context("Failed to sign certificate")?;

        let cert_der = cert.der();
        let ca_cert_der = ca_cert.der();

        let private_key = PrivatePkcs8KeyDer::from(key_pair.serialize_der())
            .clone_key()
            .into();

        info!(
            "Generated SSL certificate for domains: {:?} (valid for {} days)",
            domains, validity_days
        );

        Ok(CertificatePair {
            certificate: vec![cert_der.clone(), ca_cert_der.clone()],
            private_key,
            domain: primary_domain,
            local_domain,
            subject_alt_names: san_names,
        })
    }

    pub async fn generate_wildcard_certificate(
        &self, base_domain: &str, validity_days: u16,
    ) -> Result<CertificatePair> {
        super::ensure_crypto_provider_installed();

        let (ca_cert, ca_key_pair) = self.get_or_create_ca().await?;

        let wildcard_domain = format!("*.{}", base_domain);
        let (local_domain, wildcard_local) = if base_domain.ends_with(".local") {
            (base_domain.to_string(), format!("*.{}", base_domain))
        } else {
            (
                format!("{}.local", base_domain),
                format!("*.{}.local", base_domain),
            )
        };

        let mut san_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        let mut seen_domains = HashSet::new();

        for domain in [
            &wildcard_domain,
            &base_domain.to_string(),
            &wildcard_local,
            &local_domain,
        ] {
            if seen_domains.insert(domain.clone()) {
                san_names.push(domain.clone());
            }
        }

        let mut cert_params = CertificateParams::new(vec![
            wildcard_domain.clone(),
            base_domain.to_string(),
            wildcard_local.clone(),
            local_domain.clone(),
            "localhost".to_string(),
        ])
        .context("Failed to create certificate params")?;

        cert_params
            .subject_alt_names
            .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
                std::net::Ipv4Addr::new(127, 0, 0, 1),
            )));

        cert_params
            .distinguished_name
            .push(DnType::CommonName, &wildcard_domain);
        cert_params
            .distinguished_name
            .push(DnType::OrganizationName, "kftray");
        cert_params
            .distinguished_name
            .push(DnType::LocalityName, "Local Development");

        let now = OffsetDateTime::now_utc();
        cert_params.not_before = now - Duration::minutes(1);
        cert_params.not_after = now + Duration::days(validity_days as i64);

        let key_pair = KeyPair::generate()?;

        let ca_params = Self::build_ca_params()?;
        let ca_issuer = Issuer::from_params(&ca_params, &ca_key_pair);
        let cert = cert_params
            .signed_by(&key_pair, &ca_issuer)
            .context("Failed to sign certificate")?;

        let cert_der = cert.der();
        let ca_cert_der = ca_cert.der();

        let private_key = PrivatePkcs8KeyDer::from(key_pair.serialize_der())
            .clone_key()
            .into();

        info!(
            "Generated wildcard SSL certificate for *.{} (valid for {} days)",
            base_domain, validity_days
        );

        Ok(CertificatePair {
            certificate: vec![cert_der.clone(), ca_cert_der.clone()],
            private_key,
            domain: wildcard_domain,
            local_domain,
            subject_alt_names: san_names,
        })
    }

    pub async fn get_ca_info(&self) -> Result<Option<CaInfo>> {
        if !self.ca_cert_path.exists() || !self.ca_key_exists_in_vault().await {
            return Ok(None);
        }

        let cert_pem = fs::read_to_string(&self.ca_cert_path)
            .await
            .context("Failed to read CA certificate")?;

        let parsed = pem::parse(&cert_pem).context("Failed to parse CA certificate PEM")?;

        let installed = false;

        Ok(Some(CaInfo {
            path: self.ca_cert_path.clone(),
            fingerprint: format!(
                "{:02x}",
                parsed
                    .contents()
                    .iter()
                    .fold(0u32, |acc, &b| acc.wrapping_add(b as u32))
            ),
            installed,
        }))
    }

    async fn store_ca_private_key_in_vault(&self, private_key_pem: &str) -> Result<()> {
        let mut vault = self.load_ssl_vault().await?.unwrap_or_default();
        vault.ca_private_key = Some(private_key_pem.to_string());
        self.save_ssl_vault(&vault).await
    }

    async fn retrieve_ca_private_key_from_vault(&self) -> Result<String> {
        let vault = self
            .load_ssl_vault()
            .await?
            .ok_or_else(|| anyhow::anyhow!("SSL vault not found"))?;

        vault
            .ca_private_key
            .ok_or_else(|| anyhow::anyhow!("CA private key not found in SSL vault"))
    }

    async fn ca_key_exists_in_vault(&self) -> bool {
        if let Ok(Some(vault)) = self.load_ssl_vault().await {
            vault.ca_private_key.is_some()
        } else {
            false
        }
    }

    pub async fn cleanup_ca_from_vault(&self) -> Result<()> {
        let mut vault = self.load_ssl_vault().await?.unwrap_or_default();
        vault.ca_private_key = None;
        self.save_ssl_vault(&vault).await
    }

    async fn load_ssl_vault(&self) -> Result<Option<SslKeyVault>> {
        if std::env::var("KFTRAY_TEST_MODE").is_ok() {
            let vault = TEST_SSL_VAULT.lock().unwrap().clone();
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
}

#[derive(Debug)]
pub struct CaInfo {
    pub path: PathBuf,
    pub fingerprint: String,
    pub installed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_generate_certificate_for_alias() {
        unsafe {
            std::env::set_var("KFTRAY_TEST_MODE", "1");
            std::env::set_var("KFTRAY_SKIP_CA_INSTALL", "1");
        }
        let temp_dir = tempfile::tempdir().unwrap();
        let generator = CertificateGenerator::for_testing(temp_dir.path());

        let cert_pair = generator.generate_for_alias("test-service").await.unwrap();

        assert!(!cert_pair.certificate.is_empty());
        assert_eq!(cert_pair.domain, "test-service.local");
    }

    #[tokio::test]
    async fn test_generate_certificate_with_custom_validity() {
        unsafe {
            std::env::set_var("KFTRAY_TEST_MODE", "1");
            std::env::set_var("KFTRAY_SKIP_CA_INSTALL", "1");
        }
        let temp_dir = tempfile::tempdir().unwrap();
        let generator = CertificateGenerator::for_testing(temp_dir.path());

        let cert_pair = generator
            .generate_for_alias_with_validity("my-api", 90)
            .await
            .unwrap();

        assert!(!cert_pair.certificate.is_empty());
        assert_eq!(cert_pair.domain, "my-api.local");
    }
}
