use std::collections::HashSet;

use kftray_commons::models::hostfile::HostEntry;
use kftray_commons::utils::hostsfile::{
    HostsDocument,
    SectionEntry,
    edit_hosts,
    read_hosts,
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

        let owned = SectionEntry {
            ip: entry.ip,
            hostname: entry.hostname.clone(),
            owner: Some(id.clone()),
        };
        // An unmarked copy of the same alias left in the shared section by an
        // older version is left where it is: it may be the line an older
        // instance, still running, resolves by, and nothing here can tell.
        // Removal attributes such copies to the configuration and prunes
        // them then, keeping the ones a running forward still needs.
        edit_hosts(|document| {
            document.reconcile_owners(KFTRAY_DIRECT_HOSTS_TAG, &[id.as_str()], &[owned])?;
            Ok(())
        })
        .map_err(std::io::Error::from)
    }

    /// Removes several ids with one write.
    ///
    /// Returns whether any of the ids had a line here: a caller that reached
    /// this through a failing helper needs to tell "already gone" apart from
    /// "this manager never owned it".
    /// `protected` are mappings a still-forwarding configuration could have
    /// written; an unmarked copy of one of them is never pruned on this
    /// configuration's behalf.
    pub fn remove_host_entries(
        &self, ids: &[&str], protected: &[HostEntry],
    ) -> std::io::Result<bool> {
        debug!("Removing host entries for IDs {ids:?}");

        edit_hosts(|document| {
            // Read inside the same edit that drops them: once the owned lines
            // are gone nothing else records which mapping a legacy copy would
            // be, and a failure between the two would lose it.
            let dropping = legacy_mappings_to_prune(
                &document.section(KFTRAY_DIRECT_HOSTS_TAG)?,
                ids,
                protected,
            );
            let present = document.reconcile_owners(KFTRAY_DIRECT_HOSTS_TAG, ids, &[])?;
            prune_legacy(document, &dropping)?;
            Ok(!present.is_empty())
        })
        .map_err(std::io::Error::from)
    }

    /// Takes unmarked copies of `mappings` out of the shared section, when
    /// this process can write the hosts file.
    ///
    /// An installation that never had the helper still has the aliases an
    /// earlier version wrote into the shared section, with no owner to remove
    /// them by. They are attributed the same way the verification attributes
    /// them, by the aliases the configuration says are its own.
    pub fn prune_legacy_entries(
        &self, mappings: &[(std::net::IpAddr, String)],
    ) -> std::io::Result<()> {
        edit_hosts(|document| prune_legacy(document, mappings)).map_err(std::io::Error::from)
    }

    /// Removes these owners' marked lines from the section the privileged
    /// helper normally writes, for when the helper is gone but this process
    /// can still write the hosts file itself.
    ///
    /// The section is not privileged at the OS level, only by convention: a
    /// process that can write the file at all can take a line out of it
    /// whether the line is marked for the helper's section or this
    /// manager's own. Only entries owned by one of `ids` are touched; an
    /// unmarked line, or one owned by another configuration, stays.
    pub fn remove_owned_from_helper_section(&self, ids: &[&str]) -> std::io::Result<()> {
        edit_hosts(|document| remove_owned_lines(document, ids)).map_err(std::io::Error::from)
    }

    /// This manager's own section, as it is on disk.
    pub fn direct_section() -> std::io::Result<Vec<SectionEntry>> {
        read_hosts(|document| document.section(KFTRAY_DIRECT_HOSTS_TAG))
            .map_err(std::io::Error::from)
    }

    /// The privileged helper's section, as it is on disk.
    ///
    /// This manager cannot write there, so a caller that needs verified
    /// cleanup has to look at what is actually left.
    pub fn helper_section() -> std::io::Result<Vec<SectionEntry>> {
        read_hosts(|document| document.section(KFTRAY_HOSTS_TAG)).map_err(std::io::Error::from)
    }

    /// Removes both sections whole, unowned lines included.
    ///
    /// Entries written by a version that shared the helper's section belong to
    /// this application too, and a purge that left them would report complete
    /// cleanup with aliases still resolving.
    pub fn remove_all_host_entries(&self) -> std::io::Result<()> {
        debug!("Removing all host entries");

        edit_hosts(|document| {
            document.clear_section(KFTRAY_DIRECT_HOSTS_TAG)?;
            document.clear_section(KFTRAY_HOSTS_TAG)
        })
        .map_err(std::io::Error::from)
    }

    /// Every owned alias in this manager's section.
    pub fn list_host_entries(&self) -> std::io::Result<Vec<(String, HostEntry)>> {
        Ok(
            read_hosts(|document| document.section(KFTRAY_DIRECT_HOSTS_TAG))
                .map_err(std::io::Error::from)?
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
                .collect(),
        )
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
}

