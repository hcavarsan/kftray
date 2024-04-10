use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use rusqlite::{params, Connection, Result};

/// Initializes the application by ensuring that both the database file and the server configuration
/// manifest file exist.
pub fn init() -> Result<(), Box<dyn std::error::Error>> {
    if !db_file_exists() {
        create_db_file()?;
    }

    if !pod_manifest_file_exists() {
        create_server_config_manifest()?;
    }

    create_db_table()?;

    Ok(())
}

fn create_db_table() -> Result<(), rusqlite::Error> {
    let db_path = get_db_path();
    let conn = Connection::open(db_path)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS configs (
            id INTEGER PRIMARY KEY,
            data TEXT NOT NULL
        )",
        params![],
    )?;

    Ok(())
}

/// Creates the server configuration manifest file with placeholders.
fn create_server_config_manifest() -> Result<(), std::io::Error> {
    let manifest_path = get_pod_manifest_path();
    let manifest_dir = manifest_path
        .parent()
        .expect("Failed to get manifest directory");

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

/// Checks if the pod manifest file already exists.
fn pod_manifest_file_exists() -> bool {
    get_pod_manifest_path().exists()
}

/// Returns the path to the pod manifest file.
fn get_pod_manifest_path() -> PathBuf {
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    home_dir.join(".kftray/proxy_manifest.json")
}

/// Creates a new database file if it doesn't exist already.
fn create_db_file() -> Result<(), std::io::Error> {
    let db_path = get_db_path();
    let db_dir = Path::new(&db_path)
        .parent()
        .expect("Failed to get db directory");

    if !db_dir.exists() {
        fs::create_dir_all(db_dir)?;
    }

    fs::File::create(db_path)?;
    Ok(())
}

fn db_file_exists() -> bool {
    let db_path = get_db_path();
    Path::new(&db_path).exists()
}

pub fn get_db_path() -> String {
    let home_dir = dirs::home_dir().expect("Failed to get home directory");
    home_dir.to_str().unwrap().to_string() + "/.kftray/configs.db"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Sets up a temporary test environment and overrides the home directory.
    fn setup_test_environment() -> TempDir {
        let temp = tempfile::tempdir().expect("Failed to create a temp dir");
        std::env::set_var("HOME", temp.path());
        temp
    }

    /// Tests if the initialization creates the required database and manifest files.
    #[test]
    fn test_initialization_creates_files() {
        let _temp_dir = setup_test_environment();
        init().expect("Initialization failed");

        assert!(db_file_exists());
        assert!(pod_manifest_file_exists());
    }

    /// Tests if the contents of the created pod manifest file match the expected default structure.
    #[test]
    fn test_pod_manifest_content() {
        let _temp_dir = setup_test_environment();
        init().expect("Initialization failed");

        let manifest_path = get_pod_manifest_path();
        let manifest_content =
            fs::read_to_string(manifest_path).expect("Failed to read pod manifest");

        assert!(manifest_content.contains("\"apiVersion\": \"v1\""));
        assert!(manifest_content.contains("\"kind\": \"Pod\""));
    }

    /// Confirms that the database file gets created successfully.
    #[test]
    fn test_db_file_creation() {
        let _temp_dir = setup_test_environment();
        create_db_file().expect("Failed to create db file");

        assert!(db_file_exists());
    }
}
