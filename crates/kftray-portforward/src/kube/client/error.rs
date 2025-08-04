use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum KubeClientError {
    ConfigError {
        message: String,
        source: Option<Box<dyn Error + Send + Sync>>,
    },
    ConnectionError {
        message: String,
        source: Option<Box<dyn Error + Send + Sync>>,
    },
    AuthError {
        message: String,
        source: Option<Box<dyn Error + Send + Sync>>,
    },
    IoError(std::io::Error),
    KubeError(kube::Error),
    ParseError {
        message: String,
        context: Option<String>,
    },
    InvalidPath {
        path: String,
        reason: String,
    },
    StrategyFailed {
        strategy: String,
        attempts: Vec<String>,
    },
    ProxyError {
        message: String,
        proxy_url: String,
        source: Option<Box<dyn Error + Send + Sync>>,
    },
}

impl fmt::Display for KubeClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KubeClientError::ConfigError { message, .. } => {
                write!(f, "Configuration error: {message}")
            }
            KubeClientError::ConnectionError { message, .. } => {
                write!(f, "Connection error: {message}")
            }
            KubeClientError::AuthError { message, .. } => {
                write!(f, "Authentication error: {message}")
            }
            KubeClientError::IoError(err) => write!(f, "IO error: {err}"),
            KubeClientError::KubeError(err) => write!(f, "Kubernetes error: {err}"),
            KubeClientError::ParseError { message, context } => {
                if let Some(ctx) = context {
                    write!(f, "Parse error in {ctx}: {message}")
                } else {
                    write!(f, "Parse error: {message}")
                }
            }
            KubeClientError::InvalidPath { path, reason } => {
                write!(f, "Invalid path '{path}': {reason}")
            }
            KubeClientError::StrategyFailed { strategy, attempts } => {
                write!(
                    f,
                    "Strategy '{strategy}' failed after {} attempts: [{}]",
                    attempts.len(),
                    attempts.join(", ")
                )
            }
            KubeClientError::ProxyError {
                message, proxy_url, ..
            } => {
                write!(f, "Proxy error for '{proxy_url}': {message}")
            }
        }
    }
}

impl Error for KubeClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            KubeClientError::ConfigError { source, .. }
            | KubeClientError::ConnectionError { source, .. }
            | KubeClientError::AuthError { source, .. }
            | KubeClientError::ProxyError { source, .. } => {
                source.as_ref().map(|e| &**e as &(dyn Error + 'static))
            }
            KubeClientError::IoError(err) => Some(err),
            KubeClientError::KubeError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for KubeClientError {
    fn from(err: std::io::Error) -> Self {
        KubeClientError::IoError(err)
    }
}

impl From<kube::Error> for KubeClientError {
    fn from(err: kube::Error) -> Self {
        KubeClientError::KubeError(err)
    }
}

impl From<anyhow::Error> for KubeClientError {
    fn from(err: anyhow::Error) -> Self {
        KubeClientError::ConfigError {
            message: err.to_string(),
            source: Some(err.into()),
        }
    }
}

impl KubeClientError {
    pub fn config_error(message: impl Into<String>) -> Self {
        Self::ConfigError {
            message: message.into(),
            source: None,
        }
    }

    pub fn config_error_with_source(
        message: impl Into<String>, source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self::ConfigError {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn connection_error(message: impl Into<String>) -> Self {
        Self::ConnectionError {
            message: message.into(),
            source: None,
        }
    }

    pub fn connection_error_with_source(
        message: impl Into<String>, source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self::ConnectionError {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn auth_error(message: impl Into<String>) -> Self {
        Self::AuthError {
            message: message.into(),
            source: None,
        }
    }

    pub fn auth_error_with_source(
        message: impl Into<String>, source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self::AuthError {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::ParseError {
            message: message.into(),
            context: None,
        }
    }

    pub fn parse_error_with_context(
        message: impl Into<String>, context: impl Into<String>,
    ) -> Self {
        Self::ParseError {
            message: message.into(),
            context: Some(context.into()),
        }
    }

    pub fn invalid_path(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidPath {
            path: path.into(),
            reason: reason.into(),
        }
    }

    pub fn strategy_failed(strategy: impl Into<String>, attempts: Vec<String>) -> Self {
        Self::StrategyFailed {
            strategy: strategy.into(),
            attempts,
        }
    }

    pub fn proxy_error(message: impl Into<String>, proxy_url: impl Into<String>) -> Self {
        Self::ProxyError {
            message: message.into(),
            proxy_url: proxy_url.into(),
            source: None,
        }
    }

    pub fn proxy_error_with_source(
        message: impl Into<String>, proxy_url: impl Into<String>,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self::ProxyError {
            message: message.into(),
            proxy_url: proxy_url.into(),
            source: Some(Box::new(source)),
        }
    }
}

pub type KubeResult<T> = std::result::Result<T, KubeClientError>;
