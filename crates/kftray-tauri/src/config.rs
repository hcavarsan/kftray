use hostsfile::HostsBuilder;
use rusqlite::{
    params,
    types::ToSql,
    Connection,
    Result,
};
use serde_json::{
    json,
    Value as JsonValue,
};

use crate::models::config::Config;
use crate::utils::config_dir::get_db_file_path;

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

//  function to delete a config from the database
#[tauri::command]

pub async fn delete_config(id: i64) -> Result<(), String> {
    println!("Deleting config with id: {}", id);

    let db_dir = get_db_file_path()?;

    let conn = match Connection::open(db_dir) {
        Ok(conn) => conn,
        Err(e) => return Err(format!("Failed to open database: {}", e)),
    };

    match conn.execute("DELETE FROM configs WHERE id=?1", params![id]) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to delete config: {}", e)),
    }
}

// function to delete multiple configs from the database
#[tauri::command]

pub async fn delete_configs(ids: Vec<i64>) -> Result<(), String> {
    println!("Deleting configs with ids: {:?}", ids);

    let db_dir = get_db_file_path()?;

    let mut conn = Connection::open(db_dir).map_err(|e| e.to_string())?;

    let transaction = conn.transaction().map_err(|e| e.to_string())?;

    for id in ids {
        transaction
            .execute("DELETE FROM configs WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete config with id {}: {}", id, e))?;
    }

    transaction.commit().map_err(|e| e.to_string())?;

    Ok(())
}

// function to delete all configs from the database
#[tauri::command]

pub async fn delete_all_configs() -> Result<(), String> {
    println!("Deleting all configs");

    let db_dir = get_db_file_path()?;

    let conn = Connection::open(db_dir).map_err(|e| e.to_string())?;

    match conn.execute("DELETE FROM configs", params![]) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to delete all configs: {}", e)),
    }
}

// function to insert a config into the database
#[tauri::command]

pub fn insert_config(config: Config) -> Result<(), String> {
    let db_dir = get_db_file_path()?;

    let conn = Connection::open(db_dir).map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS configs (
                  id INTEGER PRIMARY KEY,
                  data TEXT NOT NULL
                  )",
        params![],
    )
    .map_err(|e| e.to_string())?;

    let data = json!(config).to_string();

    conn.execute("INSERT INTO configs (data) VALUES (?1)", params![data])
        .map_err(|e| e.to_string())?;

    Ok(())
}

// function to read configs from the database
fn read_configs() -> Result<Vec<Config>, rusqlite::Error> {
    let db_dir = get_db_file_path().map_err(|e| rusqlite::Error::InvalidPath(e.into()))?;

    let conn = Connection::open(db_dir)?;

    let mut stmt = conn.prepare("SELECT id, data FROM configs")?;

    let rows = stmt.query_map(params![], |row| {
        let id: i64 = row.get(0)?;

        let data: String = row.get(1)?;

        let mut config: Config =
            serde_json::from_str(&data).map_err(|_| rusqlite::Error::QueryReturnedNoRows)?;

        config.id = Some(id);

        Ok(config)
    })?;

    let mut configs = Vec::new();

    for row in rows {
        configs.push(row?);
    }

    println!("Reading configs {:?}", configs);

    Ok(configs)
}

