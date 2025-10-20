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
    use kftray_commons::models::config_model::Config;

    async fn setup_isolated_test_db() {
        let _ = kftray_commons::config::delete_all_configs_with_mode(
            kftray_commons::utils::db_mode::DatabaseMode::Memory,
        )
        .await;
    }

    #[tokio::test]
    async fn test_get_config_states() {
        use kftray_commons::utils::db_mode::DatabaseMode;

        let _guard = kftray_commons::test_utils::MEMORY_MODE_TEST_MUTEX
            .lock()
            .await;
        setup_isolated_test_db().await;

        let config1 = Config {
            service: Some("config-state-test-1".to_string()),
            ..Config::default()
        };

        let config2 = Config {
            service: Some("config-state-test-2".to_string()),
            ..Config::default()
        };

        kftray_commons::config::insert_config_with_mode(config1, DatabaseMode::Memory)
            .await
            .expect("Failed to insert test config 1");
        kftray_commons::config::insert_config_with_mode(config2, DatabaseMode::Memory)
            .await
            .expect("Failed to insert test config 2");

        let configs =
            kftray_commons::config_state::get_configs_state_with_mode(DatabaseMode::Memory)
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
        use kftray_commons::utils::db_mode::DatabaseMode;

        let _guard = kftray_commons::test_utils::MEMORY_MODE_TEST_MUTEX
            .lock()
            .await;
        setup_isolated_test_db().await;

        let config = Config {
            service: Some("config-state-running-test".to_string()),
            ..Config::default()
        };

        if kftray_commons::config::insert_config_with_mode(config, DatabaseMode::Memory)
            .await
            .is_ok()
            && let Ok(initial_states) =
                kftray_commons::config_state::get_configs_state_with_mode(DatabaseMode::Memory)
                    .await
            && let Some(test_state) = initial_states.first()
        {
            let mut update_state = test_state.clone();
            update_state.is_running = true;
            let _ = kftray_commons::config_state::update_config_state_with_mode(
                &update_state,
                DatabaseMode::Memory,
            )
            .await;

            if let Ok(updated_states) =
                kftray_commons::config_state::get_configs_state_with_mode(DatabaseMode::Memory)
                    .await
                && let Some(updated_state) = updated_states
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

    #[tokio::test]
    async fn test_get_config_states_with_no_configs() {
        use kftray_commons::utils::db_mode::DatabaseMode;

        let _guard = kftray_commons::test_utils::MEMORY_MODE_TEST_MUTEX
            .lock()
            .await;
        setup_isolated_test_db().await;

        let states =
            kftray_commons::config_state::get_configs_state_with_mode(DatabaseMode::Memory)
                .await
                .expect("Failed to get config states");
        assert!(
            states.is_empty(),
            "Should have no config states when no configs exist"
        );
    }
}
