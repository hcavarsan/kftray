use std::net::IpAddr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostEntry {
    pub ip: IpAddr,
    pub hostname: String,
}
