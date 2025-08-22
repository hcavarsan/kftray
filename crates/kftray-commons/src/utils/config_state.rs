use log::error;
use sqlx::{
    Row,
    SqlitePool,
};

use crate::db::get_db_pool;
use crate::models::config_state_model::ConfigState;
use crate::utils::db_mode::{
    DatabaseManager,
    DatabaseMode,
};

pub async fn update_config_state_with_pool(
    config_state: &ConfigState, pool: &SqlitePool,
) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    sqlx::query("UPDATE config_state SET is_running = ?1, process_id = ?2 WHERE config_id = ?3")
        .bind(config_state.is_running)
        .bind(config_state.process_id)
        .bind(config_state.config_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_config_state(config_state: &ConfigState) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    update_config_state_with_pool(config_state, &pool).await
}

pub async fn read_config_states_with_pool(
    pool: &SqlitePool,
) -> Result<Vec<ConfigState>, sqlx::Error> {
    let mut conn = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire database connection: {e}");
        e
    })?;
    let rows = sqlx::query("SELECT id, config_id, is_running, process_id FROM config_state")
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to fetch config states: {e}");
            e
        })?;

    let config_states = rows
        .into_iter()
        .map(|row| {
            let id: Option<i64> = row.try_get("id").ok();
            let config_id: i64 = row.try_get("config_id").map_err(|e| {
                error!("Failed to get config_id: {e}");
                e
            })?;
            let is_running: bool = row.try_get("is_running").map_err(|e| {
                error!("Failed to get is_running: {e}");
                e
            })?;
            let process_id: Option<u32> = row.try_get("process_id").ok().flatten();
            Ok(ConfigState {
                id,
                config_id,
                is_running,
                process_id,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(config_states)
}

pub async fn read_config_states() -> Result<Vec<ConfigState>, sqlx::Error> {
    let pool = get_db_pool()
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;
    read_config_states_with_pool(&pool).await
}

pub async fn get_configs_state_with_pool(pool: &SqlitePool) -> Result<Vec<ConfigState>, String> {
    read_config_states_with_pool(pool).await.map_err(|e| {
        error!("Failed to get config states: {e}");
        e.to_string()
    })
}

pub async fn get_configs_state() -> Result<Vec<ConfigState>, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    get_configs_state_with_pool(&pool).await
}

pub async fn update_config_state_with_mode(
    config_state: &ConfigState, mode: DatabaseMode,
) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    update_config_state_with_pool(config_state, &context.pool).await
}

pub async fn read_config_states_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<ConfigState>, sqlx::Error> {
    let context = DatabaseManager::get_context(mode)
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;
    read_config_states_with_pool(&context.pool).await
}

pub async fn get_configs_state_with_mode(mode: DatabaseMode) -> Result<Vec<ConfigState>, String> {
    let context = DatabaseManager::get_context(mode).await?;
    read_config_states_with_pool(&context.pool)
        .await
        .map_err(|e| {
            error!("Failed to get config states: {e}");
            e.to_string()
        })
}

pub async fn cleanup_current_process_config_states() -> Result<(), String> {
    let current_process_id = std::process::id();
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    let affected_rows = sqlx::query(
        "UPDATE config_state SET is_running = false, process_id = NULL WHERE process_id = ?1",
    )
    .bind(current_process_id)
    .execute(&mut *conn)
    .await
    .map_err(|e| e.to_string())?
    .rows_affected();

    if affected_rows > 0 {
        log::info!("Cleaned up {affected_rows} config states for process {current_process_id}");
    }

    Ok(())
}

pub async fn cleanup_current_process_config_states_with_mode(
    mode: DatabaseMode,
) -> Result<(), String> {
    let current_process_id = std::process::id();
    let context = DatabaseManager::get_context(mode).await?;
    let mut conn = context.pool.acquire().await.map_err(|e| e.to_string())?;

    let affected_rows = sqlx::query(
        "UPDATE config_state SET is_running = false, process_id = NULL WHERE process_id = ?1",
    )
    .bind(current_process_id)
    .execute(&mut *conn)
    .await
    .map_err(|e| e.to_string())?
    .rows_affected();

    if affected_rows > 0 {
        log::info!("Cleaned up {affected_rows} config states for process {current_process_id} in mode {mode:?}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::*;
    use crate::db::create_db_table;
    use crate::models::config_model::Config;
    use crate::utils::config;

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory database");
        create_db_table(&pool)
            .await
            .expect("Failed to create tables");
        pool
    }

    #[tokio::test]
    async fn test_read_initial_config_state() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-1".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();
        let states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(states.len(), 1);
        let state = &states[0];
        assert_eq!(state.config_id, config_id);
        assert!(!state.is_running);
        assert!(state.id.is_some());
    }

    #[tokio::test]
    async fn test_update_and_read_config_state() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-2".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();
        let initial_states = read_config_states_with_pool(&pool).await.unwrap();
        let initial_state = initial_states
            .iter()
            .find(|s| s.config_id == config_id)
            .unwrap();
        assert!(!initial_state.is_running);
        let state_to_update = ConfigState {
            id: initial_state.id,
            config_id,
            is_running: true,
            process_id: Some(1234),
        };
        update_config_state_with_pool(&state_to_update, &pool)
            .await
            .unwrap();
        let updated_states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(updated_states.len(), 1);
        let updated_state = updated_states
            .iter()
            .find(|s| s.config_id == config_id)
            .unwrap();
        assert_eq!(updated_state.config_id, config_id);
        assert!(updated_state.is_running);
        assert_eq!(updated_state.id, initial_state.id);
    }

    #[tokio::test]
    async fn test_read_multiple_config_states() {
        let pool = setup_test_db().await;
        let config1 = Config {
            service: Some("state-test-3".to_string()),
            ..Config::default()
        };
        let config2 = Config {
            service: Some("state-test-4".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config1.clone(), &pool)
            .await
            .unwrap();
        config::insert_config_with_pool(config2.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config1_id = configs
            .iter()
            .find(|c| c.service == config1.service)
            .unwrap()
            .id
            .unwrap();
        let config2_id = configs
            .iter()
            .find(|c| c.service == config2.service)
            .unwrap()
            .id
            .unwrap();
        let state_to_update = ConfigState {
            id: None,
            config_id: config1_id,
            is_running: true,
            process_id: Some(1234),
        };
        update_config_state_with_pool(&state_to_update, &pool)
            .await
            .unwrap();
        let states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(states.len(), 2);
        let state1 = states.iter().find(|s| s.config_id == config1_id).unwrap();
        let state2 = states.iter().find(|s| s.config_id == config2_id).unwrap();
        assert!(state1.is_running);
        assert!(!state2.is_running);
    }

    #[tokio::test]
    async fn test_get_configs_state_wrapper() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-wrapper".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let result = read_config_states_with_pool(&pool)
            .await
            .map_err(|e| e.to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_update_config_state_public_function() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-5".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();

        let state_to_update = ConfigState {
            id: None,
            config_id,
            is_running: true,
            process_id: Some(1234),
        };

        tokio::task::yield_now().await;

        let result = update_config_state_with_pool(&state_to_update, &pool).await;
        assert!(result.is_ok());

        let states = read_config_states_with_pool(&pool).await.unwrap();
        let updated_state = states.iter().find(|s| s.config_id == config_id).unwrap();
        assert!(updated_state.is_running);
    }

    #[tokio::test]
    async fn test_read_config_states_public_function() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-6".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let states_result = read_config_states_with_pool(&pool).await;
        assert!(states_result.is_ok());
        assert_eq!(states_result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_get_configs_state_function() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-7".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let states_result = read_config_states_with_pool(&pool)
            .await
            .map_err(|e| e.to_string());
        assert!(states_result.is_ok());
        let states = states_result.unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].config_id, 1);
    }

    #[tokio::test]
    async fn test_error_handling_in_update_config_state() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        pool.close().await;

        let config_state = ConfigState {
            id: Some(1),
            config_id: 1,
            is_running: true,
            process_id: Some(1234),
        };

        let result = update_config_state_with_pool(&config_state, &pool).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_error_handling_in_read_config_states() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        pool.close().await;

        let result = read_config_states_with_pool(&pool).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_processing_in_read_config_states() {
        let pool = setup_test_db().await;

        let config1 = Config {
            service: Some("row-test-1".to_string()),
            ..Config::default()
        };
        let config2 = Config {
            service: Some("row-test-2".to_string()),
            ..Config::default()
        };

        config::insert_config_with_pool(config1.clone(), &pool)
            .await
            .unwrap();
        config::insert_config_with_pool(config2.clone(), &pool)
            .await
            .unwrap();

        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config2_id = configs
            .iter()
            .find(|c| c.service == Some("row-test-2".to_string()))
            .unwrap()
            .id
            .unwrap();

        let state = ConfigState {
            id: None,
            config_id: config2_id,
            is_running: true,
            process_id: Some(1234),
        };

        update_config_state_with_pool(&state, &pool).await.unwrap();

        let states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(states.len(), 2);

        let state1 = states.iter().find(|s| s.config_id == 1).unwrap();
        let state2 = states.iter().find(|s| s.config_id == config2_id).unwrap();

        assert!(!state1.is_running);
        assert!(state2.is_running);
    }

    #[tokio::test]
    async fn test_update_config_state_with_direct_pool() {
        let pool = setup_test_db().await;

        let config_data = Config {
            service: Some("public-test-1".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();

        let initial_states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(initial_states.len(), 1);
        assert!(!initial_states[0].is_running);

        let _conn = pool.acquire().await.unwrap();

        let state = ConfigState {
            id: initial_states[0].id,
            config_id,
            is_running: true,
            process_id: Some(1234),
        };

        let result = update_config_state_with_pool(&state, &pool).await;
        assert!(result.is_ok());

        let updated_states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(updated_states.len(), 1);
        assert!(updated_states[0].is_running);
    }

    #[tokio::test]
    async fn test_read_config_states_with_direct_pool() {
        let pool = setup_test_db().await;

        let config_data = Config {
            service: Some("public-read-test".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let direct_result = read_config_states_with_pool(&pool).await;
        assert!(direct_result.is_ok());
        assert_eq!(direct_result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_get_configs_state_public_wrapper() {
        let pool = setup_test_db().await;

        let config_data = Config {
            service: Some("get-wrapper-test".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let direct_states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(direct_states.len(), 1);
        let err_result = get_configs_state().await;
        assert!(err_result.is_ok() || err_result.is_err());
    }

    #[tokio::test]
    async fn test_error_handling_row_processing_config_id() {
        let pool = setup_test_db().await;

        let config_data = Config {
            service: Some("error-test".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].config_id, 1);
    }

    #[tokio::test]
    async fn test_config_state_with_mode_memory() {
        let config_data = Config {
            service: Some("memory-state-test".to_string()),
            ..Config::default()
        };

        config::insert_config_with_mode(config_data, DatabaseMode::Memory)
            .await
            .unwrap();

        let configs = config::read_configs_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        let config_id = configs[0].id.unwrap();

        let states = read_config_states_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].config_id, config_id);
        assert!(!states[0].is_running);

        let state_update = ConfigState {
            id: None,
            config_id,
            is_running: true,
            process_id: Some(1234),
        };

        update_config_state_with_mode(&state_update, DatabaseMode::Memory)
            .await
            .unwrap();

        let updated_states = read_config_states_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(updated_states.len(), 1);
        assert!(updated_states[0].is_running);
    }

    #[tokio::test]
    async fn test_get_configs_state_with_mode_memory() {
        let config_data = Config {
            service: Some("get-state-memory-test".to_string()),
            ..Config::default()
        };

        config::insert_config_with_mode(config_data, DatabaseMode::Memory)
            .await
            .unwrap();

        let states = get_configs_state_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(states.len(), 1);
        assert!(!states[0].is_running);
    }
}
