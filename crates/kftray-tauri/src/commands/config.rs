use kftray_commons::config::{
    delete_all_configs,
    delete_config,
    delete_configs,
    export_configs,
    get_config,
    get_configs,
    import_configs,
    insert_config,
    update_config,
};
use kftray_commons::models::config_model::Config;
use kftray_commons::utils::settings::get_ssl_enabled;
use kftray_portforward::ssl::cert_manager::CertificateManager;
use log::{
    error,
    info,
    warn,
};

fn validate_config(config: &Config) -> Result<(), String> {
    if config.auto_loopback_address && config.local_address.is_some() {
        warn!(
            "Config has auto_loopback_address enabled but local_address is also set. \
             The auto-allocated address will override the manual local_address."
        );
    }
    Ok(())
}

async fn regenerate_ssl_certificate_if_needed() -> Result<(), String> {
    // Check if SSL is enabled
    match get_ssl_enabled().await {
        Ok(true) => {
            info!("SSL is enabled, regenerating global certificate due to config changes");
            match kftray_commons::utils::settings::get_app_settings().await {
                Ok(settings) => {
                    let cert_manager = CertificateManager::new(&settings)
                        .map_err(|e| format!("Failed to create certificate manager: {}", e))?;

                    if let Err(e) = cert_manager.regenerate_certificate_for_all_configs().await {
                        warn!("Failed to regenerate SSL certificate: {}", e);
                        // Don't fail the config operation if certificate
                        // regeneration fails
                    } else {
                        info!("Successfully regenerated global SSL certificate");

                        // Restart SSL proxies for all running configs to pick up new certificates
                        info!(
                            "Certificate regeneration successful, attempting to restart SSL proxies"
                        );
                        restart_ssl_proxies_with_retry().await;
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to get app settings for SSL certificate regeneration: {}",
                        e
                    );
                }
            }
        }
        Ok(false) => {
            info!("SSL is disabled, skipping certificate regeneration");
        }
        Err(e) => {
            warn!(
                "Failed to check SSL status, skipping certificate regeneration: {}",
                e
            );
        }
    }
    Ok(())
}

async fn restart_ssl_proxies_if_running() -> Result<(), String> {
    use kftray_commons::utils::config_state::get_configs_state;
    use kftray_portforward::kube::{
        start_port_forward,
        stop_port_forward,
    };

    info!("=== Starting SSL proxy restart process ===");

    // Get all currently running configs
    let config_states = get_configs_state()
        .await
        .map_err(|e| format!("Failed to get config states: {}", e))?;

    info!("Found {} total config states", config_states.len());

    let running_config_ids: Vec<i64> = config_states
        .iter()
        .filter(|state| state.is_running)
        .map(|state| state.config_id)
        .collect();

    info!("Running config IDs: {:?}", running_config_ids);

    if running_config_ids.is_empty() {
        info!("No running configs found, no SSL proxies to restart");
        return Ok(());
    }

    // Get all configs to filter for SSL-enabled ones
    let all_configs = get_configs()
        .await
        .map_err(|e| format!("Failed to get configs: {}", e))?;

    info!("Found {} total configs in database", all_configs.len());

    let ssl_configs: Vec<Config> = all_configs
        .into_iter()
        .filter(|config| {
            let is_running = config.id.is_some_and(|id| running_config_ids.contains(&id));
            let is_ssl_enabled = config.domain_enabled.unwrap_or(false);
            info!(
                "Config {}: running={}, ssl_enabled={}, alias={:?}",
                config.id.unwrap_or(-1),
                is_running,
                is_ssl_enabled,
                config.alias
            );
            is_running && is_ssl_enabled
        })
        .collect();

    info!(
        "Filtered to {} SSL-enabled running configs",
        ssl_configs.len()
    );

    if ssl_configs.is_empty() {
        info!("No running SSL-enabled configs found, no SSL proxies to restart");
        return Ok(());
    }

    info!("Found {} SSL-enabled configs to restart", ssl_configs.len());

    // Stop and restart each SSL-enabled config
    for config in &ssl_configs {
        let config_id = config.id.unwrap().to_string();
        info!(
            "Restarting SSL proxy for config {} ({})",
            config_id,
            config.alias.as_deref().unwrap_or("unnamed")
        );

        // Stop the current port forward
        if let Err(e) = stop_port_forward(config_id.clone()).await {
            warn!(
                "Failed to stop port forward for config {}: {}",
                config_id, e
            );
            continue;
        }

        // Small delay to ensure clean shutdown
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Restart with the same protocol (assuming TCP for SSL)
        if let Err(e) = start_port_forward(vec![config.clone()], "tcp").await {
            warn!(
                "Failed to restart port forward for config {}: {}",
                config_id, e
            );
        } else {
            info!("Successfully restarted SSL proxy for config {}", config_id);
        }
    }

    info!("Completed SSL proxy restart process");
    Ok(())
}

