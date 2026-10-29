use std::time::Duration;

/// How often russh sends a keepalive request on an idle session (OpenSSH
/// `ServerAliveInterval` style).
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
/// Unanswered keepalives before the session counts as dead (OpenSSH
/// `ServerAliveCountMax` default).
pub const KEEPALIVE_MAX: usize = 3;
/// Bound for the SSH key exchange, host key check included.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
/// Bound for user authentication once the provider produced credentials.
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(20);
/// Bound for opening one `direct-tcpip` channel, waiting for a reconnect
/// included.
pub const CHANNEL_OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// OpenSSH `MaxSessions` style cap on concurrent channels per session.
pub const MAX_CHANNELS_PER_SESSION: usize = 256;
/// First wait before re-establishing a dropped session.
pub const RECONNECT_BACKOFF_MIN: Duration = Duration::from_secs(1);
/// Upper bound of the doubling reconnect wait.
pub const RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(30);
/// kftray's own key, relative to the kftray config directory.
pub const KEY_RELATIVE_PATH: &str = "ssh/id_ed25519";
/// Comment of kftray's key, visible in OS Login profiles.
pub const KEY_COMMENT: &str = "kftray";
