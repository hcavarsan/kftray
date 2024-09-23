use std::collections::BTreeMap;

use hostsfile::HostsBuilder;
use log::error;
use portpicker::pick_unused_port;
use serde_json::{
    self,
    Value,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use sqlx::Row;

use crate::db::get_db_pool;
use crate::migration::migrate_configs;
use crate::models::config_model::Config;

pub async fn delete_config(id: i64) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM configs WHERE id = ?1")
        .bind(id)
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Failed to delete config: {}", e))?;

    Ok(())
}

pub async fn delete_configs(ids: Vec<i64>) -> Result<(), String> {
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

pub async fn delete_all_configs() -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM configs")
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Failed to delete all configs: {}", e))?;

    Ok(())
}

pub async fn insert_config(config: Config) -> Result<(), String> {
    let config = prepare_config(config);

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

pub async fn read_configs() -> Result<Vec<Config>, String> {
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

pub async fn get_configs() -> Result<Vec<Config>, String> {
    read_configs().await
}

pub async fn get_config(id: i64) -> Result<Config, String> {
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
            Ok(config)
        }
        None => Err(format!("No config found with id: {}", id)),
    }
}

pub async fn update_config(config: Config) -> Result<(), String> {
    let config = prepare_config(config);

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

pub async fn export_configs() -> Result<String, String> {
    let mut configs = read_configs().await.map_err(|e| e.to_string())?;

    for config in &mut configs {
        config.id = None;
    }

    let mut json_config = serde_json::to_value(configs).map_err(|e| e.to_string())?;
    let default_config = serde_json::to_value(Config::default()).map_err(|e| e.to_string())?;
    remove_blank_or_default_fields(&mut json_config, &default_config);

    let sorted_configs: Vec<BTreeMap<String, Value>> =
        serde_json::from_value(json_config).map_err(|e| e.to_string())?;

    let json = serde_json::to_string_pretty(&sorted_configs).map_err(|e| e.to_string())?;

    Ok(json)
}

pub async fn import_configs(json: String) -> Result<(), String> {
    let configs: Vec<Config> = match serde_json::from_str(&json) {
        Ok(configs) => configs,
        Err(e) => {
            error!("Failed to parse JSON as Vec<Config>: {}", e);
            let config = serde_json::from_str::<Config>(&json)
                .map_err(|e| format!("Failed to parse config: {}", e))?;
            vec![config]
        }
    };

    for config in configs {
        insert_config(config)
            .await
            .map_err(|e| format!("Failed to insert config: {}", e))?;
    }

    if let Err(e) = migrate_configs().await {
        return Err(format!("Error migrating configs: {}", e));
    }

    Ok(())
}

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

fn prepare_config(mut config: Config) -> Config {
    if let Some(ref mut alias) = config.alias {
        *alias = alias.trim().to_string();
    }
    if let Some(ref mut kubeconfig) = config.kubeconfig {
        *kubeconfig = kubeconfig.trim().to_string();
    }

    if config.local_port == Some(0) || config.local_port.is_none() {
        match pick_unused_port() {
            Some(port) => config.local_port = Some(port),
            None => {
                config.local_port = config.remote_port;
                error!("Failed to find an unused port, using remote_port as local_port");
            }
        }
    }

    if config.alias.as_deref() == Some("") || config.alias.is_none() {
        let alias = format!(
            "{}-{}-{}",
            config.workload_type,
            config.protocol,
            config.local_port.unwrap_or_default()
        );
        config.alias = Some(alias);
    }

    if config.kubeconfig.as_deref() == Some("") || config.kubeconfig.is_none() {
        config.kubeconfig = Some("default".to_string());
    }

    config
}
