use async_trait::async_trait;
use russh::keys::{
    HashAlg,
    PublicKey,
};

use crate::error::SshError;
use crate::route::HostKeySource;

/// Persistent first-use host keys, keyed by the alias of a [`HostKeySource`].
#[async_trait]
pub trait HostKeyStore: Send + Sync {
    async fn get(&self, alias: &str) -> Result<Option<PublicKey>, SshError>;
    /// Stores `key` unless the alias already has one; returns the key stored
    /// afterwards.
    async fn insert_if_absent(&self, alias: &str, key: &PublicKey) -> Result<PublicKey, SshError>;
    async fn forget(&self, alias: &str) -> Result<(), SshError>;
}

fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

fn mismatch(label: &str, alias: &str, known: &[&PublicKey], presented: &PublicKey) -> SshError {
    SshError::HostKeyMismatch {
        route: label.to_owned(),
        alias: alias.to_owned(),
        known: known
            .iter()
            .map(|key| fingerprint(key))
            .collect::<Vec<_>>()
            .join(", "),
        presented: fingerprint(presented),
    }
}

/// Decides whether `presented` is the host key of the route: keys the
/// provider vouches for win; without any, the first key seen is stored and
/// every later one must match it.
pub(crate) async fn verify(
    presented: &PublicKey, label: &str, source: &dyn HostKeySource, store: &dyn HostKeyStore,
) -> Result<(), SshError> {
    let alias = source.alias();

    let trusted = source.trusted().await?;
    if !trusted.is_empty() {
        if trusted
            .iter()
            .any(|key| key.key_data() == presented.key_data())
        {
            return Ok(());
        }
        let known: Vec<&PublicKey> = trusted.iter().collect();
        return Err(mismatch(label, &alias, &known, presented));
    }

    let (known, first_use) = match store.get(&alias).await? {
        Some(key) => (key, false),
        None => (store.insert_if_absent(&alias, presented).await?, true),
    };
    if known.key_data() != presented.key_data() {
        return Err(mismatch(label, &alias, &[&known], presented));
    }
    if first_use {
        log::info!(
            "[ssh {label}] trusting host key {} for {alias} on first use",
            fingerprint(presented)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::testing::MemoryHostKeyStore;

    struct Source {
        alias: &'static str,
        trusted: Vec<PublicKey>,
    }

    #[async_trait]
    impl HostKeySource for Source {
        fn alias(&self) -> String {
            self.alias.to_owned()
        }

        async fn trusted(&self) -> Result<Vec<PublicKey>, SshError> {
            Ok(self.trusted.clone())
        }
    }

    fn random_key() -> PublicKey {
        russh::keys::PrivateKey::random(
            &mut russh::keys::key::safe_rng(),
            russh::keys::Algorithm::Ed25519,
        )
        .unwrap()
        .public_key()
        .clone()
    }

    #[tokio::test]
    async fn verify_should_accept_a_provider_key_without_touching_the_store() {
        let (trusted, other) = (random_key(), random_key());
        let source = Source {
            alias: "t:1",
            trusted: vec![other.clone(), trusted.clone()],
        };
        let store = MemoryHostKeyStore::default();

        verify(&trusted, "r", &source, &store).await.unwrap();

        assert_eq!(store.get("t:1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn verify_should_reject_a_key_the_provider_does_not_vouch_for_even_if_stored() {
        let (trusted, presented) = (random_key(), random_key());
        let source = Source {
            alias: "t:1",
            trusted: vec![trusted.clone()],
        };
        let store = MemoryHostKeyStore::default();
        store.insert_if_absent("t:1", &presented).await.unwrap();

        let err = verify(&presented, "r", &source, &store).await.unwrap_err();

        let SshError::HostKeyMismatch {
            known,
            presented: p,
            ..
        } = err
        else {
            panic!("expected HostKeyMismatch, got {err}");
        };
        assert_eq!(known, fingerprint(&trusted));
        assert_eq!(p, fingerprint(&presented));
    }

    #[tokio::test]
    async fn verify_should_store_the_first_key_and_accept_it_again() {
        let key = random_key();
        let source = Source {
            alias: "t:2",
            trusted: vec![],
        };
        let store = MemoryHostKeyStore::default();

        verify(&key, "r", &source, &store).await.unwrap();
        verify(&key, "r", &source, &store).await.unwrap();

        assert_eq!(store.get("t:2").await.unwrap(), Some(key));
    }

    #[tokio::test]
    async fn verify_should_reject_a_different_key_after_first_use() {
        let (first, second) = (random_key(), random_key());
        let source = Source {
            alias: "t:3",
            trusted: vec![],
        };
        let store: Arc<dyn HostKeyStore> = Arc::new(MemoryHostKeyStore::default());
        verify(&first, "r", &source, &*store).await.unwrap();

        let err = verify(&second, "r", &source, &*store).await.unwrap_err();

        assert!(matches!(err, SshError::HostKeyMismatch { .. }), "{err}");
        assert_eq!(store.get("t:3").await.unwrap(), Some(first));
    }
}
