use log::{error, info};
use serde_json::Value as JsonValue;
use sqlx::SqlitePool;
use sqlx::{Acquire, Row, Sqlite, SqliteConnection, Transaction};

use crate::db::get_db_pool;
use crate::models::config_model::Config;

async fn migrate_configs_with_pool(pool: &SqlitePool) -> Result<(), String> {
    info!("Starting configuration migration with provided pool.");
    let mut conn = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire connection: {e}");
        e.to_string()
    })?;
    let mut transaction = conn.begin().await.map_err(|e| {
        error!("Failed to begin transaction: {e}");
        e.to_string()
    })?;

    let rows = sqlx::query("SELECT id, data FROM configs")
        .fetch_all(&mut *transaction)
        .await
        .map_err(|e| {
            error!("Failed to fetch configs: {e}");
            e.to_string()
        })?;

    for row in rows {
        let id: i64 = row.try_get("id").map_err(|e| {
            error!("Failed to get id: {e}");
            e.to_string()
        })?;
        let data: String = row.try_get("data").map_err(|e| {
            error!("Failed to get data: {e}");
            e.to_string()
        })?;
        let config_json: JsonValue = serde_json::from_str(&data).map_err(|e| {
            error!("Failed to parse JSON: {e}");
            e.to_string()
        })?;
        let default_config_json = serde_json::to_value(Config::default()).map_err(|e| {
            error!("Failed to serialize default config: {e}");
            e.to_string()
        })?;
        let merged_config_json = merge_json_values(default_config_json, config_json);
        let updated_data = serde_json::to_string(&merged_config_json).map_err(|e| {
            error!("Failed to serialize merged config: {e}");
            e.to_string()
        })?;

        sqlx::query("UPDATE configs SET data = ?1 WHERE id = ?2")
            .bind(updated_data)
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| {
                error!("Failed to update config: {e}");
                e.to_string()
            })?;
    }

    drop_triggers(&mut transaction).await.map_err(|e| {
        error!("Failed to drop triggers: {e}");
        e.to_string()
    })?;

    sqlx::query(
        "INSERT INTO config_state (config_id, is_running)
         SELECT c.id, false
         FROM configs c
         LEFT JOIN config_state cs ON c.id = cs.config_id
         WHERE cs.config_id IS NULL",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| {
        error!("Failed to insert into config_state: {e}");
        e.to_string()
    })?;

    transaction.commit().await.map_err(|e| {
        error!("Failed to commit transaction: {e}");
        e.to_string()
    })?;

    let mut conn_for_triggers = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire connection for creating triggers: {e}");
        e.to_string()
    })?;
    create_triggers(&mut conn_for_triggers).await.map_err(|e| {
        error!("Failed to create triggers: {e}");
        e.to_string()
    })?;

    info!("Configuration migration with provided pool completed successfully.");
    Ok(())
}

pub async fn migrate_configs(pool_opt: Option<&SqlitePool>) -> Result<(), String> {
    info!("Starting configuration migration check.");
    let pool_result = match pool_opt {
        Some(p) => Ok(p.clone()),
        None => get_db_pool().await.map(|arc_pool| (*arc_pool).clone()),
    };
    let pool = pool_result.map_err(|e| {
        error!("Failed to get DB pool for migration: {e}");
        e.to_string()
    })?;

    migrate_schema(&pool).await?;

    migrate_configs_with_pool(&pool).await
}

async fn migrate_schema(pool: &SqlitePool) -> Result<(), String> {
    info!("Running schema migrations");
    let mut conn = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire connection for schema migration: {e}");
        e.to_string()
    })?;

    let column_exists = sqlx::query(
        "SELECT COUNT(*) as count FROM pragma_table_info('config_state') WHERE name = 'process_id'",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to check process_id column: {e}");
        e.to_string()
    })?
    .get::<i64, _>("count")
        > 0;

    if !column_exists {
        info!("Adding process_id column to config_state table");
        sqlx::query("ALTER TABLE config_state ADD COLUMN process_id INTEGER")
            .execute(&mut *conn)
            .await
            .map_err(|e| {
                error!("Failed to add process_id column: {e}");
                e.to_string()
            })?;
        info!("Successfully added process_id column");
    } else {
        info!("process_id column already exists, skipping migration");
    }

    migrate_http_logs_config_table(&mut conn).await?;

    migrate_shortcuts_table(&mut conn).await?;

    Ok(())
}

