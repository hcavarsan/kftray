use std::collections::{
    HashSet,
    VecDeque,
};

/// FIFO channel-pair allocator aligned with the URL-position semantics of
/// the Kubernetes port-forward subprotocols. The first allocation returns
/// `(0, 1)`, the second `(2, 3)`, and so on.
///
/// Channel pairs are **one-shot**: once a pair has been allocated, the
/// apiserver binds it to a single backend pod connection at WebSocket
/// upgrade time. After either side sends the v5 `[0xFF, channel]` close
/// signal, the pair is dead for the rest of the session — the server
/// will not accept reuse. Therefore `release_pair` only decrements the
/// live-count bookkeeping; it does **not** push the IDs back onto the
/// free-list.
pub(crate) struct ChannelAllocator {
    free: VecDeque<u8>,
    in_use: HashSet<u8>,
    capacity_pairs: usize,
    poisoned: bool,
    total_allocated: usize,
}

impl ChannelAllocator {
    pub(crate) fn new(capacity_pairs: usize) -> Self {
        let free: VecDeque<u8> = (0u8..).step_by(2).take(capacity_pairs).collect();
        Self {
            free,
            in_use: HashSet::new(),
            capacity_pairs,
            poisoned: false,
            total_allocated: 0,
        }
    }

    pub(crate) fn allocate_pair(&mut self) -> Option<(u8, u8)> {
        if self.poisoned {
            return None;
        }
        let data = self.free.pop_front()?;
        let error = data + 1;
        self.in_use.insert(data);
        self.in_use.insert(error);
        self.total_allocated += 1;
        Some((data, error))
    }

    /// Returns true once every preallocated pair has been handed out and
    /// every outstanding allocation has been released. A drained allocator
    /// can never produce another pair (v5 channel IDs are one-shot per the
    /// K8s wire protocol), so callers should retire the owning session.
    pub(crate) fn is_drained(&self) -> bool {
        self.total_allocated >= self.capacity_pairs && self.in_use.is_empty()
    }

    /// Decrement live-count for a previously allocated pair. The IDs are
    /// **not** returned to the free-list because v5 channel pairs are
    /// one-shot per K8s portforward semantics (see type-level docs).
    pub(crate) fn release_pair(&mut self, data: u8) {
        self.in_use.remove(&data);
        self.in_use.remove(&(data + 1));
    }

    pub(crate) fn live_count(&self) -> usize {
        self.in_use.len() / 2
    }

    pub(crate) fn capacity_pairs(&self) -> usize {
        self.capacity_pairs
    }

    pub(crate) fn is_full(&self) -> bool {
        self.free.is_empty()
    }

    #[allow(dead_code)]
    pub(crate) fn set_poisoned(&mut self) {
        self.poisoned = true;
    }

    #[allow(dead_code)]
    pub(crate) fn is_poisoned(&self) -> bool {
        self.poisoned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_in_url_order() {
        let mut a = ChannelAllocator::new(4);
        assert_eq!(a.allocate_pair(), Some((0, 1)));
        assert_eq!(a.allocate_pair(), Some((2, 3)));
        assert_eq!(a.allocate_pair(), Some((4, 5)));
        assert_eq!(a.allocate_pair(), Some((6, 7)));
        assert_eq!(a.allocate_pair(), None);
    }

    #[test]
    fn release_decrements_live_count() {
        let mut a = ChannelAllocator::new(4);
        let first = a.allocate_pair().unwrap();
        let second = a.allocate_pair().unwrap();
        assert_eq!(a.live_count(), 2);
        a.release_pair(first.0);
        assert_eq!(a.live_count(), 1);
        assert_eq!(a.allocate_pair(), Some((4, 5)));
        assert_eq!(a.live_count(), 2);
        assert_eq!(second, (2, 3));
    }

    #[test]
    fn capacity_exhaustion_returns_none() {
        let mut a = ChannelAllocator::new(1);
        assert!(a.allocate_pair().is_some());
        assert!(a.allocate_pair().is_none());
    }

    #[test]
    fn released_pair_is_not_reused() {
        let mut a = ChannelAllocator::new(2);
        let first = a.allocate_pair().unwrap();
        a.release_pair(first.0);
        let next = a.allocate_pair().unwrap();
        assert_ne!(next.0, first.0);
        assert_eq!(next, (2, 3));
        assert!(a.allocate_pair().is_none());
    }

    #[test]
    fn poisoning_short_circuits() {
        let mut a = ChannelAllocator::new(4);
        a.set_poisoned();
        assert!(a.is_poisoned());
        assert!(a.allocate_pair().is_none());
    }

    #[test]
    fn is_drained_after_full_cycle() {
        let mut a = ChannelAllocator::new(3);
        assert!(!a.is_drained());
        let p0 = a.allocate_pair().unwrap();
        let p1 = a.allocate_pair().unwrap();
        assert!(!a.is_drained());
        let p2 = a.allocate_pair().unwrap();
        assert!(!a.is_drained());
        a.release_pair(p0.0);
        a.release_pair(p1.0);
        assert!(!a.is_drained());
        a.release_pair(p2.0);
        assert!(a.is_drained());
    }

    #[test]
    fn drained_when_all_used_and_released() {
        let mut a = ChannelAllocator::new(2);
        let p0 = a.allocate_pair().unwrap();
        let p1 = a.allocate_pair().unwrap();
        assert!(a.allocate_pair().is_none());
        assert!(!a.is_drained());
        a.release_pair(p0.0);
        assert!(!a.is_drained());
        a.release_pair(p1.0);
        assert!(a.is_drained());
    }

    #[test]
    fn live_count_tracks_pairs() {
        let mut a = ChannelAllocator::new(4);
        assert_eq!(a.live_count(), 0);
        a.allocate_pair();
        a.allocate_pair();
        assert_eq!(a.live_count(), 2);
        a.release_pair(0);
        assert_eq!(a.live_count(), 1);
    }
}
