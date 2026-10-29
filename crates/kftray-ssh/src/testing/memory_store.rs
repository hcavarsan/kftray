use std::collections::HashMap;

use async_trait::async_trait;
use russh::keys::PublicKey;
use tokio::sync::Mutex;

use crate::error::SshError;
use crate::host_keys::HostKeyStore;

/// [`HostKeyStore`] that lives and dies with the value.
#[derive(Default)]
pub struct MemoryHostKeyStore {
    keys: Mutex<HashMap<String, PublicKey>>,
}

#[async_trait]
impl HostKeyStore for MemoryHostKeyStore {
    async fn get(&self, alias: &str) -> Result<Option<PublicKey>, SshError> {
        Ok(self.keys.lock().await.get(alias).cloned())
    }

    async fn insert_if_absent(&self, alias: &str, key: &PublicKey) -> Result<PublicKey, SshError> {
        Ok(self
            .keys
            .lock()
            .await
            .entry(alias.to_owned())
            .or_insert_with(|| key.clone())
            .clone())
    }

    async fn forget(&self, alias: &str) -> Result<(), SshError> {
        self.keys.lock().await.remove(alias);
        Ok(())
    }
}
