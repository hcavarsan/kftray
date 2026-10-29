use crate::error::{
    ProviderError,
    SshError,
};

pub(super) const PROVIDER: &str = "gcp-bastion";

/// Everything that can go wrong talking to Google. `Display` is the message
/// shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum GcpError {
    #[error(
        "Google credentials not found. Run \"gcloud auth application-default login\" or set GOOGLE_APPLICATION_CREDENTIALS."
    )]
    AdcMissing,
    #[error("The Google session expired. Run \"gcloud auth application-default login\" again.")]
    TokenRefresh,
    #[error("Google authentication failed: {message}")]
    Auth { message: String, retryable: bool },
    #[error(
        "Could not find the e-mail of the Google credential. Use a user account, a service account, or workload identity federation with service account impersonation."
    )]
    IdentityUnknown,
    #[error("The VM {instance} does not exist in {project}/{zone}.")]
    InstanceNotFound {
        instance: String,
        project: String,
        zone: String,
    },
    #[error("The VM {instance} is {status}.")]
    InstanceNotRunning { instance: String, status: String },
    #[error(
        "The VM does not use OS Login. The gcp-bastion provider only connects to VMs with OS Login."
    )]
    OsLoginRequired,
    #[error("The VM requires 2FA or a security key for OS Login, which kftray does not support.")]
    OsLoginInteractive,
    #[error(
        "Missing OS Login permission: roles/compute.osLogin on the VM or project, and roles/iam.serviceAccountUser on the VM's service account. Users outside the organization also need roles/compute.osLoginExternalUser."
    )]
    OsLoginPermission,
    #[error("OS Login has no POSIX account for {email} in {project}.")]
    NoPosixAccount { email: String, project: String },
    /// `url` has no query string and `body` is truncated.
    #[error("Google API {url} answered {status}: {body}")]
    Api {
        url: String,
        status: u16,
        body: String,
    },
    #[error("Could not reach the Google API: {0}")]
    Http(String),
    /// Failure of the injected `TokenSource`, kept as it came.
    #[error("{0}")]
    Token(Box<SshError>),
}

impl ProviderError for GcpError {
    fn user_message(&self) -> String {
        self.to_string()
    }

    fn is_retryable(&self) -> bool {
        match self {
            Self::InstanceNotRunning { .. } | Self::Http(_) => true,
            Self::Auth { retryable, .. } => *retryable,
            Self::Api { status, .. } => *status >= 500 || *status == 429,
            Self::Token(error) => error.is_retryable(),
            Self::AdcMissing
            | Self::TokenRefresh
            | Self::IdentityUnknown
            | Self::InstanceNotFound { .. }
            | Self::OsLoginRequired
            | Self::OsLoginInteractive
            | Self::OsLoginPermission
            | Self::NoPosixAccount { .. } => false,
        }
    }
}

impl From<GcpError> for SshError {
    fn from(error: GcpError) -> Self {
        match error {
            GcpError::Token(inner) => *inner,
            error => SshError::provider(PROVIDER, error),
        }
    }
}
