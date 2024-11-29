use std::io;
use std::num::ParseIntError;

use kube::config::KubeconfigError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("SSH error: {0}")]
    Ssh(String),

    #[error("Kubernetes error: {0}")]
    Kubernetes(#[from] kube::Error),

    #[error("Kubeconfig error: {0}")]
    KubeConfig(#[from] KubeconfigError),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Pod not ready: {0}")]
    PodNotReady(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("SSH key error: {0}")]
    SshKey(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Resource error: {0}")]
    Resource(String),

    #[error("Timeout error: {0}")]
    Timeout(String),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("Invalid port number")]
    InvalidPort,

    #[error("Failed to parse integer: {0}")]
    ParseInt(#[from] ParseIntError),

    #[error("Resource failed: {0}")]
    ResourceFailed(String),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Error::Other(anyhow::anyhow!(e))
    }
}

impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Error::Other(anyhow::anyhow!(e.to_string()))
    }
}

impl From<async_ssh2_lite::Error> for Error {
    fn from(e: async_ssh2_lite::Error) -> Self {
        Error::Ssh(e.to_string())
    }
}
