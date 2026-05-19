use std::process::Command;

use anyhow::{
    Context,
    Result,
};
use log::{
    info,
    warn,
};
use tokio::fs;

pub async fn install_ca_certificate(ca_cert_der: &[u8]) -> Result<()> {
    let cert_path = get_permanent_cert_path();

    if let Some(parent) = cert_path.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }

    let pem_data = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, ca_cert_der)
    );

    fs::write(&cert_path, pem_data)
        .await
        .context("Failed to write certificate file")?;

    install_to_keychain(&cert_path).await
}

async fn install_to_keychain(cert_path: &std::path::Path) -> Result<()> {
    info!("Installing CA certificate to System keychain");

    let cert_path_str = cert_path.display().to_string();

    let install_script = format!(
        r#"do shell script "security add-cert -k /Library/Keychains/System.keychain '{}'" with administrator privileges with prompt "KFtray needs administrator privileges to install the SSL certificate to the System keychain for secure HTTPS connections.""#,
        cert_path_str.replace("'", "'\\''")
    );

    let install_output = Command::new("osascript")
        .arg("-e")
        .arg(&install_script)
        .output()
        .context("Failed to install certificate")?;

    let install_stderr = String::from_utf8_lossy(&install_output.stderr);

    if !install_output.status.success()
        && !install_stderr.contains("already exists")
        && !install_stderr.contains("duplicate")
    {
        warn!("Certificate installation failed: {}", install_stderr.trim());
        return Err(anyhow::anyhow!("Certificate installation failed"));
    }

    info!("Configuring certificate trust settings");

    let trust_output = Command::new("security")
        .arg("add-trusted-cert")
        .arg(&cert_path_str)
        .output()
        .context("Failed to configure trust settings")?;

    if trust_output.status.success() {
        info!("CA certificate installed and trusted successfully");
        Ok(())
    } else {
        let trust_stderr = String::from_utf8_lossy(&trust_output.stderr);

        if trust_stderr.contains("User canceled") || trust_stderr.contains("(-128)") {
            warn!("User canceled trust configuration");
            Err(anyhow::anyhow!("User canceled authentication"))
        } else if trust_stderr.contains("already exists") {
            info!("CA certificate trust settings already configured");
            Ok(())
        } else {
            warn!("Trust configuration failed: {}", trust_stderr.trim());
            info!("Certificate installed but trust settings may need manual configuration");
            Ok(())
        }
    }
}

fn get_permanent_cert_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("kftray")
        .join("ssl-ca")
        .join("kftray-ca.crt")
}

pub async fn is_ca_installed(_ca_cert_der: &[u8]) -> Result<bool> {
    let output = Command::new("security")
        .arg("find-certificate")
        .arg("-c")
        .arg("kftray Local CA")
        .arg("/Library/Keychains/System.keychain")
        .output()
        .context("Failed to check certificate")?;

    if !output.status.success() {
        return Ok(false);
    }

    let trust_output = Command::new("security").arg("dump-trust-settings").output();

    match trust_output {
        Ok(result) if result.status.success() => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            Ok(stdout.contains("kftray Local CA"))
        }
        _ => Ok(false),
    }
}

pub async fn remove_ca_certificate(_ca_cert_der: &[u8]) -> Result<()> {
    info!("Removing CA certificate");

    let script = r#"do shell script "security delete-certificate -c 'kftray Local CA' /Library/Keychains/System.keychain" with administrator privileges"#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .context("Failed to remove certificate")?;

    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() || stderr.contains("not found") {
        info!("CA certificate removed");
        let cert_path = get_permanent_cert_path();
        let _ = tokio::fs::remove_file(&cert_path).await;
        Ok(())
    } else {
        warn!("Failed to remove certificate: {}", stderr.trim());
        Err(anyhow::anyhow!("Failed to remove certificate"))
    }
}
