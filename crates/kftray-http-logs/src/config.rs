use std::path::{
    Path,
    PathBuf,
};

use anyhow::{
    Context,
    Result,
};
use chrono::Utc;
use tokio::fs;

pub const DEFAULT_MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;

pub const DEFAULT_LOG_RETENTION_DAYS: u64 = 7;

pub const HTTP_LOG_EXTENSION: &str = "http";

#[derive(Debug, Clone)]
pub struct LogConfig {
    log_dir: PathBuf,
    max_log_size: u64,
    retention_days: u64,
    file_extension: String,
}

impl LogConfig {
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            log_dir,
            max_log_size: DEFAULT_MAX_LOG_SIZE,
            retention_days: DEFAULT_LOG_RETENTION_DAYS,
            file_extension: HTTP_LOG_EXTENSION.to_string(),
        }
    }

    pub fn builder(log_dir: PathBuf) -> LogConfigBuilder {
        LogConfigBuilder::new(log_dir)
    }

    pub fn default_log_directory() -> Result<PathBuf> {
        let home_dir = dirs::home_dir().context("Failed to determine home directory")?;
        Ok(home_dir.join(".kftray").join("http_logs"))
    }

    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    pub fn max_log_size(&self) -> u64 {
        self.max_log_size
    }

    pub fn retention_days(&self) -> u64 {
        self.retention_days
    }

    pub async fn create_log_file_path(&self, config_id: i64, local_port: u16) -> Result<PathBuf> {
        self.ensure_log_directory().await?;

        let file_path = self.log_dir.join(format!(
            "{}_{}.{}",
            config_id, local_port, self.file_extension
        ));

        Ok(file_path)
    }

    pub async fn ensure_log_directory(&self) -> Result<()> {
        fs::create_dir_all(&self.log_dir)
            .await
            .context("Failed to create log directory")
    }

    pub fn create_rotated_log_path(&self, config_id: i64, local_port: u16) -> PathBuf {
        let now = Utc::now();
        let timestamp = now.format("%Y%m%d_%H%M%S");

        self.log_dir.join(format!(
            "{}_{}_{}.{}",
            config_id, local_port, timestamp, self.file_extension
        ))
    }
}

#[derive(Debug)]
pub struct LogConfigBuilder {
    log_dir: PathBuf,
    max_log_size: Option<u64>,
    retention_days: Option<u64>,
    file_extension: Option<String>,
}

impl LogConfigBuilder {
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            log_dir,
            max_log_size: None,
            retention_days: None,
            file_extension: None,
        }
    }

    pub fn file_extension(mut self, extension: impl Into<String>) -> Self {
        self.file_extension = Some(extension.into());
        self
    }

    pub fn build(self) -> LogConfig {
        LogConfig {
            log_dir: self.log_dir,
            max_log_size: self.max_log_size.unwrap_or(DEFAULT_MAX_LOG_SIZE),
            retention_days: self.retention_days.unwrap_or(DEFAULT_LOG_RETENTION_DAYS),
            file_extension: self
                .file_extension
                .unwrap_or_else(|| HTTP_LOG_EXTENSION.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn test_create_log_file_path() {
        let temp_dir = TempDir::new().unwrap();
        let config = LogConfig::new(temp_dir.path().to_path_buf());

        let log_path = config.create_log_file_path(123, 8080).await.unwrap();
        assert!(log_path.ends_with("123_8080.http"));

        assert!(temp_dir.path().exists());
    }

    #[test]
    fn test_log_config_getters() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let config = LogConfig {
            log_dir: log_dir.clone(),
            max_log_size: 500,
            retention_days: 3,
            file_extension: "log".to_string(),
        };

        assert_eq!(config.log_dir(), log_dir.as_path());
        assert_eq!(config.max_log_size(), 500);
        assert_eq!(config.retention_days(), 3);
    }

    #[test]
    fn test_log_config_builder_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();

        let config = LogConfig::builder(log_dir.clone()).build();

        assert_eq!(config.log_dir(), log_dir.as_path());
        assert_eq!(config.max_log_size(), DEFAULT_MAX_LOG_SIZE);
        assert_eq!(config.retention_days(), DEFAULT_LOG_RETENTION_DAYS);
        assert_eq!(config.file_extension, HTTP_LOG_EXTENSION);
    }

    #[test]
    fn test_log_config_builder_custom() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();

        let builder = LogConfigBuilder::new(log_dir.clone());
        let config = builder.file_extension("testlog").build();

        assert_eq!(config.log_dir(), log_dir.as_path());
        assert_eq!(config.max_log_size(), DEFAULT_MAX_LOG_SIZE);
        assert_eq!(config.retention_days(), DEFAULT_LOG_RETENTION_DAYS);
        assert_eq!(config.file_extension, "testlog");
    }

    #[test]
    fn test_default_log_directory_ok() {
        let result = LogConfig::default_log_directory();
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.ends_with(".kftray/http_logs"));
    }

    #[test]
    fn test_create_rotated_log_path() {
        let temp_dir = TempDir::new().unwrap();
        let config = LogConfig::builder(temp_dir.path().to_path_buf())
            .file_extension("rotated")
            .build();

        let rotated_path = config.create_rotated_log_path(99, 1234);

        let filename = rotated_path.file_name().unwrap().to_str().unwrap();

        assert!(filename.starts_with("99_1234_"));
        assert!(filename.ends_with(".rotated"));

        let parts: Vec<&str> = filename.split('.').next().unwrap().split('_').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "99");
        assert_eq!(parts[1], "1234");
        assert_eq!(parts[2].len(), 8);
        assert_eq!(parts[3].len(), 6);
        assert!(parts[2].chars().all(|c| c.is_ascii_digit()));
        assert!(parts[3].chars().all(|c| c.is_ascii_digit()));
    }

    #[tokio::test]
    async fn test_ensure_log_directory() {
        let temp_dir = TempDir::new().unwrap();
        let log_subdir = temp_dir.path().join("test_logs");

        assert!(!log_subdir.exists());

        let config = LogConfig::new(log_subdir.clone());
        config.ensure_log_directory().await.unwrap();

        assert!(log_subdir.exists());
        assert!(log_subdir.is_dir());
    }
}
