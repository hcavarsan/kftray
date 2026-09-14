use std::collections::HashSet;

use kftray_commons::models::hostfile::HostEntry;
use kftray_commons::utils::hostsfile::{
    HostsFile,
    SectionEntry,
};
use log::debug;

/// Section the privileged helper owns, and where older versions of this
/// manager wrote before the two were separated.
pub const KFTRAY_HOSTS_TAG: &str = "kftray-hosts";
/// Section this manager owns outright.
///
/// Sharing one section with the helper meant every write had to reconstruct
/// another writer's lines from an unsynchronized read, which loses a concurrent
/// change in whichever direction the race went. Separate sections remove the
/// interaction: each writer touches only what it owns.
pub const KFTRAY_DIRECT_HOSTS_TAG: &str = "kftray-hosts-direct";

/// Writes host aliases straight into the hosts file when the privileged
/// helper is not available.
///
/// The file is the only state. Every mutation is an owner-scoped
/// read-modify-write under the cross-process hosts lock, so a line written by
/// an earlier run, by another instance, or by the helper is never rebuilt from
/// memory and never lost. A write that fails is reported to the caller, which
/// already records the cleanup it still owes and retries it; queueing the
/// change here instead would report success for an alias still on disk.
#[derive(Default)]
pub struct DirectHostfileManager;

impl DirectHostfileManager {
    pub fn new() -> Self {
        Self
    }

    pub fn add_host_entry(&self, id: String, entry: HostEntry) -> std::io::Result<()> {
        debug!("Adding host entry for ID {id}: {entry:?}");

        let mut hosts = HostsFile::new(KFTRAY_DIRECT_HOSTS_TAG);
        hosts
            .add_owned_entry(entry.ip, &entry.hostname, &id)
            .map_err(std::io::Error::other)?;
        hosts
            .reconcile_owners(&[id.as_str()])
            .map_err(std::io::Error::other)?;

        // A copy of the same alias left in the shared section by a version that
        // wrote there would keep resolving after this line is removed, so it is
        // migrated out now that the alias has an owned line of its own.
        Self::prune_legacy_entries(&[(entry.ip, entry.hostname)])
    }

    pub fn remove_host_entry(&self, id: &str) -> std::io::Result<()> {
        self.remove_host_entries(std::slice::from_ref(&id))
            .map(|_| ())
    }

    /// Removes several ids with one write.
    ///
    /// Returns whether any of the ids had a line here: a caller that reached
    /// this through a failing helper needs to tell "already gone" apart from
    /// "this manager never owned it".
    pub fn remove_host_entries(&self, ids: &[&str]) -> std::io::Result<bool> {
        debug!("Removing host entries for IDs {ids:?}");

        // Read first so the aliases being dropped are known: once their lines
        // are gone nothing else records which mapping a legacy copy would be.
        let dropping: Vec<(std::net::IpAddr, String)> = HostsFile::new(KFTRAY_DIRECT_HOSTS_TAG)
            .read_section()
            .map_err(std::io::Error::other)?
            .into_iter()
            .filter(|entry| {
                entry
                    .owner
                    .as_deref()
                    .is_some_and(|owner| ids.contains(&owner))
            })
            .map(|entry| (entry.ip, entry.hostname))
            .collect();

        let present = HostsFile::new(KFTRAY_DIRECT_HOSTS_TAG)
            .reconcile_owners(ids)
            .map_err(std::io::Error::other)?;

        Self::prune_legacy_entries(&dropping)?;

        Ok(!present.is_empty())
    }

    /// Takes unmarked copies of `mappings` out of the shared section.
    ///
    /// Only lines with no owner are candidates: they were written by a version
    /// that shared the helper's section and cannot be attributed any other
    /// way. A line the helper marked with another configuration's id is that
    /// configuration's, however similar its alias, and stays.
    fn prune_legacy_entries(mappings: &[(std::net::IpAddr, String)]) -> std::io::Result<()> {
        if mappings.is_empty() {
            return Ok(());
        }
        let legacy = HostsFile::new(KFTRAY_HOSTS_TAG);
        // Skipped without a write when the section is absent: the hosts file
        // usually needs elevated privileges even to open for writing, and a
        // rewrite that changes nothing would still fail on that.
        if !legacy.section_exists().map_err(std::io::Error::other)? {
            return Ok(());
        }
        legacy
            .retain_section(|entry: &SectionEntry| {
                entry.owner.is_some()
                    || !mappings
                        .iter()
                        .any(|(ip, hostname)| entry.ip == *ip && entry.hostname == *hostname)
            })
            .map(|_| ())
            .map_err(std::io::Error::other)
    }

    /// The privileged helper's section, as it is on disk.
    ///
    /// This manager cannot write there, so a caller that needs verified
    /// cleanup has to look at what is actually left.
    pub fn helper_section() -> std::io::Result<Vec<SectionEntry>> {
        HostsFile::new(KFTRAY_HOSTS_TAG)
            .read_section()
            .map_err(std::io::Error::other)
    }

