use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use lazy_static::lazy_static;
use log::{error, info};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::OnceCell;

use crate::config_dir::{get_db_file_path, get_pod_manifest_path};
use crate::utils::manifests::{
    create_expose_deployment_manifest, create_expose_ingress_manifest,
    create_expose_service_manifest, create_proxy_deployment_manifest,
    expose_deployment_manifest_exists, expose_ingress_manifest_exists,
    expose_service_manifest_exists, proxy_deployment_manifest_exists,
};

lazy_static! {
    static ref ENV_TEST_MUTEX: Mutex<()> = Mutex::new(());
}

pub async fn init() -> Result<(), Box<dyn std::error::Error>> {
    if !db_file_exists() {
        create_db_file()?;
    }

    if !pod_manifest_file_exists() {
        create_server_config_manifest()?;
    }

    if !proxy_deployment_manifest_exists() {
        info!("Creating proxy deployment manifest");
        create_proxy_deployment_manifest()?;
    }

    if !expose_deployment_manifest_exists() {
        info!("Creating expose deployment manifest");
        create_expose_deployment_manifest()?;
    }

    if !expose_service_manifest_exists() {
        info!("Creating expose service manifest");
        create_expose_service_manifest()?;
    }

    if !expose_ingress_manifest_exists() {
        info!("Creating expose ingress manifest");
        create_expose_ingress_manifest()?;
    }

    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    create_db_table(&pool).await?;

    Ok(())
}

pub static DB_POOL: OnceCell<Arc<SqlitePool>> = OnceCell::const_new();

pub async fn get_db_pool() -> Result<Arc<SqlitePool>, String> {
    DB_POOL
        .get_or_try_init(|| async {
            let db_dir = get_db_file_path().map_err(|e| {
                error!("Failed to get DB file path: {e}");
                e.to_string()
            })?;
            let db_dir_str = db_dir.to_str().ok_or("Invalid DB path")?;
            info!("Database file path: {db_dir_str}");
            let pool = SqlitePool::connect(db_dir_str).await.map_err(|e| {
                error!("Failed to connect to DB: {e}");
                e.to_string()
            })?;
            Ok(Arc::new(pool))
        })
        .await
        .map(Arc::clone)
}

pub async fn create_db_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    info!("Creating database tables and triggers.");
    let mut conn = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire connection: {e}");
        e
    })?;

    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to set PRAGMA foreign_keys: {e}");
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
        error!("Failed to create configs table: {e}");
        e
    })?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS config_state (
            id INTEGER PRIMARY KEY,
            config_id INTEGER NOT NULL,
            is_running BOOLEAN NOT NULL DEFAULT false,
            process_id INTEGER,
            FOREIGN KEY(config_id) REFERENCES configs(id) ON DELETE CASCADE
        )",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to create config_state table: {e}");
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

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to create settings table: {e}");
        e
    })?;

    info!("Database tables and triggers created successfully.");
    Ok(())
}

fn pod_manifest_file_exists() -> bool {
    match get_pod_manifest_path() {
        Ok(path) => {
            let exists = path.exists();
            if cfg!(test) {
                println!(
                    "pod_manifest_file_exists checking path: {}, exists: {}",
                    path.display(),
                    exists
                );
            }
            exists
        }
        Err(e) => {
            if cfg!(test) {
                println!("pod_manifest_file_exists failed to get path: {e}");
            }
            false
        }
    }
}

fn create_server_config_manifest() -> Result<(), std::io::Error> {
    let manifest_path = get_pod_manifest_path().map_err(std::io::Error::other)?;

    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| std::io::Error::other("Failed to get manifest directory"))?;

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
    match get_db_file_path() {
        Ok(db_path) => {
            let exists = db_path.exists();
            if cfg!(test) {
                println!(
                    "db_file_exists checking path: {}, exists: {}",
                    db_path.display(),
                    exists
                );
            }
            exists
        }
        Err(e) => {
            if cfg!(test) {
                println!("db_file_exists failed to get path: {e}");
            }
            false
        }
    }
}

