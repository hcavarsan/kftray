use std::fmt;

use crate::kube::client::error::KubeClientError;

#[derive(Debug)]
#[non_exhaustive]
pub enum PortForwardError {
    PodLookupFailed {
        retry_count: usize,
        last_error: String,
        selector: String,
    },
    ClientDisconnected {
        peer_addr: Option<String>,
        stage: String,
    },
    StreamCreationFailed {
        pod_name: String,
        port: u16,
        error: String,
    },
    NetworkError {
        message: String,
        recoverable: bool,
    },
    ConfigurationError {
        message: String,
    },
    ResourceExhausted {
        resource_type: String,
        current_usage: Option<usize>,
        limit: Option<usize>,
    },
    TimeoutError {
        operation: String,
        timeout_duration: std::time::Duration,
    },
    WebsocketUpgradeFailed {
        status: u16,
        message: String,
    },
    SubprotocolNegotiationFailed {
        offered: &'static str,
        returned: Option<String>,
    },
    KeepaliveTimeout {
        last_pong_age_ms: u64,
    },
    WebsocketProtocolViolation {
        context: &'static str,
        detail: String,
    },
    /// Catch-all for errors during incremental migration from `Result<T,
    /// String>`.
    Internal(String),
    /// IO errors (file operations, network).
    Io(std::io::Error),
    /// Kubernetes client construction/auth errors.
    KubeClient(KubeClientError),
    /// Kubernetes API call errors (deploy, list, delete).
    KubeApi(String),
    /// Hosts file read/write errors.
    HostsFile(String),
    /// TLS/SSL certificate errors.
    Ssl(String),
    /// Expose resource creation/deletion errors.
    Expose(String),
    /// Address allocation errors (loopback binding).
    AddressAllocation(String),
}

impl fmt::Display for PortForwardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PodLookupFailed {
                retry_count,
                last_error,
                selector,
            } => write!(
                f,
                "Pod lookup failed for selector '{selector}' after {retry_count} retries: {last_error}"
            ),
            Self::ClientDisconnected { peer_addr, stage } => {
                if let Some(addr) = peer_addr {
                    write!(f, "Client {addr} disconnected during {stage}")
                } else {
                    write!(f, "Client disconnected during {stage}")
                }
            }
            Self::StreamCreationFailed {
                pod_name,
                port,
                error,
            } => write!(
                f,
                "Failed to create stream to pod '{pod_name}' port {port}: {error}"
            ),
            Self::NetworkError {
                message,
                recoverable,
            } => {
                if *recoverable {
                    write!(f, "Recoverable network error: {message}")
                } else {
                    write!(f, "Network error: {message}")
                }
            }
            Self::ConfigurationError { message } => {
                write!(f, "Configuration error: {message}")
            }
            Self::ResourceExhausted {
                resource_type,
                current_usage,
                limit,
            } => match (current_usage, limit) {
                (Some(current), Some(max)) => {
                    write!(f, "Resource exhausted: {resource_type} ({current}/{max})")
                }
                (Some(current), None) => {
                    write!(f, "Resource exhausted: {resource_type} ({current})")
                }
                (None, Some(max)) => {
                    write!(f, "Resource exhausted: {resource_type} (limit: {max})")
                }
                (None, None) => write!(f, "Resource exhausted: {resource_type}"),
            },
            Self::TimeoutError {
                operation,
                timeout_duration,
            } => write!(
                f,
                "Operation '{operation}' timed out after {timeout_duration:?}"
            ),
            Self::WebsocketUpgradeFailed { status, message } => {
                if *status == 0 {
                    write!(f, "WebSocket upgrade failed: {message}")
                } else {
                    write!(f, "WebSocket upgrade failed (HTTP {status}): {message}")
                }
            }
            Self::SubprotocolNegotiationFailed { offered, returned } => write!(
                f,
                "Subprotocol negotiation failed: offered '{offered}', server returned {returned:?}"
            ),
            Self::KeepaliveTimeout { last_pong_age_ms } => write!(
                f,
                "WebSocket keepalive timed out: last Pong was {last_pong_age_ms}ms ago"
            ),
            Self::WebsocketProtocolViolation { context, detail } => {
                write!(f, "WebSocket protocol violation in {context}: {detail}")
            }
            Self::Internal(msg) => write!(f, "{msg}"),
            Self::Io(err) => write!(f, "IO error: {err}"),
            Self::KubeClient(err) => write!(f, "{err}"),
            Self::KubeApi(msg) => write!(f, "Kubernetes API error: {msg}"),
            Self::HostsFile(msg) => write!(f, "Hosts file error: {msg}"),
            Self::Ssl(msg) => write!(f, "SSL error: {msg}"),
            Self::Expose(msg) => write!(f, "Expose error: {msg}"),
            Self::AddressAllocation(msg) => {
                write!(f, "Address allocation error: {msg}")
            }
        }
    }
}

impl std::error::Error for PortForwardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::KubeClient(err) => Some(err),
            _ => None,
        }
    }
}

impl From<String> for PortForwardError {
    fn from(s: String) -> Self {
        Self::Internal(s)
    }
}

impl From<&str> for PortForwardError {
    fn from(s: &str) -> Self {
        Self::Internal(s.to_owned())
    }
}

impl From<std::io::Error> for PortForwardError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<KubeClientError> for PortForwardError {
    fn from(err: KubeClientError) -> Self {
        Self::KubeClient(err)
    }
}

