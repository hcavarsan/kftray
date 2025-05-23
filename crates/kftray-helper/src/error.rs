use thiserror::Error;

#[derive(Error, Debug)]
pub enum HelperError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Network configuration error: {0}")]
    NetworkConfig(String),

    #[error("Address pool error: {0}")]
    AddressPool(String),

    #[error("Communication error: {0}")]
    Communication(String),

    #[error("Platform service error: {0}")]
    PlatformService(String),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    #[error("Operation not supported on this platform")]
    UnsupportedPlatform,
}
