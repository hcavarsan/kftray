use hostsfile::HostsBuilder;
use log::{
    error,
    info,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use sqlx::Row;

use crate::db::get_db_pool;
use crate::migration::migrate_configs;
use crate::models::config::Config;

fn is_value_blank(value: &JsonValue) -> bool {
    match value {
        JsonValue::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

fn is_value_default(value: &serde_json::Value, default_config: &serde_json::Value) -> bool {
    *value == *default_config
}

fn remove_blank_or_default_fields(value: &mut JsonValue, default_config: &JsonValue) {
    match value {
        JsonValue::Object(map) => {
            let keys_to_remove: Vec<String> = map
                .iter()
                .filter(|(k, v)| {
                    let default_v = &default_config[k];
                    is_value_blank(v)
                        || (default_v != &JsonValue::Array(vec![])
                            && is_value_default(v, default_v))
                })
                .map(|(k, _)| k.clone())
                .collect();

            for key in keys_to_remove {
                map.remove(&key);
            }

            for value in map.values_mut() {
                remove_blank_or_default_fields(value, default_config);
            }
        }
        JsonValue::Array(arr) => {
            for value in arr {
                remove_blank_or_default_fields(value, default_config);
            }
        }
        _ => (),
    }
}

// Function to delete a config from the database
#[tauri::command]
pub async fn delete_config(id: i64) -> Result<(), String> {
    info!("Deleting config with id: {}", id);

    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM configs WHERE id = ?1")
        .bind(id)
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Failed to delete config: {}", e))?;

    Ok(())
}

// Function to delete multiple configs from the database
#[tauri::command]
pub async fn delete_configs(ids: Vec<i64>) -> Result<(), String> {
    info!("Deleting configs with ids: {:?}", ids);

    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut transaction = pool.begin().await.map_err(|e| e.to_string())?;

    for id in ids {
        sqlx::query("DELETE FROM configs WHERE id = ?1")
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| format!("Failed to delete config with id {}: {}", id, e))?;
    }

    transaction.commit().await.map_err(|e| e.to_string())?;

    Ok(())
}

// Function to delete all configs from the database
#[tauri::command]
pub async fn delete_all_configs() -> Result<(), String> {
    info!("Deleting all configs");

    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM configs")
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Failed to delete all configs: {}", e))?;

    Ok(())
}

// Function to insert a config into the database
#[tauri::command]
pub async fn insert_config(config: Config) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS configs (
            id INTEGER PRIMARY KEY,
            data TEXT NOT NULL
        )",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;

    let data = json!(config).to_string();

    sqlx::query("INSERT INTO configs (data) VALUES (?1)")
        .bind(data)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

async fn read_configs() -> Result<Vec<Config>, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    let rows = sqlx::query("SELECT id, data FROM configs")
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    let mut configs = Vec::new();

    for row in rows {
        let id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
        let data: String = row.try_get("data").map_err(|e| e.to_string())?;
        let mut config: Config =
            serde_json::from_str(&data).map_err(|_| "Failed to decode config".to_string())?;
        config.id = Some(id);
        configs.push(config);
    }

    info!("Reading configs {:?}", configs);

    Ok(configs)
}

pub async fn clean_all_custom_hosts_entries() -> Result<(), String> {
    let configs = read_configs().await.map_err(|e| e.to_string())?;

    for config in configs {
        let hostfile_comment = format!(
            "kftray custom host for {} - {}",
            config.service.unwrap_or_default(),
            config.id.unwrap_or_default()
        );

        let hosts_builder = HostsBuilder::new(&hostfile_comment);

        hosts_builder.write().map_err(|e| {
            format!(
                "Failed to write to the hostfile for {}: {}",
                hostfile_comment, e
            )
        })?;
    }

    Ok(())
}

// Function to get all configs from the database
#[tauri::command]
pub async fn get_configs() -> Result<Vec<Config>, String> {
    info!("get_configs called");

    let configs = read_configs().await.map_err(|e| e.to_string())?;

    info!("{:?}", configs);

    Ok(configs)
}

// Function to get a config from the database
#[tauri::command]
pub async fn get_config(id: i64) -> Result<Config, String> {
    info!("get_config called with id: {}", id);

    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    let row = sqlx::query("SELECT id, data FROM configs WHERE id = ?1")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    match row {
        Some(row) => {
            let id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
            let data: String = row.try_get("data").map_err(|e| e.to_string())?;
            let mut config: Config = serde_json::from_str(&data)
                .map_err(|e| format!("Failed to parse config: {}", e))?;
            config.id = Some(id);
            info!("{:?}", config);
            Ok(config)
        }
        None => Err(format!("No config found with id: {}", id)),
    }
}

// Function to update a config in the database
#[tauri::command]
pub async fn update_config(config: Config) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    let data = json!(config).to_string();

    sqlx::query("UPDATE configs SET data = ?1 WHERE id = ?2")
        .bind(data)
        .bind(config.id.unwrap())
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

// Function to export configs to a JSON file
#[tauri::command]
pub async fn export_configs() -> Result<String, String> {
    let mut configs = read_configs().await.map_err(|e| e.to_string())?;

    for config in &mut configs {
        config.id = None;
    }

    let mut json_config = serde_json::to_value(configs).map_err(|e| e.to_string())?;
    let default_config = serde_json::to_value(Config::default()).map_err(|e| e.to_string())?;
    remove_blank_or_default_fields(&mut json_config, &default_config);

    let json = serde_json::to_string(&json_config).map_err(|e| e.to_string())?;

    Ok(json)
}

// Function to import configs from a JSON file
#[tauri::command]
pub async fn import_configs(json: String) -> Result<(), String> {
    match serde_json::from_str::<Vec<Config>>(&json) {
        Ok(configs) => {
            for config in configs {
                insert_config(config)
                    .await
                    .map_err(|e| format!("Failed to insert config: {}", e))?;
            }
        }
        Err(_) => {
            let config = serde_json::from_str::<Config>(&json)
                .map_err(|e| format!("Failed to parse config: {}", e))?;
            insert_config(config)
                .await
                .map_err(|e| format!("Failed to insert config: {}", e))?;
        }
    }

    if let Err(e) = migrate_configs().await {
        error!("Error migrating configs: {}. Please check if the configurations are valid and compatible with the current system/version.", e);
        return Err(format!("Error migrating configs: {}", e));
    }

    Ok(())
}

#[cfg(test)]

mod tests {

    use super::*;

    #[test]

    fn test_is_value_blank() {
        assert!(is_value_blank(&json!("")));

        assert!(!is_value_blank(&json!("not blank")));

        assert!(!is_value_blank(&json!(0)));

        assert!(!is_value_blank(&json!(false)));
    }

    // Test `remove_blank_fields` function
    #[test]
    fn test_remove_blank_or_default_fields() {
        let mut obj = json!({
            "name": "Test",
            "empty_string": "   ",
            "nested": {
                "blank": "",
                "non_blank": "value"
            },
            "array": [
                {
                    "blank_field": "  "
                }
            ]
        });

        let default_config = json!({
            "name": "",
            "empty_string": "",
            "nested": {
                "blank": "",
                "non_blank": ""
            },
            "array": [
                {
                    "blank_field": ""
                }
            ]
        });

        remove_blank_or_default_fields(&mut obj, &default_config);

        assert!(obj.get("empty_string").is_none());

        assert!(obj.get("nested").unwrap().get("blank").is_none());

        assert_eq!(
            obj.get("nested").unwrap().get("non_blank"),
            Some(&json!("value"))
        );

        assert!(obj.get("array").unwrap()[0].get("blank_field").is_none());
    }
}