async fn migrate_http_logs_config_table(conn: &mut sqlx::SqliteConnection) -> Result<(), String> {
    info!("Running HTTP logs configuration table migration");

    let table_exists = sqlx::query(
        "SELECT COUNT(*) as count FROM sqlite_master WHERE type='table' AND name='http_logs_config'",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to check http_logs_config table: {e}");
        e.to_string()
    })?
    .get::<i64, _>("count")
        > 0;

    if !table_exists {
        info!("Creating http_logs_config table");
        sqlx::query(
            "CREATE TABLE http_logs_config (
                config_id INTEGER PRIMARY KEY,
                enabled BOOLEAN NOT NULL DEFAULT false,
                max_file_size INTEGER DEFAULT 10485760,
                retention_days INTEGER DEFAULT 7,
                auto_cleanup BOOLEAN DEFAULT true,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(config_id) REFERENCES configs(id) ON DELETE CASCADE
            )",
        )
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to create http_logs_config table: {e}");
            e.to_string()
        })?;
        info!("Successfully created http_logs_config table");

        sqlx::query(
            "CREATE TRIGGER IF NOT EXISTS after_insert_config_http_logs
             AFTER INSERT ON configs
             FOR EACH ROW
             BEGIN
                 INSERT INTO http_logs_config (config_id, enabled) VALUES (NEW.id, false);
             END;",
        )
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to create after_insert_config_http_logs trigger: {e}");
            e.to_string()
        })?;
        info!("Successfully created after_insert_config_http_logs trigger");

        sqlx::query(
            "INSERT INTO http_logs_config (config_id, enabled)
             SELECT c.id, false
             FROM configs c
             LEFT JOIN http_logs_config hlc ON c.id = hlc.config_id
             WHERE hlc.config_id IS NULL",
        )
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to populate http_logs_config for existing configs: {e}");
            e.to_string()
        })?;
        info!("Successfully populated http_logs_config for existing configs");
    } else {
        info!("http_logs_config table already exists, skipping migration");
    }

    Ok(())
}

