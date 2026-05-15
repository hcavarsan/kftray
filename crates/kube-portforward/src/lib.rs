//! Kubernetes port-forward over WebSocket with channel multiplexing.
//!
//! Speaks `v5.channel.k8s.io` (preferred) and `v4.channel.k8s.io` (fallback)
//! per [KEP-4006](https://github.com/kubernetes/enhancements/issues/4006).
//! Multiplexes N concurrent local TCP connections over ONE WebSocket upgrade
//! by encoding the target pod port N times in the URL.
//!
//! ## Channel lifecycle
//!
//! Channel pairs are **one-shot**: the apiserver binds each pair to a single
//! backend pod connection at upgrade time, so an ID pair cannot be reused for
//! a fresh connection after either side has sent the v5 `[0xFF, channel]`
//! close signal. The allocator therefore decrements live-count on release
//! but never returns IDs to the free-list.
//!
//! ## CLOSE_WAIT and v4 limitation
//!
//! On v5, `AsyncWrite::shutdown()` (and dropping a [`Stream`]) emits `0xFF`
//! to the apiserver, which tears down the pod-side connection and echoes
//! `0xFF` back. The reader translates that into EOF on the read half, so a
//! client like `kftray-portforward`'s `forward_streams` can finish copying
//! and drop its local socket, avoiding CLOSE_WAIT accumulation.
//!
//! On v4 there is no half-close frame, so once the local writer closes the
//! pod-side connection cannot be signalled to drain; CLOSE_WAIT on v4 is
//! unfixable from the client layer. v4 is the rare fallback for pre-1.30
//! clusters (see KEP-4006).
//!
//! ## Why not [`kube::Api::portforward`]?
//!
//! `kube::Api::portforward` opens a new WebSocket upgrade for every stream
//! pair, lacks keepalive, and surfaces opaque errors. This crate multiplexes
//! up to 64 stream pairs per upgrade, runs a Ping/Pong watchdog, exposes
//! structured `thiserror` errors, and detects pre-1.30 clusters with a
//! KEP-4006-citing error.
//!
//! ## Quick Start
//!
//! ```no_run
//! use kube_portforward::Client;
//! use tokio::io::AsyncWriteExt;
//!
//! # async fn run(kube: kube::Client, url: http::Uri) -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new(kube, url);
//! let session = client.session("default", "nginx", 80).open().await?;
//! let mut stream = session.connect().await?;
//! stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;
//! session.close().await?;
//! # Ok(()) }
//! ```

pub(crate) mod channel;
pub(crate) mod client;
pub(crate) mod connect;
pub(crate) mod error;
pub(crate) mod forwarder;
pub(crate) mod pod_watch;
pub(crate) mod session;
#[cfg(feature = "spdy-tunnel")]
pub(crate) mod spdy_tunnel;
pub(crate) mod stream;
pub(crate) mod subprotocol;
pub(crate) mod version;

pub use channel::keepalive::{
    RecoveryCallback,
    RecoverySignal,
};
pub use client::{
    Client,
    ClientBuilder,
    SessionBuilder,
};
pub use error::Error;
pub use forwarder::{
    Forwarder,
    ForwarderBuilder,
};
pub use pod_watch::{
    PodChange,
    PodSelector,
    PodWatcher,
    ReadyPod,
};
pub use session::Session;
pub use stream::{
    DataStream,
    ErrorStream,
    Stream,
};
pub use subprotocol::Subprotocol;
pub use version::VersionInfo;
#[cfg(feature = "version-cache")]
pub use version::{
    VersionCache,
    global_version_cache,
};
