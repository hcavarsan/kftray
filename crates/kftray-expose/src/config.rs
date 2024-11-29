use std::time::Duration;
use std::{
    fs,
    path::PathBuf,
};

use base64::Engine;
use dirs::home_dir;
use log::info;
use openssh_keys::{
    Data,
    PublicKey,
};
use russh_keys::key::KeyPair;
use serde::{
    Deserialize,
    Serialize,
};

use crate::error::TunnelResult;
use crate::TunnelError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub local_port: u16,
    pub remote_port: u16,
    pub namespace: String,
    pub ssh_key_path: PathBuf,
    pub buffer_size: usize,
    pub retry_delay: Duration,
    pub max_retries: u32,
    pub keepalive_interval: Duration,
    pub pod_ready_timeout: Duration,
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            local_port: 2222,
            remote_port: 8085,
            namespace: "default".to_string(),
            ssh_key_path: Self::ensure_ssh_keys().expect("Failed to ensure SSH keys"),
            buffer_size: 16384,
            retry_delay: Duration::from_secs(3),
            max_retries: 3,
            keepalive_interval: Duration::from_secs(30),
            pod_ready_timeout: Duration::from_secs(60),
        }
    }
}

impl TunnelConfig {
    pub fn builder() -> TunnelConfigBuilder {
        TunnelConfigBuilder::default()
    }

    fn generate_keypair() -> TunnelResult<KeyPair> {
        info!("Generating new Ed25519 key pair");
        Ok(KeyPair::generate_ed25519())
    }

