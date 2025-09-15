use kftray_commons::utils::settings::{
    get_app_settings,
    get_ssl_auto_regenerate as get_ssl_auto_regen,
    get_ssl_cert_validity_days,
    get_ssl_enabled,
    set_ssl_auto_regenerate as set_ssl_auto_regen,
    set_ssl_ca_auto_install,
    set_ssl_cert_validity_days,
    set_ssl_enabled,
};
use kftray_portforward::ssl::{
    CertificateInfo,
    CertificateManager,
};
use log::{
    error,
    info,
    warn,
};
use tauri::command;

#[command]
pub async fn get_ssl_settings() -> Result<serde_json::Value, String> {
    // Ensure crypto provider is initialized
    kftray_portforward::ssl::ensure_crypto_provider_installed();

    match get_app_settings().await {
        Ok(settings) => Ok(serde_json::json!({
            "ssl_enabled": settings.ssl_enabled,
            "ssl_cert_validity_days": settings.ssl_cert_validity_days,
            "ssl_auto_regenerate": settings.ssl_auto_regenerate,
            "ssl_ca_auto_install": settings.ssl_ca_auto_install,
        })),
        Err(e) => {
            error!("Failed to get SSL settings: {e}");
            Err(format!("Failed to get SSL settings: {e}"))
        }
    }
}

