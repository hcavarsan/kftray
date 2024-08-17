use std::sync::Arc;
use std::{
    fs::{
        self,
        File,
    },
    io::Write,
    path::Path,
};

use log::{
    error,
    info,
};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::OnceCell;

use crate::utils::config_dir::{
    get_db_file_path,
    get_pod_manifest_path,
};

pub async fn init() -> Result<(), Box<dyn std::error::Error>> {
    if !db_file_exists() {
        create_db_file()?;
    }

    if !pod_manifest_file_exists() {
        create_server_config_manifest()?;
    }

    create_db_table().await?;

    Ok(())
}

static DB_POOL: OnceCell<Arc<SqlitePool>> = OnceCell::const_new();

pub async fn get_db_pool() -> Result<Arc<SqlitePool>, String> {
    DB_POOL
        .get_or_try_init(|| async {
            let db_dir = get_db_file_path().map_err(|e| {
                error!("Failed to get DB file path: {}", e);
                e.to_string()
            })?;
            let db_dir_str = db_dir.to_str().ok_or("Invalid DB path")?;
            info!("Database file path: {}", db_dir_str);
            let pool = SqlitePool::connect(db_dir_str).await.map_err(|e| {
                error!("Failed to connect to DB: {}", e);
                e.to_string()
            })?;
            Ok(Arc::new(pool))
        })
        .await
        .map(Arc::clone)
}

async fn create_db_table() -> Result<(), sqlx::Error> {
    info!("Creating database tables and triggers.");
    let pool = get_db_pool().await.map_err(|e| {
        error!("Failed to get DB pool: {}", e);
        sqlx::Error::Configuration(e.into())
    })?;
    let mut conn = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire connection: {}", e);
        e
    })?;

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to set PRAGMA foreign_keys: {}", e);
            e
        })?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS configs (
            id INTEGER PRIMARY KEY,
            data TEXT NOT NULL
        )",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to create configs table: {}", e);
        e
    })?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS config_state (
            id INTEGER PRIMARY KEY,
            config_id INTEGER NOT NULL,
            is_running BOOLEAN NOT NULL DEFAULT false,
            FOREIGN KEY(config_id) REFERENCES configs(id) ON DELETE CASCADE
        )",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to create config_state table: {}", e);
        e
    })?;

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

    info!("Database tables and triggers created successfully.");
    Ok(())
}

fn pod_manifest_file_exists() -> bool {
    if let Ok(path) = get_pod_manifest_path() {
        path.exists()
    } else {
        false
    }
}

fn create_server_config_manifest() -> Result<(), std::io::Error> {
    let manifest_path =
        get_pod_manifest_path().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let manifest_dir = manifest_path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to get manifest directory",
        )
    })?;

    if !manifest_dir.exists() {
        fs::create_dir_all(manifest_dir)?;
    }

    let placeholders = json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": "{hashed_name}",
            "labels": {
                "app": "{hashed_name}",
                "config_id": "{config_id}"
            }
        },
        "spec": {
            "containers": [{
                "name": "{hashed_name}",
                "image": "ghcr.io/hcavarsan/kftray-server:latest",
                "env": [
                    {"name": "LOCAL_PORT", "value": "{local_port}"},
                    {"name": "REMOTE_PORT", "value": "{remote_port}"},
                    {"name": "REMOTE_ADDRESS", "value": "{remote_address}"},
                    {"name": "PROXY_TYPE", "value": "{protocol}"},
                    {"name": "RUST_LOG", "value": "DEBUG"},
                ],
                "resources": {
                    "limits": {
                        "cpu": "100m",
                        "memory": "200Mi"
                    },
                    "requests": {
                        "cpu": "100m",
                        "memory": "100Mi"
                    }
                }
            }],
        }
    });

    let manifest_json = serde_json::to_string_pretty(&placeholders)?;

    File::create(&manifest_path)?.write_all(manifest_json.as_bytes())
}

fn db_file_exists() -> bool {
    if let Ok(db_dir) = get_db_file_path() {
        Path::new(&db_dir).exists()
    } else {
        false
    }
}

fn create_db_file() -> Result<(), std::io::Error> {
    let db_path =
        get_db_file_path().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let db_dir = Path::new(&db_path)
        .parent()
        .expect("Failed to get db directory");

    if !db_dir.exists() {
        fs::create_dir_all(db_dir)?;
    }

    fs::File::create(db_path)?;

    Ok(())
}
