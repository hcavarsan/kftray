use std::sync::atomic::{
    AtomicU64,
    AtomicUsize,
    Ordering,
};
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::error::Error;
use crate::mux::{
    MuxConfig,
    MuxHandle,
};
use crate::stream::Stream;
use crate::transport::{
    WsFrameReader,
    WsFrameWriter,
};

/// Per-handle load metrics for P2C routing.
/// Cost = (inflight_streams + 1) * rtt_estimate_ns. Lower cost = preferred
/// handle.
///
/// RTT timestamps are tracked per-call via the [`RttSample`] guard, not
/// stored in shared state. This avoids the race where two concurrent opens
/// would clobber each other's start timestamps.
struct HandleMetrics {
    /// Exponentially weighted RTT estimate in nanoseconds.
    rtt_ns: AtomicU64,
}

impl HandleMetrics {
    fn new() -> Self {
        Self {
            // Seed with 1ms to avoid zero-cost bias before first measurement.
            rtt_ns: AtomicU64::new(1_000_000),
        }
    }

    /// Begin an RTT measurement. The returned guard records the sample
    /// when its `complete()` method is called. If dropped without calling
    /// `complete()`, no measurement is recorded. This is intentional
    /// for early-return paths (closed handle, capacity exhausted).
    fn start_sample(&self) -> RttSample<'_> {
        RttSample {
            metrics: self,
            start: Instant::now(),
        }
    }

    /// Update the RTT estimate using Peak-EWMA: adopt new peaks immediately,
    /// decay toward measurements below the peak.
    ///
    /// Uses a compare-and-swap loop so concurrent updates from racing opens
    /// don't lose samples. Failure to CAS just retries; the cost is bounded
    /// by the number of concurrent opens on one handle (typically 1-2).
    fn record_rtt(&self, elapsed_ns: u64) {
        let mut prev = self.rtt_ns.load(Ordering::Relaxed);
        loop {
            let next = if elapsed_ns > prev {
                // New peak: adopt immediately for fast spike adaptation.
                elapsed_ns
            } else {
                // Decay toward current measurement: new = prev*0.9 + elapsed*0.1
                (prev / 10) * 9 + elapsed_ns / 10
            };
            match self.rtt_ns.compare_exchange_weak(
                prev,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => prev = actual,
            }
        }
    }

    /// P2C cost metric: (inflight + 1) × rtt_estimate.
    fn cost(&self, inflight: usize) -> u64 {
        let rtt = self.rtt_ns.load(Ordering::Relaxed);
        rtt.saturating_mul((inflight as u64).saturating_add(1))
    }
}

/// Per-call RTT measurement. Calling [`complete`] records the elapsed
/// time. Dropping without completing is intentional (early-return paths).
struct RttSample<'a> {
    metrics: &'a HandleMetrics,
    start: Instant,
}

impl<'a> RttSample<'a> {
    fn complete(self) {
        let elapsed_ns = self.start.elapsed().as_nanos();
        // Cap at u64::MAX (won't happen in practice but defensive).
        let elapsed_ns = u64::try_from(elapsed_ns).unwrap_or(u64::MAX);
        self.metrics.record_rtt(elapsed_ns);
    }
}

/// SPDY/3.1 tunnel session: one or more WebSocket connections carrying
/// dynamic SPDY stream pairs for port-forwarding.
///
/// When `pool_size > 1`, each WebSocket gets its own reader/writer task pair
/// and streams are distributed round-robin across the pool for parallel TLS
/// writes at high concurrency. Pool size 1 preserves the original
/// single-connection behavior.
///
/// # Transport break contract
///
/// When a WebSocket closes or errors, its streams receive `BrokenPipe`.
/// There is no transparent reconnection. The `Forwarder` layer above handles
/// reconnection by opening a new session.
pub struct Session {
    pool: Vec<MuxHandle>,
    metrics: Vec<HandleMetrics>,
    next: AtomicUsize,
    port: u16,
    cancel: CancellationToken,
}

impl Session {
    /// Create a session with explicit configuration from pre-split WebSocket
    /// transport pairs.
    ///
    /// Each `(writer, reader)` pair gets its own `MuxHandle` with independent
    /// reader/writer tasks. All handshakes and initial PING roundtrips
    /// complete before this method returns. Streams are then distributed
    /// round-robin across the pool.
    ///
    /// # Graceful degradation
    ///
    /// If some connections fail their initial PING but at least one succeeds,
    /// the session proceeds with the healthy subset. Only returns an error
    /// when ALL connections fail (or the input is empty).
    pub async fn with_config<W, R>(
        connections: Vec<(W, R)>, port: u16, cancel: CancellationToken, config: MuxConfig,
    ) -> Result<Self, Error>
    where
        W: WsFrameWriter + 'static,
        R: WsFrameReader + 'static,
    {
        if connections.is_empty() {
            return Err(Error::MuxClosed);
        }
        let total = connections.len();
        let mut pool = Vec::with_capacity(total);
        let mut last_error = None;
        for (i, (writer, reader)) in connections.into_iter().enumerate() {
            match MuxHandle::spawn(writer, reader, cancel.clone(), config.clone()).await {
                Ok(mux) => pool.push(mux),
                Err(e) => {
                    tracing::warn!(
                        index = i,
                        total,
                        error = %e,
                        "SPDY pool: connection {}/{} failed initial PING, skipping",
                        i + 1,
                        total,
                    );
                    last_error = Some(e);
                }
            }
        }
        if pool.is_empty() {
            // All connections failed: propagate the last error.
            return Err(last_error.unwrap_or(Error::MuxClosed));
        }
        if pool.len() < total {
            tracing::info!(
                healthy = pool.len(),
                total,
                "SPDY pool: proceeding with {}/{} connections",
                pool.len(),
                total,
            );
        }
        let metrics = (0..pool.len()).map(|_| HandleMetrics::new()).collect();
        Ok(Self {
            pool,
            metrics,
            next: AtomicUsize::new(0),
            port,
            cancel,
        })
    }

