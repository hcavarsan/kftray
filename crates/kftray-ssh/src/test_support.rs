//! Helpers of the unit tests of the engine.

use std::net::{
    Ipv4Addr,
    SocketAddr,
};
use std::sync::Arc;

use async_trait::async_trait;
use russh::keys::PublicKey;
use tempfile::TempDir;

use crate::error::SshError;
use crate::key::KftrayKey;
use crate::route::{
    Authenticator,
    Credentials,
    HostKeySource,
    RouteKey,
    SshRoute,
};
use crate::testing::{
    DuplexTransport,
    MemoryHostKeyStore,
    MockSshd,
    MockSshdOptions,
};
use crate::{
    SshLease,
    SshRuntime,
};

pub(crate) const USER: &str = "kftray";

/// Logs in as [`USER`] with kftray's key.
pub(crate) struct KeyAuth;

#[async_trait]
impl Authenticator for KeyAuth {
    async fn credentials(&self, _key: &KftrayKey) -> Result<Credentials, SshError> {
        Ok(Credentials::PublicKey {
            user: USER.to_owned(),
        })
    }
}

/// Trust on first use under `alias`.
pub(crate) struct FirstUse(pub(crate) String);

#[async_trait]
impl HostKeySource for FirstUse {
    fn alias(&self) -> String {
        self.0.clone()
    }

    async fn trusted(&self) -> Result<Vec<PublicKey>, SshError> {
        Ok(Vec::new())
    }
}

/// Key login over an in-memory stream with first-use host keys.
pub(crate) fn route(mock: &MockSshd, name: &str) -> SshRoute {
    SshRoute {
        key: RouteKey::new(format!("test:{name}")),
        label: name.to_owned(),
        transport: Arc::new(DuplexTransport::new(mock)),
        auth: Arc::new(KeyAuth),
        host_keys: Arc::new(FirstUse(format!("test:{name}"))),
    }
}

pub(crate) fn peer() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 50_000))
}

/// A runtime and an sshd; the sshd accepts the runtime's key only after
/// [`Fixture::authorize_key`].
pub(crate) struct Fixture {
    pub(crate) mock: MockSshd,
    pub(crate) runtime: SshRuntime,
    _dir: TempDir,
}

impl Fixture {
    /// Key login works.
    pub(crate) async fn new() -> Self {
        let fixture = Self::with(MockSshdOptions::default()).await;
        fixture.authorize_key();
        fixture
    }

    pub(crate) async fn with(options: MockSshdOptions) -> Self {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(MemoryHostKeyStore::default());
        let runtime = SshRuntime::new(dir.path(), store).unwrap();
        let mock = MockSshd::start(options).await.unwrap();
        Self {
            mock,
            runtime,
            _dir: dir,
        }
    }

    pub(crate) fn authorize_key(&self) {
        self.mock.authorize(self.runtime.key().public());
    }

    pub(crate) async fn acquire(&self, name: &str) -> Result<SshLease, SshError> {
        self.runtime
            .acquire(route(&self.mock, name), &mut |_| {})
            .await
    }

    /// Channels to the sshd's own echo server.
    pub(crate) fn echo_opener(&self, lease: &SshLease) -> crate::SshChannelOpener {
        let echo = self.mock.echo_addr();
        lease.opener(echo.ip().to_string(), echo.port())
    }
}
