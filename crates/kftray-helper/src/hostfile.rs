use kftray_commons::models::hostfile::HostEntry;
use kftray_commons::utils::hostsfile::{
    SectionEntry,
    edit_hosts,
    read_hosts,
};
use log::{
    debug,
    info,
};
use thiserror::Error;

const KFTRAY_HOSTS_TAG: &str = "kftray-hosts";

#[derive(Error, Debug)]
pub enum HostfileError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Hosts file error: {0}")]
    HostsFile(String),
}

impl From<kftray_commons::utils::hostsfile::HostsFileError> for HostfileError {
    fn from(error: kftray_commons::utils::hostsfile::HostsFileError) -> Self {
        Self::HostsFile(error.to_string())
    }
}

/// Writes host aliases on behalf of the unprivileged application.
///
/// The hosts file is the only state. Each request is an owner-scoped
/// read-modify-write under the cross-process hosts lock, and returns only once
/// the file holds the result: a reply of success used to mean the change was
/// queued for a background writer, so the application could be told an alias
/// was gone while it still resolved. Lines this process did not write, from
/// an earlier run of the helper or from the application's own section, are
/// never rebuilt from memory.
#[derive(Default)]
pub struct HostfileManager;

impl HostfileManager {
    pub fn new() -> Self {
        Self
    }

    pub fn add_entry(&self, id: String, entry: HostEntry) -> Result<(), HostfileError> {
        debug!("Adding host entry for ID {id}: {entry:?}");

        // Marked with the configuration it belongs to: an unattributable line
        // cannot be told apart from another writer's, so the application
        // verifying one removal would have to treat every line as possibly
        // its own.
        let owned = SectionEntry {
            ip: entry.ip,
            hostname: entry.hostname,
            owner: Some(id.clone()),
        };
        edit_hosts(|document| {
            document.reconcile_owners(KFTRAY_HOSTS_TAG, &[id.as_str()], &[owned])?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn remove_entry(&self, id: &str) -> Result<(), HostfileError> {
        debug!("Removing host entry for ID {id}");

        edit_hosts(|document| {
            document.reconcile_owners(KFTRAY_HOSTS_TAG, &[id], &[])?;
            Ok(())
        })?;
        Ok(())
    }

    /// Removes the whole section, unmarked lines included.
    pub fn remove_all_entries(&self) -> Result<(), HostfileError> {
        info!("Removing all host entries");

        edit_hosts(|document| document.clear_section(KFTRAY_HOSTS_TAG))?;
        Ok(())
    }

    /// Every owned alias in this helper's section.
    pub fn list_entries(&self) -> Result<Vec<(String, HostEntry)>, HostfileError> {
        Ok(read_hosts(|document| document.section(KFTRAY_HOSTS_TAG))?
            .into_iter()
            .filter_map(|entry| {
                entry.owner.map(|owner| {
                    (
                        owner,
                        HostEntry {
                            ip: entry.ip,
                            hostname: entry.hostname,
                        },
                    )
                })
            })
            .collect())
    }
}
