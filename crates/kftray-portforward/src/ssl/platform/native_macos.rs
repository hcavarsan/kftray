use anyhow::{Context, Result};
use log::{debug, info, warn};

#[cfg(target_os = "macos")]
use security_framework::certificate::SecCertificate;
#[cfg(target_os = "macos")]
use security_framework::trust_settings::{Domain, TrustSettings, TrustSettingsForCertificate};


#[cfg(target_os = "macos")]
pub async fn install_ca_certificate_native(ca_cert_der: &[u8]) -> Result<()> {
    info!("Installing CA certificate using native macOS Security Framework");


    let _cert = SecCertificate::from_der(ca_cert_der)
        .context("Failed to create SecCertificate from DER data")?;





    warn!("Native certificate installation requires admin privileges and special entitlements");
    info!("Falling back to osascript method for proper admin authentication");

    Err(anyhow::anyhow!("Native installation not fully implemented, using fallback"))
}


#[cfg(target_os = "macos")]
pub async fn is_ca_installed_native(ca_cert_der: &[u8]) -> Result<bool> {

    if ca_cert_der.is_empty() {
        debug!("Empty certificate DER data provided");
        return Ok(false);
    }


    let target_cert = match SecCertificate::from_der(ca_cert_der) {
        Ok(cert) => {
            debug!("Successfully created SecCertificate from DER data ({} bytes)", ca_cert_der.len());
            cert
        },
        Err(e) => {
            debug!("Failed to create SecCertificate from DER data ({} bytes): {}", ca_cert_der.len(), e);
            debug!("DER data starts with: {:02X?}", &ca_cert_der[..ca_cert_der.len().min(16)]);
            return Ok(false);
        }
    };


    let trust_settings = TrustSettings::new(Domain::System);
    match trust_settings.tls_trust_settings_for_certificate(&target_cert) {
        Ok(Some(settings)) => {
            debug!("Certificate found in system trust settings");
            return Ok(is_certificate_trusted(&settings));
        }
        Ok(None) => debug!("Certificate not found in system trust settings"),
        Err(e) => debug!("Error checking system trust settings: {}", e),
    }


    let trust_settings = TrustSettings::new(Domain::Admin);
    match trust_settings.tls_trust_settings_for_certificate(&target_cert) {
        Ok(Some(settings)) => {
            debug!("Certificate found in admin trust settings");
            return Ok(is_certificate_trusted(&settings));
        }
        Ok(None) => debug!("Certificate not found in admin trust settings"),
        Err(e) => debug!("Error checking admin trust settings: {}", e),
    }


    let trust_settings = TrustSettings::new(Domain::User);
    match trust_settings.tls_trust_settings_for_certificate(&target_cert) {
        Ok(Some(settings)) => {
            debug!("Certificate found in user trust settings");
            return Ok(is_certificate_trusted(&settings));
        }
        Ok(None) => debug!("Certificate not found in user trust settings"),
        Err(e) => debug!("Error checking user trust settings: {}", e),
    }


    match is_certificate_in_keychain(&target_cert).await {
        Ok(found) => {
            if found {
                info!("Certificate found in keychain but may not be explicitly trusted");
                Ok(true)
            } else {
                debug!("Certificate not found in any keychain");
                Ok(false)
            }
        }
        Err(e) => {
            warn!("Error checking keychain for certificate: {}", e);
            Ok(false)
        }
    }
}


#[cfg(target_os = "macos")]
pub async fn remove_ca_certificate_native(ca_cert_der: &[u8]) -> Result<()> {
    info!("Removing CA certificate using native macOS Security Framework");


    let _target_cert = SecCertificate::from_der(ca_cert_der)
        .context("Failed to create SecCertificate from DER data")?;

    let removal_attempted = false;
    let mut removal_errors = Vec::new();



    warn!("Native certificate removal not fully implemented");
    removal_errors.push("native removal not implemented".to_string());

    if removal_attempted {
        if !removal_errors.is_empty() {
            warn!("Certificate removed but some errors occurred: {:?}", removal_errors);
        }
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Failed to remove certificate from any keychain: {}",
            removal_errors.join(", ")
        ))
    }
}



#[cfg(target_os = "macos")]
fn is_certificate_trusted(settings: &TrustSettingsForCertificate) -> bool {

    match settings {
        TrustSettingsForCertificate::TrustRoot => {
            debug!("Certificate explicitly trusted as root");
            true
        }
        TrustSettingsForCertificate::TrustAsRoot => {
            debug!("Certificate trusted as root");
            true
        }
        TrustSettingsForCertificate::Deny => {
            debug!("Certificate explicitly denied");
            false
        }
        TrustSettingsForCertificate::Unspecified => {
            debug!("Certificate trust unspecified (not explicitly trusted)");
            false
        }
        _ => {
            debug!("Certificate has other trust settings");
            true
        }
    }
}


#[cfg(target_os = "macos")]
async fn is_certificate_in_keychain(_cert: &SecCertificate) -> Result<bool> {


    debug!("Certificate keychain search not fully implemented with high-level APIs");
    Ok(false)
}


#[cfg(not(target_os = "macos"))]
pub async fn install_ca_certificate_native(_ca_cert_der: &[u8]) -> Result<()> {
    Err(anyhow::anyhow!("Native macOS certificate installation not available on this platform"))
}

#[cfg(not(target_os = "macos"))]
pub async fn is_ca_installed_native(_ca_cert_der: &[u8]) -> Result<bool> {
    Ok(false)
}

#[cfg(not(target_os = "macos"))]
pub async fn remove_ca_certificate_native(_ca_cert_der: &[u8]) -> Result<()> {
    Err(anyhow::anyhow!("Native macOS certificate removal not available on this platform"))
}