    /// Removes both sections whole, unowned lines included.
    ///
    /// Entries written by a version that shared the helper's section belong to
    /// this application too, and a purge that left them would report complete
    /// cleanup with aliases still resolving.
    pub fn remove_all_host_entries(&self) -> std::io::Result<()> {
        debug!("Removing all host entries");

        HostsFile::new(KFTRAY_DIRECT_HOSTS_TAG)
            .write()
            .map_err(std::io::Error::other)?;
        HostsFile::new(KFTRAY_HOSTS_TAG)
            .write()
            .map(|_| ())
            .map_err(std::io::Error::other)
    }

    /// Every owned alias in this manager's section.
    pub fn list_host_entries(&self) -> std::io::Result<Vec<(String, HostEntry)>> {
        Ok(HostsFile::new(KFTRAY_DIRECT_HOSTS_TAG)
            .read_section()
            .map_err(std::io::Error::other)?
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

    /// Whether any of `ids` still has a line in the helper's section, judged
    /// against what is on disk.
    ///
    /// A line the helper marked is attributed by its owner. One it wrote
    /// without a mark, as older versions did, counts only when `handed`
    /// records that exact mapping being given to the helper for that id.
    pub fn stranded_in_helper_section<'a>(
        section: &[SectionEntry], ids: &[&'a str], handed: &HashSet<(String, HostEntry)>,
    ) -> Vec<&'a str> {
        ids.iter()
            .copied()
            .filter(|id| {
                section.iter().any(|entry| match &entry.owner {
                    Some(owner) => owner == id,
                    None => handed.contains(&(
                        (*id).to_owned(),
                        HostEntry {
                            ip: entry.ip,
                            hostname: entry.hostname.clone(),
                        },
                    )),
                })
            })
            .collect()
    }

    #[cfg(test)]
    fn prune_legacy_entries_in(
        path: &std::path::Path, mappings: &[(std::net::IpAddr, String)],
    ) -> std::io::Result<()> {
        HostsFile::new(KFTRAY_HOSTS_TAG)
            .retain_section_in(path, |entry: &SectionEntry| {
                entry.owner.is_some()
                    || !mappings
                        .iter()
                        .any(|(ip, hostname)| entry.ip == *ip && entry.hostname == *hostname)
            })
            .map(|_| ())
            .map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;

    fn addr(last: u8) -> IpAddr {
        IpAddr::from([127, 0, 0, last])
    }

    #[test]
    fn pruning_spares_lines_the_helper_attributed_to_another_configuration() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();

        // The shared section as an upgrade finds it: one alias written by an
        // older direct writer with no owner, and the same alias the current
        // helper wrote for configuration 7.
        let mut legacy = HostsFile::new(KFTRAY_HOSTS_TAG);
        legacy
            .add_entry(addr(1), "shared.local")
            .add_owned_entry(addr(1), "shared.local", "7")
            .unwrap()
            .add_entry(addr(1), "other.local");
        legacy.write_to(&temp_path).unwrap();

        DirectHostfileManager::prune_legacy_entries_in(
            &temp_path,
            &[(addr(1), "shared.local".to_owned())],
        )
        .unwrap();

        let remaining: Vec<(String, Option<String>)> = HostsFile::new(KFTRAY_HOSTS_TAG)
            .read_section_from(&temp_path)
            .unwrap()
            .into_iter()
            .map(|entry| (entry.hostname, entry.owner))
            .collect();
        assert_eq!(
            remaining,
            vec![
                ("shared.local".to_owned(), Some("7".to_owned())),
                ("other.local".to_owned(), None),
            ],
            "only the unmarked copy of the pruned alias goes; configuration 7's line and an \
             unrelated alias stay"
        );
    }

    #[test]
    fn helper_lines_are_attributed_by_owner_or_by_what_was_handed_over() {
        let section = vec![
            SectionEntry {
                ip: addr(1),
                hostname: "marked.local".to_owned(),
                owner: Some("1-https".to_owned()),
            },
            SectionEntry {
                ip: addr(1),
                hostname: "unmarked.local".to_owned(),
                owner: None,
            },
        ];
        let handed = HashSet::from([(
            "2-https".to_owned(),
            HostEntry {
                ip: addr(1),
                hostname: "unmarked.local".to_owned(),
            },
        )]);

        let stranded = DirectHostfileManager::stranded_in_helper_section(
            &section,
            &["1-https", "2-https", "3-https"],
            &handed,
        );
        assert_eq!(
            stranded,
            vec!["1-https", "2-https"],
            "a marked line names its owner; an unmarked one counts only for the id it was \
             handed over for; an id with neither is not stranded"
        );
    }
}
