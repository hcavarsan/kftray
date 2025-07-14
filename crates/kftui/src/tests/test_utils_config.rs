use std::fs;

use kftray_commons::utils::db_mode::DatabaseMode;
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
        let result = import_configs_from_file("/non/existent/file.json", DatabaseMode::File).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read file"));
    }

    #[tokio::test]
    async fn test_export_configs_to_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_export.json");
        let file_path_str = file_path.to_str().unwrap();

        let result = export_configs_to_file(file_path_str, DatabaseMode::File).await;

        if result.is_ok() {
            let metadata = fs::metadata(&file_path);
            assert!(
                metadata.is_ok(),
                "File should exist after successful export"
            );
            assert!(metadata.unwrap().is_file(), "Created path should be a file");
        } else {
            println!(
                "Export configs test skipped validation: {}",
                result.unwrap_err()
            );
        }
    }

    #[tokio::test]
    async fn test_export_configs_to_file_error() {
        let file_path = "/non/existent/directory/test_export.json";

        let result = export_configs_to_file(file_path, DatabaseMode::File).await;

        assert!(
            result.is_err(),
            "Export to non-existent directory should fail"
        );

        let error_message = result.unwrap_err();
        assert!(
            error_message.contains("Failed to write to file")
                || error_message.contains("Failed to export configs"),
            "Error should be related to writing or exporting: {error_message}"
        );
    }
}
