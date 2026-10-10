use std::path::{
    Path,
    PathBuf,
};
use std::time::{
    Duration,
    SystemTime,
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

const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Clone)]
pub struct LogConfig {
    log_dir: PathBuf,
    max_log_size: u64,
    retention_days: u64,
    auto_cleanup: bool,
    file_extension: String,
}

impl LogConfig {
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            log_dir,
            max_log_size: DEFAULT_MAX_LOG_SIZE,
            retention_days: DEFAULT_LOG_RETENTION_DAYS,
            auto_cleanup: true,
            file_extension: HTTP_LOG_EXTENSION.to_string(),
        }
    }

    pub fn builder(log_dir: PathBuf) -> LogConfigBuilder {
        LogConfigBuilder::new(log_dir)
    }

    pub fn default_log_directory() -> Result<PathBuf> {
        kftray_commons::utils::config_dir::get_log_folder_path()
            .map_err(anyhow::Error::msg)
            .context("Failed to resolve the HTTP log directory")
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

    pub fn auto_cleanup(&self) -> bool {
        self.auto_cleanup
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

    pub async fn rotated_log_path(&self, log_file_path: &Path) -> PathBuf {
        let stem = Self::file_stem(log_file_path);
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let dir = log_file_path.parent().unwrap_or(&self.log_dir);

        let mut rotated = dir.join(format!("{stem}_{timestamp}.{}", self.file_extension));
        let mut suffix = 1u32;
        while fs::try_exists(&rotated).await.unwrap_or(false) {
            rotated = dir.join(format!(
                "{stem}_{timestamp}_{suffix}.{}",
                self.file_extension
            ));
            suffix += 1;
        }
        rotated
    }

    pub async fn remove_expired_rotated_logs(&self, log_file_path: &Path) -> Result<()> {
        let Some(dir) = log_file_path.parent() else {
            return Ok(());
        };
        let prefix = format!("{}_", Self::file_stem(log_file_path));
        let suffix = format!(".{}", self.file_extension);
        let max_age = Duration::from_secs(self.retention_days.saturating_mul(SECONDS_PER_DAY));
        let now = SystemTime::now();

        let mut entries = fs::read_dir(dir)
            .await
            .context("Failed to read log directory")?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .context("Failed to read log directory entry")?
        {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let is_rotated = name
                .strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(&suffix))
                .is_some_and(Self::is_rotation_stamp);
            if !is_rotated {
                continue;
            }

            let metadata = entry
                .metadata()
                .await
                .context("Failed to read rotated log metadata")?;
            let age = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok());
            if metadata.is_file() && age.is_some_and(|age| age > max_age) {
                fs::remove_file(entry.path())
                    .await
                    .with_context(|| format!("Failed to remove {}", entry.path().display()))?;
            }
        }

        Ok(())
    }

    fn file_stem(log_file_path: &Path) -> String {
        log_file_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn is_rotation_stamp(stamp: &str) -> bool {
        let is_digits =
            |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
        let mut parts = stamp.split('_');
        let (Some(date), Some(time)) = (parts.next(), parts.next()) else {
            return false;
        };
        let suffix_ok = match parts.next() {
            None => true,
            Some(suffix) => !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()),
        };
        is_digits(date, 8) && is_digits(time, 6) && suffix_ok && parts.next().is_none()
    }
}

#[derive(Debug)]
pub struct LogConfigBuilder {
    log_dir: PathBuf,
    max_log_size: Option<u64>,
    retention_days: Option<u64>,
    auto_cleanup: Option<bool>,
    file_extension: Option<String>,
}

impl LogConfigBuilder {
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            log_dir,
            max_log_size: None,
            retention_days: None,
            auto_cleanup: None,
            file_extension: None,
        }
    }

    pub fn max_log_size(mut self, max_log_size: u64) -> Self {
        self.max_log_size = Some(max_log_size);
        self
    }

    pub fn retention_days(mut self, retention_days: u64) -> Self {
        self.retention_days = Some(retention_days);
        self
    }

    pub fn auto_cleanup(mut self, auto_cleanup: bool) -> Self {
        self.auto_cleanup = Some(auto_cleanup);
        self
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
            auto_cleanup: self.auto_cleanup.unwrap_or(true),
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
            auto_cleanup: true,
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

    #[tokio::test]
    async fn rotated_log_path_never_reuses_an_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let config = LogConfig::new(temp_dir.path().to_path_buf());
        let log_path = config.create_log_file_path(99, 1234).await.unwrap();

        let first = config.rotated_log_path(&log_path).await;
        std::fs::write(&first, "first").unwrap();
        let second = config.rotated_log_path(&log_path).await;

        let first_name = first.file_name().unwrap().to_str().unwrap();
        assert!(first_name.starts_with("99_1234_"), "{first_name}");
        assert!(first_name.ends_with(".http"), "{first_name}");
        assert_eq!(first.parent(), log_path.parent());
        assert_ne!(second, first);
        assert!(!second.exists());
    }

    #[tokio::test]
    async fn expired_cleanup_removes_only_old_rotated_files_of_this_forward() {
        let temp_dir = TempDir::new().unwrap();
        let config = LogConfig::builder(temp_dir.path().to_path_buf())
            .retention_days(2)
            .build();
        let log_path = config.create_log_file_path(7, 8080).await.unwrap();
        let old = SystemTime::now() - Duration::from_secs(3 * SECONDS_PER_DAY);

        let touch = |name: &str, modified: SystemTime| {
            let path = temp_dir.path().join(name);
            let file = std::fs::File::create(&path).unwrap();
            file.set_modified(modified).unwrap();
            path
        };
        let active = touch("7_8080.http", old);
        let old_rotated = touch("7_8080_20260101_000000.http", old);
        let old_rotated_suffix = touch("7_8080_20260101_000000_2.http", old);
        let new_rotated = touch("7_8080_20260109_000000.http", SystemTime::now());
        let other_forward = touch("7_80801_20260101_000000.http", old);
        let other_extension = touch("7_8080_20260101_000000.txt", old);
        let user_file = touch("7_8080_backup.http", old);

        config.remove_expired_rotated_logs(&log_path).await.unwrap();

        assert!(!old_rotated.exists());
        assert!(!old_rotated_suffix.exists());
        assert!(active.exists());
        assert!(new_rotated.exists());
        assert!(other_forward.exists());
        assert!(other_extension.exists());
        assert!(user_file.exists());
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

    #[test]
    fn test_default_log_directory_uses_kftray_config() {
        let temp_dir = TempDir::new().unwrap();
        let _guard = kftray_commons::test_utils::EnvVarGuard::set(
            "KFTRAY_CONFIG",
            temp_dir.path().to_str().unwrap(),
        );

        let log_dir =
            LogConfig::default_log_directory().expect("default_log_directory should resolve");
        assert_eq!(log_dir, temp_dir.path().join("http_logs"));
    }
}