    fn write_keypair(key_pair: &KeyPair, key_path: &PathBuf) -> TunnelResult<()> {
        let KeyPair::Ed25519(signing_key) = key_pair else {
            return Err(anyhow::anyhow!("Only Ed25519 keys are supported").into());
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

        // Format and write keys
        let private_key = Self::format_private_key(signing_key.to_bytes(), public_key_bytes)?;
        fs::write(key_path, private_key)?;
        fs::write(key_path.with_extension("pub"), public_key.to_string())?;

        #[cfg(unix)]
        Self::set_key_permissions(key_path)?;

        Ok(())
    }

    fn format_private_key(
        signing_key: impl AsRef<[u8]>, public_key_bytes: &[u8],
    ) -> TunnelResult<String> {
        let mut key_bytes = Vec::new();

        // Magic identifier
        key_bytes.extend_from_slice(b"openssh-key-v1\0");

        // Cipher, KDF, KDF options (none for unencrypted key)
        key_bytes.extend_from_slice(&[0, 0, 0, 4]); // length of "none"
        key_bytes.extend_from_slice(b"none"); // cipher name
        key_bytes.extend_from_slice(&[0, 0, 0, 4]); // length of "none"
        key_bytes.extend_from_slice(b"none"); // kdf name
        key_bytes.extend_from_slice(&[0, 0, 0, 0]); // kdf options (empty)

        // Number of keys (1)
        key_bytes.extend_from_slice(&[0, 0, 0, 1]);

        // Public key
        let pub_key_data = public_key_bytes;
        key_bytes.extend_from_slice(&(pub_key_data.len() as u32).to_be_bytes());
        key_bytes.extend_from_slice(pub_key_data);

        // Private key section
        let mut priv_key_data = Vec::new();
        let check = rand::random::<u32>().to_be_bytes();
        priv_key_data.extend_from_slice(&check); // random check bytes
        priv_key_data.extend_from_slice(&check); // repeated check bytes

        // Key identifier
        let key_id = b"ssh-ed25519";
        priv_key_data.extend_from_slice(&(key_id.len() as u32).to_be_bytes());
        priv_key_data.extend_from_slice(key_id);

        // Public key
        priv_key_data.extend_from_slice(&(public_key_bytes.len() as u32).to_be_bytes());
        priv_key_data.extend_from_slice(public_key_bytes);

        // Private key (includes both secret and public parts)
        let private_key_bytes = signing_key.as_ref();
        let full_private_key = [private_key_bytes, public_key_bytes].concat();
        priv_key_data.extend_from_slice(&(full_private_key.len() as u32).to_be_bytes());
        priv_key_data.extend_from_slice(&full_private_key);

        // Comment
        let comment = b"kftray-server";
        priv_key_data.extend_from_slice(&(comment.len() as u32).to_be_bytes());
        priv_key_data.extend_from_slice(comment);

        // Pad to block size
        while priv_key_data.len() % 8 != 0 {
            priv_key_data.push(rand::random::<u8>());
        }

        key_bytes.extend_from_slice(&(priv_key_data.len() as u32).to_be_bytes());
        key_bytes.extend_from_slice(&priv_key_data);

        // Base64 encode with proper line wrapping
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
    fn set_key_permissions(key_path: &PathBuf) -> TunnelResult<()> {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(key_path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(key_path, perms)?;
        Ok(())
    }

    pub fn ensure_ssh_keys() -> TunnelResult<PathBuf> {
        let ssh_dir = home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?
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

    pub fn from_env() -> TunnelResult<Self> {
        let mut builder = TunnelConfigBuilder::default();

        if let Ok(local_port) = std::env::var("LOCAL_PORT") {
            builder = builder.local_port(local_port.parse().unwrap_or(2222));
        }

        if let Ok(remote_port) = std::env::var("REMOTE_PORT") {
            builder = builder.remote_port(remote_port.parse().unwrap_or(8085));
        }

        if let Ok(namespace) = std::env::var("NAMESPACE") {
            builder = builder.namespace(namespace);
        }

        if let Ok(key_path) = std::env::var("SSH_KEY_PATH") {
            builder = builder.ssh_key_path(PathBuf::from(key_path));
        }

        if let Ok(buffer_size) = std::env::var("BUFFER_SIZE") {
            if let Ok(size) = buffer_size.parse() {
                builder = builder.buffer_size(size);
            }
        }

        if let Ok(retry_delay) = std::env::var("RETRY_DELAY") {
            if let Ok(secs) = retry_delay.parse() {
                builder = builder.retry_delay(Duration::from_secs(secs));
            }
        }

        if let Ok(max_retries) = std::env::var("MAX_RETRIES") {
            if let Ok(retries) = max_retries.parse() {
                builder = builder.max_retries(retries);
            }
        }

        if let Ok(keepalive) = std::env::var("KEEPALIVE_INTERVAL") {
            if let Ok(secs) = keepalive.parse() {
                builder = builder.keepalive_interval(Duration::from_secs(secs));
            }
        }

        if let Ok(timeout) = std::env::var("POD_READY_TIMEOUT") {
            if let Ok(secs) = timeout.parse() {
                builder = builder.pod_ready_timeout(Duration::from_secs(secs));
            }
        }

        builder.build()
    }
}

#[derive(Default)]
pub struct TunnelConfigBuilder {
    config: TunnelConfig,
}

impl TunnelConfigBuilder {
    pub fn local_port(mut self, port: u16) -> Self {
        self.config.local_port = port;
        self
    }

    pub fn remote_port(mut self, port: u16) -> Self {
        self.config.remote_port = port;
        self
    }

    pub fn namespace(mut self, namespace: String) -> Self {
        self.config.namespace = namespace;
        self
    }

    pub fn buffer_size(mut self, size: usize) -> Self {
        self.config.buffer_size = size;
        self
    }

    pub fn retry_delay(mut self, delay: Duration) -> Self {
        self.config.retry_delay = delay;
        self
    }

    pub fn max_retries(mut self, retries: u32) -> Self {
        self.config.max_retries = retries;
        self
    }

    pub fn keepalive_interval(mut self, interval: Duration) -> Self {
        self.config.keepalive_interval = interval;
        self
    }

    pub fn pod_ready_timeout(mut self, timeout: Duration) -> Self {
        self.config.pod_ready_timeout = timeout;
        self
    }

    pub fn ssh_key_path(mut self, path: PathBuf) -> Self {
        self.config.ssh_key_path = path;
        self
    }

    pub fn build(self) -> TunnelResult<TunnelConfig> {
        // Validate configuration
        if self.config.local_port == 0 {
            return Err(TunnelError::Other(anyhow::anyhow!(
                "Local port cannot be 0"
            )));
        }
        if self.config.remote_port == 0 {
            return Err(TunnelError::Other(anyhow::anyhow!(
                "Remote port cannot be 0"
            )));
        }
        if self.config.buffer_size == 0 {
            return Err(TunnelError::Other(anyhow::anyhow!(
                "Buffer size cannot be 0"
            )));
        }
        Ok(self.config)
    }
}

// Example usage in tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_pattern() {
        let config = TunnelConfigBuilder::default()
            .local_port(2222)
            .remote_port(8085)
            .namespace("default".to_string())
            .buffer_size(16384)
            .retry_delay(Duration::from_secs(3))
            .max_retries(3)
            .keepalive_interval(Duration::from_secs(30))
            .pod_ready_timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build config");

        assert_eq!(config.local_port, 2222);
        assert_eq!(config.remote_port, 8085);
        assert_eq!(config.namespace, "default");
        assert_eq!(config.buffer_size, 16384);
    }
}
