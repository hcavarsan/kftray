use std::fs::read_to_string;
use std::io;
use std::path::Path;

use kftray_commons::config::{
    export_configs,
    import_configs,
};

pub fn get_file_content(path: &Path) -> io::Result<String> {
    let mut content = String::new();

    if path.is_file() {
        content = read_to_string(path)?;
    }

    Ok(content)
}

pub async fn import_configs_from_file(file_path: &str) -> Result<(), String> {
    let json = std::fs::read_to_string(file_path).map_err(|e| e.to_string())?;
    import_configs(json).await
}

pub async fn export_configs_to_file(file_path: &str) -> Result<(), String> {
    let json = export_configs().await?;
    std::fs::write(file_path, json).map_err(|e| e.to_string())
}
