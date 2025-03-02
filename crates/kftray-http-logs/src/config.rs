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
}
