//! Test doubles shared by kftray-ssh, kftray-portforward and kftray-e2e.

mod conformance;
mod duplex;
mod memory_store;
mod mock_sshd;

pub use conformance::route_conformance;
pub use duplex::DuplexTransport;
pub use memory_store::MemoryHostKeyStore;
pub use mock_sshd::{
    MockSshd,
    MockSshdOptions,
};
