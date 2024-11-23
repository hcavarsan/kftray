use std::error::Error;
use std::fmt;
use std::io;

use futures_io;

#[derive(Debug)]
pub enum ProxyError {
    Io(io::Error),
    Configuration(String),
    Connection(String),
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

impl From<futures_io::Error> for ProxyError {
    fn from(err: futures_io::Error) -> Self {
        ProxyError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            err.to_string(),
        ))
    }
}

impl From<String> for ProxyError {
    fn from(err: String) -> Self {
        ProxyError::Configuration(err)
    }
}
