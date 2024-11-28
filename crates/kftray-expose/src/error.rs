use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TunnelError {
    #[error("SSH error: {0}")]
    Ssh(String),

    #[error("Kubernetes error: {0}")]
    Kubernetes(#[from] kube::Error),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Pod not ready: {0}")]
    PodNotReady(String),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),

    #[error("SSH key error: {0}")]
    SshKey(#[from] russh_keys::Error),
}

impl From<async_ssh2_lite::Error> for TunnelError {
    fn from(e: async_ssh2_lite::Error) -> Self {
        TunnelError::Ssh(e.to_string())
    }
}

pub type TunnelResult<T> = Result<T, TunnelError>;
