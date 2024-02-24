use log::LevelFilter;
use std::{env, fs::OpenOptions, io, path::PathBuf};
use thiserror::Error;

/// Defines the types of errors that can occur when trying to set up logging.
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

/// Gets the path to the log file.
pub fn get_log_path() -> Result<PathBuf, LogError> {
    // Retrieve the home directory or return a `HomeDirNotFound` error.
    let home_dir = dirs::home_dir().ok_or(LogError::HomeDirNotFound)?;
    // Construct and return the path to the log file.
    Ok(home_dir.join(".kftray").join("app.log"))
}

/// Sets up the logging environment using `env_logger`.
pub fn setup_logging() -> Result<(), LogError> {
    // Get the log filter level from the environment variable or fall back to `LevelFilter::Off`.
    let log_filter = env::var("RUST_LOG")
        .map(|filter| filter.parse().unwrap_or(LevelFilter::Info))
        .unwrap_or(LevelFilter::Off);

    let mut builder = env_logger::builder();
    // Configure the builder's filter level and timestamp format.
    builder.filter_level(log_filter).format_timestamp_secs();

    // If the `KFTRAY_DEBUG` environment variable is set, set up file-based logging.
    if env::var("KFTRAY_DEBUG").is_ok() {
        // Get the log path, or return an error if it fails.
        let log_path = get_log_path()?;
        let log_dir = log_path
            .parent()
            .ok_or(LogError::LogDirNotFound(io::Error::new(
                io::ErrorKind::NotFound,
                "Log directory path cannot be determined",
            )))?;

        // Try to create the log directory, if it does not already exist.
        std::fs::create_dir_all(log_dir).map_err(LogError::LogDirCreationFailed)?;

        // Open the log file for appending, or return an error if it fails.
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(LogError::LogFileOpenFailed)?;

        // Set the target of the logger to the log file.
        builder.target(env_logger::Target::Pipe(Box::new(log_file)));
    }

    // Initialize the logging infrastructure.
    builder.init();

    Ok(())
}