/// The mappings whose unmarked copies go with the owned lines of `ids`, less
/// the ones another running configuration still needs.
fn legacy_mappings_to_prune(
    owned: &[SectionEntry], ids: &[&str], protected: &[HostEntry],
) -> Vec<(std::net::IpAddr, String)> {
    owned
        .iter()
        .filter(|entry| {
            entry
                .owner
                .as_deref()
                .is_some_and(|owner| ids.contains(&owner))
        })
        .filter(|entry| {
            !protected
                .iter()
                .any(|kept| kept.ip == entry.ip && kept.hostname == entry.hostname)
        })
        .map(|entry| (entry.ip, entry.hostname.clone()))
        .collect()
}

/// Takes unmarked copies of `mappings` out of the shared section.
///
/// Only lines with no owner are candidates: they were written by a version
/// that shared the helper's section and cannot be attributed any other way. A
/// line the helper marked with another configuration's id is that
/// configuration's, however similar its alias, and stays.
fn prune_legacy(
    document: &mut HostsDocument, mappings: &[(std::net::IpAddr, String)],
) -> kftray_commons::utils::hostsfile::Result<()> {
    if mappings.is_empty() {
        return Ok(());
    }
    document.retain(KFTRAY_HOSTS_TAG, |entry: &SectionEntry| {
        entry.owner.is_some()
            || !mappings
                .iter()
                .any(|(ip, hostname)| entry.ip == *ip && entry.hostname == *hostname)
    })
}

/// Takes these owners' marked lines out of the section the privileged
/// helper normally writes.
///
/// The section is not privileged at the OS level, only by convention: a
/// process that can write the file at all can take a line out of it
/// whether the line is marked for the helper's section or this manager's
/// own. Only entries owned by one of `ids` are touched; an unmarked line,
/// or one owned by another configuration, stays.
fn remove_owned_lines(
    document: &mut HostsDocument, ids: &[&str],
) -> kftray_commons::utils::hostsfile::Result<()> {
    document
        .reconcile_owners(KFTRAY_HOSTS_TAG, ids, &[])
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use kftray_commons::utils::hostsfile::HostsFile;

    use super::*;

    fn addr(last: u8) -> IpAddr {
        IpAddr::from([127, 0, 0, last])
    }

    #[test]
    fn a_mapping_a_running_configuration_still_needs_is_not_pruned_on_anothers_behalf() {
        let owned = vec![
            SectionEntry {
                ip: addr(1),
                hostname: "shared.local".to_owned(),
                owner: Some("7".to_owned()),
            },
            SectionEntry {
                ip: addr(1),
                hostname: "only-mine.local".to_owned(),
                owner: Some("7".to_owned()),
            },
        ];
        let protected = [HostEntry {
            ip: addr(1),
            hostname: "shared.local".to_owned(),
        }];
        assert_eq!(
            legacy_mappings_to_prune(&owned, &["7"], &protected),
            vec![(addr(1), "only-mine.local".to_owned())]
        );
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

        kftray_commons::utils::hostsfile::edit_hosts_at(&temp_path, |document| {
            prune_legacy(document, &[(addr(1), "shared.local".to_owned())])
        })
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

    #[test]
    fn no_helper_fallback_removes_only_the_stranded_ids_own_marked_line() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();

        let mut file = HostsFile::new(KFTRAY_HOSTS_TAG);
        file.add_owned_entry(addr(1), "mine.local", "7")
            .unwrap()
            .add_owned_entry(addr(1), "theirs.local", "9")
            .unwrap()
            .add_entry(addr(1), "unmarked.local");
        file.write_to(&temp_path).unwrap();

        kftray_commons::utils::hostsfile::edit_hosts_at(&temp_path, |document| {
            remove_owned_lines(document, &["7"])
        })
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
                ("theirs.local".to_owned(), Some("9".to_owned())),
                ("unmarked.local".to_owned(), None),
            ],
            "the no-helper fallback removes only the stranded id's own marked line; another \
             configuration's line and an unmarked one stay, so a stop does not fail forever \
             once the helper is gone"
        );
    }
}
