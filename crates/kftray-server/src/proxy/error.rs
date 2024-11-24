use std::{
    error::Error,
    fmt,
    io,
    net::AddrParseError,
};

/// Represents the various error types that can occur during proxy operations.
#[derive(Debug)]
pub enum ProxyError {
    /// Wraps standard IO errors from networking operations
    Io(io::Error),
    /// Indicates invalid configuration settings or parameters
    Configuration(String),
    /// Represents failures in establishing or maintaining connections
    Connection(String),
    /// Indicates invalid or malformed data received during proxy operations
    InvalidData(String),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyError::Io(err) => write!(f, "IO Error: {}", err),
            ProxyError::Configuration(msg) => write!(f, "Configuration Error: {}", msg),
            ProxyError::Connection(msg) => write!(f, "Connection Error: {}", msg),
            ProxyError::InvalidData(msg) => write!(f, "Invalid Data Error: {}", msg),
        }
    }
}

impl Error for ProxyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ProxyError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for ProxyError {
    fn from(err: io::Error) -> Self {
        ProxyError::Io(err)
    }
}

impl From<String> for ProxyError {
    fn from(msg: String) -> Self {
        ProxyError::Configuration(msg)
    }
}

impl From<AddrParseError> for ProxyError {
    fn from(err: AddrParseError) -> Self {
        ProxyError::Configuration(format!("Invalid address format: {}", err))
    }
}