async fn migrate_shortcuts_table(conn: &mut sqlx::SqliteConnection) -> Result<(), String> {
    info!("Running shortcuts table migration");

    let table_exists = sqlx::query(
        "SELECT COUNT(*) as count FROM sqlite_master WHERE type='table' AND name='shortcuts'",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to check shortcuts table existence: {e}");
        e.to_string()
    })?
    .get::<i64, _>("count")
        > 0;

    if !table_exists {
        info!("Creating shortcuts table");
        sqlx::query(
            "CREATE TABLE shortcuts (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                shortcut_key TEXT NOT NULL,
                action_type TEXT NOT NULL,
                action_data TEXT,
                config_id INTEGER,
                enabled BOOLEAN NOT NULL DEFAULT true,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(config_id) REFERENCES configs(id) ON DELETE SET NULL
            )",
        )
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to create shortcuts table: {e}");
            e.to_string()
        })?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_shortcuts_config_id ON shortcuts(config_id)")
            .execute(&mut *conn)
            .await
            .map_err(|e| {
                error!("Failed to create shortcuts config_id index: {e}");
                e.to_string()
            })?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_shortcuts_shortcut_key ON shortcuts(shortcut_key)",
        )
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to create shortcuts shortcut_key index: {e}");
            e.to_string()
        })?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_shortcuts_enabled ON shortcuts(enabled)")
            .execute(&mut *conn)
            .await
            .map_err(|e| {
                error!("Failed to create shortcuts enabled index: {e}");
                e.to_string()
            })?;

        sqlx::query(
            "CREATE TRIGGER IF NOT EXISTS after_update_shortcuts
             AFTER UPDATE ON shortcuts
             FOR EACH ROW
             BEGIN
                 UPDATE shortcuts SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
             END;",
        )
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to create shortcuts update trigger: {e}");
            e.to_string()
        })?;

        info!("Migrating existing global shortcut setting to shortcuts table");
        let existing_shortcut =
            sqlx::query("SELECT value FROM settings WHERE key = 'global_shortcut'")
                .fetch_optional(&mut *conn)
                .await
                .map_err(|e| {
                    error!("Failed to fetch existing global shortcut: {e}");
                    e.to_string()
                })?;

        if let Some(row) = existing_shortcut {
            let shortcut_key: String = row.get("value");
            info!("Migrating existing global shortcut: {}", shortcut_key);

            sqlx::query(
                "INSERT INTO shortcuts (name, shortcut_key, action_type, enabled)
                 VALUES ('Toggle Window', ?, 'toggle_window', true)",
            )
            .bind(&shortcut_key)
            .execute(&mut *conn)
            .await
            .map_err(|e| {
                error!("Failed to migrate existing shortcut: {e}");
                e.to_string()
            })?;

            info!("Successfully migrated existing global shortcut to shortcuts table");
        } else {
            info!("Creating default toggle window shortcut");
            sqlx::query(
                "INSERT INTO shortcuts (name, shortcut_key, action_type, enabled)
                 VALUES ('Toggle Window', 'Ctrl+Shift+F1', 'toggle_window', true)",
            )
            .execute(&mut *conn)
            .await
            .map_err(|e| {
                error!("Failed to create default shortcut: {e}");
                e.to_string()
            })?;
        }

        info!("Successfully created shortcuts table and migrated data");
    } else {
        info!("shortcuts table already exists, skipping migration");
    }

    Ok(())
}

async fn drop_triggers(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    info!("Dropping triggers.");
    sqlx::query("DROP TRIGGER IF EXISTS after_insert_config;")
        .execute(&mut **transaction)
        .await
        .map_err(|e| {
            error!("Failed to drop after_insert_config trigger: {e}");
            e
        })?;

    sqlx::query("DROP TRIGGER IF EXISTS after_delete_config;")
        .execute(&mut **transaction)
        .await
        .map_err(|e| {
            error!("Failed to drop after_delete_config trigger: {e}");
            e
        })?;

    sqlx::query("DROP TRIGGER IF EXISTS after_insert_config_http_logs;")
        .execute(&mut **transaction)
        .await
        .map_err(|e| {
            error!("Failed to drop after_insert_config_http_logs trigger: {e}");
            e
        })?;

    info!("Triggers dropped successfully.");
    Ok(())
}

async fn create_triggers(conn: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    info!("Creating triggers.");
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS after_insert_config
         AFTER INSERT ON configs
         FOR EACH ROW
         BEGIN
             INSERT INTO config_state (config_id, is_running) VALUES (NEW.id, false);
         END;",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to create after_insert_config trigger: {e}");
        e
    })?;

    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS after_delete_config
         AFTER DELETE ON configs
         FOR EACH ROW
         BEGIN
             DELETE FROM config_state WHERE config_id = OLD.id;
         END;",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to create after_delete_config trigger: {e}");
        e
    })?;

    let http_logs_table_exists = sqlx::query(
        "SELECT COUNT(*) as count FROM sqlite_master WHERE type='table' AND name='http_logs_config'"
    )
    .fetch_one(&mut *conn)
    .await?
    .get::<i64, _>("count") > 0;

    if http_logs_table_exists {
        sqlx::query(
            "CREATE TRIGGER IF NOT EXISTS after_insert_config_http_logs
             AFTER INSERT ON configs
             FOR EACH ROW
             BEGIN
                 INSERT INTO http_logs_config (config_id, enabled) VALUES (NEW.id, false);
             END;",
        )
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to create after_insert_config_http_logs trigger: {e}");
            e
        })?;
    } else {
        info!("Skipping after_insert_config_http_logs — http_logs_config table not found");
    }

    info!("Triggers created successfully.");
    Ok(())
}