#[command]
pub async fn set_ssl_settings(
    ssl_enabled: bool, ssl_cert_validity_days: u16, ssl_auto_regenerate: bool,
    ssl_ca_auto_install: bool,
) -> Result<String, String> {
    info!(
        "Setting SSL configuration: enabled={}, validity_days={}, auto_regenerate={}, ca_auto_install={}",
        ssl_enabled, ssl_cert_validity_days, ssl_auto_regenerate, ssl_ca_auto_install
    );

    if let Err(e) = set_ssl_enabled(ssl_enabled).await {
        error!("Failed to set SSL enabled: {e}");
        return Err(format!("Failed to set SSL enabled: {e}"));
    }

    if let Err(e) = set_ssl_cert_validity_days(ssl_cert_validity_days).await {
        error!("Failed to set SSL certificate validity days: {e}");
        return Err(format!("Failed to set SSL certificate validity days: {e}"));
    }

    if let Err(e) = set_ssl_auto_regen(ssl_auto_regenerate).await {
        error!("Failed to set SSL auto regenerate: {e}");
        return Err(format!("Failed to set SSL auto regenerate: {e}"));
    }

    if let Err(e) = set_ssl_ca_auto_install(ssl_ca_auto_install).await {
        error!("Failed to set SSL CA auto install: {e}");
        return Err(format!("Failed to set SSL CA auto install: {e}"));
    }

    if ssl_enabled {
        info!("SSL enabled, generating global certificate for all configs");

        match get_app_settings().await {
            Ok(settings) => {
                let cert_manager = CertificateManager::new(&settings)
                    .map_err(|e| format!("Failed to create certificate manager: {e}"))?;

                // Force regenerate global certificate when SSL is enabled to ensure consistency
                match cert_manager.regenerate_certificate_for_all_configs().await {
                    Ok(_) => {
                        info!("Successfully generated global SSL certificate for all configs");

                        // Install CA to system trust store if auto_install is enabled
                        if ssl_ca_auto_install {
                            info!("Auto-installing CA certificate to system trust store");
                            match cert_manager.ensure_ca_installed_and_trusted().await {
                                Ok(_) => info!(
                                    "Successfully installed CA certificate to system trust store"
                                ),
                                Err(e) => warn!("Failed to install CA certificate: {e}"),
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to generate global SSL certificate: {e}");
                        return Err(format!("Failed to generate SSL certificate: {e}"));
                    }
                }
            }
            Err(e) => {
                warn!("Failed to get app settings for SSL certificate generation: {e}");
                return Err(format!("Failed to get app settings: {e}"));
            }
        }
    } else if !ssl_enabled {
        // Clean up all SSL artifacts when SSL is being disabled
        info!("Cleaning up SSL artifacts due to SSL being disabled");
        if let Err(e) = CertificateManager::cleanup_all_ssl_artifacts().await {
            warn!("Failed to cleanup SSL artifacts: {}", e);
            // Don't fail the settings update due to cleanup issues
        } else {
            info!("Successfully cleaned up all SSL artifacts");
        }
    }

    info!("SSL settings updated successfully");
    Ok("SSL settings updated successfully".to_string())
}

#[command]
pub async fn regenerate_certificate(alias: String) -> Result<String, String> {
    info!("Regenerating SSL certificate for alias: {}", alias);

    let settings = get_app_settings()
        .await
        .map_err(|e| format!("Failed to get app settings: {e}"))?;

    let cert_manager = CertificateManager::new(&settings)
        .map_err(|e| format!("Failed to create certificate manager: {e}"))?;

    cert_manager
        .regenerate_certificate(&alias)
        .await
        .map_err(|e| format!("Failed to regenerate certificate: {e}"))?;

    info!("Certificate regenerated successfully for: {}", alias);
    Ok(format!("Certificate regenerated for: {}", alias))
}

#[command]
pub async fn get_certificate_info(alias: String) -> Result<CertificateInfo, String> {
    let settings = get_app_settings()
        .await
        .map_err(|e| format!("Failed to get app settings: {e}"))?;

    let cert_manager = CertificateManager::new(&settings)
        .map_err(|e| format!("Failed to create certificate manager: {e}"))?;

    cert_manager
        .get_certificate_info(&alias)
        .await
        .map_err(|e| format!("Failed to get certificate info: {e}"))
}

#[command]
pub async fn list_certificates() -> Result<Vec<CertificateInfo>, String> {
    let settings = get_app_settings()
        .await
        .map_err(|e| format!("Failed to get app settings: {e}"))?;

    let cert_manager = CertificateManager::new(&settings)
        .map_err(|e| format!("Failed to create certificate manager: {e}"))?;

    cert_manager
        .list_all_certificates()
        .await
        .map_err(|e| format!("Failed to list certificates: {e}"))
}

#[command]
pub async fn remove_certificate(alias: String) -> Result<String, String> {
    info!("Removing SSL certificate for alias: {}", alias);

    let settings = get_app_settings()
        .await
        .map_err(|e| format!("Failed to get app settings: {e}"))?;

    let cert_manager = CertificateManager::new(&settings)
        .map_err(|e| format!("Failed to create certificate manager: {e}"))?;

    cert_manager
        .remove_certificate(&alias)
        .await
        .map_err(|e| format!("Failed to remove certificate: {e}"))?;

    info!("Certificate removed successfully for: {}", alias);
    Ok(format!("Certificate removed for: {}", alias))
}

#[command]
pub async fn is_ssl_enabled() -> Result<bool, String> {
    get_ssl_enabled()
        .await
        .map_err(|e| format!("Failed to get SSL enabled status: {e}"))
}

#[command]
pub async fn enable_ssl() -> Result<String, String> {
    info!("Enabling SSL");

    set_ssl_enabled(true)
        .await
        .map_err(|e| format!("Failed to enable SSL: {e}"))?;

    // Generate global certificate immediately when SSL is enabled
    info!("SSL enabled, generating global certificate for all configs");
    match get_app_settings().await {
        Ok(settings) => {
            let cert_manager = CertificateManager::new(&settings)
                .map_err(|e| format!("Failed to create certificate manager: {e}"))?;

            match cert_manager.regenerate_certificate_for_all_configs().await {
                Ok(_) => {
                    info!("Successfully generated global SSL certificate for all configs");

                    // Always try to install CA when SSL is enabled via this command
                    match cert_manager.ensure_ca_installed_and_trusted().await {
                        Ok(_) => info!("Successfully ensured CA certificate installation"),
                        Err(e) => warn!("Failed to ensure CA certificate installation: {e}"),
                    }
                }
                Err(e) => {
                    warn!("Failed to generate global SSL certificate: {e}");
                    return Err(format!("Failed to generate SSL certificate: {e}"));
                }
            }
        }
        Err(e) => {
            warn!("Failed to get app settings for SSL certificate generation: {e}");
            return Err(format!("Failed to get app settings: {e}"));
        }
    }

    info!("SSL enabled successfully");
    Ok("SSL enabled successfully".to_string())
}

#[command]
pub async fn disable_ssl() -> Result<String, String> {
    info!("Disabling SSL");

    set_ssl_enabled(false)
        .await
        .map_err(|e| format!("Failed to disable SSL: {e}"))?;

    // Clean up all SSL artifacts when SSL is disabled
    info!("Cleaning up SSL artifacts due to SSL being disabled");
    if let Err(e) = CertificateManager::cleanup_all_ssl_artifacts().await {
        warn!("Failed to cleanup SSL artifacts: {}", e);
        // Don't fail the disable operation due to cleanup issues
    } else {
        info!("Successfully cleaned up all SSL artifacts");
    }

    info!("SSL disabled successfully");
    Ok("SSL disabled successfully".to_string())
}

#[command]
pub async fn get_ssl_cert_validity() -> Result<u16, String> {
    get_ssl_cert_validity_days()
        .await
        .map_err(|e| format!("Failed to get SSL certificate validity: {e}"))
}

#[command]
pub async fn set_ssl_cert_validity(days: u16) -> Result<String, String> {
    info!("Setting SSL certificate validity to {} days", days);

    set_ssl_cert_validity_days(days)
        .await
        .map_err(|e| format!("Failed to set SSL certificate validity: {e}"))?;

    info!("SSL certificate validity set to {} days", days);
    Ok(format!("SSL certificate validity set to {} days", days))
}

#[command]
pub async fn get_ssl_auto_regenerate() -> Result<bool, String> {
    get_ssl_auto_regen()
        .await
        .map_err(|e| format!("Failed to get SSL auto regenerate status: {e}"))
}

#[command]
pub async fn set_ssl_auto_regenerate(enabled: bool) -> Result<String, String> {
    info!("Setting SSL auto regenerate to: {}", enabled);

    set_ssl_auto_regen(enabled)
        .await
        .map_err(|e| format!("Failed to set SSL auto regenerate: {e}"))?;

    info!("SSL auto regenerate set to: {}", enabled);
    Ok(format!("SSL auto regenerate set to: {}", enabled))
}
