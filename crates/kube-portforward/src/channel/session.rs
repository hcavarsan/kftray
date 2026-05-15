use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite;
use tokio_util::sync::CancellationToken;
use tungstenite::Message;

use super::allocator::ChannelAllocator;
use crate::error::Error;
use super::keepalive::{
    KeepaliveHandle,
    RecoveryCallback,
};
use super::routing::Router;
use super::shutdown;
use super::stream::{
    ChannelHalf,
    ShutdownSignal,
    Stream,
};
use crate::subprotocol::Subprotocol;

#[derive(Clone, Copy)]
pub(crate) struct AllocatedIds {
    data: u8,
    error: u8,
}

pub(crate) struct ReleaseGuard {
    ids: Option<AllocatedIds>,
    allocator: Arc<Mutex<ChannelAllocator>>,
    router: Router,
    shutdown_signal: Arc<ShutdownSignal>,
    cancel: CancellationToken,
}

impl ReleaseGuard {
    fn new(
        ids: AllocatedIds, allocator: Arc<Mutex<ChannelAllocator>>, router: Router,
        shutdown_signal: Arc<ShutdownSignal>, cancel: CancellationToken,
    ) -> Self {
        Self {
            ids: Some(ids),
            allocator,
            router,
            shutdown_signal,
            cancel,
        }
    }
}

impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        let Some(ids) = self.ids.take() else { return };
        self.shutdown_signal.fire();
        self.router.remove(ids.data);
        self.router.remove(ids.error);
        let drained = {
            let mut alloc = self.allocator.lock();
            alloc.release_pair(ids.data);
            alloc.is_drained()
        };
        if drained {
            self.cancel.cancel();
        }
    }
}

/// Channel-backend session: one WebSocket port-forward session that multiplexes
/// up to `capacity_pairs` concurrent local connections over a single upgrade.
pub(crate) struct Session {
    allocator: Arc<Mutex<ChannelAllocator>>,
    router: Router,
    writer_mailbox: mpsc::Sender<Message>,
    cancel: CancellationToken,
    #[allow(dead_code)]
    keepalive: KeepaliveHandle,
    protocol: Subprotocol,
    join_set: JoinSet<Result<(), Error>>,
    drain_timeout: Duration,
    #[allow(dead_code)]
    recovery_callback: RecoveryCallback,
}

#[allow(clippy::too_many_arguments)]
impl Session {
    pub(crate) fn new(
        allocator: Arc<Mutex<ChannelAllocator>>, router: Router,
        writer_mailbox: mpsc::Sender<Message>, cancel: CancellationToken,
        keepalive: KeepaliveHandle, protocol: Subprotocol, join_set: JoinSet<Result<(), Error>>,
        drain_timeout: Duration, recovery_callback: RecoveryCallback,
    ) -> Self {
        Self {
            allocator,
            router,
            writer_mailbox,
            cancel,
            keepalive,
            protocol,
            join_set,
            drain_timeout,
            recovery_callback,
        }
    }

    /// Allocate the next channel pair and return a bidirectional [`Stream`].
    pub(crate) async fn connect(&self) -> Result<Stream, Error> {
        let (data_id, error_id) = {
            let mut alloc = self.allocator.lock();
            let live = alloc.live_count();
            let capacity = alloc.capacity_pairs();
            alloc.allocate_pair().ok_or(Error::CapacityExhausted {
                in_use: live,
                capacity,
            })?
        };

        tracing::debug!(data_id, error_id, "opening channel pair");

        let (data_half, data_inbound_tx) = ChannelHalf::pair(data_id, self.writer_mailbox.clone());
        let (error_half, error_inbound_tx) =
            ChannelHalf::pair(error_id, self.writer_mailbox.clone());

        self.router.insert(data_id, data_inbound_tx, false);
        self.router.insert(error_id, error_inbound_tx, false);

        let shutdown_signal = ShutdownSignal::new(
            data_id,
            error_id,
            self.writer_mailbox.clone(),
            self.protocol.supports_half_close(),
        );

        let guard = ReleaseGuard::new(
            AllocatedIds {
                data: data_id,
                error: error_id,
            },
            Arc::clone(&self.allocator),
            self.router.clone(),
            Arc::clone(&shutdown_signal),
            self.cancel.clone(),
        );

        Ok(Stream::new(data_half, error_half, shutdown_signal, guard))
    }

    pub(crate) fn protocol(&self) -> Subprotocol {
        self.protocol
    }

    /// Maximum number of concurrent streams this session can hold.
    pub(crate) fn capacity(&self) -> usize {
        self.allocator.lock().capacity_pairs()
    }

    /// Number of channel pairs currently in use.
    pub(crate) fn in_use(&self) -> usize {
        self.allocator.lock().live_count()
    }

    /// Pairs still available for new streams.
    pub(crate) fn available(&self) -> usize {
        let alloc = self.allocator.lock();
        alloc.capacity_pairs() - alloc.live_count()
    }

    pub(crate) fn is_full(&self) -> bool {
        self.allocator.lock().is_full()
    }

    pub(crate) fn is_drained(&self) -> bool {
        self.allocator.lock().is_drained()
    }

    pub(crate) fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Gracefully close the session.
    pub(crate) async fn close(mut self) -> Result<(), Error> {
        shutdown::shutdown(
            self.writer_mailbox.clone(),
            self.cancel.clone(),
            &mut self.join_set,
            self.drain_timeout,
        )
        .await;
        Ok(())
    }
}