pub fn merge_json_values(default: JsonValue, custom: JsonValue) -> JsonValue {
    match (default, custom) {
        (JsonValue::Object(mut default_map), JsonValue::Object(custom_map)) => {
            for (key, custom_value) in custom_map {
                if let Some(default_value) = default_map.get_mut(&key) {
                    *default_value = merge_json_values(default_value.take(), custom_value);
                } else {
                    default_map.insert(key, custom_value);
                }
            }
            JsonValue::Object(default_map)
        }
        (_, custom) => custom,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlx::SqlitePool;

    use super::*;
    use crate::db::create_db_table;
    use crate::models::config_model::Config;
    use crate::utils::config::{insert_config_with_pool, read_configs_with_pool};
    use crate::utils::config_state::read_config_states_with_pool;

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory database");
        create_db_table(&pool)
            .await
            .expect("Failed to create tables");
        migrate_configs(Some(&pool))
            .await
            .expect("Failed to run migrations");
        pool
    }

    async fn check_trigger_exists(pool: &SqlitePool, trigger_name: &str) -> bool {
        let query = format!(
            "SELECT name FROM sqlite_master WHERE type='trigger' AND name='{trigger_name}'"
        );
        sqlx::query(&query)
            .fetch_optional(pool)
            .await
            .unwrap()
            .is_some()
    }

    #[test]
    fn test_merge_json_values_simple() {
        let default = json!({ "a": 1, "b": 2 });
        let custom = json!({ "b": 3, "c": 4 });
        let merged = merge_json_values(default, custom);
        assert_eq!(merged, json!({ "a": 1, "b": 3, "c": 4 }));
    }

    #[test]
    fn test_merge_json_values_nested() {
        let default = json!({ "user": { "name": "Default", "age": 30 } });
        let custom = json!({ "user": { "age": 31, "city": "Testville" } });
        let merged = merge_json_values(default, custom);
        assert_eq!(
            merged,
            json!({ "user": { "name": "Default", "age": 31, "city": "Testville" } })
        );
    }

    #[test]
    fn test_merge_json_values_custom_overwrites_default_type() {
        let default = json!({ "value": 123 });
        let custom = json!({ "value": "string" });
        let merged = merge_json_values(default, custom);
        assert_eq!(merged, json!({ "value": "string" }));
    }

    #[test]
    fn test_merge_json_values_custom_is_not_object() {
        let default = json!({ "a": 1 });
        let custom = json!("string_value");
        let merged = merge_json_values(default, custom);
        assert_eq!(merged, json!("string_value"));
    }

    #[test]
    fn test_merge_json_values_default_is_not_object() {
        let default = json!(123);
        let custom = json!({ "a": 1 });
        let merged = merge_json_values(default, custom);
        assert_eq!(merged, json!({ "a": 1 }));
    }

    #[test]
    fn test_merge_json_values_with_null() {
        let default = json!({ "a": 1, "b": null });
        let custom = json!({ "b": 2, "c": null });
        let merged = merge_json_values(default, custom);
        assert_eq!(merged, json!({ "a": 1, "b": 2, "c": null }));

        let default2 = json!({ "a": 1, "b": 2 });
        let custom2 = json!({ "b": null });
        let merged2 = merge_json_values(default2, custom2);
        assert_eq!(merged2, json!({ "a": 1, "b": null }));
    }

    #[test]
    fn test_merge_json_values_arrays() {
        let default = json!({ "items": [1, 2] });
        let custom = json!({ "items": [3, 4] });
        let merged = merge_json_values(default, custom);
        assert_eq!(merged, json!({ "items": [3, 4] }));
    }

    #[test]
    fn test_merge_json_values_empty_objects() {
        let default = json!({});
        let custom = json!({ "a": 1 });
        let merged = merge_json_values(default, custom);
        assert_eq!(merged, json!({ "a": 1 }));

        let default2 = json!({ "a": 1 });
        let custom2 = json!({});
        let merged2 = merge_json_values(default2, custom2);
        assert_eq!(merged2, json!({ "a": 1 }));
    }

    #[test]
    fn test_merge_json_values_auto_loopback_address_default() {
        let default_config_json = serde_json::to_value(Config::default()).unwrap();
        let old_config_without_auto_loopback = json!({
            "service": "old-service",
            "namespace": "old-namespace"
        });

        let merged = merge_json_values(default_config_json, old_config_without_auto_loopback);

        assert_eq!(merged["service"], "old-service");
        assert_eq!(merged["namespace"], "old-namespace");

        // Deserialize to verify default values are applied correctly
        let merged_config: Config = serde_json::from_value(merged).unwrap();
        assert_eq!(merged_config.service, Some("old-service".to_string()));
        assert_eq!(merged_config.namespace, "old-namespace");
        assert!(!merged_config.auto_loopback_address);
    }

    #[tokio::test]
    async fn test_create_triggers() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect");
        sqlx::query("CREATE TABLE configs (id INTEGER PRIMARY KEY, data TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE config_state (id INTEGER PRIMARY KEY, config_id INTEGER NOT NULL, is_running BOOLEAN NOT NULL DEFAULT false, FOREIGN KEY(config_id) REFERENCES configs(id) ON DELETE CASCADE)")
            .execute(&pool).await.unwrap();

        assert!(!check_trigger_exists(&pool, "after_insert_config").await);
        assert!(!check_trigger_exists(&pool, "after_delete_config").await);

        let mut conn = pool.acquire().await.unwrap();
        create_triggers(&mut conn).await.unwrap();

        assert!(check_trigger_exists(&pool, "after_insert_config").await);
        assert!(check_trigger_exists(&pool, "after_delete_config").await);
    }

    #[tokio::test]
    async fn test_drop_triggers() {
        let pool = setup_test_db().await;

        // Verify triggers exist initially
        assert!(check_trigger_exists(&pool, "after_insert_config").await);
        assert!(check_trigger_exists(&pool, "after_delete_config").await);

        let mut transaction = pool.begin().await.unwrap();
        drop_triggers(&mut transaction).await.unwrap();
        transaction.commit().await.unwrap();

        assert!(!check_trigger_exists(&pool, "after_insert_config").await);
        assert!(!check_trigger_exists(&pool, "after_delete_config").await);
    }

    #[tokio::test]
    async fn test_migrate_configs_merges_and_updates_state() {
        let pool = setup_test_db().await;

        let old_config_json = json!({
            "service": "old-service",
            "namespace": "old-ns"
        })
        .to_string();
        let insert_res = sqlx::query("INSERT INTO configs (data) VALUES (?1)")
            .bind(&old_config_json)
            .execute(&pool)
            .await
            .unwrap();
        let old_config_id = insert_res.last_insert_rowid();

        let new_config_full_data = Config {
            service: Some("new-full".to_string()),
            ..Config::default()
        };
        insert_config_with_pool(new_config_full_data.clone(), &pool)
            .await
            .unwrap();

        let configs_temp = read_configs_with_pool(&pool).await.unwrap();
        let new_config_id = configs_temp
            .iter()
            .find(|c| c.id != Some(old_config_id))
            .unwrap()
            .id
            .unwrap();

        sqlx::query("DELETE FROM config_state WHERE config_id = ?1")
            .bind(old_config_id)
            .execute(&pool)
            .await
            .unwrap();

        let states_before = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(
            states_before.len(),
            1,
            "Expected 1 state before migration after manual delete"
        );
        assert_eq!(states_before[0].config_id, new_config_id);
        assert!(!states_before[0].is_running);
        assert!(check_trigger_exists(&pool, "after_insert_config").await);

        migrate_configs_with_pool(&pool).await.unwrap();

        let configs_after = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs_after.len(), 2);

        let migrated_old_config = configs_after
            .iter()
            .find(|c| c.id == Some(old_config_id))
            .unwrap();
        assert_eq!(migrated_old_config.service, Some("old-service".to_string()));
        assert_eq!(migrated_old_config.namespace, "old-ns".to_string());
        assert_ne!(
            migrated_old_config.protocol, "",
            "Default protocol should have been merged"
        );
        assert_eq!(migrated_old_config.protocol, Config::default().protocol);

        let migrated_new_config = configs_after
            .iter()
            .find(|c| c.id == Some(new_config_id))
            .unwrap();
        assert_eq!(migrated_new_config.service, new_config_full_data.service);
        assert_eq!(migrated_new_config.protocol, Config::default().protocol);

        let states_after = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(states_after.len(), 2, "Expected 2 states after migration");
        let old_config_state = states_after
            .iter()
            .find(|s| s.config_id == old_config_id)
            .unwrap();
        assert!(!old_config_state.is_running);
        let new_config_state = states_after
            .iter()
            .find(|s| s.config_id == new_config_id)
            .unwrap();
        assert!(!new_config_state.is_running);

        assert!(check_trigger_exists(&pool, "after_insert_config").await);
        assert!(check_trigger_exists(&pool, "after_delete_config").await);
    }

    #[tokio::test]
    async fn test_migrate_http_logs_config_table() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect");

        sqlx::query("CREATE TABLE configs (id INTEGER PRIMARY KEY, data TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();

        let mut conn = pool.acquire().await.unwrap();

        let table_exists_before = sqlx::query(
            "SELECT COUNT(*) as count FROM sqlite_master WHERE type='table' AND name='http_logs_config'"
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap()
        .get::<i64, _>("count") > 0;
        assert!(!table_exists_before);

        migrate_http_logs_config_table(&mut conn).await.unwrap();

        let table_exists_after = sqlx::query(
            "SELECT COUNT(*) as count FROM sqlite_master WHERE type='table' AND name='http_logs_config'"
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap()
        .get::<i64, _>("count") > 0;
        assert!(table_exists_after);

        assert!(check_trigger_exists(&pool, "after_insert_config_http_logs").await);

        let config_insert_result = sqlx::query("INSERT INTO configs (data) VALUES (?1)")
            .bind("{\"service\": \"test\"}")
            .execute(&mut *conn)
            .await
            .unwrap();
        let config_id = config_insert_result.last_insert_rowid();

        let http_logs_config =
            sqlx::query("SELECT config_id, enabled FROM http_logs_config WHERE config_id = ?1")
                .bind(config_id)
                .fetch_one(&mut *conn)
                .await
                .unwrap();

        assert_eq!(http_logs_config.get::<i64, _>("config_id"), config_id);
        assert!(!http_logs_config.get::<bool, _>("enabled"));

        migrate_http_logs_config_table(&mut conn).await.unwrap();

        let count = sqlx::query("SELECT COUNT(*) as count FROM http_logs_config")
            .fetch_one(&mut *conn)
            .await
            .unwrap()
            .get::<i64, _>("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_full_schema_migration_with_http_logs() {
        let pool = setup_test_db().await;

        let config_insert_result = sqlx::query("INSERT INTO configs (data) VALUES (?1)")
            .bind("{\"service\": \"test-service\"}")
            .execute(&pool)
            .await
            .unwrap();
        let config_id = config_insert_result.last_insert_rowid();

        migrate_configs(Some(&pool)).await.unwrap();

        let http_logs_config = sqlx::query(
            "SELECT config_id, enabled, max_file_size, retention_days, auto_cleanup FROM http_logs_config WHERE config_id = ?1"
        )
        .bind(config_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(http_logs_config.get::<i64, _>("config_id"), config_id);
        assert!(!http_logs_config.get::<bool, _>("enabled"));
        assert_eq!(http_logs_config.get::<i64, _>("max_file_size"), 10485760);
        assert_eq!(http_logs_config.get::<i64, _>("retention_days"), 7);
        assert!(http_logs_config.get::<bool, _>("auto_cleanup"));

        assert!(check_trigger_exists(&pool, "after_insert_config").await);
        assert!(check_trigger_exists(&pool, "after_delete_config").await);
        assert!(check_trigger_exists(&pool, "after_insert_config_http_logs").await);
    }
}
