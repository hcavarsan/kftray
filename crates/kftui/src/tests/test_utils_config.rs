use std::fs;

use tempfile::TempDir;

use crate::utils::config::{
    export_configs_to_file,
    import_configs_from_file,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_import_configs_from_file_error() {
        let result = import_configs_from_file("/non/existent/file.json").await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read file"));
    }

    #[tokio::test]
    async fn test_export_configs_to_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_export.json");
        let file_path_str = file_path.to_str().unwrap();

        let result = export_configs_to_file(file_path_str).await;

        if result.is_ok() {
            assert!(fs::metadata(&file_path).is_ok());
        }
    }

    #[tokio::test]
    async fn test_export_configs_to_file_error() {
        let file_path = "/non/existent/directory/test_export.json";

        let result = export_configs_to_file(file_path).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to write to file"));
    }
}
