use std::fmt;
use std::io;

#[derive(Debug)]
pub enum ProxyError {
    Io(io::Error),
    Configuration(String),
    Connection(String),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyError::Io(err) => write!(f, "IO Error: {}", err),
            ProxyError::Configuration(msg) => write!(f, "Configuration Error: {}", msg),
            ProxyError::Connection(msg) => write!(f, "Connection Error: {}", msg),
        }
    }
}

impl From<io::Error> for ProxyError {
    fn from(err: io::Error) -> Self {
        ProxyError::Io(err)
    }
}
