/// Protocol negotiated for a port-forward session.
///
/// Both variants speak the SPDY/3.1 frame format on the wire. They differ
/// only in how the frames reach the apiserver: tunnelled inside WebSocket
/// binary messages, or sent directly over a raw HTTP/1.1 upgrade.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Subprotocol {
    /// `SPDY/3.1+portforward.k8s.io` — SPDY frames tunnelled inside
    /// WebSocket binary messages. Negotiated via `Sec-WebSocket-Protocol`
    /// against apiservers that implement KEP-4006 phase 1 or later
    /// (Kubernetes >= 1.30). This is the default path.
    Spdy31Tunnel,
    /// Legacy SPDY/3.1 over a raw HTTP Upgrade (no WebSocket framing).
    /// Used as a fallback when the apiserver rejects the WebSocket
    /// upgrade. Matches the original `kubectl port-forward` wire
    /// protocol for pre-1.30 clusters and clusters that disable
    /// `PortForwardWebsockets`.
    LegacySpdy,
}

impl std::fmt::Display for Subprotocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spdy31Tunnel => f.write_str("SPDY/3.1+ws"),
            Self::LegacySpdy => f.write_str("SPDY/3.1"),
        }
    }
}
