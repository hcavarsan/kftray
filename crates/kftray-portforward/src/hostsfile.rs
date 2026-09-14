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
        // Unit tests never talk to an installed helper: its state is shared
        // with every other test and with the machine, so a test's outcome
        // would depend on what happens to be in the system hosts file.
        let helper_client = if cfg!(test) {
            None
        } else {
            HostfileHelperClient::new().ok()
        };
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

    /// Removes several ids from wherever they were written.
    ///
    /// Success means verified: after both writers have had their turn, the
    /// helper's section is read back and any of the ids still on disk fails the
    /// call, whichever writer reported what. An alias the application cannot
    /// remove itself is reported so the caller keeps the cleanup owed and a
    /// later stop, with the helper back, retries it.
    pub fn remove_host_entries(
        &self, ids: &[&str], expected: &[(String, HostEntry)],
    ) -> std::io::Result<()> {
        let mut asked_helper = false;
        if let Some(helper) = self.helper() {
            asked_helper = true;
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
        // Attribution for unmarked lines comes from two places: what this run
        // handed to the helper, and what the configuration itself says its
        // aliases are. The second survives restarts and a reply that never
        // arrived, which the first does not.
        let mut handed = self
            .handed_to_helper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        handed.extend(expected.iter().cloned());
        // A helper from before this change answers before it writes: it queues
        // the removal for a background writer that runs within a fraction of
        // a second. Its lines are given that long to disappear before they are
        // reported, so an installed helper that has not been upgraded yet does
        // not fail every stop.
        const HELPER_SETTLE: std::time::Duration = std::time::Duration::from_millis(100);
        const HELPER_SETTLE_ATTEMPTS: usize = 20;
        let mut stranded = Vec::new();
        for attempt in 0..HELPER_SETTLE_ATTEMPTS {
            let section = DirectHostfileManager::helper_section()?;
            stranded = DirectHostfileManager::stranded_in_helper_section(&section, ids, &handed);
            if stranded.is_empty() || !asked_helper {
                break;
            }
            if attempt + 1 < HELPER_SETTLE_ATTEMPTS {
                std::thread::sleep(HELPER_SETTLE);
            }
        }
        // What is left after the owned removal are lines a helper from before
        // ownership wrote: it never marked them, so the per-id removal cannot
        // reach them. They are attributed the same way they were detected, by
        // the aliases the configuration says are its own, and the upgraded
        // helper removes exactly those.
        if !stranded.is_empty() {
            let legacy: Vec<HostEntry> = handed
                .iter()
                .filter(|(id, _)| stranded.contains(&id.as_str()))
                .map(|(_, entry)| entry.clone())
                .collect();
            let removed = match self.helper() {
                Some(helper) => helper
                    .remove_unowned_host_entries(legacy)
                    .map_err(|e| e.to_string()),
                // Without a helper the same attributed lines are pruned here,
                // if this process can write the file: an installation that
                // only ever wrote directly has them from before ownership.
                None => {
                    let mappings: Vec<(std::net::IpAddr, String)> = legacy
                        .into_iter()
                        .map(|entry| (entry.ip, entry.hostname))
                        .collect();
                    self.direct_manager
                        .prune_legacy_entries(&mappings)
                        .map_err(|e| e.to_string())
                }
            };
            match removed {
                Ok(()) => {
                    let section = DirectHostfileManager::helper_section()?;
                    stranded =
                        DirectHostfileManager::stranded_in_helper_section(&section, ids, &handed);
                }
                Err(e) => warn!("Could not remove unmarked host entries: {e}"),
            }
        }
        if !stranded.is_empty() {
            return Err(std::io::Error::other(format!(
                "Host entries for {} are still on disk and could not be removed",
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

        if !errors.is_empty() {
            // The attribution stays: an unmarked line an older helper left
            // behind can only be tied to its id through what was handed over,
            // and a later per-id removal would otherwise pass verification with
            // the alias still resolving.
            return Err(std::io::Error::other(errors.join("; ")));
        }
        self.handed_to_helper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        Ok(())
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

/// Every alias a configuration may have written, keyed by the id each was
/// written under.
///
/// Derived from the configuration rather than remembered: it is the one
/// description of these aliases that survives a restart, and it is what
/// lets a removal recognise an unmarked line an older helper left behind.
/// Only aliases this configuration could have created are listed: the
/// domain line is written only for a domain-enabled service, and the HTTPS
/// lines only for a TCP forward, so a configuration that merely shares an
/// alias with one that did write them does not claim their lines.
pub fn config_host_entries(
    id: i64, config: Option<&kftray_commons::models::config_model::Config>,
) -> Vec<(String, HostEntry)> {
    let Some(config) = config else {
        return Vec::new();
    };
    let Some(alias) = config.alias.as_deref().filter(|alias| !alias.is_empty()) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    let loopback: std::net::IpAddr = "127.0.0.1".parse().unwrap();
    // The same default creation uses: a configuration with no address gets
    // its domain alias on 127.0.0.1, so that is the line to look for.
    if config.domain_enabled.unwrap_or_default()
        && config.service.is_some()
        && let Some(ip) = config
            .local_address
            .as_deref()
            .map_or(Some(loopback), |address| address.parse().ok())
    {
        entries.push((
            id.to_string(),
            HostEntry {
                ip,
                hostname: alias.to_owned(),
            },
        ));
    }
    if config.protocol == "tcp" {
        entries.push((
            format!("{id}-https"),
            HostEntry {
                ip: loopback,
                hostname: alias.to_owned(),
            },
        ));
        entries.push((
            format!("{id}-https-local"),
            HostEntry {
                ip: loopback,
                hostname: format!("{alias}.local"),
            },
        ));
    }
    entries
}

/// Removes every alias a configuration may have written, domain and SSL
/// alike, with one reconciliation and one verification.
///
/// `in_use` are the configurations still forwarding: an unmarked line that
/// one of them could have written is theirs to keep, however much it looks
/// like this configuration's, and is neither pruned nor reported.
///
/// Reported rather than swallowed, so a caller keeps the configuration
/// tracked for retry when an alias could not be verified gone.
pub fn remove_config_host_entries(
    id: i64, config: Option<&kftray_commons::models::config_model::Config>,
    in_use: &[kftray_commons::models::config_model::Config],
) -> std::io::Result<()> {
    let ids = [
        id.to_string(),
        format!("{id}-https"),
        format!("{id}-https-local"),
    ];
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let protected: Vec<HostEntry> = in_use
        .iter()
        .filter(|other| other.id != Some(id))
        .flat_map(|other| config_host_entries(other.id.unwrap_or_default(), Some(other)))
        .map(|(_, entry)| entry)
        .collect();
    let expected: Vec<(String, HostEntry)> = config_host_entries(id, config)
        .into_iter()
        .filter(|(_, entry)| !protected.contains(entry))
        .collect();
    HOSTFILE_MANAGER.remove_host_entries(&ids, &expected)
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
    fn a_configuration_names_the_aliases_its_removal_must_verify() {
        use kftray_commons::models::config_model::Config;

        let config = Config {
            id: Some(41),
            alias: Some("app.local".to_owned()),
            local_address: Some("127.0.0.7".to_owned()),
            domain_enabled: Some(true),
            service: Some("app".to_owned()),
            protocol: "tcp".to_owned(),
            ..Config::default()
        };
        let entries = config_host_entries(41, Some(&config));
        let ids: Vec<String> = entries.iter().map(|(id, _)| id.clone()).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        assert_eq!(ids, vec!["41", "41-https", "41-https-local"]);

        // A configuration that could not have written a line does not claim
        // it: no domain line without the domain feature, no HTTPS lines for
        // UDP.
        let plain = Config {
            domain_enabled: Some(false),
            protocol: "udp".to_owned(),
            ..config.clone()
        };
        assert!(config_host_entries(41, Some(&plain)).is_empty());
        assert_eq!(
            entries[0].1.ip,
            "127.0.0.7".parse::<std::net::IpAddr>().unwrap()
        );
        assert_eq!(entries[2].1.hostname, "app.local.local");

        // What survives a restart: nothing was handed to the helper in this
        // run, yet an unmarked line an older helper wrote for this alias is
        // still this configuration's and must be reported as stranded.
        let section = vec![kftray_commons::utils::hostsfile::SectionEntry {
            ip: "127.0.0.7".parse().unwrap(),
            hostname: "app.local".to_owned(),
            owner: None,
        }];
        let handed: HashSet<(String, HostEntry)> = entries.into_iter().collect();
        assert_eq!(
            DirectHostfileManager::stranded_in_helper_section(&section, &ids, &handed),
            vec!["41"]
        );
        assert!(
            DirectHostfileManager::stranded_in_helper_section(&section, &ids, &HashSet::new())
                .is_empty(),
            "without the configuration the same line is unattributable"
        );
    }

    #[test]
    fn unit_tests_never_reach_an_installed_helper() {
        init();
        let manager = HostfileManager::new();

        assert!(
            manager.helper_client.is_none(),
            "a test outcome must not depend on the machine's helper or hosts file"
        );
    }
}
