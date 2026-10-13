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

/// Renders `head` followed by every distinct message in the source chain, so
/// errors that wrap a cause (`connection refused`, a failed exec plugin) do
/// not hide it behind a generic summary.
fn with_causes(head: String, mut cause: Option<&(dyn Error + 'static)>) -> String {
    let mut text = head;
    while let Some(err) = cause {
        let part = err.to_string();
        if !part.is_empty() && !text.contains(&part) {
            text.push_str(": ");
            text.push_str(&part);
        }
        cause = err.source();
    }
    text
}

impl fmt::Display for KubeClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cause = self.source();
        match self {
            KubeClientError::ConfigError { message, .. } => f.write_str(&with_causes(
                format!("Configuration error: {message}"),
                cause,
            )),
            KubeClientError::ConnectionError { message, .. } => {
                f.write_str(&with_causes(format!("Connection error: {message}"), cause))
            }
            KubeClientError::AuthError { message, .. } => f.write_str(&with_causes(
                format!("Authentication error: {message}"),
                cause,
            )),
            KubeClientError::IoError(err) => write!(f, "IO error: {err}"),
            KubeClientError::KubeError(err) => f.write_str(&with_causes(
                format!("Kubernetes error: {err}"),
                err.source(),
            )),
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
            } => f.write_str(&with_causes(
                format!("Proxy error for '{proxy_url}': {message}"),
                cause,
            )),
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

#[cfg(test)]
mod tests {
    use std::io;

    use kftray_telemetry::{
        FailureKind,
        classify,
    };

    use super::*;

    #[test]
    fn connection_error_display_includes_the_source() {
        let err = KubeClientError::connection_error_with_source(
            "Failed to connect to Kubernetes API server",
            io::Error::from(io::ErrorKind::ConnectionRefused),
        );

        let text = err.to_string();
        assert!(
            text.contains("Failed to connect to Kubernetes API server"),
            "{text}"
        );
        assert!(text.contains("connection refused"), "{text}");
        assert_eq!(classify(&text), FailureKind::Connection);
    }

    #[test]
    fn display_walks_the_whole_source_chain() {
        let inner = io::Error::other("dns error: no record found");
        let middle = KubeClientError::connection_error_with_source("lookup failed", inner);
        let err = KubeClientError::proxy_error_with_source("proxy down", "http://p:1", middle);

        let text = err.to_string();
        assert!(text.contains("lookup failed"), "{text}");
        assert!(text.contains("dns error: no record found"), "{text}");
    }

    #[test]
    fn display_does_not_repeat_a_source_already_in_the_message() {
        let err = KubeClientError::from(anyhow::anyhow!("bad kubeconfig"));

        assert_eq!(err.to_string(), "Configuration error: bad kubeconfig");
    }

    #[test]
    fn auth_error_display_keeps_the_exec_plugin_failure() {
        let err = KubeClientError::auth_error_with_source(
            "Failed to create auth layer",
            io::Error::new(
                io::ErrorKind::NotFound,
                "unable to run auth exec: gke-gcloud-auth-plugin",
            ),
        );

        assert!(err.to_string().contains("gke-gcloud-auth-plugin"), "{err}");
    }
}