fn create_db_file() -> Result<(), std::io::Error> {
    let db_path = get_db_file_path().map_err(std::io::Error::other)?;

    let db_dir = Path::new(&db_path)
        .parent()
        .expect("Failed to get db directory");

    if !db_dir.exists() {
        fs::create_dir_all(db_dir)?;
    }

    fs::File::create(db_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs::{self, File};
    use std::sync::Mutex;

    use lazy_static::lazy_static;
    use sqlx::SqlitePool;
    use tempfile::tempdir;

    use super::*;
    use crate::config_dir::{get_config_dir, get_db_file_path, get_pod_manifest_path};

    lazy_static! {
        static ref ENV_TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

    struct StrictEnvGuard {
        saved_vars: Vec<(String, Option<String>)>,
    }

    impl StrictEnvGuard {
        fn new(keys: &[&str]) -> Self {
            let saved_vars = keys
                .iter()
                .map(|&key| (key.to_string(), env::var(key).ok()))
                .collect::<Vec<_>>();

            for key in keys {
                unsafe { env::remove_var(key) };
            }

            StrictEnvGuard { saved_vars }
        }
    }

    impl Drop for StrictEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved_vars.drain(..) {
                match value {
                    Some(val) => unsafe { env::set_var(key, val) },

                    None => unsafe { env::remove_var(key) },
                }
            }
        }
    }

    #[test]
    fn test_db_file_exists_and_create() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = StrictEnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path();

        let db_path = temp_path.join("configs.db");

        unsafe { env::set_var("KFTRAY_CONFIG", temp_path.to_str().unwrap()) };

        assert_eq!(
            get_config_dir().unwrap().to_str().unwrap(),
            temp_path.to_str().unwrap(),
            "Config directory should match our temporary directory"
        );

        assert!(!db_path.exists(), "DB file should not exist initially");
        assert!(
            !db_file_exists(),
            "db_file_exists() should return false initially"
        );

        File::create(&db_path).unwrap();
        assert!(db_path.exists(), "DB file should exist after creation");

        let db_exists = db_file_exists();

        assert!(
            db_exists,
            "db_file_exists() should return true after file creation"
        );
    }

    #[test]
    fn test_pod_manifest_file_exists_and_create() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = StrictEnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let temp_dir = tempdir().unwrap();
        let test_dir = temp_dir.path();
        std::fs::create_dir_all(test_dir).unwrap();
        assert!(test_dir.exists(), "Test directory should exist");

        let config_path = test_dir.to_str().unwrap();

        unsafe { env::set_var("KFTRAY_CONFIG", config_path) };
        println!("Set KFTRAY_CONFIG to: {config_path}");

        assert!(env::var("HOME").is_err(), "HOME should not be set");
        assert!(
            env::var("XDG_CONFIG_HOME").is_err(),
            "XDG_CONFIG_HOME should not be set"
        );
        assert_eq!(
            env::var("KFTRAY_CONFIG").unwrap(),
            config_path,
            "KFTRAY_CONFIG should be set to test dir"
        );

        let expected_manifest_path = test_dir.join("proxy_manifest.json");
        println!(
            "Expected manifest path: {}",
            expected_manifest_path.display()
        );

        assert!(
            !expected_manifest_path.exists(),
            "Manifest file should not exist initially"
        );
        assert!(
            !pod_manifest_file_exists(),
            "pod_manifest_file_exists() should return false initially"
        );

        println!("Creating manifest file...");
        create_server_config_manifest().unwrap();

        assert!(
            expected_manifest_path.exists(),
            "Manifest file should exist at: {}",
            expected_manifest_path.display()
        );

        let func_result = pod_manifest_file_exists();
        println!("pod_manifest_file_exists() returned: {func_result}");
        assert!(func_result, "pod_manifest_file_exists() should return true");

        let content = fs::read_to_string(&expected_manifest_path).unwrap();
        assert!(
            content.contains("apiVersion"),
            "Manifest should contain apiVersion"
        );
        assert!(
            content.contains("kftray-server"),
            "Manifest should contain kftray-server"
        );
    }

    #[test]
    fn test_pod_manifest_create_directory() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _env_guard = StrictEnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let temp_dir = tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_str().unwrap().to_string();

        let manifest_dir = std::path::Path::new(&temp_dir_path).join("manifest_dir");
        std::fs::create_dir_all(&manifest_dir).unwrap();

        let manifest_dir_str = manifest_dir.to_str().unwrap();

        unsafe { env::set_var("KFTRAY_CONFIG", manifest_dir_str) };

        assert!(manifest_dir.exists(), "Directory should exist");

        let manifest_path = manifest_dir.join("proxy_manifest.json");
        let content = r#"{"apiVersion":"v1","kind":"Pod"}"#;
        std::fs::write(&manifest_path, content).unwrap();

        // Verify file exists
        assert!(manifest_path.exists(), "Manifest file should exist at path");
        println!("Created manifest file at: {}", manifest_path.display());

        if let Ok(config_path) = get_config_dir() {
            println!(
                "Config dir from get_config_dir(): {}",
                config_path.display()
            );
            let expected_manifest = config_path.join("proxy_manifest.json");
            println!("Expected manifest path: {}", expected_manifest.display());
            println!(
                "File exists at expected path: {}",
                expected_manifest.exists()
            );
        }

        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5);
        let poll_interval = std::time::Duration::from_millis(50);

        let mut result = false;
        while start.elapsed() < timeout {
            result = pod_manifest_file_exists();
            if result {
                break;
            }
            std::thread::sleep(poll_interval);
        }

        assert!(result, "pod_manifest_file_exists() should return true");

        let file_content = std::fs::read_to_string(&manifest_path).unwrap();
        assert!(file_content.contains("apiVersion"));
    }

    #[tokio::test]
    async fn test_create_db_table() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory database");
        let result = create_db_table(&pool).await;
        assert!(result.is_ok());

        let mut conn = pool.acquire().await.unwrap();
        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='configs'")
                .fetch_optional(&mut *conn)
                .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_some(), "configs table should exist");

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='config_state'",
        )
        .fetch_optional(&mut *conn)
        .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_some(), "config_state table should exist");

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='trigger' AND name='after_insert_config'",
        )
        .fetch_optional(&mut *conn)
        .await;

        assert!(result.is_ok());
        assert!(
            result.unwrap().is_some(),
            "after_insert_config trigger should exist"
        );
    }

    #[test]
    fn test_init_creates_files_and_db_direct() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = StrictEnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path();

        unsafe { env::set_var("KFTRAY_CONFIG", temp_path.to_str().unwrap()) };

        let db_path = temp_path.join("configs.db");
        let manifest_path = temp_path.join("proxy_manifest.json");

        assert!(!db_path.exists(), "DB file should not exist initially");
        assert!(
            !manifest_path.exists(),
            "Manifest file should not exist initially"
        );

        let db_result = create_db_file();
        assert!(db_result.is_ok(), "create_db_file() should succeed");
        assert!(db_path.exists(), "DB file should exist after creation");

        let manifest_content = r#"{"apiVersion":"v1","kind":"Pod"}"#;
        let manifest_result = fs::write(&manifest_path, manifest_content);
        assert!(
            manifest_result.is_ok(),
            "Writing manifest file should succeed"
        );
        assert!(
            manifest_path.exists(),
            "Manifest file should exist after creation"
        );

        println!("Created DB file at: {}", db_path.to_str().unwrap());
        println!("Does path exist? {}", db_path.exists());

        if let Ok(cfg_path) = get_db_file_path() {
            println!("Config DB path: {}", cfg_path.display());
            assert!(
                cfg_path.exists(),
                "Path returned by get_db_file_path() must exist"
            );
            assert_eq!(
                cfg_path, db_path,
                "get_db_file_path() should return the expected path"
            );
            assert!(db_file_exists(), "db_file_exists() should return true");
        } else {
            panic!("get_db_file_path() failed unexpectedly");
        }

        if let Ok(manifest_cfg_path) = get_pod_manifest_path() {
            println!("Config manifest path: {}", manifest_cfg_path.display());
            assert!(
                manifest_cfg_path.exists(),
                "Path returned by get_pod_manifest_path() must exist"
            );
            assert_eq!(
                manifest_cfg_path, manifest_path,
                "get_pod_manifest_path() should return the expected path"
            );
            assert!(
                pod_manifest_file_exists(),
                "pod_manifest_file_exists() should return true"
            );
        } else {
            panic!("get_pod_manifest_path() failed unexpectedly");
        }
    }
}
