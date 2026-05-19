use std::path::{
    Path,
    PathBuf,
};

use anyhow::{
    Context,
    Result,
};
use log::{
    error,
    info,
    warn,
};
use tokio::fs;
use tokio::process::Command;

fn get_ca_cert_directory() -> PathBuf {
    let possible_dirs = [
        "/usr/local/share/ca-certificates",
        "/etc/pki/ca-trust/source/anchors",
        "/usr/share/ca-certificates",
        "/etc/ssl/certs",
    ];

    for dir in &possible_dirs {
        if Path::new(dir).exists() {
            return PathBuf::from(dir);
        }
    }

    PathBuf::from("/usr/local/share/ca-certificates")
}

fn get_update_command() -> Option<&'static str> {
    if Path::new("/usr/sbin/update-ca-certificates").exists() {
        Some("update-ca-certificates")
    } else if Path::new("/usr/bin/update-ca-trust").exists() {
        Some("update-ca-trust")
    } else if Path::new("/usr/bin/trust").exists() {
        Some("trust")
    } else {
        None
    }
}

pub async fn install_ca_certificate(ca_cert_pem: &str) -> Result<()> {
    let ca_dir = get_ca_cert_directory();
    let cert_path = ca_dir.join("kftray-ca.crt");

    if !ca_dir.exists() {
        fs::create_dir_all(&ca_dir)
            .await
            .context("Failed to create CA certificate directory")?;
    }

    if !is_writable(&ca_dir).await {
        error!("No write permissions to CA directory: {}", ca_dir.display());
        return Err(anyhow::anyhow!(
            "Cannot install CA certificate: insufficient permissions to write to {}. \
            Please run with sudo or install manually.",
            ca_dir.display()
        ));
    }

    fs::write(&cert_path, ca_cert_pem)
        .await
        .context("Failed to write CA certificate file")?;

    info!("Wrote kftray CA certificate to: {}", cert_path.display());

    if let Some(update_cmd) = get_update_command() {
        let output = Command::new(update_cmd)
            .output()
            .await
            .context(format!("Failed to execute {}", update_cmd))?;

        if output.status.success() {
            info!("Successfully updated system CA certificate store");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to update CA certificate store: {}", stderr);
            warn!("You may need to run 'sudo {}' manually", update_cmd);
        }
    } else {
        warn!(
            "Could not find CA update command. You may need to update the certificate store manually"
        );
    }

    Ok(())
}

pub async fn is_ca_installed(_ca_cert_der: &[u8]) -> Result<bool> {
    let ca_dir = get_ca_cert_directory();
    let cert_path = ca_dir.join("kftray-ca.crt");

    Ok(cert_path.exists())
}

pub async fn remove_ca_certificate(_ca_cert_der: &[u8]) -> Result<()> {
    let ca_dir = get_ca_cert_directory();
    let cert_path = ca_dir.join("kftray-ca.crt");

    if !cert_path.exists() {
        info!("kftray CA certificate not found (already removed)");
        return Ok(());
    }

    if !is_writable(&ca_dir).await {
        error!("No write permissions to CA directory: {}", ca_dir.display());
        return Err(anyhow::anyhow!(
            "Cannot remove CA certificate: insufficient permissions to write to {}. \
            Please run with sudo or remove manually: {}",
            ca_dir.display(),
            cert_path.display()
        ));
    }

    fs::remove_file(&cert_path)
        .await
        .context("Failed to remove CA certificate file")?;

    info!(
        "Removed kftray CA certificate from: {}",
        cert_path.display()
    );

    if let Some(update_cmd) = get_update_command() {
        let output = Command::new(update_cmd)
            .output()
            .await
            .context(format!("Failed to execute {}", update_cmd))?;

        if output.status.success() {
            info!("Successfully updated system CA certificate store");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to update CA certificate store: {}", stderr);
        }
    }

    Ok(())
}

async fn is_writable(path: &Path) -> bool {
    use std::fs::OpenOptions;

    let test_file = path.join(".kftray-write-test");

    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&test_file)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&test_file);
            true
        }
        Err(_) => false,
    }
}
