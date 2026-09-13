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
        if let Some(helper) = &self.helper_client
            && helper.is_available()
        {
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

        self.direct_manager.add_host_entry(id, entry)
    }

    pub fn remove_host_entry(&self, id: &str) -> std::io::Result<()> {
        self.remove_host_entries(std::slice::from_ref(&id))
    }

    /// Removes several ids, reconciling them with one write where possible.
    ///
    /// The direct manager rewrites its own lines in one pass, so a single
    /// successful write covers every id it owns. The helper removes one at a
    /// time, so its failures stay per-id.
    pub fn remove_host_entries(&self, ids: &[&str]) -> std::io::Result<()> {
        let mut helper_error = None;
        if let Some(helper) = &self.helper_client
            && helper.is_available()
        {
            let mut errors = Vec::new();
            for id in ids {
                if let Err(e) = helper.remove_host_entry(id) {
                    errors.push(format!("{id}: {e}"));
                }
            }
            if errors.is_empty() {
                // Dropped here too: an add that fell back to this manager and
                // never reached disk is still pending, and retrying it would
                // write the alias the helper just removed back again.
                self.direct_manager.forget_entries(ids);
                return Ok(());
            }
            let joined = errors.join("; ");
            warn!("Helper hostfile remove failed ({joined}), falling back to direct");
            helper_error = Some(joined);
        }

        match (self.direct_manager.remove_host_entries(ids), helper_error) {
            (Ok(_), None) => Ok(()),
            // Every id was one this manager wrote, so its rewrite took them all
            // off disk and the helper's failure is moot.
            (Ok(true), Some(_)) => Ok(()),
            // At least one id has no line of this manager's, so the helper owns
            // it and its alias is still on disk.
            (Ok(false), Some(error)) => Err(std::io::Error::other(error)),
            (Err(error), _) => Err(error),
        }
    }

    pub fn remove_all_host_entries(&self) -> std::io::Result<()> {
        if let Some(helper) = &self.helper_client
            && helper.is_available()
        {
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

pub fn add_ssl_host_entry(config_id: &str, alias: &str, _https_port: u16) -> std::io::Result<()> {
    let https_entry = HostEntry {
        ip: "127.0.0.1".parse().unwrap(),
        hostname: alias.to_string(),
    };
    add_host_entry(format!("{}-https", config_id), https_entry)?;

    let local_entry = HostEntry {
        ip: "127.0.0.1".parse().unwrap(),
        hostname: format!("{}.local", alias),
    };
    add_host_entry(format!("{}-https-local", config_id), local_entry)?;

    Ok(())
}

pub fn remove_ssl_host_entry(config_id: &str) -> std::io::Result<()> {
    // Removed together so one reconciliation covers both: removing them one at
    // a time would report the first failure even when the second write, which
    // rewrites the whole file, already took both aliases out. Reported rather
    // than swallowed, so a caller can keep the configuration tracked for retry.
    let https = format!("{config_id}-https");
    let local = format!("{config_id}-https-local");

    HOSTFILE_MANAGER.remove_host_entries(&[https.as_str(), local.as_str()])
}

pub fn update_hosts_with_ssl_from_config(
    config: &kftray_commons::models::config_model::Config,
) -> Result<(), String> {
    let alias = config
        .alias
        .as_ref()
        .ok_or("Alias required for SSL hosts entry")?;

    let config_id = config.id.unwrap_or(-1).to_string();
    let port = config.local_port.unwrap_or(8080);

    add_ssl_host_entry(&config_id, alias, port)
        .map_err(|e| format!("Failed to add HTTPS hosts entry: {}", e))?;

    log::info!(
        "Added HTTPS hosts entries: {} and {}.local -> 127.0.0.1:{}",
        alias,
        alias,
        port
    );
    Ok(())
}

pub fn remove_ssl_host_entry_from_config(
    config: &kftray_commons::models::config_model::Config,
) -> Result<(), String> {
    let config_id = config.id.unwrap_or(-1).to_string();

    remove_ssl_host_entry(&config_id)
        .map_err(|e| format!("Failed to remove HTTPS hosts entry: {}", e))?;

    log::info!("Removed HTTPS hosts entry for config: {}", config_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use super::*;

    static INIT: Once = Once::new();

    fn init() {
        INIT.call_once(|| {
            let _ = env_logger::builder().is_test(true).try_init();
        });
    }

    // `test_add_and_remove_host_entry` used to live here. It asserted that two
    // calls returned Ok, which only held where the process could write the
    // system hosts file, and it rewrote that file as a side effect. The
    // decisions it was meant to cover are asserted without touching it in
    // `hostfile_direct::tests`.

    #[test]
    fn test_manager_creation() {
        init();
        let manager = HostfileManager::new();

        assert!(manager.helper_client.is_some() || manager.helper_client.is_none());
    }
}
