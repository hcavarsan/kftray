use std::sync::Arc;

use async_trait::async_trait;

use super::mock_sshd::{
    MockSshd,
    Shared,
};
use crate::error::SshError;
use crate::route::{
    BoxStream,
    Transport,
};

const BUFFER: usize = 64 * 1024;

/// Reaches a [`MockSshd`] over an in-memory stream: no sockets, no provider.
pub struct DuplexTransport {
    sshd: Arc<Shared>,
}

impl DuplexTransport {
    pub fn new(sshd: &MockSshd) -> Self {
        Self {
            sshd: sshd.shared(),
        }
    }
}

#[async_trait]
impl Transport for DuplexTransport {
    async fn connect(&self) -> Result<BoxStream, SshError> {
        let (client, server) = tokio::io::duplex(BUFFER);
        self.sshd.serve(server);
        Ok(Box::new(client))
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::route;
    use crate::testing::route_conformance;

    #[tokio::test(flavor = "multi_thread")]
    async fn duplex_transport_should_pass_route_conformance() {
        route_conformance(async |mock| route(mock, "conformance")).await;
    }
}
