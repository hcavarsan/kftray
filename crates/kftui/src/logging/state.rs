use std::sync::Arc;

use super::config::LogConfig;

#[derive(Clone)]
pub(crate) struct LoggerState {
    config: Arc<LogConfig>,
}

impl LoggerState {
    pub(crate) fn new(config: LogConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub(crate) fn is_file_output_enabled(&self) -> bool {
        self.config.is_file_output_enabled()
    }

    pub(crate) fn file_path(&self) -> Option<String> {
        self.config
            .file_path()
            .and_then(|path| path.to_str())
            .map(ToString::to_string)
    }
}
