use futures::stream::{
    FuturesUnordered,
    StreamExt,
};
use log::warn;
use tokio::time::timeout;

use crate::types::MonitorConfig;

pub struct NetworkChecker {
    config: MonitorConfig,
}

impl NetworkChecker {
    pub fn new(config: MonitorConfig) -> Self {
        Self { config }
    }

    pub async fn check_connectivity(&self) -> bool {
        let mut futs: FuturesUnordered<_> = self
            .config
            .network_endpoints
            .iter()
            .map(|&endpoint| {
                let dur = self.config.network_timeout;
                async move {
                    matches!(
                        timeout(dur, tokio::net::TcpStream::connect(endpoint)).await,
                        Ok(Ok(_))
                    )
                }
            })
            .collect();

        while let Some(ok) = futs.next().await {
            if ok {
                return true;
            }
        }
        false
    }

    pub async fn get_network_fingerprint(&self) -> String {
        let mut fingerprint = Vec::new();
        let mut found_local_addr = false;

        for endpoint in &self.config.network_endpoints {
            if let Ok(Ok(socket)) = timeout(
                self.config.sleep_down,
                tokio::net::TcpStream::connect(endpoint),
            )
            .await
            {
                if let Ok(local_addr) = socket.local_addr() {
                    fingerprint.push(local_addr.ip().to_string());
                    found_local_addr = true;
                    break;
                }
            }
        }

        let mut route_count = 0;
        for test_ip in &self.config.network_endpoints {
            if timeout(
                self.config.sleep_down,
                tokio::net::TcpStream::connect(test_ip),
            )
            .await
            .is_ok()
            {
                route_count += 1;
            }
        }
        fingerprint.push(route_count.to_string());

        if !found_local_addr && route_count == 0 {
            "no_network".to_string()
        } else {
            fingerprint.join("_")
        }
    }
}

pub async fn is_port_listening(address: &str, port: u16) -> bool {
    use std::net::{
        SocketAddr,
        TcpListener,
    };

    let addr: SocketAddr = match format!("{address}:{port}").parse() {
        Ok(addr) => addr,
        Err(_) => return false,
    };

    match TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            false
        }
        Err(err) => match err.kind() {
            std::io::ErrorKind::AddrInUse => true,
            other => {
                warn!("port-check bind failed: {other}");
                false
            }
        },
    }
}
