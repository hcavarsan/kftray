use std::sync::Once;

#[cfg(target_os = "linux")]
use crate::ssl::composite_store::LinuxCompositeStore;

static INSTALL: Once = Once::new();

pub fn install_default_keyring_store() {
    INSTALL.call_once(|| {
        if let Err(e) = try_install() {
            log::error!("failed to install default keyring store: {e}");
        }
    });
}

fn try_install() -> anyhow::Result<()> {
    if std::env::var("KFTRAY_TEST_MODE").is_ok() {
        keyring_core::set_default_store(keyring_core::mock::Store::new()?);
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let store = apple_native_keyring_store::keychain::Store::new()?;
        keyring_core::set_default_store(store);
    }

    #[cfg(target_os = "linux")]
    {
        let store = LinuxCompositeStore::new()?;
        keyring_core::set_default_store(store);
    }

    #[cfg(target_os = "windows")]
    {
        let store = windows_native_keyring_store::Store::new()?;
        keyring_core::set_default_store(store);
    }

    Ok(())
}
