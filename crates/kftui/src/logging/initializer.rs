use std::{
    fs::OpenOptions,
    io,
};

use super::config::LogConfig;

pub trait LoggerInitializer {
    fn initialize(&self, config: &LogConfig) -> Result<(), Box<dyn std::error::Error>>;
}

pub struct FileLoggerInitializer;

impl LoggerInitializer for FileLoggerInitializer {
    fn initialize(&self, config: &LogConfig) -> Result<(), Box<dyn std::error::Error>> {
        let file_config = config.file_output.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "No file path configured")
        })?;

        if let Some(parent) = file_config.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_config.path)?;

        env_logger::Builder::new()
            .target(env_logger::Target::Pipe(Box::new(file)))
            .filter_level(config.level)
            .filter_module("sqlx::query", log::LevelFilter::Warn)
            .init();

        Ok(())
    }
}

pub struct TuiLoggerInitializer;

impl LoggerInitializer for TuiLoggerInitializer {
    fn initialize(&self, config: &LogConfig) -> Result<(), Box<dyn std::error::Error>> {
        log::set_max_level(config.level);

        tui_logger::set_default_level(config.level);

        tui_logger::init_logger(config.level)
            .map_err(|e| format!("Failed to initialize TUI logger: {e}"))?;

        tui_logger::set_hot_buffer_depth(2000);
        tui_logger::set_buffer_depth(20000);

        Ok(())
    }
}