impl PortForwardError {
    pub fn pod_lookup_failed(
        retry_count: usize, last_error: impl Into<String>, selector: impl Into<String>,
    ) -> Self {
        Self::PodLookupFailed {
            retry_count,
            last_error: last_error.into(),
            selector: selector.into(),
        }
    }

    pub fn client_disconnected(peer_addr: Option<String>, stage: impl Into<String>) -> Self {
        Self::ClientDisconnected {
            peer_addr,
            stage: stage.into(),
        }
    }

    pub fn stream_creation_failed(
        pod_name: impl Into<String>, port: u16, error: impl Into<String>,
    ) -> Self {
        Self::StreamCreationFailed {
            pod_name: pod_name.into(),
            port,
            error: error.into(),
        }
    }

    pub fn recoverable_network_error(message: impl Into<String>) -> Self {
        Self::NetworkError {
            message: message.into(),
            recoverable: true,
        }
    }

    pub fn fatal_network_error(message: impl Into<String>) -> Self {
        Self::NetworkError {
            message: message.into(),
            recoverable: false,
        }
    }

    pub fn configuration_error(message: impl Into<String>) -> Self {
        Self::ConfigurationError {
            message: message.into(),
        }
    }

    pub fn resource_exhausted(
        resource_type: impl Into<String>, current_usage: Option<usize>, limit: Option<usize>,
    ) -> Self {
        Self::ResourceExhausted {
            resource_type: resource_type.into(),
            current_usage,
            limit,
        }
    }

    pub fn timeout_error(
        operation: impl Into<String>, timeout_duration: std::time::Duration,
    ) -> Self {
        Self::TimeoutError {
            operation: operation.into(),
            timeout_duration,
        }
    }

    pub const fn is_recoverable(&self) -> bool {
        match self {
            Self::PodLookupFailed { .. } => true,
            Self::ClientDisconnected { .. } => true,
            Self::StreamCreationFailed { .. } => true,
            Self::NetworkError { recoverable, .. } => *recoverable,
            Self::ConfigurationError { .. } => false,
            Self::ResourceExhausted { .. } => true,
            Self::TimeoutError { .. } => true,
            Self::WebsocketUpgradeFailed { .. } => true,
            Self::SubprotocolNegotiationFailed { .. } => false,
            Self::KeepaliveTimeout { .. } => true,
            Self::WebsocketProtocolViolation { .. } => true,
            Self::Io(_) => true,
            Self::KubeClient(_) => true,
            Self::KubeApi(_) => true,
            Self::AddressAllocation(_) => true,
            Self::HostsFile(_) => false,
            Self::Ssl(_) => false,
            Self::Expose(_) => true,
            Self::Internal(_) => false,
        }
    }

    pub fn should_stop_server(&self) -> bool {
        match self {
            Self::ConfigurationError { .. } => true,
            Self::NetworkError { recoverable, .. } => !recoverable,
            Self::SubprotocolNegotiationFailed { .. } => true,
            Self::Ssl(_) => true,
            _ => false,
        }
    }
}

pub type PortForwardResult<T> = Result<T, PortForwardError>;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_pod_lookup_failed() {
        let error = PortForwardError::pod_lookup_failed(3, "Pod not found", "app=web");
        assert!(error.is_recoverable());
        assert!(!error.should_stop_server());
        assert!(error.to_string().contains("app=web"));
        assert!(error.to_string().contains("3 retries"));
    }

    #[test]
    fn test_client_disconnected() {
        let error = PortForwardError::client_disconnected(
            Some("127.0.0.1:12345".to_string()),
            "authentication",
        );
        assert!(error.is_recoverable());
        assert!(!error.should_stop_server());
        assert!(error.to_string().contains("127.0.0.1:12345"));
        assert!(error.to_string().contains("authentication"));
    }

    #[test]
    fn test_stream_creation_failed() {
        let error =
            PortForwardError::stream_creation_failed("test-pod", 8080, "Connection refused");
        assert!(error.is_recoverable());
        assert!(!error.should_stop_server());
        assert!(error.to_string().contains("test-pod"));
        assert!(error.to_string().contains("8080"));
    }

    #[test]
    fn test_network_errors() {
        let recoverable = PortForwardError::recoverable_network_error("Temporary DNS failure");
        assert!(recoverable.is_recoverable());
        assert!(!recoverable.should_stop_server());

        let fatal = PortForwardError::fatal_network_error("Network interface down");
        assert!(!fatal.is_recoverable());
        assert!(fatal.should_stop_server());
    }

    #[test]
    fn test_configuration_error() {
        let error = PortForwardError::configuration_error("Invalid kubeconfig");
        assert!(!error.is_recoverable());
        assert!(error.should_stop_server());
    }

    #[test]
    fn test_resource_exhausted() {
        let error = PortForwardError::resource_exhausted("connections", Some(100), Some(100));
        assert!(error.is_recoverable());
        assert!(!error.should_stop_server());
        assert!(error.to_string().contains("(100/100)"));
    }

    #[test]
    fn test_timeout_error() {
        let error = PortForwardError::timeout_error("pod lookup", Duration::from_secs(30));
        assert!(error.is_recoverable());
        assert!(!error.should_stop_server());
        assert!(error.to_string().contains("30s"));
    }
}
