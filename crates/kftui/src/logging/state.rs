use std::sync::Arc;

use super::config::LogConfig;

#[derive(Clone)]
pub struct LoggerState {
    config: Arc<LogConfig>,
}

impl LoggerState {
    pub fn new(config: LogConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub fn is_file_output_enabled(&self) -> bool {
        self.config.is_file_output_enabled()
    }

    pub fn file_path(&self) -> Option<String> {
        self.config
            .file_path()
            .and_then(|path| path.to_str())
            .map(|s| s.to_string())
    }
}