    /// Open a new port-forward stream pair using Power of Two Choices (P2C)
    /// with Peak-EWMA load estimation.
    ///
    /// Picks two random live handles, compares their cost (inflight × RTT
    /// estimate), and opens a stream on the cheaper one. Falls back to
    /// round-robin scan if P2C picks fail (capacity exhausted or closed).
    pub async fn connect(&self) -> Result<Stream, Error> {
        let pool_size = self.pool.len();

        // P2C: pick least-loaded of two random handles.
        if pool_size >= 2 {
            let (a, b) = self.pick_two(pool_size);
            let preferred = if self.handle_cost(a) <= self.handle_cost(b) {
                [a, b]
            } else {
                [b, a]
            };
            for &idx in &preferred {
                if let Some(stream) = self.try_open(idx).await? {
                    return Ok(stream);
                }
            }
        }

        // Fallback: sequential scan (handles P2C misses due to capacity).
        for attempt in 0..pool_size {
            let idx = self.next.fetch_add(1, Ordering::Relaxed) % pool_size;
            if let Some(stream) = self.try_open(idx).await? {
                return Ok(stream);
            }
            tracing::debug!(
                handle = idx,
                attempt,
                "SPDY session: handle unavailable, trying next"
            );
        }

        Err(Error::CapacityExhausted {
            in_use: self.in_use(),
            limit: self.capacity() as u32,
        })
    }

    /// Attempt to open a stream on the given handle index.
    /// Returns Ok(Some(stream)) on success, Ok(None) if handle is closed or
    /// at capacity, Err on fatal errors.
    ///
    /// RTT is measured per-call via [`RttSample`], only recorded on success
    /// to avoid contaminating the load estimate with capacity-rejection latency
    /// (which is fast and unrepresentative of actual stream-open cost).
    async fn try_open(&self, idx: usize) -> Result<Option<Stream>, Error> {
        let mux = &self.pool[idx];
        if mux.is_closed() {
            return Ok(None);
        }
        let sample = self.metrics[idx].start_sample();
        match mux.open_portforward_pair(self.port).await {
            Ok(stream) => {
                sample.complete();
                tracing::debug!(
                    handle = idx,
                    active = mux.active_pairs(),
                    cost = self.handle_cost(idx),
                    "SPDY session: stream opened via P2C"
                );
                Ok(Some(stream))
            }
            Err(Error::CapacityExhausted { .. }) => {
                // Sample dropped without complete(): no spurious RTT record.
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// P2C cost for a handle: inflight × rtt_estimate.
    /// Closed handles get u64::MAX cost (never selected).
    fn handle_cost(&self, idx: usize) -> u64 {
        let mux = &self.pool[idx];
        if mux.is_closed() {
            return u64::MAX;
        }
        self.metrics[idx].cost(mux.active_pairs())
    }

    /// Pick two distinct random indices using xorshift on the atomic counter.
    /// Cheap and good enough for load balancing (no rand dependency needed).
    fn pick_two(&self, pool_size: usize) -> (usize, usize) {
        // Use fetch_add as a cheap entropy source.
        let seed = self.next.fetch_add(1, Ordering::Relaxed) as u64;
        let a = (seed % pool_size as u64) as usize;
        // LCG multiplier from Knuth's MMIX (also used by PCG family).
        let b =
            ((seed.wrapping_mul(6364136223846793005).wrapping_add(1)) % pool_size as u64) as usize;
        if a == b {
            (a, (a + 1) % pool_size)
        } else {
            (a, b)
        }
    }

    /// Total capacity across all live pool members (hard cap).
    pub fn capacity(&self) -> usize {
        self.pool
            .iter()
            .filter(|m| !m.is_closed())
            .map(|m| m.max_concurrent() as usize)
            .sum()
    }

    /// Total operating capacity across all live pool members (scheduling cap).
    pub fn operating_capacity(&self) -> usize {
        self.pool
            .iter()
            .filter(|m| !m.is_closed())
            .map(|m| m.operating_capacity())
            .sum()
    }

    pub fn in_use(&self) -> usize {
        self.pool.iter().map(|m| m.active_pairs()).sum()
    }

    pub fn available(&self) -> usize {
        self.capacity().saturating_sub(self.in_use())
    }

    pub fn is_full(&self) -> bool {
        self.pool
            .iter()
            .all(|m| m.is_closed() || m.active_pairs() >= m.max_concurrent() as usize)
    }

    /// Returns true when all underlying WebSockets have closed.
    pub fn is_drained(&self) -> bool {
        self.pool.iter().all(|m| m.is_closed())
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Close the SPDY session by cancelling the mux tasks.
    pub async fn close(self) -> Result<(), Error> {
        self.cancel.cancel();
        Ok(())
    }
}
