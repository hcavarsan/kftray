use kftray_commons::models::hostfile::HostEntry;
use kftray_commons::utils::hostsfile::HostsFile;
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

        let mut hosts = HostsFile::new(KFTRAY_HOSTS_TAG);
        // Marked with the configuration it belongs to: an unattributable line
        // cannot be told apart from another writer's, so the application
        // verifying one removal would have to treat every line as possibly
        // its own.
        hosts.add_owned_entry(entry.ip, &entry.hostname, &id)?;
        hosts.reconcile_owners(&[id.as_str()])?;
        Ok(())
    }

    pub fn remove_entry(&self, id: &str) -> Result<(), HostfileError> {
        debug!("Removing host entry for ID {id}");

        HostsFile::new(KFTRAY_HOSTS_TAG).reconcile_owners(&[id])?;
        Ok(())
    }

    /// Removes the whole section, unmarked lines included.
    pub fn remove_all_entries(&self) -> Result<(), HostfileError> {
        info!("Removing all host entries");

        HostsFile::new(KFTRAY_HOSTS_TAG).write()?;
        Ok(())
    }

    /// Every owned alias in this helper's section.
    pub fn list_entries(&self) -> Result<Vec<(String, HostEntry)>, HostfileError> {
        Ok(HostsFile::new(KFTRAY_HOSTS_TAG)
            .read_section()?
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
