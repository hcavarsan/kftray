use async_trait::async_trait;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::ssh_host_keys::{
    StoredHostKey,
    delete_ssh_host_key_with_mode,
    get_ssh_host_key_with_mode,
    insert_ssh_host_key_if_absent_with_mode,
};
use kftray_ssh::russh::keys::PublicKey;
use kftray_ssh::{
    HostKeyStore,
    SshError,
};

/// First-use host keys kept in the `ssh_host_keys` table.
pub(crate) struct DbHostKeyStore {
    pub(crate) mode: DatabaseMode,
}

fn decode(stored: &StoredHostKey) -> Result<PublicKey, SshError> {
    PublicKey::from_openssh(&format!("{} {}", stored.key_type, stored.key_b64))
        .map_err(|e| SshError::HostKeyStore(format!("stored host key is invalid: {e}")))
}

fn encode(key: &PublicKey) -> Result<(String, String), SshError> {
    let openssh = key
        .to_openssh()
        .map_err(|e| SshError::HostKeyStore(e.to_string()))?;
    let mut parts = openssh.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(key_type), Some(key_b64)) => Ok((key_type.to_owned(), key_b64.to_owned())),
        _ => Err(SshError::HostKeyStore(
            "host key has no OpenSSH encoding".to_owned(),
        )),
    }
}

#[async_trait]
impl HostKeyStore for DbHostKeyStore {
    async fn get(&self, alias: &str) -> Result<Option<PublicKey>, SshError> {
        get_ssh_host_key_with_mode(alias, self.mode)
            .await
            .map_err(SshError::HostKeyStore)?
            .as_ref()
            .map(decode)
            .transpose()
    }

    async fn insert_if_absent(&self, alias: &str, key: &PublicKey) -> Result<PublicKey, SshError> {
        let (key_type, key_b64) = encode(key)?;
        let stored = insert_ssh_host_key_if_absent_with_mode(alias, &key_type, &key_b64, self.mode)
            .await
            .map_err(SshError::HostKeyStore)?;
        decode(&stored)
    }

    async fn forget(&self, alias: &str) -> Result<(), SshError> {
        delete_ssh_host_key_with_mode(alias, self.mode)
            .await
            .map_err(SshError::HostKeyStore)
    }
}
