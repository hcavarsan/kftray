use std::time::Duration;

pub(crate) const COMPUTE: &str = "https://compute.googleapis.com/compute/v1";
pub(crate) const OSLOGIN: &str = "https://oslogin.googleapis.com/v1";
pub(crate) const TOKENINFO: &str = "https://oauth2.googleapis.com/tokeninfo";
/// Same endpoint gcloud uses for IAP TCP forwarding.
pub(crate) const IAP_WS: &str = "wss://tunnel.cloudproxy.app";

pub(crate) const SCOPES: [&str; 2] = [
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
];

pub(crate) const REST_TIMEOUT: Duration = Duration::from_secs(30);
/// Renewed on every connection, so the key vanishes from the OS Login
/// profile once the user stops using kftray.
pub(crate) const KEY_TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub(crate) const SSH_PORT: u16 = 22;
/// Key types a VM publishes under the `hostkeys/` guest attributes namespace.
pub(crate) const HOST_KEY_TYPES: [&str; 3] = ["ssh-ed25519", "ecdsa-sha2-nistp256", "ssh-rsa"];
pub(crate) const USER_AGENT: &str = concat!("kftray/", env!("CARGO_PKG_VERSION"));

/// JSON `GcpEndpoints` that replace the Google endpoints (e2e only).
#[cfg(feature = "e2e")]
pub(crate) const ENV_ENDPOINTS: &str = "KFTRAY_GCP_ENDPOINTS";
#[cfg(feature = "e2e")]
pub(crate) const ENV_STATIC_TOKEN: &str = "KFTRAY_GCP_STATIC_TOKEN";
