mod address_pool;
mod auth;
pub mod client;
pub mod communication;
pub mod error;
#[cfg(unix)]
mod fd_passing;
mod hostfile;
pub mod messages;
mod network;
pub mod platforms;
#[cfg(unix)]
mod socket_bind;
#[cfg(target_os = "windows")]
mod win_identity;

pub use client::HelperClient;
pub use error::HelperError;
