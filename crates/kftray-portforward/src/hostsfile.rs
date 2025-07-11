use std::sync::LazyLock;

use kftray_commons::models::hostfile::HostEntry;
use log::{
    debug,
    warn,
};

use crate::hostfile_direct::DirectHostfileManager;
use crate::hostfile_helper::HostfileHelperClient;

static HOSTFILE_MANAGER: LazyLock<HostfileManager> = LazyLock::new(HostfileManager::new);

pub struct HostfileManager {
    helper_client: Option<HostfileHelperClient>,
    direct_manager: DirectHostfileManager,
}

impl HostfileManager {
    pub fn new() -> Self {
        let helper_client = HostfileHelperClient::new().ok();
        Self {
            helper_client,
            direct_manager: DirectHostfileManager::new(),
        }
    }

    pub fn add_host_entry(&self, id: String, entry: HostEntry) -> std::io::Result<()> {
        if let Some(helper) = &self.helper_client {
            if helper.is_available() {
                match helper.add_host_entry(id.clone(), entry.clone()) {
                    Ok(_) => {
                        debug!("Successfully added host entry via helper for ID: {id}");
                        return Ok(());
                    }
                    Err(e) => {
                        warn!("Helper hostfile add failed: {e}, falling back to direct");
                    }
                }
            }
        }

        self.direct_manager.add_host_entry(id, entry)
    }

    pub fn remove_host_entry(&self, id: &str) -> std::io::Result<()> {
        if let Some(helper) = &self.helper_client {
            if helper.is_available() {
                match helper.remove_host_entry(id) {
                    Ok(_) => {
                        debug!("Successfully removed host entry via helper for ID: {id}");
                        return Ok(());
                    }
                    Err(e) => {
                        warn!("Helper hostfile remove failed: {e}, falling back to direct");
                    }
                }
            }
        }

        self.direct_manager.remove_host_entry(id)
    }

    pub fn remove_all_host_entries(&self) -> std::io::Result<()> {
        // Try helper first
        if let Some(helper) = &self.helper_client {
            if helper.is_available() {
                match helper.remove_all_host_entries() {
                    Ok(_) => {
                        debug!("Successfully removed all host entries via helper");
                        return Ok(());
                    }
                    Err(e) => {
                        warn!("Helper hostfile remove_all failed: {e}, falling back to direct");
                    }
                }
            }
        }

        self.direct_manager.remove_all_host_entries()
    }
}

impl Default for HostfileManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn add_host_entry(id: String, entry: HostEntry) -> std::io::Result<()> {
    HOSTFILE_MANAGER.add_host_entry(id, entry)
}

pub fn remove_host_entry(id: &str) -> std::io::Result<()> {
    HOSTFILE_MANAGER.remove_host_entry(id)
}

pub fn remove_all_host_entries() -> std::io::Result<()> {
    HOSTFILE_MANAGER.remove_all_host_entries()
}

#[cfg(test)]
mod tests {
    use std::net::{
        IpAddr,
        Ipv4Addr,
    };
    use std::sync::Once;

    use super::*;

    static INIT: Once = Once::new();

    fn init() {
        INIT.call_once(|| {
            let _ = env_logger::builder().is_test(true).try_init();
        });
    }

    fn get_test_entry() -> HostEntry {
        HostEntry {
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)),
            hostname: "test.local".to_string(),
        }
    }

    #[test]
    fn test_add_and_remove_host_entry() {
        init();
        let _ = remove_all_host_entries();

        let id = "test-id-1".to_string();
        let entry = get_test_entry();

        let result = add_host_entry(id.clone(), entry.clone());
        assert!(result.is_ok());

        let result = remove_host_entry(&id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_manager_creation() {
        init();
        let manager = HostfileManager::new();

        assert!(manager.helper_client.is_some() || manager.helper_client.is_none());
    }
}
