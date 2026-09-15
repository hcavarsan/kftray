use std::sync::Arc;

use kftray_commons::config::{
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
use kftray_portforward::kube::clear_stopped_by_timeout;
use kftray_portforward::ssl::cert_manager::CertificateManager;
use log::{
    error,
    info,
    warn,
};

use crate::init_check::{
    PortOperations,
    RealPortOperations,
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
                        restart_ssl_proxies_with_retry(Arc::new(RealPortOperations)).await;
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

/// Restarts the SSL-enabled candidates found among `target_ids` (or, when
/// `None`, among the currently-running configs). Returns `Ok(None)` when no
/// SSL-enabled candidate was found at all, and `Ok(Some(failed_ids))`
/// otherwise, `failed_ids` being empty when every candidate that was
/// actually attempted restarted successfully. Callers must not conflate the
/// two: an empty `target_ids` pass that finds nothing is not evidence
/// everything is running, only that nothing was found yet.
async fn restart_ssl_proxies_if_running(
    port_ops: &Arc<dyn PortOperations>, target_ids: Option<&[i64]>,
) -> Result<Option<Vec<i64>>, String> {
    use kftray_commons::utils::config_state::get_configs_state;

    info!("=== Starting SSL proxy restart process ===");

    // On a retry, restart exactly the configs that failed last time: a
    // config that failed to restart typically no longer shows as running
    // in config_state, so re-deriving the candidate set from "currently
    // running" would drop it and keep re-restarting whatever DID succeed.
    let running_config_ids: Vec<i64> = if let Some(ids) = target_ids {
        ids.to_vec()
    } else {
        let config_states = get_configs_state()
            .await
            .map_err(|e| format!("Failed to get config states: {}", e))?;

        info!("Found {} total config states", config_states.len());

        config_states
            .iter()
            .filter(|state| state.is_running)
            .map(|state| state.config_id)
            .collect()
    };

    info!("Candidate config IDs: {:?}", running_config_ids);

    if running_config_ids.is_empty() {
        info!("No candidate configs found, no SSL proxies to restart");
        return Ok(None);
    }

    // Get all configs to filter for SSL-enabled ones
    let all_configs = get_configs()
        .await
        .map_err(|e| format!("Failed to get configs: {}", e))?;

    info!("Found {} total configs in database", all_configs.len());

    let ssl_configs: Vec<Config> = all_configs
        .into_iter()
        .filter(|config| {
            let is_candidate = config.id.is_some_and(|id| running_config_ids.contains(&id));
            let is_ssl_enabled = config.domain_enabled.unwrap_or(false);
            info!(
                "Config {}: candidate={}, ssl_enabled={}, alias={:?}",
                config.id.unwrap_or(-1),
                is_candidate,
                is_ssl_enabled,
                config.alias
            );
            is_candidate && is_ssl_enabled
        })
        .collect();

    info!(
        "Filtered to {} SSL-enabled candidate configs",
        ssl_configs.len()
    );

    if ssl_configs.is_empty() {
        info!("No SSL-enabled candidate configs found, no SSL proxies to restart");
        return Ok(None);
    }

    info!("Found {} SSL-enabled configs to restart", ssl_configs.len());

    // Stop and restart each SSL-enabled config through the same
    // workload/protocol dispatch the global shortcuts use: a proxy or udp
    // config restarted through a plain tcp port-forward would come back on
    // the wrong forwarding path.
    let mut failed_ids: Vec<i64> = Vec::new();
    for config in &ssl_configs {
        let id = config.id.unwrap();
        info!(
            "Restarting SSL proxy for config {} ({})",
            id,
            config.alias.as_deref().unwrap_or("unnamed")
        );

        // A candidate may already be stopped from a previous retry attempt
        // (its stop succeeded last time but the start failed) or may never
        // have started; only require a successful stop when it is still
        // registered, and always attempt the start below so a down
        // candidate keeps getting retried instead of being skipped forever.
        if port_ops.is_forward_registered(id).await {
            if let Err(e) = port_ops.stop_port_forward(id).await {
                warn!("Failed to stop port forward for config {}: {}", id, e);
                failed_ids.push(id);
                continue;
            }

            // Small delay to ensure clean shutdown
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        match port_ops.dispatch_start(config).await {
            Ok(()) => info!("Successfully restarted SSL proxy for config {}", id),
            Err(e) => {
                warn!("Failed to restart port forward for config {}: {}", id, e);
                failed_ids.push(id);
            }
        }
    }

    info!("Completed SSL proxy restart process");
    Ok(Some(failed_ids))
}

async fn restart_ssl_proxies_with_retry(port_ops: Arc<dyn PortOperations>) {
    // Try multiple times with increasing delays to catch configs as they start up
    let delays = [100, 500, 1000]; // milliseconds
    let mut target_ids: Option<Vec<i64>> = None;

    for (attempt, delay) in delays.iter().enumerate() {
        info!(
            "SSL proxy restart attempt {} (after {}ms delay)",
            attempt + 1,
            delay
        );
        tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;

        match restart_ssl_proxies_if_running(&port_ops, target_ids.as_deref()).await {
            Ok(Some(failed_ids)) if failed_ids.is_empty() => {
                info!(
                    "SSL proxy restart attempt {} completed successfully",
                    attempt + 1
                );
                return;
            }
            Ok(Some(failed_ids)) => {
                warn!(
                    "SSL proxy restart attempt {} left {} config(s) failing: {:?}",
                    attempt + 1,
                    failed_ids.len(),
                    failed_ids
                );
                target_ids = Some(failed_ids);
            }
            Ok(None) => {
                // No SSL-enabled candidate was found at all: auto-start may
                // not have marked the configs running yet. Keep retrying
                // rather than mistaking this for "everything is running".
                info!(
                    "SSL proxy restart attempt {} found no running candidates yet",
                    attempt + 1
                );
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
    // Deleting only removes the database row, so a forward that is running,
    // starting or still being cleaned up would be left with nothing to stop it
    // by. Coordinated in the backend under the lifecycle lock, because
    // shortcuts start forwards without going through the interface.
    let result = kftray_portforward::kube::delete_configs_if_idle(
        &[id],
        kftray_commons::utils::db_mode::DatabaseMode::File,
        || async move {
            clear_stopped_by_timeout(id);
            delete_config(id).await
        },
    )
    .await;
    if result.is_ok() {
        let _ = regenerate_ssl_certificate_if_needed().await;
    }
    result
}

#[tauri::command]
pub async fn delete_configs_cmd(ids: Vec<i64>) -> Result<(), String> {
    info!("Deleting configs with ids: {ids:?}");
    let targets = ids.clone();
    let result = kftray_portforward::kube::delete_configs_if_idle(
        &targets,
        kftray_commons::utils::db_mode::DatabaseMode::File,
        || async move {
            for id in &ids {
                clear_stopped_by_timeout(*id);
            }
            delete_configs(ids).await
        },
    )
    .await;
    if result.is_ok() {
        let _ = regenerate_ssl_certificate_if_needed().await;
    }
    result
}

#[tauri::command]
pub async fn delete_all_configs_cmd() -> Result<(), String> {
    info!("Deleting all configs");
    let ids: Vec<i64> = get_configs()
        .await?
        .into_iter()
        .filter_map(|c| c.id)
        .collect();
    delete_configs_cmd(ids).await
}

#[tauri::command]
pub async fn insert_config_cmd(config: Config) -> Result<(), String> {
    validate_config(&config)?;
    let result = insert_config(config).await;
    if result.is_ok() {
        let _ = regenerate_ssl_certificate_if_needed().await;
    }
    result.map(|_| ())
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
    async fn deleting_enumerated_configs_leaves_a_row_added_after_enumeration() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        insert_config_cmd(Config::default())
            .await
            .expect("Failed to insert test config");
        let enumerated: Vec<i64> = get_configs_cmd()
            .await
            .expect("Failed to enumerate configs")
            .into_iter()
            .filter_map(|c| c.id)
            .collect();
        insert_config_cmd(Config {
            service: Some("added-after-enumeration".to_string()),
            ..Config::default()
        })
        .await
        .expect("Failed to insert the later config");

        delete_configs_cmd(enumerated)
            .await
            .expect("Deleting the enumerated configs should succeed");

        let configs_after = get_configs_cmd()
            .await
            .expect("Failed to get configs after deletion");
        assert_eq!(
            configs_after
                .iter()
                .filter_map(|c| c.service.as_deref())
                .collect::<Vec<_>>(),
            vec!["added-after-enumeration"],
            "only the enumerated rows may be deleted"
        );
    }

    #[tokio::test]
    async fn test_delete_all_configs_cmd_propagates_get_configs_error() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        // `delete_all_configs_cmd` used to call `get_configs().unwrap_or_default()`,
        // so a DB read failure enumerated zero ids and `delete_all_configs()`
        // still wiped every row unconditionally.
        let good_config = Config::default();
        insert_config_cmd(good_config)
            .await
            .expect("Failed to insert good config");

        let pool = kftray_commons::utils::db::get_db_pool()
            .await
            .expect("Failed to get db pool");

        // A row whose `data` column is not valid JSON makes `get_configs()`
        // fail without touching the pool itself, unlike closing it.
        sqlx::query("INSERT INTO configs (data) VALUES (?)")
            .bind("not json")
            .execute(&*pool)
            .await
            .expect("Failed to insert malformed row");

        let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM configs")
            .fetch_one(&*pool)
            .await
            .expect("Failed to count rows before delete");

        let result = delete_all_configs_cmd().await;

        let count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM configs")
            .fetch_one(&*pool)
            .await
            .expect("Failed to count rows after delete");

        // Clean up the malformed row unconditionally so it cannot poison
        // later tests that share this process-global DB pool.
        sqlx::query("DELETE FROM configs")
            .execute(&*pool)
            .await
            .expect("cleanup failed");

        assert!(
            result.is_err(),
            "delete_all_configs_cmd must propagate a get_configs failure instead of \
             silently deleting"
        );
        assert_eq!(
            count_before, count_after,
            "no rows should be deleted when config ids cannot be enumerated"
        );
    }

    #[tokio::test]
    async fn test_delete_all_configs_cmd_skips_running() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config = Config::default();
        insert_config_cmd(config)
            .await
            .expect("Failed to insert test config");

        let configs = get_configs_cmd().await.expect("Failed to get configs");
        let id = configs[0].id.expect("Config should have an ID");

        // `delete_all_configs_cmd` used to call `delete_all_configs()`
        // directly, deleting every row regardless of what is registered
        // here as a live forward.
        let handle: tokio::task::JoinHandle<anyhow::Result<()>> = tokio::spawn(async { Ok(()) });
        kftray_portforward::port_forward::CHILD_PROCESSES.insert(
            id,
            kftray_portforward::port_forward::PortForwardProcess::new(handle, id.to_string()),
        );

        let result = delete_all_configs_cmd().await;
        kftray_portforward::port_forward::CHILD_PROCESSES.remove(&id);

        assert!(
            result.is_err(),
            "Delete all configs should refuse to delete a config with a live forward"
        );

        let configs_after = get_configs_cmd()
            .await
            .expect("Failed to get configs after refused deletion");
        assert!(
            configs_after.iter().any(|c| c.id == Some(id)),
            "Config with a live forward must not have been deleted"
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
        assert!(
            result.is_ok(),
            "Import configs command should succeed. Error: {:?}",
            result.err()
        );

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

    #[tokio::test]
    async fn test_restart_ssl_proxies_starts_already_down_config() {
        // Regression: a config that stopped cleanly on a previous retry
        // attempt (so it is no longer registered) must still be started
        // here, not skipped because a stop was attempted and failed with
        // "no process found".
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config = Config {
            alias: Some("ssl-restart-down-candidate".to_string()),
            domain_enabled: Some(true),
            ..Config::default()
        };
        insert_config_cmd(config)
            .await
            .expect("Failed to insert test config");

        // The global DB pool is a process-wide OnceCell shared by every
        // test in this binary (only the first `setup_isolated_test_db`
        // call actually wins), so other tests' rows can still be present;
        // find this test's own row by its unique alias rather than
        // assuming it is the only or first one.
        let configs = get_configs_cmd().await.expect("Failed to get configs");
        let id = configs
            .iter()
            .find(|c| c.alias.as_deref() == Some("ssl-restart-down-candidate"))
            .and_then(|c| c.id)
            .expect("inserted config should be present");

        let mut mock = crate::init_check::MockPortOperations::new();
        mock.expect_is_forward_registered()
            .with(mockall::predicate::eq(id))
            .times(1)
            .returning(|_| false);
        mock.expect_stop_port_forward().times(0);
        mock.expect_dispatch_start()
            .withf(move |config: &Config| config.id == Some(id))
            .times(1)
            .returning(|_| Ok(()));

        let port_ops: Arc<dyn PortOperations> = Arc::new(mock);

        let failed_ids = restart_ssl_proxies_if_running(&port_ops, Some(&[id]))
            .await
            .expect("restart should succeed")
            .expect("a target id list must always yield a candidate result");

        assert!(
            failed_ids.is_empty(),
            "a down candidate must still be started, not skipped"
        );
    }

    #[tokio::test]
    async fn test_restart_ssl_proxies_with_retry_does_not_give_up_before_auto_start_catches_up() {
        // Regression: the first retry attempt runs before auto-start has
        // marked the config running, so `restart_ssl_proxies_if_running`
        // finds no candidates and returns `Ok(vec![])`. Treating that as
        // "all restarted" ended the loop before a later attempt could see
        // the config once it actually started running.
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config = Config {
            alias: Some("ssl-retry-not-yet-running".to_string()),
            domain_enabled: Some(true),
            ..Config::default()
        };
        insert_config_cmd(config)
            .await
            .expect("Failed to insert test config");

        let configs = get_configs_cmd().await.expect("Failed to get configs");
        let id = configs
            .iter()
            .find(|c| c.alias.as_deref() == Some("ssl-retry-not-yet-running"))
            .and_then(|c| c.id)
            .expect("inserted config should be present");

        let mut mock = crate::init_check::MockPortOperations::new();
        mock.expect_is_forward_registered()
            .with(mockall::predicate::eq(id))
            .times(1)
            .returning(|_| false);
        mock.expect_dispatch_start()
            .withf(move |config: &Config| config.id == Some(id))
            .times(1)
            .returning(|_| Ok(()));

        let port_ops: Arc<dyn PortOperations> = Arc::new(mock);

        // The first retry attempt fires after the 100ms delay; mark the
        // config running only after that, so it is invisible to the first
        // attempt and must be picked up by a later one instead of the loop
        // having already returned.
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            kftray_commons::utils::config_state::update_config_state(
                &kftray_commons::models::config_state_model::ConfigState::new_without_process(
                    id, true,
                ),
            )
            .await
            .expect("Failed to mark config running");
        });

        // `mock`'s expectations (`times(1)` each) are verified when it is
        // dropped; a premature return from the loop leaves
        // `dispatch_start` unmet and panics there.
        restart_ssl_proxies_with_retry(port_ops).await;
    }
}
