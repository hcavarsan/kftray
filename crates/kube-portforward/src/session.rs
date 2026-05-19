use std::sync::atomic::{
    AtomicBool,
    Ordering,
};

use crossbeam_queue::ArrayQueue;
use tokio_util::sync::CancellationToken;

use crate::error::Error;
use crate::stream::Stream;
use crate::subprotocol::Subprotocol;

/// Maximum number of pre-opened spare streams per session.
const SPARE_STREAM_CAP: usize = 16;

/// When spare count drops to or below this threshold, the background
/// replenisher refills up to `SPARE_STREAM_CAP`.
const SPARE_STREAM_LOW_WATERMARK: usize = 8;

/// One SPDY-tunnelled port-forward session that multiplexes many concurrent
/// local connections over a pool of upgraded connections to the apiserver.
pub struct Session {
    inner: spdy_mux::Session,
    protocol: Subprotocol,
    /// Pre-opened spare streams for instant connect(). Background task
    /// replenishes when count drops to or below `SPARE_STREAM_LOW_WATERMARK`.
    spare_streams: ArrayQueue<Stream>,
    /// Guard against concurrent replenishment. Set by `replenish_spare_streams`
    /// on entry, cleared on exit.
    replenishing: AtomicBool,
}

impl Session {
    pub(crate) fn from_spdy(session: spdy_mux::Session, protocol: Subprotocol) -> Self {
        Self {
            spare_streams: ArrayQueue::new(SPARE_STREAM_CAP),
            replenishing: AtomicBool::new(false),
            inner: session,
            protocol,
        }
    }

    /// Allocate the next stream and return a bidirectional [`Stream`].
    ///
    /// Fast path: pops pre-opened spare streams (lock-free), discarding any
    /// that the remote has already closed (FIN/RST while idle). Falls back
    /// to opening a new stream if no usable spare is available.
    pub async fn connect(&self) -> Result<Stream, Error> {
        while let Some(stream) = self.spare_streams.pop() {
            if !stream.is_read_closed() {
                return Ok(stream);
            }
            tracing::debug!("spare stream stale (remote closed while idle), discarding");
        }
        self.open_new_stream().await
    }

    async fn open_new_stream(&self) -> Result<Stream, Error> {
        self.inner
            .connect()
            .await
            .map(Stream::from_spdy)
            .map_err(Error::from)
    }

    /// Pre-open spare streams up to `SPARE_STREAM_CAP`.
    pub async fn replenish_spare_streams(&self) {
        if self
            .replenishing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        let _guard = ReplenishGuard(&self.replenishing);

        while self.spare_streams.len() < SPARE_STREAM_CAP {
            if self.is_full() || self.cancellation_token().is_cancelled() {
                break;
            }
            match self.open_new_stream().await {
                Ok(stream) => {
                    if self.spare_streams.push(stream).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }

    pub fn spare_count(&self) -> usize {
        self.spare_streams.len()
    }

    pub fn needs_replenish(&self) -> bool {
        self.spare_count() <= SPARE_STREAM_LOW_WATERMARK
    }

    pub fn protocol(&self) -> Subprotocol {
        self.protocol
    }

    /// Maximum number of concurrent streams this session can hold (hard cap).
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Operating capacity: the scheduling cap below the hard cap.
    pub fn operating_capacity(&self) -> usize {
        self.inner.operating_capacity()
    }

    pub fn in_use(&self) -> usize {
        self.inner.in_use()
    }

    pub fn available(&self) -> usize {
        self.inner.available()
    }

    pub fn is_full(&self) -> bool {
        self.inner.is_full()
    }

    pub fn is_drained(&self) -> bool {
        self.inner.is_drained()
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.cancellation_token()
    }

    /// Gracefully close the session.
    pub async fn close(self) -> Result<(), Error> {
        self.inner.close().await.map_err(Error::from)
    }
}

/// RAII guard that clears the `replenishing` flag on drop.
struct ReplenishGuard<'a>(&'a AtomicBool);

impl Drop for ReplenishGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
