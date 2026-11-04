use std::fmt;
use std::sync::Arc;

use tokio::sync::watch;

mod connect;
mod handler;
mod opener;
mod pool;

pub(crate) use connect::establish;
pub use opener::SshChannelOpener;
pub(crate) use opener::open_direct_tcpip;
use pool::SessionEntry;
pub(crate) use pool::SshPool;

/// Step of establishing a session, reported to callers for telemetry and
/// the connection test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Transport,
    Handshake,
    Auth,
    Channel,
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Transport => "transport",
            Self::Handshake => "SSH handshake",
            Self::Auth => "authentication",
            Self::Channel => "channel open",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    Connecting,
    Ready,
    Reconnecting { attempt: u32 },
    Failed(String),
}

/// A share in one pooled SSH session. The session closes when the last
/// lease (and every opener made from it) is dropped.
#[derive(Clone)]
pub struct SshLease(Arc<SessionEntry>);

impl SshLease {
    /// Opens `direct-tcpip` channels to `host:port`; the sshd resolves
    /// `host`, so names private to the bastion's network work.
    pub fn opener(&self, host: String, port: u16) -> SshChannelOpener {
        SshChannelOpener::new(self.clone(), host, port)
    }

    pub fn state(&self) -> watch::Receiver<SessionState> {
        self.0.state.subscribe()
    }

    pub fn label(&self) -> &str {
        &self.0.label
    }
}
