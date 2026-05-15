use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tungstenite::Message;

use super::allocator::ChannelAllocator;
use super::keepalive::{
    RecoveryCallback,
    spawn_keepalive,
};
use super::reader::spawn_reader;
use super::routing::Router;
use super::session::Session;
use super::writer::writer_task;
use crate::connect::{
    KeepaliveConfig,
    UpgradedTransport,
};
use crate::error::Error;

/// Build a channel-based Session from an already-upgraded WebSocket transport.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn build_channel_session(
    upgraded: UpgradedTransport, capacity_pairs: usize, cancel: CancellationToken,
    keepalive_config: KeepaliveConfig, drain_timeout: Duration,
    recovery_callback: RecoveryCallback,
) -> Result<Session, Error> {
    let protocol = upgraded.protocol;

    let mut join_set: JoinSet<Result<(), Error>> = JoinSet::new();
    let (writer_tx, writer_rx) = mpsc::channel::<Message>(256);

    let keepalive = spawn_keepalive(
        writer_tx.clone(),
        cancel.clone(),
        Arc::clone(&recovery_callback),
        keepalive_config.ping_interval,
        keepalive_config.watchdog_timeout,
        &mut join_set,
    );

    let (sink, stream) = upgraded.ws.split();
    join_set.spawn(writer_task(sink, writer_rx, cancel.clone()));
    let router = Router::new();

    // Pre-register every channel the apiserver pre-allocated at handshake
    for pair_index in 0..capacity_pairs {
        let data_id = (pair_index as u8) * 2;
        let error_id = data_id + 1;
        let (discard_tx_data, _) = mpsc::channel::<bytes::Bytes>(1);
        let (discard_tx_error, _) = mpsc::channel::<bytes::Bytes>(1);
        router.insert(data_id, discard_tx_data, false);
        router.insert(error_id, discard_tx_error, false);
    }

    spawn_reader(
        protocol,
        stream,
        router.clone(),
        writer_tx.clone(),
        cancel.clone(),
        keepalive.clone(),
        Arc::clone(&recovery_callback),
        &mut join_set,
    );

    let allocator = Arc::new(Mutex::new(ChannelAllocator::new(capacity_pairs)));

    Ok(Session::new(
        allocator,
        router,
        writer_tx,
        cancel,
        keepalive,
        protocol,
        join_set,
        drain_timeout,
        recovery_callback,
    ))
}
