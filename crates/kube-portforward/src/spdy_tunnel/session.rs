use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use tokio_tungstenite::WebSocketStream;
use tokio_util::sync::CancellationToken;

use super::error::Error;
use super::mux::MuxHandle;
use super::stream::Stream;
use crate::subprotocol::Subprotocol;

/// SPDY/3.1 tunnel session: one WebSocket carrying unlimited dynamic SPDY
/// stream pairs for port-forwarding.
pub(crate) struct Session {
    mux: MuxHandle,
    port: u16,
    cancel: CancellationToken,
}

impl Session {
    /// Create a session from an already-upgraded WebSocket transport.
    ///
    /// Sends an initial PING to verify the upstream SPDY connection through
    /// the API server's TunnelingHandler is established before returning.
    pub(crate) async fn new(
        ws: WebSocketStream<TokioIo<Upgraded>>, port: u16, cancel: CancellationToken,
    ) -> Result<Self, Error> {
        let mux = MuxHandle::spawn(ws, cancel.clone()).await?;
        Ok(Self { mux, port, cancel })
    }

    /// Open a new port-forward stream pair through this session.
    pub(crate) async fn connect(&self) -> Result<Stream, Error> {
        self.mux.open_portforward_pair(self.port).await
    }

    pub(crate) fn protocol(&self) -> Subprotocol {
        Subprotocol::Spdy31Tunnel
    }

    /// SPDY sessions have no fixed capacity — they can open unlimited streams.
    pub(crate) fn capacity(&self) -> usize {
        usize::MAX
    }

    pub(crate) fn in_use(&self) -> usize {
        self.mux.active_pairs()
    }

    pub(crate) fn available(&self) -> usize {
        usize::MAX
    }

    pub(crate) fn is_full(&self) -> bool {
        false
    }

    /// Returns true when the underlying WebSocket has closed.
    pub(crate) fn is_drained(&self) -> bool {
        self.mux.is_closed()
    }

    pub(crate) fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Close the SPDY session by cancelling the mux task.
    pub(crate) async fn close(self) -> Result<(), crate::error::Error> {
        self.cancel.cancel();
        Ok(())
    }
}
