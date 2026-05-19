use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("port-forward upgrade failed (HTTP {status:?}): {message}")]
    UpgradeFailed {
        status: Option<u16>,
        message: String,
    },

    #[error("protocol violation in {context}: {detail}")]
    ProtocolViolation {
        context: &'static str,
        detail: String,
    },

    #[error("session capacity exhausted: {in_use}/{capacity} channel pairs in use")]
    CapacityExhausted { in_use: usize, capacity: usize },

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error(transparent)]
    Kube(#[from] kube::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Spdy(#[from] spdy_mux::Error),
}
