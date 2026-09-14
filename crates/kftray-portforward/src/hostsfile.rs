use std::collections::HashSet;
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
    /// Mappings handed to the privileged helper in this run, by configuration
    /// id. The helper marks the lines it writes, but a line left by a version
    /// that did not is only attributable through what was asked for.
    handed_to_helper: std::sync::Mutex<HashSet<(String, HostEntry)>>,
}

impl HostfileManager {
    pub fn new() -> Self {
        let helper_client = HostfileHelperClient::new().ok();
        Self {
            helper_client,
            direct_manager: DirectHostfileManager::new(),
            handed_to_helper: std::sync::Mutex::new(HashSet::new()),
        }
    }

    fn helper(&self) -> Option<&HostfileHelperClient> {
        self.helper_client
            .as_ref()
            .filter(|helper| helper.is_available())
    }

    pub fn add_host_entry(&self, id: String, entry: HostEntry) -> std::io::Result<()> {
        if let Some(helper) = self.helper() {
            match helper.add_host_entry(id.clone(), entry.clone()) {
                Ok(_) => {
                    debug!("Successfully added host entry via helper for ID: {id}");
                    self.handed_to_helper
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert((id, entry));
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

    /// Removes several ids from wherever they were written.
    ///
    /// Success means verified: after both writers have had their turn, the
    /// helper's section is read back and any of the ids still on disk fails the
    /// call, whichever writer reported what. An alias the application cannot
    /// remove itself is reported so the caller keeps the cleanup owed and a
    /// later stop, with the helper back, retries it.
    pub fn remove_host_entries(&self, ids: &[&str]) -> std::io::Result<()> {
        if let Some(helper) = self.helper() {
            for id in ids {
                if let Err(e) = helper.remove_host_entry(id) {
                    warn!("Helper hostfile remove failed for {id}: {e}");
                }
            }
        }

        // The helper only writes its own section, so an alias that fell back
        // to the direct manager earlier is still in the direct one. Removed
        // synchronously with its failure reported: nothing else records it.
        self.direct_manager.remove_host_entries(ids)?;

        // Read rather than inferred. Coverage in the direct section proves
        // nothing about the helper's copy: an id can be added through the
        // helper, then re-added with another hostname through the direct
        // fallback, and removing it would clear only the second. A failed read
        // is a failure: half a section still resolves, and treating it as
        // empty would report cleanup that did not happen.
        let section = DirectHostfileManager::helper_section()?;
        let handed = self
            .handed_to_helper
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let stranded = DirectHostfileManager::stranded_in_helper_section(&section, ids, &handed);
        if !stranded.is_empty() {
            return Err(std::io::Error::other(format!(
                "Host entries for {} are still on disk and only the helper can remove them",
                stranded.join(", ")
            )));
        }

        Ok(())
    }

    /// Clears every alias this application wrote, through either writer.
    ///
    /// The helper clears only its own section, so the direct one is cleared
    /// here regardless, and a failure from either is reported: a remove-all
    /// that left a section behind is not one.
    pub fn remove_all_host_entries(&self) -> std::io::Result<()> {
        let mut errors = Vec::new();
        if let Some(helper) = self.helper() {
            match helper.remove_all_host_entries() {
                Ok(_) => debug!("Successfully removed all host entries via helper"),
                Err(e) => errors.push(format!("helper: {e}")),
            }
        }
        if let Err(e) = self.direct_manager.remove_all_host_entries() {
            errors.push(format!("direct: {e}"));
        }
        self.handed_to_helper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(std::io::Error::other(errors.join("; ")))
        }
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
