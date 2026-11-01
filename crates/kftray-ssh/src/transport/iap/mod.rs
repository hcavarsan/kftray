//! Byte stream to a VM port through Google IAP TCP forwarding: a WebSocket
//! speaking the relay protocol of gcloud, with ACK based resume.
//!
//! Knows nothing about SSH or `providers::gcp`; an IAP-for-host workload can
//! reuse it as is.

mod connect;
pub mod consts;
mod error;
pub mod frame;
mod proxy;
mod tunnel;

use std::sync::Arc;

use async_trait::async_trait;
pub use error::IapError;
use secrecy::SecretString;

use self::connect::{
    Dialer,
    percent_encode,
};
use crate::error::SshError;
use crate::route::{
    BoxStream,
    Transport,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IapTarget {
    Instance {
        project: String,
        zone: String,
        instance: String,
        interface: String,
        port: u16,
    },
    /// Only builds URLs; no route uses it yet.
    Host {
        project: String,
        region: String,
        network: String,
        host: String,
        dest_group: Option<String>,
        port: u16,
    },
}

impl IapTarget {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Instance { instance, .. } => instance,
            Self::Host { host, .. } => host,
        }
    }

    fn query(pairs: &[(&str, &str)]) -> String {
        pairs
            .iter()
            .map(|(key, value)| format!("{key}={}", percent_encode(value)))
            .collect::<Vec<_>>()
            .join("&")
    }

    // `True` with a capital T, as gcloud's urlencode emits it.
    pub(crate) fn connect_query(&self) -> String {
        match self {
            Self::Instance {
                project,
                zone,
                instance,
                interface,
                port,
            } => {
                let port = port.to_string();
                Self::query(&[
                    ("project", project.as_str()),
                    ("port", port.as_str()),
                    ("newWebsocket", "True"),
                    ("zone", zone.as_str()),
                    ("instance", instance.as_str()),
                    ("interface", interface.as_str()),
                ])
            }
            Self::Host {
                project,
                region,
                network,
                host,
                dest_group,
                port,
            } => {
                let port = port.to_string();
                let mut pairs = vec![
                    ("project", project.as_str()),
                    ("port", port.as_str()),
                    ("newWebsocket", "True"),
                    ("region", region.as_str()),
                    ("network", network.as_str()),
                    ("host", host.as_str()),
                ];
                if let Some(group) = dest_group {
                    pairs.push(("group", group.as_str()));
                }
                Self::query(&pairs)
            }
        }
    }

    pub(crate) fn reconnect_query(&self, sid: &str, ack: u64) -> String {
        let (scope_key, scope) = match self {
            Self::Instance { zone, .. } => ("zone", zone.as_str()),
            Self::Host { region, .. } => ("region", region.as_str()),
        };
        let ack = ack.to_string();
        Self::query(&[
            ("sid", sid),
            ("ack", ack.as_str()),
            ("newWebsocket", "True"),
            (scope_key, scope),
        ])
    }
}

/// Supplies a fresh OAuth access token for every (re)connect of the tunnel.
#[async_trait]
pub trait TokenSource: Send + Sync {
    async fn access_token(&self) -> Result<SecretString, SshError>;
}

pub struct IapTransport {
    dialer: Arc<Dialer>,
}

impl IapTransport {
    /// `endpoint` is the tunnel service, e.g. `wss://tunnel.cloudproxy.app`.
    pub fn new(target: IapTarget, endpoint: String, tokens: Arc<dyn TokenSource>) -> Self {
        Self {
            dialer: Arc::new(Dialer::new(target, endpoint, tokens)),
        }
    }
}

#[async_trait]
impl Transport for IapTransport {
    async fn connect(&self) -> Result<BoxStream, SshError> {
        Ok(Box::new(tunnel::open(self.dialer.clone()).await?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> IapTarget {
        IapTarget::Instance {
            project: "my-proj".into(),
            zone: "us-central1-b".into(),
            instance: "jump er".into(),
            interface: "nic0".into(),
            port: 22,
        }
    }

    #[test]
    fn connect_query_should_follow_gcloud_order_when_target_is_instance() {
        assert_eq!(
            instance().connect_query(),
            "project=my-proj&port=22&newWebsocket=True&zone=us-central1-b&instance=jump%20er&interface=nic0"
        );
    }

    #[test]
    fn reconnect_query_should_only_carry_sid_ack_and_scope_when_target_is_instance() {
        assert_eq!(
            instance().reconnect_query("a+b", 42),
            "sid=a%2Bb&ack=42&newWebsocket=True&zone=us-central1-b"
        );
    }

    #[test]
    fn queries_should_use_region_and_group_when_target_is_host() {
        let target = IapTarget::Host {
            project: "p".into(),
            region: "us-east1".into(),
            network: "net".into(),
            host: "10.0.0.1".into(),
            dest_group: Some("grp".into()),
            port: 443,
        };
        assert_eq!(
            target.connect_query(),
            "project=p&port=443&newWebsocket=True&region=us-east1&network=net&host=10.0.0.1&group=grp"
        );
        assert_eq!(
            target.reconnect_query("s", 0),
            "sid=s&ack=0&newWebsocket=True&region=us-east1"
        );
    }
}
