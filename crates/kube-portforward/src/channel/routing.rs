use std::sync::Arc;

use bytes::Bytes;
use parking_lot::RwLock;
use tokio::sync::mpsc;

use super::frame;

const ROUTER_CAPACITY: usize = 256;

#[derive(Clone)]
pub(crate) struct Router {
    // Channel IDs are u8 → exactly 256 slots. Direct indexing avoids the
    // hashing/sharding overhead present in DashMap.
    slots: Arc<[RwLock<Option<ChannelEntry>>; ROUTER_CAPACITY]>,
}

struct ChannelEntry {
    sender: mpsc::Sender<Bytes>,
    port_seen: bool,
}

impl Router {
    pub(crate) fn new() -> Self {
        let slots: [RwLock<Option<ChannelEntry>>; ROUTER_CAPACITY] =
            std::array::from_fn(|_| RwLock::new(None));
        Self {
            slots: Arc::new(slots),
        }
    }

    /// Insert (or replace) a channel entry. Preserves `port_seen=true` on
    /// existing entries so a channel pre-registered at handshake (and whose
    /// port-frame has already been absorbed) keeps its "ready" state when
    /// later swapped to a real stream.
    pub(crate) fn insert(&self, channel: u8, sender: mpsc::Sender<Bytes>, port_seen: bool) {
        let mut slot = self.slots[channel as usize].write();
        match slot.as_mut() {
            Some(existing) => existing.sender = sender,
            None => *slot = Some(ChannelEntry { sender, port_seen }),
        }
    }

    pub(crate) fn remove(&self, channel: u8) {
        *self.slots[channel as usize].write() = None;
    }

    /// Dispatch an inbound payload. On first frame for a not-yet-port-seen
    /// channel, parses as the initial port-frame and flips the state without
    /// forwarding (handshake absorption). Holds the slot's RwLock only across
    /// the synchronous flip / sender-clone — never across the `.await`
    /// (parking_lot::RwLock is not async-aware).
    pub(crate) async fn dispatch(&self, channel: u8, payload: Bytes) {
        let maybe_send = {
            let mut slot = self.slots[channel as usize].write();
            let Some(entry) = slot.as_mut() else {
                tracing::debug!(channel, "dropping payload for unknown channel");
                return;
            };
            if entry.port_seen {
                Some(entry.sender.clone())
            } else {
                match frame::parse_initial_port_frame(&payload) {
                    Ok(_) => {
                        entry.port_seen = true;
                    }
                    Err(e) => {
                        tracing::warn!(channel, error = %e, "failed to parse initial port frame");
                    }
                }
                None
            }
        };
        if let Some(sender) = maybe_send
            && sender.send(payload).await.is_err()
        {
            tracing::trace!(channel, "channel receiver gone; discarding payload");
        }
    }

    #[allow(dead_code)]
    pub(crate) fn clear(&self) {
        for slot in self.slots.iter() {
            *slot.write() = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn router_dispatch_routes_to_correct_channel() {
        let router = Router::new();
        let (tx, mut rx) = mpsc::channel::<Bytes>(8);
        router.insert(4, tx, true);

        router.dispatch(4, Bytes::from_static(b"payload")).await;
        let got = rx.recv().await.unwrap();
        assert_eq!(&got[..], b"payload");
    }

    #[tokio::test]
    async fn router_insert_preserves_port_seen_on_existing() {
        let router = Router::new();
        let (tx1, _rx1) = mpsc::channel::<Bytes>(1);
        router.insert(2, tx1, true);

        let (tx2, mut rx2) = mpsc::channel::<Bytes>(8);
        router.insert(2, tx2, false);

        router.dispatch(2, Bytes::from_static(b"data")).await;
        let got = rx2.recv().await.unwrap();
        assert_eq!(&got[..], b"data");
    }

    #[tokio::test]
    async fn router_dispatch_drops_first_frame_as_port_frame_for_unseen() {
        let router = Router::new();
        let (tx, mut rx) = mpsc::channel::<Bytes>(8);
        router.insert(0, tx, false);

        router.dispatch(0, Bytes::from_static(&[0x50, 0x00])).await;
        assert!(rx.try_recv().is_err());

        router.dispatch(0, Bytes::from_static(b"real")).await;
        let got = rx.recv().await.unwrap();
        assert_eq!(&got[..], b"real");
    }

    #[tokio::test]
    async fn router_unknown_channel_drops() {
        let router = Router::new();
        router.dispatch(99, Bytes::from_static(b"x")).await;
    }
}
