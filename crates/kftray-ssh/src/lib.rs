//! Generic SSH port-forwarding engine: one pooled SSH session per route,
//! one `direct-tcpip` channel per local connection. Providers supply the
//! transport, the authenticator and the host keys of a route.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::net::{
    Ipv4Addr,
    SocketAddr,
};
use std::path::Path;
use std::sync::Arc;

use russh::Disconnect;
use tokio::time::Instant;

pub mod consts;
mod error;
mod host_keys;
mod key;
mod route;
pub mod session;
#[cfg(test)]
mod test_support;
#[cfg(feature = "testing")]
pub mod testing;
#[cfg(feature = "iap")]
pub mod transport;

use consts::CHANNEL_OPEN_TIMEOUT;
pub use error::{
    ProviderError,
    SshError,
};
pub use host_keys::HostKeyStore;
pub use key::KftrayKey;
pub use route::{
    AsyncReadWrite,
    Authenticator,
    BoxStream,
    Credentials,
    HostKeySource,
    RouteKey,
    SshRoute,
    StreamOpener,
    Transport,
};
/// Consumers implement [`HostKeyStore`] and build keys with the same russh.
pub use russh;
pub use session::{
    SessionState,
    SshChannelOpener,
    SshLease,
    Stage,
};
use session::{
    SshPool,
    establish,
    open_direct_tcpip,
};

/// The pool of SSH sessions and kftray's key. One per process and store.
pub struct SshRuntime {
    pool: SshPool,
    key: KftrayKey,
    host_keys: Arc<dyn HostKeyStore>,
}

impl SshRuntime {
    /// Loads (or creates) kftray's key under `data_dir`.
    pub fn new(data_dir: &Path, host_keys: Arc<dyn HostKeyStore>) -> Result<Self, SshError> {
        Ok(Self {
            pool: SshPool::default(),
            key: KftrayKey::load_or_create(data_dir)?,
            host_keys,
        })
    }

    /// Shares the pooled session of `route`, connecting when there is none.
    /// `on_stage` is only called when a new session is established.
    pub async fn acquire(
        &self, route: SshRoute, on_stage: &mut (dyn FnMut(Stage) + Send),
    ) -> Result<SshLease, SshError> {
        self.pool
            .acquire(route, &self.key, &self.host_keys, on_stage)
            .await
    }

    /// Fresh, unpooled session: transport -> handshake/host key -> auth ->
    /// one channel to `host:port`, then disconnect.
    pub async fn probe(
        &self, route: SshRoute, host: &str, port: u16, on_stage: &mut (dyn FnMut(Stage) + Send),
    ) -> Result<(), SshError> {
        let (handle, _closed) = establish(&route, &self.key, &self.host_keys, on_stage).await?;

        on_stage(Stage::Channel);
        let deadline = Instant::now() + CHANNEL_OPEN_TIMEOUT;
        let peer = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let opened = open_direct_tcpip(&handle, &route.label, host, port, peer, deadline).await;
        let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
        opened.map(drop)
    }

    pub async fn forget_host_key(&self, alias: &str) -> Result<(), SshError> {
        self.host_keys.forget(alias).await
    }

    pub fn key(&self) -> &KftrayKey {
        &self.key
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;
    use crate::test_support::{
        Fixture,
        route,
    };

    #[tokio::test]
    async fn probe_should_report_every_stage_and_leave_no_session_open() {
        let fixture = Fixture::new().await;
        let echo = fixture.mock.echo_addr();
        let mut stages = Vec::new();

        fixture
            .runtime
            .probe(
                route(&fixture.mock, "probe"),
                &echo.ip().to_string(),
                echo.port(),
                &mut |stage| stages.push(stage),
            )
            .await
            .unwrap();

        assert_eq!(
            stages,
            [
                Stage::Transport,
                Stage::Handshake,
                Stage::Auth,
                Stage::Channel
            ]
        );
        assert!(fixture.mock.wait_session_count(0).await);
    }

    #[tokio::test]
    async fn probe_should_report_the_channel_stage_before_a_closed_port_fails() {
        let fixture = Fixture::new().await;
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let mut stages = Vec::new();

        let err = fixture
            .runtime
            .probe(
                route(&fixture.mock, "probe"),
                "127.0.0.1",
                port,
                &mut |stage| stages.push(stage),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, SshError::TargetUnreachable { .. }), "{err}");
        assert_eq!(stages.last(), Some(&Stage::Channel));
    }

    #[tokio::test]
    async fn forget_host_key_should_let_a_rebuilt_host_in() {
        let fixture = Fixture::new().await;
        drop(fixture.acquire("rebuilt").await.unwrap());
        fixture.mock.rotate_host_key();
        let refused = fixture.acquire("rebuilt").await.err().unwrap();
        assert!(
            matches!(refused, SshError::HostKeyMismatch { .. }),
            "{refused}"
        );

        fixture
            .runtime
            .forget_host_key("test:rebuilt")
            .await
            .unwrap();

        let lease = fixture.acquire("rebuilt").await.unwrap();
        assert_eq!(*lease.state().borrow(), SessionState::Ready);
    }
}
