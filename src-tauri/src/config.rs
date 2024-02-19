use kubeforward::port_forward::Config;
use rusqlite::types::ToSql;
use rusqlite::{params, Connection, Result};
use serde_json::to_value;
use serde_json::{json, Value as JsonValue};

fn is_value_blank(value: &JsonValue) -> bool {
    match value {
        JsonValue::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

fn remove_blank_fields(value: &mut JsonValue) {
    match value {
        JsonValue::Object(map) => {
            let keys_to_remove: Vec<String> = map
                .iter()
                .filter(|(_, v)| is_value_blank(v))
                .map(|(k, _)| k.clone())
                .collect();
            for key in keys_to_remove {
                map.remove(&key);
            }
            for value in map.values_mut() {
                remove_blank_fields(value);
            }
        }
        JsonValue::Array(arr) => {
            for value in arr {
                remove_blank_fields(value);
            }
        }
        _ => (),
    }
}

#[tauri::command]
pub async fn delete_config(id: i64) -> Result<(), String> {
    println!("Deleting config with id: {}", id);
    let home_dir = dirs::home_dir().unwrap();
    let db_dir = home_dir.to_str().unwrap().to_string() + "/.kftray/configs.db";
    let conn = match Connection::open(db_dir) {
        Ok(conn) => conn,
        Err(e) => return Err(format!("Failed to open database: {}", e)),
    };

    match conn.execute("DELETE FROM configs WHERE id=?1", params![id]) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to delete config: {}", e)),
    }
}

#[tauri::command]
pub fn insert_config(config: Config) -> Result<(), String> {
    let home_dir = dirs::home_dir().unwrap();
    let db_dir = home_dir.to_str().unwrap().to_string() + "/.kftray/configs.db";

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

fn read_configs() -> Result<Vec<Config>, rusqlite::Error> {
    let home_dir = dirs::home_dir().unwrap();
    let db_dir = home_dir.to_str().unwrap().to_string() + "/.kftray/configs.db";
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

#[tauri::command]
pub async fn get_configs() -> Result<Vec<Config>, String> {
    println!("get_configs called");
    let configs = read_configs().map_err(|e| e.to_string())?;
    println!("{:?}", configs);
    Ok(configs)
}

#[tauri::command]
pub async fn get_config(id: i64) -> Result<Config, String> {
    println!("get_config called with id: {}", id);
    let home_dir = dirs::home_dir().ok_or("Unable to determine home directory")?;
    let db_dir = format!("{}/.kftray/configs.db", home_dir.to_string_lossy());
    let conn = Connection::open(db_dir).map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT id, data FROM configs WHERE id = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params![id], |row| {
            // For `row.get`, we directly use `rusqlite::Result` with `?`.
            let _id: i64 = row.get(0)?;
            let data: String = row.get(1)?;
            // The error from `serde_json` is converted to a `rusqlite::Error` before using `?`.
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

#[tauri::command]
pub fn update_config(config: Config) -> Result<(), String> {
    let home_dir = dirs::home_dir().unwrap();
    let db_dir = home_dir.to_str().unwrap().to_string() + "/.kftray/configs.db";

    let conn = Connection::open(db_dir).map_err(|e| e.to_string())?;

    let data = json!(config).to_string();
    conn.execute(
        "UPDATE configs SET data = ?1 WHERE id = ?2",
        params![data, config.id.unwrap()],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn export_configs() -> Result<String, String> {
    let mut configs = read_configs().map_err(|e| e.to_string())?;
    for config in &mut configs {
        config.id = None; // Ensure that the id is None before exporting
    }

    let mut json_config = to_value(configs).map_err(|e| e.to_string())?;
    remove_blank_fields(&mut json_config);
    let json = serde_json::to_string(&json_config).map_err(|e| e.to_string())?;

    Ok(json)
}

#[tauri::command]
pub async fn import_configs(json: String) -> Result<(), String> {
    let parse_result = serde_json::from_str::<Vec<Config>>(&json);

    match parse_result {
        Ok(configs) => {
            for config in configs {
                insert_config(config)?;
            }
            Ok(())
        }
        Err(_) => {
            let config = serde_json::from_str::<Config>(&json)
                .map_err(|e| format!("Failed to parse config: {}", e))?;
            insert_config(config)?;
            Ok(())
        }
    }
}

pub fn migrate_configs() -> Result<(), String> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| "Unable to determine home directory".to_owned())?;
    let db_dir = format!("{}/.kftray/configs.db", home_dir.to_string_lossy());

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
