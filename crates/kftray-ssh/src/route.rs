use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use russh::keys::{
    Certificate,
    PublicKey,
};
use tokio::io::{
    AsyncRead,
    AsyncWrite,
};

use crate::error::SshError;
use crate::key::KftrayKey;

/// A bidirectional byte stream.
pub trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite> AsyncReadWrite for T {}

pub type BoxStream = Box<dyn AsyncReadWrite + Unpin + Send>;

/// Identity of a session in the pool: routes with the same key share one
/// SSH session. Providers prefix it with their type name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RouteKey(String);

impl RouteKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }
}

impl fmt::Display for RouteKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Everything the engine needs to reach and log in to one sshd.
pub struct SshRoute {
    pub key: RouteKey,
    /// Human-readable name used in logs and messages.
    pub label: String,
    pub transport: Arc<dyn Transport>,
    pub auth: Arc<dyn Authenticator>,
    pub host_keys: Arc<dyn HostKeySource>,
}

#[async_trait]
pub trait Transport: Send + Sync {
    /// Byte stream to sshd. Low-level resume (IAP) lives inside.
    async fn connect(&self) -> Result<BoxStream, SshError>;
}

#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Called for every new session; may renew a key or sign a certificate.
    async fn credentials(&self, key: &KftrayKey) -> Result<Credentials, SshError>;
}

#[expect(
    clippy::large_enum_variant,
    reason = "built once per session; boxing buys nothing"
)]
pub enum Credentials {
    PublicKey { user: String },
    Certificate { user: String, cert: Certificate },
}

impl Credentials {
    pub fn user(&self) -> &str {
        match self {
            Self::PublicKey { user } | Self::Certificate { user, .. } => user,
        }
    }
}

#[async_trait]
pub trait HostKeySource: Send + Sync {
    /// Key under which the first-use host key is stored, e.g.
    /// `gcp:compute.{id}`.
    fn alias(&self) -> String;
    /// Host keys the provider vouches for; empty means trust on first use.
    async fn trusted(&self) -> Result<Vec<PublicKey>, SshError>;
}

/// Opens one stream per accepted local connection.
#[async_trait]
pub trait StreamOpener: Send + Sync {
    async fn open(&self, peer: SocketAddr) -> Result<BoxStream, SshError>;
}
