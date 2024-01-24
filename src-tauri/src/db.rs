use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn init() {
    if !db_file_exists() {
        create_db_file();
    }

    if !pod_manifest_file_exists() {
        create_server_config_manifest();
    }
}

fn create_server_config_manifest() {
    let manifest_path = get_pod_manifest_path();
    let manifest_dir = manifest_path.parent().unwrap();

    if !manifest_dir.exists() {
        fs::create_dir_all(manifest_dir).unwrap();
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

    let manifest_json = serde_json::to_string_pretty(&placeholders).unwrap();
    File::create(&manifest_path)
        .and_then(|mut file| file.write_all(manifest_json.as_bytes()))
        .unwrap();
}

fn pod_manifest_file_exists() -> bool {
    get_pod_manifest_path().exists()
}

fn get_pod_manifest_path() -> PathBuf {
    let home_dir = dirs::home_dir().unwrap();
    home_dir.join(".kftray/proxy_manifest.json")
}

fn create_db_file() {
    let db_path = get_db_path();
    let db_dir = Path::new(&db_path).parent().unwrap();

    // If the parent directory does not exist, create it.
    if !db_dir.exists() {
        fs::create_dir_all(db_dir).unwrap();
    }

    // Create the database file.
    fs::File::create(db_path).unwrap();
}

// Check whether the database file exists.
fn db_file_exists() -> bool {
    let db_path = get_db_path();
    Path::new(&db_path).exists()
}

// Get the path where the database file should be located.
pub fn get_db_path() -> String {
    let home_dir = dirs::home_dir().unwrap();
    home_dir.to_str().unwrap().to_string() + "/.kftray/configs.db"
}
