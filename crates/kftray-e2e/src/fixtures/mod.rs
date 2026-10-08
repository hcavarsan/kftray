mod forwards;

pub use forwards::{
    pod_http,
    service_http,
};
use kftray_commons::models::config_model::Config;

use crate::harness::workload::Workload;

#[derive(Clone, Debug)]
pub struct Fixture {
    pub name: &'static str,
    pub workload: Workload,
    pub config: Config,
    pub expect: Expect,
}

#[derive(Clone, Debug)]
pub enum Expect {
    Http { body: &'static str },
}

impl Fixture {
    pub fn local_port(&self) -> u16 {
        self.config.local_port.unwrap_or_default()
    }

    pub fn alias(&self) -> &str {
        self.config.alias.as_deref().unwrap_or(self.name)
    }
}
