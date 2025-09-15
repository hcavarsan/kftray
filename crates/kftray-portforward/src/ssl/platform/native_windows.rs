



use anyhow::{Context, Result};
use log::{info, warn, error};


#[cfg(target_os = "windows")]
pub async fn install_ca_certificate_native(ca_cert_der: &[u8]) -> Result<()> {
    use schannel::cert_store::CertStore;
    use schannel::cert_context::CertContext;

    info!("Installing CA certificate using native Windows APIs");


    let store = CertStore::open_local_machine("ROOT")
        .context("Failed to open ROOT certificate store")?;


    let cert_context = CertContext::from_der(ca_cert_der)
        .context("Failed to create certificate context from DER data")?;


    store.add_cert(&cert_context, schannel::cert_store::CertAdd::ReplaceExisting)
        .context("Failed to add certificate to Windows certificate store")?;

    info!("Successfully installed kftray CA certificate to Windows ROOT store using native APIs");
    Ok(())
}


#[cfg(target_os = "windows")]
pub async fn is_ca_installed_native(ca_cert_der: &[u8]) -> Result<bool> {
    use schannel::cert_store::CertStore;
    use schannel::cert_context::CertContext;


    let store = CertStore::open_local_machine("ROOT")
        .context("Failed to open ROOT certificate store")?;


    let target_cert = CertContext::from_der(ca_cert_der)
        .context("Failed to create certificate context from DER data")?;


    for cert in store.certs() {
        if cert.to_der() == ca_cert_der {
            return Ok(true);
        }
    }

    Ok(false)
}


#[cfg(target_os = "windows")]
pub async fn remove_ca_certificate_native(ca_cert_der: &[u8]) -> Result<()> {
    use schannel::cert_store::CertStore;
    use schannel::cert_context::CertContext;

    info!("Removing CA certificate using native Windows APIs");


    let store = CertStore::open_local_machine("ROOT")
        .context("Failed to open ROOT certificate store")?;


    let mut found = false;
    for cert in store.certs() {
        if cert.to_der() == ca_cert_der {

            store.delete_cert(&cert)
                .context("Failed to delete certificate from Windows certificate store")?;
            found = true;
            break;
        }
    }

    if found {
        info!("Successfully removed kftray CA certificate from Windows ROOT store");
    } else {
        info!("kftray CA certificate not found in Windows ROOT store (already removed)");
    }

    Ok(())
}


#[cfg(not(target_os = "windows"))]
pub async fn install_ca_certificate_native(_ca_cert_der: &[u8]) -> Result<()> {
    Err(anyhow::anyhow!("Native Windows certificate installation not available on this platform"))
}

#[cfg(not(target_os = "windows"))]
pub async fn is_ca_installed_native(_ca_cert_der: &[u8]) -> Result<bool> {
    Ok(false)
}

#[cfg(not(target_os = "windows"))]
pub async fn remove_ca_certificate_native(_ca_cert_der: &[u8]) -> Result<()> {
    Err(anyhow::anyhow!("Native Windows certificate removal not available on this platform"))
}