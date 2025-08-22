use kftray_commons::config_state::get_configs_state;
use kftray_commons::models::config_state_model::ConfigState;

#[tauri::command]
pub async fn get_config_states() -> Result<Vec<ConfigState>, String> {
    log::info!("get_configs state called");
    let configs = get_configs_state().await?;
    log::info!("{configs:?}");
    Ok(configs)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kftray_commons::models::config_model::Config;
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
    async fn test_get_config_states() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config1 = Config {
            service: Some("config-state-test-1".to_string()),
            ..Config::default()
        };

        let config2 = Config {
            service: Some("config-state-test-2".to_string()),
            ..Config::default()
        };

        kftray_commons::config::insert_config(config1)
            .await
            .expect("Failed to insert test config 1");
        kftray_commons::config::insert_config(config2)
            .await
            .expect("Failed to insert test config 2");

        let configs = get_config_states()
            .await
            .expect("Failed to get config states");

        assert_eq!(configs.len(), 2, "Should have two config states");

        for config in configs.iter() {
            assert!(
                !config.is_running,
                "Config should not be running by default"
            );
            assert!(config.id.is_some(), "Config state should have an ID");
            assert!(
                config.config_id > 0,
                "Config state should have a valid config_id"
            );
        }
    }

    #[tokio::test]
    async fn test_get_config_states_with_running_state() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config = Config {
            service: Some("config-state-running-test".to_string()),
            ..Config::default()
        };

        if kftray_commons::config::insert_config(config).await.is_ok() {
            if let Ok(initial_states) = get_config_states().await {
                if let Some(test_state) = initial_states.first() {
                    let mut update_state = test_state.clone();
                    update_state.is_running = true;
                    let _ = kftray_commons::config_state::update_config_state(&update_state).await;

                    if let Ok(updated_states) = get_config_states().await {
                        if let Some(updated_state) = updated_states
                            .iter()
                            .find(|s| s.config_id == test_state.config_id)
                        {
                            assert!(
                                updated_state.is_running,
                                "Config state should now be running"
                            );
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_get_config_states_with_no_configs() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let states = get_config_states()
            .await
            .expect("Failed to get config states");
        assert!(
            states.is_empty(),
            "Should have no config states when no configs exist"
        );
    }
}
