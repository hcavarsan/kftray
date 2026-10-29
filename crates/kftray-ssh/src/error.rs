use crate::session::Stage;

/// Error of a provider piece (transport, authenticator, host key source).
///
/// `Display` is for logs; `user_message` is what the user reads.
pub trait ProviderError: std::error::Error + Send + Sync + 'static {
    fn user_message(&self) -> String;
    fn is_retryable(&self) -> bool;
}

/// Every error the engine reports. `Display` is the user-facing message.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error(
        "The identity of {route} changed: expected host key {known}, got {presented}. If the bastion was rebuilt, edit the config, run Test connection and choose Forget host key."
    )]
    HostKeyMismatch {
        route: String,
        alias: String,
        known: String,
        presented: String,
    },
    #[error("{route} rejected the login of {user}")]
    AuthRejected { route: String, user: String },
    #[error("The sshd of {route} does not allow port forwarding (AllowTcpForwarding)")]
    ForwardingProhibited { route: String },
    #[error(
        "{route} could not reach {host}:{port}. Check the firewall and that the service listens on that port"
    )]
    TargetUnreachable {
        route: String,
        host: String,
        port: u16,
    },
    #[error("Connection to {route} lost: {reason}")]
    Transport { route: String, reason: String },
    #[error("Timed out during {stage} with {route}")]
    Timeout { route: String, stage: Stage },
    #[error("SSH error with {route}: {reason}")]
    Protocol { route: String, reason: String },
    #[error("{}", error.user_message())]
    Provider {
        provider: &'static str,
        error: Box<dyn ProviderError>,
    },
    #[error("kftray SSH key: {0}")]
    Key(String),
    #[error("Host key store: {0}")]
    HostKeyStore(String),
}

impl SshError {
    /// Wraps a provider error.
    pub fn provider(provider: &'static str, error: impl ProviderError) -> Self {
        Self::Provider {
            provider,
            error: Box::new(error),
        }
    }

    /// Whether trying again later can succeed without user action.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::TargetUnreachable { .. }
            | Self::Transport { .. }
            | Self::Timeout { .. }
            | Self::Protocol { .. } => true,
            Self::Provider { error, .. } => error.is_retryable(),
            Self::HostKeyMismatch { .. }
            | Self::AuthRejected { .. }
            | Self::ForwardingProhibited { .. }
            | Self::Key(_)
            | Self::HostKeyStore(_) => false,
        }
    }
}
