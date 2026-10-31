//! Wire constants of the IAP TCP forwarding protocol. Every value mirrors
//! gcloud's `api_lib/compute/iap_tunnel_websocket*.py`.

use std::time::Duration;

pub const PATH_ROOT: &str = "/v4";
pub const CONNECT: &str = "connect";
pub const RECONNECT: &str = "reconnect";
pub const SUBPROTOCOL: &str = "relay.tunnel.cloudproxy.app";
pub const ORIGIN: &str = "bot:iap-tunneler";

pub const TAG_CONNECT_SUCCESS_SID: u16 = 0x0001;
pub const TAG_RECONNECT_SUCCESS_ACK: u16 = 0x0002;
pub const TAG_DATA: u16 = 0x0004;
pub const TAG_ACK: u16 = 0x0007;

/// Largest payload of one DATA frame.
pub const MAX_DATA_FRAME: usize = 16_384;
/// An ACK is sent when the unacknowledged received bytes are strictly above this.
pub const ACK_THRESHOLD: u64 = 32_768;
/// An ACK is also sent this often when received bytes are still unacknowledged.
pub const ACK_IDLE_FLUSH: Duration = Duration::from_secs(2);
/// Unacknowledged outgoing bytes kept for resend; reading the local side
/// pauses above it.
pub const RESEND_LIMIT: usize = 1024 * 1024;
pub const DUPLEX_BUFFER: usize = 64 * 1024;
/// Bound for one (re)connect, until the server answers with its first frame.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(60);
pub const RECONNECT_INITIAL: Duration = Duration::from_millis(1500);
pub const RECONNECT_MULTIPLIER: f64 = 1.1;
pub const RECONNECT_JITTER: Duration = Duration::from_millis(1000);
pub const RECONNECT_MAX_SLEEP: Duration = Duration::from_secs(20);
/// Total time spent trying to resume one interrupted tunnel.
pub const RECONNECT_BUDGET: Duration = Duration::from_secs(15 * 60);
/// How long the actor waits for its Close frame to leave before dropping the socket.
pub const CLOSE_GRACE: Duration = Duration::from_secs(2);
/// Header block limit of the HTTP proxy's answer to CONNECT.
pub const PROXY_HEADER_LIMIT: usize = 8 * 1024;
pub const USER_AGENT: &str = concat!("kftray/", env!("CARGO_PKG_VERSION"));
