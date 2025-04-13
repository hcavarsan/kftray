use std::fs::File;
use std::io::Write;
use std::path::Path;

use tempfile::TempDir;

use crate::utils::file::get_file_content;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_file_content_success() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_file.txt");

        let test_content = "This is test content";
        let mut file = File::create(&file_path).unwrap();
        file.write_all(test_content.as_bytes()).unwrap();

        let result = get_file_content(&file_path);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_content);
    }

    #[test]
    fn test_get_file_content_not_a_file() {
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path();

        let result = get_file_content(dir_path);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_get_file_content_nonexistent_file() {
        let nonexistent_path = Path::new("/nonexistent/file.txt");

        let result = get_file_content(nonexistent_path);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
    }
}
