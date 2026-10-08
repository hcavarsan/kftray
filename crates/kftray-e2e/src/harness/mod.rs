pub mod build;
pub mod cluster;
pub mod display;
pub mod env;
pub mod nextest;
pub mod wait;
pub mod workload;

use std::net::{
    Ipv4Addr,
    TcpListener,
};
use std::path::{
    Path,
    PathBuf,
};

use anyhow::{
    Context,
    Result,
};
pub use env::{
    TestEnv,
    Timeouts,
};

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

pub fn free_port() -> Result<u16> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).context("bind a free local port")?;
    Ok(listener.local_addr().context("read the bound port")?.port())
}
