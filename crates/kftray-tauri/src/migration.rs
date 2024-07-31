use serde_json::Value as JsonValue;
use sqlx::{
    Acquire,
    Row,
    Sqlite,
    SqliteConnection,
    Transaction,
};

use crate::db::get_db_pool;
use crate::models::config::Config;

pub async fn migrate_configs() -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let mut transaction = conn.begin().await.map_err(|e| e.to_string())?;

    let rows = sqlx::query("SELECT id, data FROM configs")
        .fetch_all(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;

    for row in rows {
        let id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
        let data: String = row.try_get("data").map_err(|e| e.to_string())?;
        let config_json: JsonValue = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        let default_config_json =
            serde_json::to_value(Config::default()).map_err(|e| e.to_string())?;
        let merged_config_json = merge_json_values(default_config_json, config_json);
        let updated_data = serde_json::to_string(&merged_config_json).map_err(|e| e.to_string())?;

        sqlx::query("UPDATE configs SET data = ?1 WHERE id = ?2")
            .bind(updated_data)
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
    }

    drop_triggers(&mut transaction)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS config_state_new (
            id INTEGER PRIMARY KEY,
            config_id INTEGER NOT NULL,
            is_running BOOLEAN NOT NULL DEFAULT false,
            FOREIGN KEY(config_id) REFERENCES configs(id) ON DELETE CASCADE
        )",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        "INSERT INTO config_state_new (id, config_id, is_running)
         SELECT id, config_id, is_running FROM config_state",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query("DROP TABLE IF EXISTS config_state")
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("ALTER TABLE config_state_new RENAME TO config_state")
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;

    transaction.commit().await.map_err(|e| e.to_string())?;

    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    create_triggers(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

async fn drop_triggers(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    sqlx::query("DROP TRIGGER IF EXISTS after_insert_config;")
        .execute(&mut **transaction)
        .await?;

    sqlx::query("DROP TRIGGER IF EXISTS after_delete_config;")
        .execute(&mut **transaction)
        .await?;

    Ok(())
}

async fn create_triggers(conn: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS after_insert_config
         AFTER INSERT ON configs
         FOR EACH ROW
         BEGIN
             INSERT INTO config_state (config_id, is_running) VALUES (NEW.id, false);
         END;",
    )
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS after_delete_config
         AFTER DELETE ON configs
         FOR EACH ROW
         BEGIN
             DELETE FROM config_state WHERE config_id = OLD.id;
         END;",
    )
    .execute(&mut *conn)
    .await?;

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
