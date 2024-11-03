use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Invalid path: {0}")]
    Path(PathBuf),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Host file error: {0}")]
    HostFile(String),

    #[error("Port error: {0}")]
    Port(String),

    #[error("Database connection error: {0}")]
    DatabaseConnection(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("GitHub error: {0}")]
    Github(String),

    #[error("Kubernetes error: {0}")]
    Kubernetes(String),

    #[error("State error: {0}")]
    State(String),
}

impl Error {
    pub fn config<T: Into<String>>(msg: T) -> Self {
        Error::Config(msg.into())
    }

    pub fn validation<T: Into<String>>(msg: T) -> Self {
        Error::Validation(msg.into())
    }

    pub fn db_connection<T: Into<String>>(msg: T) -> Self {
        Error::DatabaseConnection(msg.into())
    }

    pub fn host_file<T: Into<String>>(msg: T) -> Self {
        Error::HostFile(msg.into())
    }

    pub fn port<T: Into<String>>(msg: T) -> Self {
        Error::Port(msg.into())
    }

    pub fn github<T: Into<String>>(msg: T) -> Self {
        Error::Github(msg.into())
    }

    pub fn kubernetes<T: Into<String>>(msg: T) -> Self {
        Error::Kubernetes(msg.into())
    }

    pub fn state<T: Into<String>>(msg: T) -> Self {
        Error::State(msg.into())
    }

    pub fn empty_namespace() -> Self {
        Error::Validation("Namespace must be specified".into())
    }

    pub fn empty_protocol() -> Self {
        Error::Validation("Protocol must be specified".into())
    }

    pub fn invalid_local_port() -> Self {
        Error::Validation("Local port cannot be 0".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let config_err = Error::config("test error");
        assert!(matches!(config_err, Error::Config(_)));

        let validation_err = Error::validation("invalid input");
        assert!(matches!(validation_err, Error::Validation(_)));

        let github_err = Error::github("github error");
        assert!(matches!(github_err, Error::Github(_)));
    }

    #[test]
    fn test_error_display() {
        let err = Error::config("test error");
        assert_eq!(err.to_string(), "Configuration error: test error");

        let err = Error::kubernetes("kube error");
        assert_eq!(err.to_string(), "Kubernetes error: kube error");
    }
}
