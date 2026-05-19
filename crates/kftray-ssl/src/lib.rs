pub mod cert_generator;
pub mod cert_manager;
pub mod cert_store;
#[cfg(target_os = "linux")]
pub mod composite_store;
pub mod keyring_init;
pub mod platform;

use std::sync::Once;

static CRYPTO_PROVIDER_INIT: Once = Once::new();

pub fn ensure_crypto_provider_installed() {
    CRYPTO_PROVIDER_INIT.call_once(|| {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            rustls::crypto::ring::default_provider()
                .install_default()
                .expect("Failed to install rustls crypto provider");
        }
    });
}

pub use cert_generator::{
    CertificateGenerator,
    CertificatePair,
};
pub use cert_manager::{
    CertificateInfo,
    CertificateManager,
};
pub use cert_store::CertificateStore;
pub use keyring_init::install_default_keyring_store;
