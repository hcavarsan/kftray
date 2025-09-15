use anyhow::{
    Context,
    Result,
};
use log::{
    error,
    info,
    warn,
};

#[cfg(target_os = "windows")]
pub async fn install_ca_certificate(ca_cert_der: &[u8]) -> Result<()> {
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::Cryptography::Certificates::*;
    use windows::core::{
        PCSTR,
        w,
    };

    anyhow::ensure!(!ca_cert_der.is_empty(), "Empty certificate DER provided");

    unsafe {
        let store_name = w!("ROOT");

        let cert_store = CertOpenStore(
            CERT_STORE_PROV_SYSTEM,
            0,
            None,
            CERT_SYSTEM_STORE_LOCAL_MACHINE | CERT_STORE_OPEN_EXISTING_FLAG,
            Some(store_name.as_ptr() as *const core::ffi::c_void),
        )
        .or_else(|e| {
            warn!(
                "Failed to open LocalMachine ROOT: {:?}. Falling back to CurrentUser ROOT.",
                e
            );
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM,
                0,
                None,
                CERT_SYSTEM_STORE_CURRENT_USER | CERT_STORE_OPEN_EXISTING_FLAG,
                Some(store_name.as_ptr() as *const core::ffi::c_void),
            )
        })
        .context("Failed to open Windows ROOT certificate store (LocalMachine/CurrentUser)")?;

        let cert_context =
            CertCreateCertificateContext(X509_ASN_ENCODING | PKCS_7_ASN_ENCODING, ca_cert_der)
                .context("Failed to create certificate context")?;

        let result = CertAddCertificateContextToStore(
            cert_store,
            cert_context,
            CERT_STORE_ADD_REPLACE_EXISTING,
            None,
        );

        CertFreeCertificateContext(cert_context);
        let _ = CertCloseStore(cert_store, 0);

        if result.is_ok() {
            info!("Successfully installed kftray CA certificate to Windows certificate store");
            Ok(())
        } else {
            let error_code = windows::core::Error::from_win32();
            error!("Failed to add certificate to store: {:?}", error_code);
            Err(anyhow::anyhow!(
                "Failed to install CA certificate: {:?}",
                error_code
            ))
        }
    }
}

#[cfg(target_os = "windows")]
pub async fn is_ca_installed(ca_cert_der: &[u8]) -> Result<bool> {
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::Cryptography::Certificates::*;
    use windows::core::w;

    anyhow::ensure!(!ca_cert_der.is_empty(), "Empty certificate DER provided");

    unsafe {
        let store_name = w!("ROOT");

        let cert_store = CertOpenStore(
            CERT_STORE_PROV_SYSTEM,
            0,
            None,
            CERT_SYSTEM_STORE_LOCAL_MACHINE | CERT_STORE_OPEN_EXISTING_FLAG,
            Some(store_name.as_ptr() as *const core::ffi::c_void),
        )
        .or_else(|e| {
            warn!(
                "Failed to open LocalMachine ROOT: {:?}. Falling back to CurrentUser ROOT.",
                e
            );
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM,
                0,
                None,
                CERT_SYSTEM_STORE_CURRENT_USER | CERT_STORE_OPEN_EXISTING_FLAG,
                Some(store_name.as_ptr() as *const core::ffi::c_void),
            )
        })
        .context("Failed to open Windows ROOT certificate store (LocalMachine/CurrentUser)")?;

        let cert_context =
            CertCreateCertificateContext(X509_ASN_ENCODING | PKCS_7_ASN_ENCODING, ca_cert_der)
                .context("Failed to create certificate context")?;

        let found_cert = CertFindCertificateInStore(
            cert_store,
            X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            0,
            CERT_FIND_EXISTING,
            Some(cert_context.as_ptr() as *const _),
            None,
        );

        CertFreeCertificateContext(cert_context);
        if !found_cert.is_null() {
            CertFreeCertificateContext(found_cert);
        }
        let _ = CertCloseStore(cert_store, 0);

        Ok(!found_cert.is_null())
    }
}

#[cfg(target_os = "windows")]
pub async fn remove_ca_certificate(ca_cert_der: &[u8]) -> Result<()> {
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::Cryptography::Certificates::*;
    use windows::core::w;

    info!("Removing CA certificate using native Windows APIs");
    anyhow::ensure!(!ca_cert_der.is_empty(), "Empty certificate DER provided");

    unsafe {
        let store_name = w!("ROOT");

        let cert_store = CertOpenStore(
            CERT_STORE_PROV_SYSTEM,
            0,
            None,
            CERT_SYSTEM_STORE_LOCAL_MACHINE | CERT_STORE_OPEN_EXISTING_FLAG,
            Some(store_name.as_ptr() as *const core::ffi::c_void),
        )
        .or_else(|e| {
            warn!(
                "Failed to open LocalMachine ROOT: {:?}. Falling back to CurrentUser ROOT.",
                e
            );
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM,
                0,
                None,
                CERT_SYSTEM_STORE_CURRENT_USER | CERT_STORE_OPEN_EXISTING_FLAG,
                Some(store_name.as_ptr() as *const core::ffi::c_void),
            )
        })
        .context("Failed to open Windows ROOT certificate store (LocalMachine/CurrentUser)")?;

        let cert_context =
            CertCreateCertificateContext(X509_ASN_ENCODING | PKCS_7_ASN_ENCODING, ca_cert_der)
                .context("Failed to create certificate context")?;

        let found_cert = CertFindCertificateInStore(
            cert_store,
            X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            0,
            CERT_FIND_EXISTING,
            Some(cert_context.as_ptr() as *const _),
            None,
        );

        CertFreeCertificateContext(cert_context);

        if found_cert.is_null() {
            let _ = CertCloseStore(cert_store, 0);
            info!("kftray CA certificate not found in Windows certificate store (already removed)");
            return Ok(());
        }

        let result = CertDeleteCertificateFromStore(found_cert);
        let _ = CertCloseStore(cert_store, 0);

        if result.is_ok() {
            info!("Successfully removed kftray CA certificate from Windows certificate store");
            Ok(())
        } else {
            let error_code = windows::core::Error::from_win32();
            warn!("Failed to remove CA certificate: {:?}", error_code);
            Err(anyhow::anyhow!(
                "Failed to remove CA certificate: {:?}",
                error_code
            ))
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub async fn install_ca_certificate(_ca_cert_der: &[u8]) -> Result<()> {
    Err(anyhow::anyhow!(
        "Windows certificate operations not supported on this platform"
    ))
}

#[cfg(not(target_os = "windows"))]
pub async fn is_ca_installed(_ca_cert_der: &[u8]) -> Result<bool> {
    Err(anyhow::anyhow!(
        "Windows certificate operations not supported on this platform"
    ))
}

#[cfg(not(target_os = "windows"))]
pub async fn remove_ca_certificate(_ca_cert_der: &[u8]) -> Result<()> {
    Err(anyhow::anyhow!(
        "Windows certificate operations not supported on this platform"
    ))
}
