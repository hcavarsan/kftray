use tokio_tungstenite::tungstenite::Error as WsError;

use crate::error::{
    ProviderError,
    SshError,
};

/// Failures of the IAP tunnel. `Display` is the message the user reads.
#[derive(Debug, thiserror::Error)]
pub enum IapError {
    #[error(
        "No access to the IAP tunnel of the VM. It needs roles/iap.tunnelResourceAccessor, or the organization requires access with a device certificate."
    )]
    PermissionDenied,
    #[error(
        "IAP could not reach port 22 of the VM. Check the firewall rule for 35.235.240.0/20 and that sshd is running."
    )]
    BackendUnreachable,
    #[error("IAP could not look up the VM: it does not exist, is stopped, or IAP is failing.")]
    InstanceLookup,
    #[error("IAP closed the tunnel: {code} {reason}")]
    Closed { code: u16, reason: String },
    #[error("IAP rejected the tunnel: HTTP {status}")]
    Http { status: u16 },
    #[error("The proxy {proxy} refused the connection to IAP: {status}")]
    ProxyConnect { proxy: String, status: u16 },
    #[error("Timed out opening the IAP tunnel")]
    Timeout,
    #[error("IAP tunnel error: {0}")]
    Io(String),
    #[error("IAP protocol error: {0}")]
    Protocol(String),
}

impl IapError {
    /// Maps a WebSocket close code received from IAP.
    pub(crate) fn from_close(code: u16, reason: &str) -> Self {
        match code {
            4033 => Self::PermissionDenied,
            4003 => Self::BackendUnreachable,
            4047 => Self::InstanceLookup,
            _ => Self::Closed {
                code,
                reason: reason.to_owned(),
            },
        }
    }

    /// Maps a WebSocket client error; HTTP statuses of a refused handshake keep their meaning.
    pub(crate) fn from_ws(error: WsError) -> Self {
        match error {
            WsError::Http(response) => match response.status().as_u16() {
                401 | 403 => Self::PermissionDenied,
                status => Self::Http { status },
            },
            other => Self::Io(other.to_string()),
        }
    }
}

impl ProviderError for IapError {
    fn user_message(&self) -> String {
        self.to_string()
    }

    fn is_retryable(&self) -> bool {
        match self {
            Self::PermissionDenied | Self::BackendUnreachable => false,
            Self::Http { status } => *status >= 500 || *status == 429,
            Self::InstanceLookup
            | Self::Closed { .. }
            | Self::ProxyConnect { .. }
            | Self::Timeout
            | Self::Io(_)
            | Self::Protocol(_) => true,
        }
    }
}

impl From<IapError> for SshError {
    fn from(error: IapError) -> Self {
        SshError::provider("iap", error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_close_should_map_known_codes_when_iap_closes_the_tunnel() {
        assert!(matches!(
            IapError::from_close(4033, ""),
            IapError::PermissionDenied
        ));
        assert!(matches!(
            IapError::from_close(4003, ""),
            IapError::BackendUnreachable
        ));
        assert!(matches!(
            IapError::from_close(4047, ""),
            IapError::InstanceLookup
        ));
        assert!(matches!(
            IapError::from_close(4999, "x"),
            IapError::Closed { code: 4999, .. }
        ));
    }

    #[test]
    fn is_retryable_should_follow_the_plan_table_when_given_each_error() {
        assert!(!IapError::PermissionDenied.is_retryable());
        assert!(!IapError::BackendUnreachable.is_retryable());
        assert!(IapError::InstanceLookup.is_retryable());
        assert!(IapError::Http { status: 503 }.is_retryable());
        assert!(IapError::Http { status: 429 }.is_retryable());
        assert!(!IapError::Http { status: 404 }.is_retryable());
        assert!(IapError::Timeout.is_retryable());
    }

    #[test]
    fn from_ws_should_map_auth_statuses_when_handshake_is_refused() {
        let response = |status: u16| {
            let mut response = tokio_tungstenite::tungstenite::http::Response::new(None);
            *response.status_mut() =
                tokio_tungstenite::tungstenite::http::StatusCode::from_u16(status).unwrap();
            WsError::Http(Box::new(response))
        };
        assert!(matches!(
            IapError::from_ws(response(401)),
            IapError::PermissionDenied
        ));
        assert!(matches!(
            IapError::from_ws(response(403)),
            IapError::PermissionDenied
        ));
        assert!(matches!(
            IapError::from_ws(response(502)),
            IapError::Http { status: 502 }
        ));
    }
}
