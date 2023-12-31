use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct Config {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<i64>,
    service: Option<String>,
    namespace: String,
    local_port: u16,
    remote_port: u16,
    context: String,
    workload_type: String,
    remote_address: Option<String>,
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
    let json = serde_json::to_string(&configs).map_err(|e| e.to_string())?;
    Ok(json)
}

#[tauri::command]
pub async fn import_configs(json: String) -> Result<(), String> {
    let configs: Vec<Config> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    for config in configs {
        insert_config(config)?;
    }
    Ok(())
}
