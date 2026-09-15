use kftray_commons::models::hostfile::HostEntry;
use kftray_commons::utils::hostsfile::{
    SectionEntry,
    edit_hosts,
    read_hosts,
    validate_hostname,
    validate_owner,
};
use log::debug;
use thiserror::Error;

const KFTRAY_HOSTS_TAG: &str = "kftray-hosts";
/// Section the application writes when it does not go through this helper.
const KFTRAY_DIRECT_HOSTS_TAG: &str = "kftray-hosts-direct";
/// A well-behaved application never asks to remove zero entries, or more
/// than a handful at once; a payload outside that range is a mistake or an
/// injection, never a real request.
const MAX_REMOVAL_ENTRIES: usize = 256;
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

fn survives_legacy_removal(line: &SectionEntry, entries: &[HostEntry]) -> bool {
    match &line.owner {
        Some(_) => true,
        None => !entries
            .iter()
            .any(|entry| entry.ip == line.ip && entry.hostname == line.hostname),
    }
}

/// Validates a list of hosts entries an unauthenticated local client asked
/// this privileged helper to remove.
///
/// `allow_empty` distinguishes `RemoveUnowned`, which always names at least
/// one alias, from `RemoveDirectOwned`'s `legacy` field, which legitimately
/// carries nothing to prune.
fn validate_removal_entries(entries: &[HostEntry], allow_empty: bool) -> Result<(), HostfileError> {
    if (!allow_empty && entries.is_empty()) || entries.len() > MAX_REMOVAL_ENTRIES {
        return Err(HostfileError::HostsFile(format!(
            "expected {}-{MAX_REMOVAL_ENTRIES} host entries, got {}",
            usize::from(!allow_empty),
            entries.len()
        )));
    }
    for entry in entries {
        validate_hostname(&entry.hostname)?;
    }
    Ok(())
}

/// Whether `id` looks like a configuration id (`42`, `42-https`,
/// `42-https-local`): the only ids a well-behaved application ever asks the
/// helper to remove.
fn looks_like_config_id(id: &str) -> bool {
    let digits = id.split('-').next().unwrap_or_default();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    matches!(&id[digits.len()..], "" | "-https" | "-https-local")
}

/// Validates one id the way `validate_config_ids` validates a list: the
/// plain-token owner alphabet plus the configuration-id shape.
fn validate_config_id(id: &str) -> Result<(), HostfileError> {
    validate_owner(id)?;
    if !looks_like_config_id(id) {
        return Err(HostfileError::HostsFile(format!(
            "id {id:?} does not look like a configuration id"
        )));
    }
    Ok(())
}

/// Validates the ids a `RemoveDirectOwned` request names.
fn validate_config_ids(ids: &[String]) -> Result<(), HostfileError> {
    if ids.is_empty() || ids.len() > MAX_REMOVAL_ENTRIES {
        return Err(HostfileError::HostsFile(format!(
            "expected 1-{MAX_REMOVAL_ENTRIES} ids, got {}",
            ids.len()
        )));
    }
    ids.iter().try_for_each(|id| validate_config_id(id))
}

impl HostfileManager {
    pub fn new() -> Self {
        Self
    }

    pub fn add_entry(&self, id: String, entry: HostEntry) -> Result<(), HostfileError> {
        validate_config_id(&id)?;
        validate_hostname(&entry.hostname)?;
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
        validate_config_id(id)?;
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
        validate_removal_entries(entries, false)?;
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
    pub fn remove_direct_owned(
        &self, ids: &[String], legacy: &[HostEntry],
    ) -> Result<(), HostfileError> {
        validate_config_ids(ids)?;
        validate_removal_entries(legacy, true)?;
        debug!("Removing direct host entries for IDs {ids:?}");

        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        edit_hosts(|document| {
            document.reconcile_owners(KFTRAY_DIRECT_HOSTS_TAG, &ids, &[])?;
            document.retain(KFTRAY_HOSTS_TAG, |line| {
                survives_legacy_removal(line, legacy)
            })
        })?;
        Ok(())
    }

    /// Clears both sections this helper writes, whoever wrote each line.
    ///
    /// Kept for a client of another version still sending `RemoveAll`.
    pub fn remove_all_entries(&self) -> Result<(), HostfileError> {
        debug!("Removing all host entries from both kftray sections");
        edit_hosts(|document| {
            document.clear_section(KFTRAY_HOSTS_TAG)?;
            document.clear_section(KFTRAY_DIRECT_HOSTS_TAG)?;
            Ok(())
        })?;
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

    #[test]
    fn removal_entries_reject_an_empty_or_oversized_list() {
        assert!(
            validate_removal_entries(&[], false).is_err(),
            "RemoveUnowned always names at least one alias"
        );
        assert!(
            validate_removal_entries(&[], true).is_ok(),
            "RemoveDirectOwned's legacy field legitimately carries nothing to prune"
        );
        let oversized: Vec<HostEntry> = (0..=MAX_REMOVAL_ENTRIES)
            .map(|i| entry(&format!("svc-{i}.local")))
            .collect();
        assert!(validate_removal_entries(&oversized, false).is_err());
        assert!(validate_removal_entries(&[entry("svc.local")], false).is_ok());
    }

    #[test]
    fn remove_unowned_matching_rejects_a_hostname_that_would_not_stay_on_its_own_line() {
        let bad = HostEntry {
            ip: [127, 0, 0, 1].into(),
            hostname: "svc.local\n# injected".to_owned(),
        };
        assert!(
            HostfileManager::new()
                .remove_unowned_matching(&[bad])
                .is_err(),
            "a hostname carrying a newline or comment marker must never reach the write"
        );
    }

    #[test]
    fn config_ids_reject_a_bad_owner_or_a_shape_that_is_not_a_configuration_id() {
        assert!(
            validate_config_ids(&[]).is_err(),
            "an empty id list is never legitimate"
        );
        assert!(
            validate_config_ids(&["../etc/passwd".to_owned()]).is_err(),
            "an id outside the plain-token owner alphabet must be rejected"
        );
        assert!(
            validate_config_ids(&["7-bogus".to_owned()]).is_err(),
            "a valid owner token that is not a configuration id shape must still be rejected"
        );
        assert!(validate_config_ids(&["7".to_owned()]).is_ok());
        assert!(validate_config_ids(&["7-https".to_owned()]).is_ok());
        assert!(validate_config_ids(&["7-https-local".to_owned()]).is_ok());
        let oversized: Vec<String> = (0..=MAX_REMOVAL_ENTRIES).map(|i| i.to_string()).collect();
        assert!(validate_config_ids(&oversized).is_err());
    }

    #[test]
    fn remove_direct_owned_rejects_a_bad_id_before_touching_disk() {
        let err = HostfileManager::new()
            .remove_direct_owned(&["not-a-config-id!".to_owned()], &[])
            .unwrap_err();
        assert!(matches!(err, HostfileError::HostsFile(_)));
    }

    #[test]
    fn add_entry_rejects_an_owner_that_does_not_look_like_a_config_id() {
        let err = HostfileManager::new()
            .add_entry("not-a-config-id!".to_owned(), entry("app.local"))
            .unwrap_err();
        assert!(matches!(err, HostfileError::HostsFile(_)));
    }

    #[test]
    fn remove_entry_rejects_an_id_that_does_not_look_like_a_config_id() {
        let err = HostfileManager::new()
            .remove_entry("not-a-config-id!")
            .unwrap_err();
        assert!(matches!(err, HostfileError::HostsFile(_)));
    }
}
