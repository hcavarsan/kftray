use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Notify;

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
};

/// Defines the core proxy handling behavior that must be implemented by
/// concrete proxy types. This trait enables a common interface for different
/// proxy implementations (TCP, UDP).
#[async_trait]
pub trait ProxyHandler: Send + Sync {
    /// Starts the proxy server with the given configuration and shutdown
    /// signal.
    ///
    /// # Parameters
    /// * `config` - Configuration containing proxy settings like ports and
    ///   target details
    /// * `shutdown` - Notification mechanism to signal when the proxy should
    ///   stop
    ///
    /// # Returns
    /// * `Result<(), ProxyError>` - Success if proxy runs and shuts down
    ///   cleanly, or error details
    async fn start(&self, config: ProxyConfig, shutdown: Arc<Notify>) -> Result<(), ProxyError>;
}
