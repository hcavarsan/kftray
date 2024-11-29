use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use dirs::home_dir;
use kftray_commons::models::config_model::Config;
use log::info;
use openssh_keys::{
    Data,
    PublicKey,
};
use russh_keys::key::KeyPair;

use crate::error::Error;

#[derive(Debug, Clone)]
pub struct TunnelConfig {
    pub local_port: u16,
    pub remote_port: u16,
    pub ssh_key_path: PathBuf,
    pub buffer_size: usize,
    pub retry_delay: Duration,
    pub max_retries: u32,
    pub keepalive_interval: Duration,
    pub pod_ready_timeout: Duration,
}

impl TunnelConfig {
    pub fn from_common_config(config: &Config) -> Result<Self, Error> {
        Ok(Self {
            local_port: config.local_port.unwrap_or(0),
            remote_port: config.remote_port.unwrap_or(0),
            ssh_key_path: Self::ensure_ssh_keys()?,
            buffer_size: 16384,
            retry_delay: Duration::from_secs(3),
            max_retries: 3,
            keepalive_interval: Duration::from_secs(30),
            pod_ready_timeout: Duration::from_secs(60),
        })
    }

    fn generate_keypair() -> Result<KeyPair, Error> {
        info!("Generating new Ed25519 key pair");
        Ok(KeyPair::generate_ed25519())
    }

    fn write_keypair(key_pair: &KeyPair, key_path: &PathBuf) -> Result<(), Error> {
        let KeyPair::Ed25519(signing_key) = key_pair else {
            return Err(Error::Other(anyhow::anyhow!(
                "Only Ed25519 keys are supported"
            )));
        };

        let verifying_key = signing_key.verifying_key();
        let public_key_bytes = verifying_key.as_bytes();

        let public_key = PublicKey {
            options: None,
            data: Data::Ed25519 {
                key: public_key_bytes.to_vec(),
            },
            comment: Some("kftray-server".to_string()),
        };

        let private_key = Self::format_private_key(signing_key.to_bytes(), public_key_bytes)?;
        fs::write(key_path, private_key)?;
        fs::write(key_path.with_extension("pub"), public_key.to_string())?;

        #[cfg(unix)]
        Self::set_key_permissions(key_path)?;

        Ok(())
    }

    fn format_private_key(
        signing_key: impl AsRef<[u8]>, public_key_bytes: &[u8],
    ) -> Result<String, Error> {
        let mut key_bytes = Vec::new();
        key_bytes.extend_from_slice(b"openssh-key-v1\0");
        key_bytes.extend_from_slice(&[0, 0, 0, 4]);
        key_bytes.extend_from_slice(b"none");
        key_bytes.extend_from_slice(&[0, 0, 0, 4]);
        key_bytes.extend_from_slice(b"none");
        key_bytes.extend_from_slice(&[0, 0, 0, 0]);
        key_bytes.extend_from_slice(&[0, 0, 0, 1]);

        let pub_key_data = public_key_bytes;
        key_bytes.extend_from_slice(&(pub_key_data.len() as u32).to_be_bytes());
        key_bytes.extend_from_slice(pub_key_data);

        let mut priv_key_data = Vec::new();
        let check = rand::random::<u32>().to_be_bytes();
        priv_key_data.extend_from_slice(&check);
        priv_key_data.extend_from_slice(&check);

        let key_id = b"ssh-ed25519";
        priv_key_data.extend_from_slice(&(key_id.len() as u32).to_be_bytes());
        priv_key_data.extend_from_slice(key_id);

        priv_key_data.extend_from_slice(&(public_key_bytes.len() as u32).to_be_bytes());
        priv_key_data.extend_from_slice(public_key_bytes);

        let private_key_bytes = signing_key.as_ref();
        let full_private_key = [private_key_bytes, public_key_bytes].concat();
        priv_key_data.extend_from_slice(&(full_private_key.len() as u32).to_be_bytes());
        priv_key_data.extend_from_slice(&full_private_key);

        let comment = b"kftray-server";
        priv_key_data.extend_from_slice(&(comment.len() as u32).to_be_bytes());
        priv_key_data.extend_from_slice(comment);

        while priv_key_data.len() % 8 != 0 {
            priv_key_data.push(rand::random::<u8>());
        }

        key_bytes.extend_from_slice(&(priv_key_data.len() as u32).to_be_bytes());
        key_bytes.extend_from_slice(&priv_key_data);

        let encoded = base64::engine::general_purpose::STANDARD.encode(&key_bytes);
        let wrapped = encoded
            .chars()
            .collect::<Vec<_>>()
            .chunks(70)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(format!(
            "-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n-----END OPENSSH PRIVATE KEY-----\n",
            wrapped
        ))
    }

    #[cfg(unix)]
    fn set_key_permissions(key_path: &PathBuf) -> Result<(), Error> {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(key_path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(key_path, perms)?;
        Ok(())
    }

    pub fn ensure_ssh_keys() -> Result<PathBuf, Error> {
        let ssh_dir = home_dir()
            .ok_or_else(|| Error::Other(anyhow::anyhow!("Could not find home directory")))?
            .join(".ssh")
            .join("kftray-server");

        fs::create_dir_all(&ssh_dir)?;

        let key_path = ssh_dir.join("id_ed25519");
        if !key_path.exists() {
            info!("Generating new SSH key pair...");
            let key_pair = Self::generate_keypair()?;
            Self::write_keypair(&key_pair, &key_path)?;
            info!("SSH key pair generated at {:?}", key_path);
        }

        Ok(key_path)
    }
}