pub fn clean_all_custom_hosts_entries() -> Result<(), String> {
    let configs = read_configs().map_err(|e| e.to_string())?;

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

// function to get all configs from the database
#[tauri::command]

pub async fn get_configs() -> Result<Vec<Config>, String> {
    println!("get_configs called");

    let configs = read_configs().map_err(|e| e.to_string())?;

    println!("{:?}", configs);

    Ok(configs)
}

// function to get a config from the database
#[tauri::command]

pub async fn get_config(id: i64) -> Result<Config, String> {
    println!("get_config called with id: {}", id);

    let db_dir = get_db_file_path()?;

    let conn = Connection::open(db_dir).map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT id, data FROM configs WHERE id = ?1")
        .map_err(|e| e.to_string())?;

    let mut rows = stmt
        .query_map(params![id], |row| {
            // For `row.get`, we directly use `rusqlite::Result` with `?`.
            let _id: i64 = row.get(0)?;

            let data: String = row.get(1)?;

            // The error from `serde_json` is converted to a `rusqlite::Error` before using
            // `?`.
            let config: Config = serde_json::from_str(&data)
                .map_err(|_e| rusqlite::Error::ExecuteReturnedResults)?;

            Ok(config)
        })
        .map_err(|e| e.to_string())?;

    match rows.next() {
        Some(row_result) => {
            let mut config = row_result.map_err(|e| e.to_string())?;

            config.id = Some(id);

            println!("{:?}", config);

            Ok(config)
        }
        None => Err(format!("No config found with id: {}", id)),
    }
}

// function to update a config in the database
#[tauri::command]

pub fn update_config(config: Config) -> Result<(), String> {
    let db_dir = get_db_file_path()?;

    let conn = Connection::open(db_dir).map_err(|e| e.to_string())?;

    let data = json!(config).to_string();

    conn.execute(
        "UPDATE configs SET data = ?1 WHERE id = ?2",
        params![data, config.id.unwrap()],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

// function to export configs to a json file
#[tauri::command]
pub async fn export_configs() -> Result<String, String> {
    let mut configs = read_configs().map_err(|e| e.to_string())?;

    for config in &mut configs {
        config.id = None; // Ensure that the id is None before exporting
    }

    let mut json_config = serde_json::to_value(configs).map_err(|e| e.to_string())?;
    let default_config = serde_json::to_value(Config::default()).map_err(|e| e.to_string())?;
    remove_blank_or_default_fields(&mut json_config, &default_config);

    let json = serde_json::to_string(&json_config).map_err(|e| e.to_string())?;

    Ok(json)
}

// function to import configs from a json file
#[tauri::command]

pub async fn import_configs(json: String) -> Result<(), String> {
    match serde_json::from_str::<Vec<Config>>(&json) {
        Ok(configs) => {
            for config in configs {
                insert_config(config).map_err(|e| format!("Failed to insert config: {}", e))?;
            }
        }
        Err(_) => {
            let config = serde_json::from_str::<Config>(&json)
                .map_err(|e| format!("Failed to parse config: {}", e))?;

            insert_config(config).map_err(|e| format!("Failed to insert config: {}", e))?;
        }
    }

    if let Err(e) = migrate_configs() {
        eprintln!("Error migrating configs: {}. Please check if the configurations are valid and compatible with the current system/version.", e);

        return Err(format!("Error migrating configs: {}", e));
    }

    Ok(())
}

pub fn migrate_configs() -> Result<(), String> {
    let db_dir = get_db_file_path()?;

    let mut conn = Connection::open(db_dir).map_err(|e| e.to_string())?;

    let transaction = conn.transaction().map_err(|e| e.to_string())?;

    {
        let mut stmt = transaction
            .prepare("SELECT id, data FROM configs")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        for (id, data) in rows {
            let config_json: JsonValue = serde_json::from_str(&data).map_err(|e| e.to_string())?;

            let default_config_json =
                serde_json::to_value(Config::default()).map_err(|e| e.to_string())?;

            let merged_config_json = merge_json_values(default_config_json, config_json);

            let updated_data =
                serde_json::to_string(&merged_config_json).map_err(|e| e.to_string())?;

            transaction
                .execute(
                    "UPDATE configs SET data = ?1 WHERE id = ?2",
                    [&updated_data as &dyn ToSql, &id],
                )
                .map_err(|e| e.to_string())?;
        }
    }

    transaction.commit().map_err(|e| e.to_string())?;

    Ok(())
}

fn merge_json_values(default: JsonValue, mut config: JsonValue) -> JsonValue {
    match (&default, &mut config) {
        (JsonValue::Object(default_map), JsonValue::Object(config_map)) => {
            for (key, default_value) in default_map {
                #[allow(clippy::redundant_pattern_matching)]
                let should_replace = matches!(config_map.get(key), None);

                if should_replace {
                    config_map.insert(key.clone(), default_value.clone());

                    continue;
                }

                config_map
                    .entry(key.clone())
                    .and_modify(|e| *e = merge_json_values(default_value.clone(), e.clone()));
            }
        }
        (JsonValue::Null, _) => return default,
        _ => (),
    }

    config
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
    #[test]

    fn test_merge_json_values() {
        let default = json!({
            "field1": "default value",
            "field2": null,
            "nested": {
                "nested_field1": "nested default"
            }
        });

        let to_merge = json!({
            "field2": "overridden value",
            "nested": {}
        });

        let merged = merge_json_values(default, to_merge);

        assert_eq!(merged["field1"], "default value");

        assert_eq!(merged["field2"], "overridden value");

        assert_eq!(merged["nested"]["nested_field1"], "nested default");
    }
}
