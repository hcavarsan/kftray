/// WebSocket subprotocol negotiated for a port-forward session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Subprotocol {
    /// `v5.channel.k8s.io` — supports the `[0xFF, channel]` half-close signal,
    /// letting the client release channel IDs back to the allocator's free-list
    /// within a session.
    V5,
    /// `v4.channel.k8s.io` — original framing, no half-close. Released IDs
    /// stay reserved until the session ends.
    V4,
}

impl std::fmt::Display for Subprotocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V5 => f.write_str("v5"),
            Self::V4 => f.write_str("v4"),
        }
    }
}

impl Subprotocol {
    /// Whether this subprotocol supports half-close (channel ID reuse).
    pub fn supports_half_close(&self) -> bool {
        matches!(self, Self::V5)
    }

    pub(crate) fn offered_header_value() -> &'static str {
        "v5.channel.k8s.io, v4.channel.k8s.io"
    }
}
