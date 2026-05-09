#![cfg(target_os = "linux")]

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use keyring_core::api::{
    CredentialApi,
    CredentialStoreApi,
};
use keyring_core::{
    Credential,
    CredentialPersistence,
    CredentialStore,
    Entry,
    Error,
    Result,
};

pub struct LinuxCompositeStore {
    primary: Arc<CredentialStore>,
    fallback: Option<Arc<CredentialStore>>,
}

impl LinuxCompositeStore {
    pub fn new() -> Result<Arc<CredentialStore>> {
        let primary = dbus_secret_service_keyring_store::Store::new()
            .map_err(|e| Error::PlatformFailure(Box::new(e)))?;
        let fallback = match linux_keyutils_keyring_store::Store::new() {
            Ok(store) => Some(store),
            Err(e) => {
                log::warn!(
                    "keyutils fallback unavailable: {e}; legacy keyutils credentials will not be migrated"
                );
                None
            }
        };
        Ok(Arc::new(Self { primary, fallback }))
    }
}

impl CredentialStoreApi for LinuxCompositeStore {
    fn vendor(&self) -> String {
        "kftray-linux-composite (dbus-secret-service primary, keyutils fallback)".into()
    }

    fn id(&self) -> String {
        "kftray-linux-composite".into()
    }

    fn persistence(&self) -> CredentialPersistence {
        self.primary.persistence()
    }

    fn build(
        &self, service: &str, user: &str, modifiers: Option<&HashMap<&str, &str>>,
    ) -> Result<Entry> {
        let primary = self.primary.build(service, user, modifiers)?;
        let fallback = self
            .fallback
            .as_ref()
            .and_then(|f| f.build(service, user, modifiers).ok());
        let cred: Arc<Credential> = Arc::new(CompositeCredential { primary, fallback });
        Ok(Entry::new_with_credential(cred))
    }

    fn search(&self, spec: &HashMap<&str, &str>) -> Result<Vec<Entry>> {
        self.primary.search(spec)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct CompositeCredential {
    primary: Entry,
    fallback: Option<Entry>,
}

impl CredentialApi for CompositeCredential {
    fn set_secret(&self, secret: &[u8]) -> Result<()> {
        self.primary.set_secret(secret)
    }

    fn get_secret(&self) -> Result<Vec<u8>> {
        match self.primary.get_secret() {
            Err(Error::NoEntry) => self.read_fallback_bytes(),
            other => other,
        }
    }

    fn set_password(&self, password: &str) -> Result<()> {
        self.primary.set_password(password)
    }

    fn get_password(&self) -> Result<String> {
        match self.primary.get_password() {
            Err(Error::NoEntry) => self.read_fallback_string(),
            other => other,
        }
    }

    fn delete_credential(&self) -> Result<()> {
        let primary_result = self.primary.delete_credential();
        let fallback_result = self.fallback.as_ref().map(|f| f.delete_credential());
        match (primary_result, fallback_result) {
            (Ok(()), _) => Ok(()),
            (Err(Error::NoEntry), Some(Ok(()))) => Ok(()),
            (Err(e), _) => Err(e),
        }
    }

    fn get_credential(&self) -> Result<Option<Arc<Credential>>> {
        Ok(None)
    }

    fn get_specifiers(&self) -> Option<(String, String)> {
        self.primary.get_specifiers()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl CompositeCredential {
    fn read_fallback_string(&self) -> Result<String> {
        let Some(f) = &self.fallback else {
            return Err(Error::NoEntry);
        };
        let value = f.get_password()?;
        if let Err(e) = self.primary.set_password(&value) {
            log::warn!("keyring migration: copy keyutils → secret-service failed: {e}");
        }
        Ok(value)
    }

    fn read_fallback_bytes(&self) -> Result<Vec<u8>> {
        let Some(f) = &self.fallback else {
            return Err(Error::NoEntry);
        };
        let value = f.get_secret()?;
        if let Err(e) = self.primary.set_secret(&value) {
            log::warn!("keyring migration: copy keyutils → secret-service failed: {e}");
        }
        Ok(value)
    }
}
