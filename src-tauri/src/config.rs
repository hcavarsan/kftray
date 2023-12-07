use rusqlite::{params, Connection, Result};
use serde_json::json;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct Config {
    id: Option<i64>,
    service: String,
    namespace: String,
    local_port: u16,
    remote_port: u16,
    context: String,
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
    return Ok(configs);
}