async fn restart_ssl_proxies_with_retry() {
    // Try multiple times with increasing delays to catch configs as they start up
    let delays = [100, 500, 1000]; // milliseconds

    for (attempt, delay) in delays.iter().enumerate() {
        info!(
            "SSL proxy restart attempt {} (after {}ms delay)",
            attempt + 1,
            delay
        );
        tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;

        match restart_ssl_proxies_if_running().await {
            Ok(()) => {
                info!(
                    "SSL proxy restart attempt {} completed successfully",
                    attempt + 1
                );
                return;
            }
            Err(e) => {
                warn!("SSL proxy restart attempt {} failed: {}", attempt + 1, e);
            }
        }
    }

    warn!("All SSL proxy restart attempts failed");
}

#[tauri::command]
pub async fn delete_config_cmd(id: i64) -> Result<(), String> {
    info!("Deleting config with id: {id}");
    let result = delete_config(id).await;
    if result.is_ok() {
        let _ = regenerate_ssl_certificate_if_needed().await;
    }
    result
}

#[tauri::command]
pub async fn delete_configs_cmd(ids: Vec<i64>) -> Result<(), String> {
    info!("Deleting configs with ids: {ids:?}");
    let result = delete_configs(ids).await;
    if result.is_ok() {
        let _ = regenerate_ssl_certificate_if_needed().await;
    }
    result
}

#[tauri::command]
pub async fn delete_all_configs_cmd() -> Result<(), String> {
    info!("Deleting all configs");
    let result = delete_all_configs().await;
    if result.is_ok() {
        let _ = regenerate_ssl_certificate_if_needed().await;
    }
    result
}

#[tauri::command]
pub async fn insert_config_cmd(config: Config) -> Result<(), String> {
    validate_config(&config)?;
    let result = insert_config(config).await;
    if result.is_ok() {
        let _ = regenerate_ssl_certificate_if_needed().await;
    }
    result
}

#[tauri::command]
pub async fn get_configs_cmd() -> Result<Vec<Config>, String> {
    info!("get_configs called");
    let configs = get_configs().await?;
    Ok(configs)
}

#[tauri::command]
pub async fn get_config_cmd(id: i64) -> Result<Config, String> {
    info!("get_config called with id: {id}");
    get_config(id).await
}

#[tauri::command]
pub async fn update_config_cmd(config: Config) -> Result<(), String> {
    info!(
        "=== UPDATE_CONFIG_CMD CALLED with id={:?}, alias={:?} ===",
        config.id, config.alias
    );
    validate_config(&config)?;
    let result = update_config(config).await;
    if result.is_ok() {
        let _ = regenerate_ssl_certificate_if_needed().await;
    }
    result
}

#[tauri::command]
pub async fn export_configs_cmd() -> Result<String, String> {
    export_configs().await
}

