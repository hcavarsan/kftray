use crate::error::Error;
use crate::stream::Stream;
use crate::subprotocol::Subprotocol;
use tokio_util::sync::CancellationToken;

/// One WebSocket port-forward session that multiplexes up to `capacity_pairs`
/// concurrent local connections (each backed by a data/error channel pair)
/// over a single upgrade.
pub struct Session {
    inner: SessionInner,
}

enum SessionInner {
    Channel(crate::channel::Session),
    #[cfg(feature = "spdy-tunnel")]
    Spdy(crate::spdy_tunnel::Session),
}

impl Session {
    pub(crate) fn from_channel(session: crate::channel::Session) -> Self {
        Self {
            inner: SessionInner::Channel(session),
        }
    }

    #[cfg(feature = "spdy-tunnel")]
    pub(crate) fn from_spdy(session: crate::spdy_tunnel::Session) -> Self {
        Self {
            inner: SessionInner::Spdy(session),
        }
    }

    /// Allocate the next channel pair and return a bidirectional [`Stream`].
    pub async fn connect(&self) -> Result<Stream, Error> {
        match &self.inner {
            SessionInner::Channel(s) => s.connect().await.map(Stream::from_channel),
            #[cfg(feature = "spdy-tunnel")]
            SessionInner::Spdy(s) => s.connect().await.map(Stream::from_spdy).map_err(Error::from),
        }
    }

    pub fn protocol(&self) -> Subprotocol {
        match &self.inner {
            SessionInner::Channel(s) => s.protocol(),
            #[cfg(feature = "spdy-tunnel")]
            SessionInner::Spdy(s) => s.protocol(),
        }
    }

    /// Maximum number of concurrent streams this session can hold.
    pub fn capacity(&self) -> usize {
        match &self.inner {
            SessionInner::Channel(s) => s.capacity(),
            #[cfg(feature = "spdy-tunnel")]
            SessionInner::Spdy(s) => s.capacity(),
        }
    }

    /// Number of channel pairs currently in use.
    pub fn in_use(&self) -> usize {
        match &self.inner {
            SessionInner::Channel(s) => s.in_use(),
            #[cfg(feature = "spdy-tunnel")]
            SessionInner::Spdy(s) => s.in_use(),
        }
    }

    /// Pairs still available for new streams.
    pub fn available(&self) -> usize {
        match &self.inner {
            SessionInner::Channel(s) => s.available(),
            #[cfg(feature = "spdy-tunnel")]
            SessionInner::Spdy(s) => s.available(),
        }
    }

    pub fn is_full(&self) -> bool {
        match &self.inner {
            SessionInner::Channel(s) => s.is_full(),
            #[cfg(feature = "spdy-tunnel")]
            SessionInner::Spdy(s) => s.is_full(),
        }
    }

    /// Returns true if every channel pair this session preallocated has been
    /// allocated AND released. A drained session can never produce another
    /// stream — callers should drop it and open a new session.
    pub fn is_drained(&self) -> bool {
        match &self.inner {
            SessionInner::Channel(s) => s.is_drained(),
            #[cfg(feature = "spdy-tunnel")]
            SessionInner::Spdy(s) => s.is_drained(),
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        match &self.inner {
            SessionInner::Channel(s) => s.cancellation_token(),
            #[cfg(feature = "spdy-tunnel")]
            SessionInner::Spdy(s) => s.cancellation_token(),
        }
    }

    /// Gracefully close the session, draining background tasks within the
    /// configured drain timeout before aborting any leftover work.
    pub async fn close(self) -> Result<(), Error> {
        match self.inner {
            SessionInner::Channel(s) => s.close().await,
            #[cfg(feature = "spdy-tunnel")]
            SessionInner::Spdy(s) => s.close().await,
        }
    }
}
