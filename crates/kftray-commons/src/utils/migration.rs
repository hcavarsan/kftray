use log::{
    error,
    info,
};
use serde_json::Value as JsonValue;
use sqlx::{
    Acquire,
    Row,
    Sqlite,
    SqliteConnection,
    Transaction,
};

use crate::db::get_db_pool;
use crate::models::config_model::Config;

pub async fn migrate_configs() -> Result<(), String> {
    info!("Starting configuration migration.");
    let pool = get_db_pool().await.map_err(|e| {
        error!("Failed to get DB pool: {}", e);
        e.to_string()
    })?;
    let mut conn = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire connection: {}", e);
        e.to_string()
    })?;
    let mut transaction = conn.begin().await.map_err(|e| {
        error!("Failed to begin transaction: {}", e);
        e.to_string()
    })?;

    let rows = sqlx::query("SELECT id, data FROM configs")
        .fetch_all(&mut *transaction)
        .await
        .map_err(|e| {
            error!("Failed to fetch configs: {}", e);
            e.to_string()
        })?;

    for row in rows {
        let id: i64 = row.try_get("id").map_err(|e| {
            error!("Failed to get id: {}", e);
            e.to_string()
        })?;
        let data: String = row.try_get("data").map_err(|e| {
            error!("Failed to get data: {}", e);
            e.to_string()
        })?;
        let config_json: JsonValue = serde_json::from_str(&data).map_err(|e| {
            error!("Failed to parse JSON: {}", e);
            e.to_string()
        })?;
        let default_config_json = serde_json::to_value(Config::default()).map_err(|e| {
            error!("Failed to serialize default config: {}", e);
            e.to_string()
        })?;
        let merged_config_json = merge_json_values(default_config_json, config_json);
        let updated_data = serde_json::to_string(&merged_config_json).map_err(|e| {
            error!("Failed to serialize merged config: {}", e);
            e.to_string()
        })?;

        sqlx::query("UPDATE configs SET data = ?1 WHERE id = ?2")
            .bind(updated_data)
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| {
                error!("Failed to update config: {}", e);
                e.to_string()
            })?;
    }

    drop_triggers(&mut transaction).await.map_err(|e| {
        error!("Failed to drop triggers: {}", e);
        e.to_string()
    })?;

    sqlx::query(
        "INSERT INTO config_state (id, config_id, is_running)
         SELECT c.id, c.id, false
         FROM configs c
         LEFT JOIN config_state cs ON c.id = cs.config_id
         WHERE cs.config_id IS NULL",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| {
        error!("Failed to insert into config_state: {}", e);
        e.to_string()
    })?;

    transaction.commit().await.map_err(|e| {
        error!("Failed to commit transaction: {}", e);
        e.to_string()
    })?;

    let mut conn = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire connection: {}", e);
        e.to_string()
    })?;
    create_triggers(&mut conn).await.map_err(|e| {
        error!("Failed to create triggers: {}", e);
        e.to_string()
    })?;

    info!("Configuration migration completed successfully.");
    Ok(())
}

async fn drop_triggers(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    info!("Dropping triggers.");
    sqlx::query("DROP TRIGGER IF EXISTS after_insert_config;")
        .execute(&mut **transaction)
        .await
        .map_err(|e| {
            error!("Failed to drop after_insert_config trigger: {}", e);
            e
        })?;

    sqlx::query("DROP TRIGGER IF EXISTS after_delete_config;")
        .execute(&mut **transaction)
        .await
        .map_err(|e| {
            error!("Failed to drop after_delete_config trigger: {}", e);
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
        error!("Failed to create after_insert_config trigger: {}", e);
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
        error!("Failed to create after_delete_config trigger: {}", e);
        e
    })?;

    info!("Triggers created successfully.");
    Ok(())
}
pub fn merge_json_values(default: JsonValue, custom: JsonValue) -> JsonValue {
    match (default, custom) {
        (JsonValue::Object(mut default_map), JsonValue::Object(custom_map)) => {
            for (key, custom_value) in custom_map {
                let default_value = default_map.entry(key.clone()).or_insert(JsonValue::Null);
                *default_value = merge_json_values(default_value.take(), custom_value);
            }
            JsonValue::Object(default_map)
        }
        (_, custom) => custom,
    }
}
