use anyhow::{
    Context,
    Result,
};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

pub async fn install_ca_certificate(ca_cert_der: &[u8], _ca_cert_pem: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::install_ca_certificate(ca_cert_der)
            .await
            .context("Failed to install CA certificate on macOS")
    }

    #[cfg(target_os = "windows")]
    {
        windows::install_ca_certificate(ca_cert_der)
            .await
            .context("Failed to install CA certificate on Windows")
    }

    #[cfg(target_os = "linux")]
    {
        let ca_cert_pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, ca_cert_der)
        );
        linux::install_ca_certificate(&ca_cert_pem)
            .await
            .context("Failed to install CA certificate on Linux")
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        log::warn!("CA certificate installation not supported on this platform");
        Ok(())
    }
}

pub async fn is_ca_installed(ca_cert_der: &[u8]) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        macos::is_ca_installed(ca_cert_der).await
    }

    #[cfg(target_os = "windows")]
    {
        windows::is_ca_installed(ca_cert_der).await
    }

    #[cfg(target_os = "linux")]
    {
        linux::is_ca_installed(ca_cert_der).await
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Ok(false)
    }
}

pub async fn remove_ca_certificate(ca_cert_der: &[u8]) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::remove_ca_certificate(ca_cert_der).await
    }

    #[cfg(target_os = "windows")]
    {
        windows::remove_ca_certificate(ca_cert_der)
            .await
            .context("Failed to remove CA certificate on Windows")
    }

    #[cfg(target_os = "linux")]
    {
        linux::remove_ca_certificate(ca_cert_der)
            .await
            .context("Failed to remove CA certificate on Linux")
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        log::warn!("CA certificate removal not supported on this platform");
        Ok(())
    }
}
