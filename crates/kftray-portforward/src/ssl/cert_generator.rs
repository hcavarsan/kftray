use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
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
    ca_key_path: PathBuf,
}

impl CertificateGenerator {
    pub fn new() -> Result<Self> {
        let cert_dir = dirs::config_dir()
            .context("No config directory found")?
            .join("kftray")
            .join("ssl-ca");

        let ca_cert_path = cert_dir.join("kftray-ca.crt");
        let ca_key_path = cert_dir.join("kftray-ca.key");

        Ok(Self {
            ca_cert_path,
            ca_key_path,
        })
    }

    pub fn with_paths(ca_cert_path: PathBuf, ca_key_path: PathBuf) -> Self {
        Self {
            ca_cert_path,
            ca_key_path,
        }
    }

    pub fn for_testing(temp_dir: &std::path::Path) -> Self {
        let ca_cert_path = temp_dir.join("test-ca.crt");
        let ca_key_path = temp_dir.join("test-ca.key");
        Self {
            ca_cert_path,
            ca_key_path,
        }
    }

    async fn get_or_create_ca(&self) -> Result<(Certificate, KeyPair)> {
        if self.ca_cert_path.exists() && self.ca_key_path.exists() {
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
        let key_pem = fs::read_to_string(&self.ca_key_path)
            .await
            .context("Failed to read CA private key PEM file")?;

        let ca_key_pair =
            KeyPair::from_pem(&key_pem).context("Failed to create key pair from PEM")?;

        let ca_params = Self::build_ca_params()?;

        let ca_cert = ca_params
            .self_signed(&ca_key_pair)
            .context("Failed to recreate CA certificate from existing key")?;

        info!("Successfully recreated CA certificate from existing key");
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
        fs::write(&self.ca_key_path, key_pem)
            .await
            .context("Failed to write CA private key PEM file")?;

        info!(
            "CA certificate and key saved to: {}",
            self.ca_cert_path.parent().unwrap().display()
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
            (alias.to_string(), format!("{}.local", alias))
        } else {
            (format!("{}.local", alias), format!("{}.local", alias))
        };

        let san_names = vec![
            domain.clone(),
            local_domain.clone(),
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ];

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
        super::ensure_crypto_provider_installed();

        let (ca_cert, ca_key_pair) = self.get_or_create_ca().await?;

        let primary_domain = domains.first().unwrap().clone();
        let local_domain = format!("{}.local", primary_domain);

        let mut san_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];

        let mut cert_params = CertificateParams::new(vec!["localhost".to_string()])
            .context("Failed to create certificate params")?;

        cert_params
            .subject_alt_names
            .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
                std::net::Ipv4Addr::new(127, 0, 0, 1),
            )));

        for domain in &domains {
            cert_params.subject_alt_names.push(rcgen::SanType::DnsName(
                domain.as_str().try_into().expect("Invalid domain"),
            ));
            cert_params.subject_alt_names.push(rcgen::SanType::DnsName(
                format!("{}.local", domain)
                    .as_str()
                    .try_into()
                    .expect("Invalid domain"),
            ));
            san_names.push(domain.clone());
            san_names.push(format!("{}.local", domain));
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
        let local_domain = format!("{}.local", base_domain);
        let wildcard_local = format!("*.{}.local", base_domain);

        let san_names = vec![
            wildcard_domain.clone(),
            base_domain.to_string(),
            wildcard_local.clone(),
            local_domain.clone(),
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ];

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
        if !self.ca_cert_path.exists() {
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
        let temp_dir = tempfile::tempdir().unwrap();
        let generator = CertificateGenerator::for_testing(temp_dir.path());

        let cert_pair = generator.generate_for_alias("test-service").await.unwrap();

        assert!(!cert_pair.certificate.is_empty());
        assert_eq!(cert_pair.domain, "test-service.local");
    }

    #[tokio::test]
    async fn test_generate_certificate_with_custom_validity() {
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
