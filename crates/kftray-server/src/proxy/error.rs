use std::{
    error::Error,
    fmt,
    io,
    net::AddrParseError,
};

/// Represents the various error types that can occur during proxy operations.
#[derive(Debug)]
pub(crate) enum ProxyError {
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
            Self::Io(err) => write!(f, "IO Error: {err}"),
            Self::Configuration(msg) => write!(f, "Configuration Error: {msg}"),
            Self::Connection(msg) => write!(f, "Connection Error: {msg}"),
            Self::InvalidData(msg) => write!(f, "Invalid Data Error: {msg}"),
        }
    }
}

impl Error for ProxyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for ProxyError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<String> for ProxyError {
    fn from(msg: String) -> Self {
        Self::Configuration(msg)
    }
}

impl From<AddrParseError> for ProxyError {
    fn from(err: AddrParseError) -> Self {
        Self::Configuration(format!("Invalid address format: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;

    #[test]
    fn test_error_display() {
        let io_err = ProxyError::Io(io::Error::other("test io error"));
        assert_eq!(io_err.to_string(), "IO Error: test io error");

        let config_err = ProxyError::Configuration("invalid config".to_string());
        assert_eq!(
            config_err.to_string(),
            "Configuration Error: invalid config"
        );

        let conn_err = ProxyError::Connection("connection failed".to_string());
        assert_eq!(conn_err.to_string(), "Connection Error: connection failed");

        let data_err = ProxyError::InvalidData("bad data".to_string());
        assert_eq!(data_err.to_string(), "Invalid Data Error: bad data");
    }

    #[test]
    fn test_error_source() {
        let io_err = io::Error::other("test error");
        let proxy_err = ProxyError::Io(io_err);
        assert!(proxy_err.source().is_some());

        let config_err = ProxyError::Configuration("test error".to_string());
        assert!(config_err.source().is_none());

        let conn_err = ProxyError::Connection("test error".to_string());
        assert!(conn_err.source().is_none());

        let data_err = ProxyError::InvalidData("test error".to_string());
        assert!(data_err.source().is_none());
    }

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::other("test error");
        let proxy_err: ProxyError = io_err.into();
        assert!(matches!(proxy_err, ProxyError::Io(_)));
    }

    #[test]
    fn test_from_string() {
        let err_msg = "test error".to_string();
        let proxy_err: ProxyError = err_msg.into();
        assert!(matches!(proxy_err, ProxyError::Configuration(_)));
    }

    #[test]
    fn test_from_addr_parse_error() {
        let addr_err = "invalid".parse::<IpAddr>().unwrap_err();
        let proxy_err: ProxyError = addr_err.into();
        assert!(matches!(proxy_err, ProxyError::Configuration(_)));
        assert!(proxy_err.to_string().contains("Invalid address format"));
    }

    #[test]
    fn test_error_debug() {
        let err = ProxyError::Configuration("test error".to_string());
        assert!(format!("{err:?}").contains("Configuration"));
    }
}
