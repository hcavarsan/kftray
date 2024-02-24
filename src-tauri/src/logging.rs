use log::LevelFilter;
use std::{env, fs::OpenOptions, io, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LogError {
    #[error("home directory could not be determined")]
    HomeDirNotFound,

    #[error("log directory could not be verified: {0}")]
    LogDirNotFound(io::Error),

    #[error("Failed to create log directory: {0}")]
    LogDirCreationFailed(io::Error),

    #[error("Failed to open log file: {0}")]
    LogFileOpenFailed(io::Error),
}

pub fn get_log_path() -> Result<PathBuf, LogError> {
    let home_dir = dirs::home_dir().ok_or(LogError::HomeDirNotFound)?;
    Ok(home_dir.join(".kftray").join("app.log"))
}

pub fn setup_logging() -> Result<(), LogError> {
    let log_filter = env::var("RUST_LOG")
        .map(|filter| filter.parse().unwrap_or(LevelFilter::Info))
        .unwrap_or(LevelFilter::Off);

    let mut builder = env_logger::builder();
    builder.filter_level(log_filter).format_timestamp_secs();

    if env::var("KFTRAY_DEBUG").is_ok() {
        let log_path = get_log_path()?;
        let log_dir = log_path
            .parent()
            .ok_or(LogError::LogDirNotFound(io::Error::new(
                io::ErrorKind::NotFound,
                "Log directory path cannot be determined",
            )))?;

        std::fs::create_dir_all(log_dir).map_err(LogError::LogDirCreationFailed)?;

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(LogError::LogFileOpenFailed)?;

        builder.target(env_logger::Target::Pipe(Box::new(log_file)));
    }

    builder.init();

    Ok(())
}
