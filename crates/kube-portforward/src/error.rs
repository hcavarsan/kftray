use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("WebSocket upgrade failed (HTTP {status:?}): {message}")]
    UpgradeFailed {
        status: Option<u16>,
        message: String,
    },

    #[error(
        "Kubernetes API server version {detected} does not support WebSocket port-forward; \
         minimum required is {required} (KEP-4006). Upgrade the cluster or use SPDY portforward instead."
    )]
    ServerVersionTooOld {
        detected: String,
        required: &'static str,
    },

    #[error("WebSocket protocol violation in {context}: {detail}")]
    ProtocolViolation {
        context: &'static str,
        detail: String,
    },

    #[error("Session capacity exhausted: {in_use}/{capacity} channel pairs in use")]
    CapacityExhausted { in_use: usize, capacity: usize },

    // TODO: Consider adding typed source variants (e.g. `ConfigurationSource { message, #[source]
    // source }`) to preserve error provenance. Currently stringly-typed for simplicity in a
    // pre-1.0 crate.
    #[error("Configuration error: {0}")]
    Configuration(String),

    // TODO: Same as Configuration — consider carrying a `#[source]` field to preserve original
    // error chain.
    #[error("Network error: {0}")]
    Network(String),

    #[error(transparent)]
    Kube(#[from] kube::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
