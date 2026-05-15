use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::channel::connect::build_channel_session;
use crate::channel::keepalive::{
    RecoveryCallback,
    RecoverySignal,
};
#[cfg(feature = "spdy-tunnel")]
use crate::connect::upgrade_spdy_portforward;
use crate::connect::{
    KeepaliveConfig,
    upgrade_portforward,
};
use crate::error::Error;
use crate::session::Session;
use crate::subprotocol::Subprotocol;

const DEFAULT_CAPACITY: usize = 64;
const MAX_CAPACITY: usize = 127;
const DEFAULT_PING: Duration = Duration::from_secs(15);
const DEFAULT_WATCHDOG: Duration = Duration::from_secs(30);
const DEFAULT_DRAIN: Duration = Duration::from_secs(2);

/// Top-level entry point bundling a `kube::Client` with its cluster URL.
#[derive(Clone)]
pub struct Client {
    kube: kube::Client,
    cluster_url: http::Uri,
}

impl Client {
    pub fn new(kube_client: kube::Client, cluster_url: http::Uri) -> Self {
        Self {
            kube: kube_client,
            cluster_url,
        }
    }

    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    pub fn session<'c>(
        &'c self, namespace: impl Into<String>, pod: impl Into<String>, port: u16,
    ) -> SessionBuilder<'c> {
        SessionBuilder {
            client: self,
            namespace: namespace.into(),
            pod: pod.into(),
            port,
            capacity: DEFAULT_CAPACITY,
            subprotocols: vec![Subprotocol::V5, Subprotocol::V4],
            ping_interval: DEFAULT_PING,
            watchdog_timeout: DEFAULT_WATCHDOG,
            drain_timeout: DEFAULT_DRAIN,
            cancel: None,
            recovery_callback: None,
        }
    }

    pub fn kube_client(&self) -> &kube::Client {
        &self.kube
    }

    pub fn cluster_url(&self) -> &http::Uri {
        &self.cluster_url
    }
}

#[derive(Default)]
pub struct ClientBuilder {
    kube: Option<kube::Client>,
    cluster_url: Option<http::Uri>,
}

impl ClientBuilder {
    pub fn kube_client(mut self, c: kube::Client) -> Self {
        self.kube = Some(c);
        self
    }

    pub fn cluster_url(mut self, u: http::Uri) -> Self {
        self.cluster_url = Some(u);
        self
    }

    pub fn build(self) -> Result<Client, Error> {
        let kube = self
            .kube
            .ok_or_else(|| Error::Configuration("kube_client is required".into()))?;
        let cluster_url = self
            .cluster_url
            .ok_or_else(|| Error::Configuration("cluster_url is required".into()))?;
        Ok(Client::new(kube, cluster_url))
    }
}

/// Builder for opening a [`Session`].
pub struct SessionBuilder<'c> {
    client: &'c Client,
    namespace: String,
    pod: String,
    port: u16,
    capacity: usize,
    subprotocols: Vec<Subprotocol>,
    ping_interval: Duration,
    watchdog_timeout: Duration,
    drain_timeout: Duration,
    cancel: Option<CancellationToken>,
    recovery_callback: Option<RecoveryCallback>,
}

impl<'c> SessionBuilder<'c> {
    /// Number of pre-allocated channel pairs (default 64, max 127).
    /// Cap is 127 because each pair uses (data, error) channel IDs and
    /// 127 * 2 = 254 fits in the single-byte channel space (0xFF).
    pub fn capacity(mut self, n: usize) -> Self {
        self.capacity = n;
        self
    }

    pub fn subprotocols(mut self, prefs: &[Subprotocol]) -> Self {
        self.subprotocols = prefs.to_vec();
        self
    }

    pub fn keepalive(mut self, ping: Duration, watchdog: Duration) -> Self {
        self.ping_interval = ping;
        self.watchdog_timeout = watchdog;
        self
    }

    pub fn shutdown_grace(mut self, drain: Duration) -> Self {
        self.drain_timeout = drain;
        self
    }

    pub fn cancellation_token(mut self, t: CancellationToken) -> Self {
        self.cancel = Some(t);
        self
    }

    pub fn on_recovery<F>(mut self, cb: F) -> Self
    where
        F: Fn(RecoverySignal) + Send + Sync + 'static,
    {
        self.recovery_callback = Some(Arc::new(cb));
        self
    }

    pub async fn open(self) -> Result<Session, Error> {
        if self.capacity == 0 {
            return Err(Error::Configuration("capacity must be > 0".into()));
        }
        if self.capacity > MAX_CAPACITY {
            return Err(Error::Configuration(format!(
                "capacity {} exceeds maximum of {MAX_CAPACITY}",
                self.capacity
            )));
        }
        let cancel = self.cancel.unwrap_or_default();
        let recovery_callback: RecoveryCallback = self
            .recovery_callback
            .unwrap_or_else(|| Arc::new(|_signal| {}));

        // Try SPDY tunnel first (no ?ports= in URL, SPDY-only subprotocol).
        // Falls back to channel protocol if the server rejects SPDY.
        #[cfg(feature = "spdy-tunnel")]
        {
            match upgrade_spdy_portforward(
                self.client.kube_client(),
                self.client.cluster_url(),
                &self.namespace,
                &self.pod,
                &recovery_callback,
            )
            .await
            {
                Ok(upgraded) => {
                    match crate::spdy_tunnel::Session::new(
                        upgraded.ws, self.port, cancel.clone(),
                    )
                    .await
                    {
                        Ok(spdy_session) => return Ok(Session::from_spdy(spdy_session)),
                        Err(e) => {
                            tracing::debug!("SPDY session init failed: {e}");
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("SPDY upgrade failed, falling back to channel: {e}");
                }
            }
        }

        // Channel protocol path: ports in URL, v5/v4 subprotocols
        let upgraded = upgrade_portforward(
            self.client.kube_client(),
            self.client.cluster_url(),
            &self.namespace,
            &self.pod,
            self.port,
            self.capacity,
            &recovery_callback,
        )
        .await?;

        let keepalive_config = KeepaliveConfig {
            ping_interval: self.ping_interval,
            watchdog_timeout: self.watchdog_timeout,
        };

        match upgraded.protocol {
            Subprotocol::V4 | Subprotocol::V5 => {
                let channel_session = build_channel_session(
                    upgraded,
                    self.capacity,
                    cancel,
                    keepalive_config,
                    self.drain_timeout,
                    recovery_callback,
                )
                .await?;
                Ok(Session::from_channel(channel_session))
            }
            #[cfg(feature = "spdy-tunnel")]
            Subprotocol::Spdy31Tunnel => {
                // Shouldn't happen since upgrade_portforward uses channel subprotocols,
                // but handle it gracefully.
                let spdy_session = crate::spdy_tunnel::Session::new(upgraded.ws, self.port, cancel).await?;
                Ok(Session::from_spdy(spdy_session))
            }
        }
    }
}
