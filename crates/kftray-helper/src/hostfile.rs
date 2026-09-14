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
/// Section the application writes when it does not go through this helper.
const KFTRAY_DIRECT_HOSTS_TAG: &str = "kftray-hosts-direct";

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

/// Whether a line stays when unmarked copies of `entries` are removed.
///
/// A line the helper marked is some configuration's, however similar its
/// alias, and only an unmarked line that matches one of the given aliases can
/// be tied to the configuration asking for the removal.
fn survives_legacy_removal(line: &SectionEntry, entries: &[HostEntry]) -> bool {
    line.owner.is_some()
        || !entries
            .iter()
            .any(|entry| entry.ip == line.ip && entry.hostname == line.hostname)
}

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
            hostname: entry.hostname.clone(),
            owner: Some(id.clone()),
        };
        // An unmarked copy of the same alias, written by a version of this
        // helper that did not mark its lines, is left where it is: an older
        // application instance may still be forwarding on it, and only a
        // removal that knows which forwards are running can take it out.
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

    /// Removes unmarked lines matching `entries`, leaving every owned line and
    /// every other unmarked alias where it is.
    pub fn remove_unowned_matching(&self, entries: &[HostEntry]) -> Result<(), HostfileError> {
        debug!("Removing {} unmarked legacy host entries", entries.len());

        edit_hosts(|document| {
            document.retain(KFTRAY_HOSTS_TAG, |line| {
                survives_legacy_removal(line, entries)
            })
        })?;
        Ok(())
    }

    /// Removes the given owners' lines from the application's direct section,
    /// leaving every other owner's line where it is.
    pub fn remove_direct_owned(&self, ids: &[String]) -> Result<(), HostfileError> {
        debug!("Removing direct host entries for IDs {ids:?}");

        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        edit_hosts(|document| {
            document.reconcile_owners(KFTRAY_DIRECT_HOSTS_TAG, &ids, &[])?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hostname: &str) -> HostEntry {
        HostEntry {
            ip: [127, 0, 0, 1].into(),
            hostname: hostname.to_owned(),
        }
    }

    fn line(hostname: &str, owner: Option<&str>) -> SectionEntry {
        SectionEntry {
            ip: [127, 0, 0, 1].into(),
            hostname: hostname.to_owned(),
            owner: owner.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn legacy_removal_takes_only_unmarked_copies_of_the_named_aliases() {
        let asked = [entry("svc.local")];
        assert!(!survives_legacy_removal(&line("svc.local", None), &asked));
        assert!(survives_legacy_removal(
            &line("svc.local", Some("7")),
            &asked
        ));
        assert!(survives_legacy_removal(&line("other.local", None), &asked));
    }
}
