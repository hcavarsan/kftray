use std::sync::atomic::{
    AtomicI64,
    Ordering,
};

use futures::task::AtomicWaker;

/// Per-stream (or session-level) send window. The writer side
/// (`Stream`/`DataStream`) checks `available()` and `consume()`s bytes;
/// the reader side calls `replenish()` when a WINDOW_UPDATE arrives.
pub(crate) struct SendWindow {
    remaining: AtomicI64,
    waker: AtomicWaker,
}

impl SendWindow {
    pub(crate) fn new(initial: u32) -> Self {
        Self {
            remaining: AtomicI64::new(initial as i64),
            waker: AtomicWaker::new(),
        }
    }

    /// Check available window. Returns 0 if exhausted.
    pub(crate) fn available(&self) -> i64 {
        self.remaining.load(Ordering::Acquire)
    }

    /// Consume `n` bytes from the window, returning `true` on success.
    ///
    /// Returns `false` if the window was poisoned (stream closed) between
    /// the caller's `is_closed()` check and this call.
    pub(crate) fn consume(&self, n: usize) -> bool {
        let n = n as i64;
        loop {
            let cur = self.remaining.load(Ordering::Acquire);
            if cur == i64::MIN {
                return false; // poisoned: stream closed
            }
            match self
                .remaining
                .compare_exchange(cur, cur - n, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return true,
                Err(_) => continue,
            }
        }
    }

    /// Add `delta` from a WINDOW_UPDATE and wake any blocked writer.
    pub(crate) fn replenish(&self, delta: u32) {
        self.remaining.fetch_add(delta as i64, Ordering::Release);
        self.waker.wake();
    }

    /// Apply a delta (positive or negative) for SETTINGS window resize.
    pub(crate) fn apply_delta(&self, delta: i64) {
        self.remaining.fetch_add(delta, Ordering::Release);
        if delta > 0 {
            self.waker.wake();
        }
    }

    /// Register a waker to be notified when window becomes available.
    pub(crate) fn register_waker(&self, waker: &std::task::Waker) {
        self.waker.register(waker);
    }

    /// Poison the window so pending/future poll_write returns immediately
    /// with `BrokenPipe`.
    pub(crate) fn close(&self) {
        self.remaining.store(i64::MIN, Ordering::Release);
        self.waker.wake();
    }

    /// Check if the window has been poisoned (stream closed).
    pub(crate) fn is_closed(&self) -> bool {
        self.remaining.load(Ordering::Acquire) == i64::MIN
    }
}
