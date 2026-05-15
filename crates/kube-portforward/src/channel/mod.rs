//! Channel-multiplexed WebSocket backend for port-forward sessions.

pub(crate) mod allocator;
pub(crate) mod connect;
pub(crate) mod frame;
pub(crate) mod keepalive;
pub(crate) mod reader;
pub(crate) mod routing;
pub(crate) mod session;
pub(crate) mod shutdown;
pub(crate) mod stream;
pub(crate) mod writer;

pub(crate) use session::Session;