#[tauri::command]
pub async fn import_configs_cmd(json: String) -> Result<(), String> {
    let result = import_configs(json).await;
    match &result {
        Ok(_) => {
            let _ = regenerate_ssl_certificate_if_needed().await;
        }
        Err(e) => {
            error!(
                "Error migrating configs: {e}. Please check if the configurations are valid and compatible with the current system/version."
            );
            return Err(format!("Error migrating configs: {e}"));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lazy_static::lazy_static;
    use sqlx::SqlitePool;
    use tokio::sync::Mutex;

    use super::*;

    lazy_static! {
        static ref TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

    async fn setup_isolated_test_db() -> Arc<SqlitePool> {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        kftray_commons::utils::db::create_db_table(&pool)
            .await
            .unwrap();
        kftray_commons::utils::migration::migrate_configs(Some(&pool))
            .await
            .unwrap();

        let arc_pool = Arc::new(pool);

        // Set this as the global pool for command functions to use
        let _ = kftray_commons::utils::db::DB_POOL.set(arc_pool.clone());

        arc_pool
    }

    #[tokio::test]
    async fn test_delete_config_cmd() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config = Config::default();
        insert_config_cmd(config)
            .await
            .expect("Failed to insert test config");

        let configs = get_configs_cmd().await.expect("Failed to get configs");
        let id = configs[0].id.expect("Config should have an ID");

        let result = delete_config_cmd(id).await;
        assert!(result.is_ok(), "Delete config command should succeed");

        let configs_after = get_configs_cmd()
            .await
            .expect("Failed to get configs after deletion");
        assert!(
            !configs_after.iter().any(|c| c.id == Some(id)),
            "Config should have been deleted"
        );
    }

    #[tokio::test]
    async fn test_delete_configs_cmd() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config1 = Config {
            service: Some("test-service-1".to_string()),
            ..Config::default()
        };
        let config2 = Config {
            service: Some("test-service-2".to_string()),
            ..Config::default()
        };

        insert_config_cmd(config1)
            .await
            .expect("Failed to insert test config 1");
        insert_config_cmd(config2)
            .await
            .expect("Failed to insert test config 2");

        let configs = get_configs_cmd().await.expect("Failed to get configs");
        let ids: Vec<i64> = configs.iter().map(|c| c.id.unwrap()).collect();

        let result = delete_configs_cmd(ids).await;
        assert!(result.is_ok(), "Delete configs command should succeed");

        let configs_after = get_configs_cmd()
            .await
            .expect("Failed to get configs after deletion");
        assert!(
            configs_after.is_empty(),
            "All configs should have been deleted"
        );
    }

    #[tokio::test]
    async fn test_delete_all_configs_cmd() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config1 = Config::default();
        let config2 = Config::default();

        insert_config_cmd(config1)
            .await
            .expect("Failed to insert test config 1");
        insert_config_cmd(config2)
            .await
            .expect("Failed to insert test config 2");

        let configs_before = get_configs_cmd().await.expect("Failed to get configs");
        assert!(
            !configs_before.is_empty(),
            "Should have test configs before deletion"
        );

        let result = delete_all_configs_cmd().await;
        assert!(result.is_ok(), "Delete all configs command should succeed");

        let configs_after = get_configs_cmd()
            .await
            .expect("Failed to get configs after deletion");
        assert!(
            configs_after.is_empty(),
            "All configs should have been deleted"
        );
    }

    #[tokio::test]
    async fn test_insert_config_cmd() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let test_config = Config {
            service: Some("insert-test-service".to_string()),
            namespace: "insert-test-namespace".to_string(),
            ..Config::default()
        };

        let result = insert_config_cmd(test_config.clone()).await;
        assert!(result.is_ok(), "Insert config command should succeed");

        let configs = get_configs_cmd()
            .await
            .expect("Failed to get configs after insertion");
        assert!(
            configs
                .iter()
                .any(|c| c.service == Some("insert-test-service".to_string())
                    && c.namespace == "insert-test-namespace"),
            "Config should have been inserted"
        );
    }

    #[tokio::test]
    async fn test_get_configs_cmd() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config = Config {
            service: Some("get-configs-test".to_string()),
            ..Config::default()
        };

        insert_config_cmd(config)
            .await
            .expect("Failed to insert test config");

        let result = get_configs_cmd().await;
        assert!(result.is_ok(), "Get configs command should succeed");

        let configs = result.expect("Failed to get configs");
        assert!(!configs.is_empty(), "Should have at least one config");
        assert!(
            configs
                .iter()
                .any(|c| c.service == Some("get-configs-test".to_string())),
            "Should find the inserted config"
        );
    }

    #[tokio::test]
    async fn test_get_config_cmd() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config = Config {
            service: Some("get-config-test".to_string()),
            ..Config::default()
        };

        insert_config_cmd(config)
            .await
            .expect("Failed to insert test config");

        let configs = get_configs_cmd().await.expect("Failed to get configs");
        let test_config = configs
            .iter()
            .find(|c| c.service == Some("get-config-test".to_string()))
            .expect("Should find the inserted config");
        let id = test_config.id.expect("Config should have an ID");

        let result = get_config_cmd(id).await;
        assert!(result.is_ok(), "Get config command should succeed");

        let config = result.expect("Failed to get specific config");
        assert_eq!(
            config.id,
            Some(id),
            "Retrieved config should have correct ID"
        );
        assert_eq!(
            config.service,
            Some("get-config-test".to_string()),
            "Retrieved config should have correct service name"
        );
    }

    #[tokio::test]
    async fn test_update_config_cmd() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config = Config {
            service: Some("update-test-original".to_string()),
            ..Config::default()
        };

        insert_config_cmd(config)
            .await
            .expect("Failed to insert test config");

        let configs = get_configs_cmd().await.expect("Failed to get configs");
        let mut test_config = configs
            .iter()
            .find(|c| c.service == Some("update-test-original".to_string()))
            .expect("Should find the inserted config")
            .clone();

        test_config.service = Some("update-test-modified".to_string());

        let result = update_config_cmd(test_config.clone()).await;
        assert!(result.is_ok(), "Update config command should succeed");

        let updated_config = get_config_cmd(test_config.id.unwrap())
            .await
            .expect("Failed to get updated config");
        assert_eq!(
            updated_config.service,
            Some("update-test-modified".to_string()),
            "Config should have been updated with new service name"
        );
    }

    #[tokio::test]
    async fn test_export_configs_cmd() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config = Config {
            service: Some("export-test-service".to_string()),
            namespace: "export-test-namespace".to_string(),
            ..Config::default()
        };

        insert_config_cmd(config)
            .await
            .expect("Failed to insert test config");

        let result = export_configs_cmd().await;
        assert!(result.is_ok(), "Export configs command should succeed");

        let exported_json = result.expect("Failed to export configs");
        assert!(
            exported_json.contains("export-test-service"),
            "Exported JSON should contain service name"
        );
        assert!(
            exported_json.contains("export-test-namespace"),
            "Exported JSON should contain namespace"
        );
    }

    #[tokio::test]
    async fn test_import_configs_cmd() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let test_config_json = serde_json::json!([{
            "service": "import-test-service",
            "namespace": "import-test-namespace",
            "local_port": 5000,
            "workload_type": "service",
            "protocol": "tcp",
            "context": "test-context"
        }])
        .to_string();

        let result = import_configs_cmd(test_config_json).await;
        assert!(result.is_ok(), "Import configs command should succeed");

        let configs = get_configs_cmd()
            .await
            .expect("Failed to get configs after import");
        assert!(
            configs
                .iter()
                .any(|c| c.service == Some("import-test-service".to_string())
                    && c.namespace == "import-test-namespace"
                    && c.local_port == Some(5000)),
            "Imported config should exist in the database"
        );
    }

    #[tokio::test]
    async fn test_import_configs_cmd_error() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let invalid_json = "{\"service\": \"malformed\",";

        let result = import_configs_cmd(invalid_json.to_string()).await;
        assert!(result.is_err(), "Import with invalid JSON should fail");
    }

    #[test]
    fn test_validate_config_auto_loopback_only() {
        let config = Config {
            auto_loopback_address: true,
            local_address: None,
            ..Config::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_manual_address_only() {
        let config = Config {
            auto_loopback_address: false,
            local_address: Some("127.0.0.1".to_string()),
            ..Config::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_both_set_warning() {
        let config = Config {
            auto_loopback_address: true,
            local_address: Some("127.0.0.1".to_string()),
            ..Config::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[tokio::test]
    async fn test_get_configs_cmd_format() {
        let _pool = setup_isolated_test_db().await;
        let _ = get_configs_cmd().await;
    }

    #[tokio::test]
    async fn test_get_config_cmd_format() {
        let _pool = setup_isolated_test_db().await;
        let id = 123;
        let _ = get_config_cmd(id).await;
    }

    #[tokio::test]
    async fn test_delete_config_cmd_format() {
        let _pool = setup_isolated_test_db().await;
        let id = 123;
        let _ = delete_config_cmd(id).await;
    }
}
