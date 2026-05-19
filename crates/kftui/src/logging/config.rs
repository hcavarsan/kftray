use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct LogConfig {
    pub level: log::LevelFilter,
    pub file_output: Option<FileOutputConfig>,
}

#[derive(Clone, Debug)]
pub(crate) struct FileOutputConfig {
    pub path: PathBuf,
}

impl LogConfig {
    pub(crate) const fn new(level: log::LevelFilter) -> Self {
        Self {
            level,
            file_output: None,
        }
    }

    pub(crate) const fn with_default_level(level: log::LevelFilter) -> Self {
        Self {
            level,
            file_output: None,
        }
    }

    pub(crate) fn with_file_output(mut self, path: PathBuf) -> Self {
        self.file_output = Some(FileOutputConfig { path });
        self
    }

    pub(crate) const fn is_file_output_enabled(&self) -> bool {
        self.file_output.is_some()
    }

    pub(crate) fn file_path(&self) -> Option<&PathBuf> {
        self.file_output.as_ref().map(|config| &config.path)
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self::with_default_level(log::LevelFilter::Warn)
    }
}
