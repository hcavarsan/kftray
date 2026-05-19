//! Kubernetes port-forward over SPDY/3.1 with kubectl-style fallback.
//!
//! Speaks `SPDY/3.1+portforward.k8s.io` (tunnelled inside WebSocket
//! binary messages) by default, falling back to the legacy `Upgrade:
//! SPDY/3.1` path against apiservers that reject the WebSocket
//! subprotocol. Multiplexes many concurrent local TCP connections over a
//! small pool of upgraded connections using the SPDY/3.1 frame format.
//!
//! ## Why not `kube::Api::portforward`?
//!
//! `kube::Api::portforward` opens a new WebSocket upgrade for every stream
//! pair, lacks keepalive, and surfaces opaque errors. This crate
//! multiplexes many stream pairs per upgrade, runs a Ping/Pong watchdog,
//! exposes structured `thiserror` errors, and gracefully degrades to the
//! legacy SPDY/3.1 path when the apiserver is too old for KEP-4006.
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

pub(crate) mod client;
pub(crate) mod connect;
pub(crate) mod error;
pub(crate) mod forwarder;
pub(crate) mod pod_watch;
pub(crate) mod recovery;
pub(crate) mod session;
pub(crate) mod stream;
pub(crate) mod subprotocol;

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
pub use recovery::{
    RecoveryCallback,
    RecoverySignal,
};
pub use session::Session;
pub use stream::{
    DataStream,
    ErrorStream,
    Stream,
};
pub use subprotocol::Subprotocol;
