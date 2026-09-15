use std::collections::HashSet;
use std::sync::LazyLock;

use kftray_commons::models::hostfile::HostEntry;
use kftray_commons::utils::db_mode::DatabaseMode;
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
    /// Full ids (`{config_id}-https`, `{config_id}-https-local`) actually
    /// written by `add_ssl_host_entry` in this run. Whether a configuration's
    /// HTTPS aliases were written depends on the SSL setting at the time it
    /// started, which the configuration itself does not record, so an
    /// unrelated unmarked line sharing the same alias must not be attributed
    /// to an id that was never written.
    ssl_ids_written: std::sync::Mutex<HashSet<String>>,
}

/// The outcome of a failed SSL host-entry write, once any rollback attempt
/// has had its turn.
struct SslWriteError {
    error: std::io::Error,
    /// Whether a line this call wrote is still on disk despite the
    /// rollback attempt. `false` means the caller may forget any record it
    /// made of these ids before the write; `true` means a leftover line
    /// remains and the record must survive so a later stop can still
    /// attribute and remove it.
    left_on_disk: bool,
}

impl HostfileManager {
    pub fn new() -> Self {
        Self {
            helper_client: HostfileHelperClient::new().ok(),
            direct_manager: DirectHostfileManager::new(),
            handed_to_helper: std::sync::Mutex::new(HashSet::new()),
            ssl_ids_written: std::sync::Mutex::new(HashSet::new()),
        }
    }

    /// A manager that never talks to an installed helper.
    ///
    /// Unit tests share the machine's helper and its hosts file with every
    /// other test, so their outcome must not depend on what happens to be
    /// installed and running there. This is the explicit seam for that,
    /// rather than a runtime `cfg!(test)` check baked into the production
    /// constructor.
    #[cfg(test)]
    fn without_helper() -> Self {
        Self {
            helper_client: None,
            direct_manager: DirectHostfileManager::new(),
            handed_to_helper: std::sync::Mutex::new(HashSet::new()),
            ssl_ids_written: std::sync::Mutex::new(HashSet::new()),
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

    /// Writes the HTTPS aliases for `config_id` as one pair.
    ///
    /// The pair is written together: if the second entry fails, the first is
    /// rolled back rather than left on disk. Whether the caller's persisted
    /// record of these ids may be forgotten depends on how that rollback
    /// went, reported through `SslWriteError::left_on_disk` rather than
    /// decided here, since forgetting them is a durability decision the
    /// caller owns.
    fn add_ssl_host_entry(
        &self, config_id: &str, alias: &str,
    ) -> Result<(String, String), SslWriteError> {
        let loopback: std::net::IpAddr = "127.0.0.1".parse().unwrap();

        let https_id = format!("{config_id}-https");
        let https_local_id = format!("{config_id}-https-local");

        self.add_host_entry(
            https_id.clone(),
            HostEntry {
                ip: loopback,
                hostname: alias.to_string(),
            },
        )
        .map_err(|error| SslWriteError {
            error,
            left_on_disk: false,
        })?;

        if let Err(error) = self.add_host_entry(
            https_local_id.clone(),
            HostEntry {
                ip: loopback,
                hostname: format!("{alias}.local"),
            },
        ) {
            let rollback = self.remove_host_entries(&[https_id.as_str()], &[], &[]);
            let left_on_disk = rollback.is_err();
            if let Err(cleanup_error) = rollback {
                warn!("Failed to roll back {https_id} after SSL write failure: {cleanup_error}");
            }
            return Err(SslWriteError {
                error,
                left_on_disk,
            });
        }

        {
            let mut written = self
                .ssl_ids_written
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            written.insert(https_id.clone());
            written.insert(https_local_id.clone());
        }

        Ok((https_id, https_local_id))
    }

    /// Removes several ids from wherever they were written.
    ///
    /// Success means verified: after both writers have had their turn, the
    /// helper's section is read back and any of the ids still on disk fails the
    /// call, whichever writer reported what. An alias the application cannot
    /// remove itself is reported so the caller keeps the cleanup owed and a
    /// later stop, with the helper back, retries it.
    /// `protected` are the aliases a configuration still forwarding could
    /// have written. They are never pruned, never verified against, and never
    /// handed to the helper for removal, whatever this run's history says.
    pub fn remove_host_entries(
        &self, ids: &[&str], expected: &[(String, HostEntry)], protected: &[HostEntry],
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

        // What the direct section says these ids map to, read before anything
        // removes it: it is the only on-disk record of an alias the row no
        // longer names (edited, then restarted), and the unmarked copy of
        // that alias in the helper's section can only be found through it.
        let mut recorded: Vec<(String, HostEntry)> = DirectHostfileManager::direct_section()?
            .into_iter()
            .filter_map(|entry| {
                let owner = entry.owner?;
                if !ids.contains(&owner.as_str()) {
                    return None;
                }
                Some((
                    owner,
                    HostEntry {
                        ip: entry.ip,
                        hostname: entry.hostname,
                    },
                ))
            })
            .collect();
        recorded.extend(expected.iter().cloned());
        let expected = recorded;

        // The helper only writes its own section, so an alias that fell back
        // to the direct manager earlier is still in the direct one. Removed
        // synchronously with its failure reported: nothing else records it.
        // An application that wrote there and has since lost write access
        // (the helper installed after the fact, permissions tightened) asks
        // the helper to take its owned lines out, and then verifies they are
        // gone, since the helper does not prune legacy copies for it: those
        // are attributed below through the mappings read above.
        match self.direct_manager.remove_host_entries(ids, protected) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                let Some(helper) = self.helper() else {
                    return Err(error);
                };
                // The direct lines and the legacy copies they attribute go in
                // one request: the lines are the only record of those copies,
                // and a failure between two requests would lose it.
                let legacy: Vec<HostEntry> = expected
                    .iter()
                    .filter(|(_, entry)| !protected.contains(entry))
                    .map(|(_, entry)| entry.clone())
                    .collect();
                helper
                    .remove_direct_owned_entries(ids, legacy)
                    .map_err(|helper_error| {
                        std::io::Error::other(format!(
                            "{error}; the helper could not remove them either: {helper_error}"
                        ))
                    })?;
                let section = DirectHostfileManager::direct_section()?;
                let left: Vec<&str> = ids
                    .iter()
                    .copied()
                    .filter(|id| {
                        section
                            .iter()
                            .any(|entry| entry.owner.as_deref() == Some(*id))
                    })
                    .collect();
                if !left.is_empty() {
                    return Err(std::io::Error::other(format!(
                        "Host entries for {} are still on disk after the helper removed them",
                        left.join(", ")
                    )));
                }
            }
            Err(error) => return Err(error),
        }

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
        let handed = attributable_lines(
            &self
                .handed_to_helper
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            &expected,
            protected,
        );
        // A helper from before this change answers before it writes: it queues
        // the removal for a background writer that runs within a fraction of
        // a second. Its lines are given that long to disappear before they are
        // reported, so an installed helper that has not been upgraded yet does
        // not fail every stop. An upgraded helper's write has already landed by
        // the time it replies, so this budget only ever costs latency against
        // one that has not been upgraded.
        const HELPER_SETTLE: std::time::Duration = std::time::Duration::from_millis(100);
        const HELPER_SETTLE_ATTEMPTS: usize = 3;
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
                // Without a helper the same attributed unmarked lines are
                // pruned here, and so are the stranded ids' own marked lines:
                // the helper's section is not privileged at the OS level,
                // only by convention, and a process that can write the hosts
                // file at all can take a line out of it whether the line is
                // marked for the helper or not.
                None => {
                    let mappings: Vec<(std::net::IpAddr, String)> = legacy
                        .into_iter()
                        .map(|entry| (entry.ip, entry.hostname))
                        .collect();
                    self.direct_manager
                        .prune_legacy_entries(&mappings)
                        .and_then(|_| {
                            self.direct_manager
                                .remove_owned_from_helper_section(&stranded)
                        })
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

        self.handed_to_helper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(owner, _)| !ids.contains(&owner.as_str()));
        self.ssl_ids_written
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|owner| !ids.contains(&owner.as_str()));

        Ok(())
    }

    /// Clears every alias this application wrote, through either writer.
    ///
    /// The helper clears only its own section, so the direct one is cleared
    /// here regardless. A failure from either is reported unless what is
    /// actually left on disk says otherwise: the direct clear above already
    /// reaches both sections when this process can write the hosts file
    /// itself, so a helper IPC failure alongside a successful direct clear
    /// must not be reported as an incomplete remove-all.
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
            let direct_section = DirectHostfileManager::direct_section()?;
            let helper_section = DirectHostfileManager::helper_section()?;
            if !remove_all_verified_despite(&errors, &direct_section, &helper_section) {
                // The attribution stays: an unmarked line an older helper left
                // behind can only be tied to its id through what was handed over,
                // and a later per-id removal would otherwise pass verification with
                // the alias still resolving.
                return Err(std::io::Error::other(errors.join("; ")));
            }
        }
        self.handed_to_helper
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.ssl_ids_written
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        Ok(())
    }

    /// This process's SSL ids plus whatever a run before a restart
    /// persisted. `ssl_ids_written` alone starts empty after a restart, and
    /// a configuration's HTTPS aliases are only ever claimed for ids this
    /// set actually names.
    async fn ssl_ids_written_including_persisted(&self, mode: DatabaseMode) -> HashSet<String> {
        let mut ids = self
            .ssl_ids_written
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        ids.extend(persisted_ssl_ids_written(mode).await);
        ids
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

/// Whether errors from `remove_all_host_entries` still count as complete
/// cleanup.
///
/// The direct clear reaches both sections whenever this process can write
/// the hosts file itself, so what is actually left on disk decides,
/// not which writer reported trouble getting there: a helper IPC failure
/// alongside sections that are verified empty is not a leftover alias.
fn remove_all_verified_despite(
    errors: &[String], direct_section: &[kftray_commons::utils::hostsfile::SectionEntry],
    helper_section: &[kftray_commons::utils::hostsfile::SectionEntry],
) -> bool {
    let verified = direct_section.is_empty() && helper_section.is_empty();
    if verified {
        warn!(
            "remove_all_host_entries reported errors but both sections are already empty: {}",
            errors.join("; ")
        );
    }
    verified
}

/// The unmarked lines a removal may attribute to the ids it is removing: what
/// this run handed to the helper plus what the configuration says its aliases
/// are, less anything a configuration still forwarding could have written.
fn attributable_lines(
    handed: &HashSet<(String, HostEntry)>, expected: &[(String, HostEntry)],
    protected: &[HostEntry],
) -> HashSet<(String, HostEntry)> {
    handed
        .iter()
        .chain(expected)
        .filter(|(_, entry)| !protected.contains(entry))
        .cloned()
        .collect()
}

/// Every alias a configuration may have written, keyed by the id each was
/// written under.
///
/// The domain alias is derived from the configuration alone: it is the one
/// description that survives a restart, and a configuration that could not
/// have written it (no domain feature, no service) does not claim it. The
/// HTTPS aliases cannot be derived the same way: whether they were written
/// depends on the SSL setting at the time the forward started, which the
/// configuration itself does not record. They are claimed only for the ids
/// `ssl_ids_written` actually recorded, so an unrelated unmarked line that
/// merely shares an HTTPS alias is never attributed to a configuration that
/// never wrote it.
fn config_host_entries(
    id: i64, config: Option<&kftray_commons::models::config_model::Config>,
    ssl_ids_written: &HashSet<String>,
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
        let https_id = format!("{id}-https");
        if ssl_ids_written.contains(&https_id) {
            entries.push((
                https_id,
                HostEntry {
                    ip: loopback,
                    hostname: alias.to_owned(),
                },
            ));
        }
        let https_local_id = format!("{id}-https-local");
        if ssl_ids_written.contains(&https_local_id) {
            entries.push((
                https_local_id,
                HostEntry {
                    ip: loopback,
                    hostname: format!("{alias}.local"),
                },
            ));
        }
    }
    entries
}

const SSL_HOSTS_WRITTEN_PREFIX: &str = "ssl_hosts_written";

fn ssl_hosts_written_key(full_id: &str, mode: DatabaseMode) -> String {
    format!(
        "{SSL_HOSTS_WRITTEN_PREFIX}:{}:{full_id}",
        kftray_commons::utils::settings::mode_scope(mode)
    )
}

/// Persists that `full_id` (`{config_id}-https` or `{config_id}-https-local`)
/// was written, so a process restart still attributes and protects it.
/// Best-effort: a failure here only costs the durability a restart would
/// otherwise get, not the write itself.
async fn persist_ssl_id_written(full_id: &str, mode: DatabaseMode) {
    let key = ssl_hosts_written_key(full_id, mode);
    if let Err(error) =
        kftray_commons::utils::settings::set_setting_with_mode(&key, "1", mode).await
    {
        warn!("Failed to persist SSL host id {full_id} as written: {error}");
    }
}

/// Forgets a durable record once its removal has been verified gone.
async fn forget_ssl_id_written(full_id: &str, mode: DatabaseMode) {
    let key = ssl_hosts_written_key(full_id, mode);
    if let Err(error) = kftray_commons::utils::settings::delete_setting_with_mode(&key, mode).await
    {
        warn!("Failed to remove persisted SSL host id {full_id}: {error}");
    }
}

/// After a failed SSL write, forgets the record persisted before the write
/// unless something could not be rolled back: a line left on disk must stay
/// attributable so a later stop can still find and remove it.
async fn settle_ssl_write_failure(
    https_id: &str, https_local_id: &str, left_on_disk: bool, mode: DatabaseMode,
) {
    if left_on_disk {
        return;
    }
    forget_ssl_id_written(https_id, mode).await;
    forget_ssl_id_written(https_local_id, mode).await;
}

/// Every full id a run before a restart recorded as written, for this
/// database mode.
async fn persisted_ssl_ids_written(mode: DatabaseMode) -> HashSet<String> {
    let prefix = format!(
        "{SSL_HOSTS_WRITTEN_PREFIX}:{}:",
        kftray_commons::utils::settings::mode_scope(mode)
    );
    match kftray_commons::utils::settings::get_settings_with_prefix_and_mode(&prefix, mode).await {
        Ok(entries) => entries
            .into_iter()
            .filter_map(|(key, _)| key.strip_prefix(&prefix).map(str::to_owned))
            .collect(),
        Err(error) => {
            warn!("Failed to read persisted SSL host ids: {error}");
            HashSet::new()
        }
    }
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
pub async fn remove_config_host_entries(
    id: i64, config: Option<&kftray_commons::models::config_model::Config>,
    in_use: &[kftray_commons::models::config_model::Config], mode: DatabaseMode,
) -> std::io::Result<()> {
    let https_id = format!("{id}-https");
    let https_local_id = format!("{id}-https-local");
    let ids = [id.to_string(), https_id.clone(), https_local_id.clone()];
    // Unioned with what a run before a restart persisted: `ssl_ids_written`
    // alone starts empty after a restart, and without the persisted ids
    // neither this configuration's own HTTPS aliases nor another still-
    // forwarding configuration's would be attributable or protected.
    let ssl_ids_written = HOSTFILE_MANAGER
        .ssl_ids_written_including_persisted(mode)
        .await;
    let protected: Vec<HostEntry> = in_use
        .iter()
        .filter(|other| other.id != Some(id))
        .flat_map(|other| {
            config_host_entries(other.id.unwrap_or_default(), Some(other), &ssl_ids_written)
        })
        .map(|(_, entry)| entry)
        .collect();
    let expected = config_host_entries(id, config, &ssl_ids_written);

    tokio::task::spawn_blocking(move || {
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        HOSTFILE_MANAGER.remove_host_entries(&ids, &expected, &protected)
    })
    .await
    .map_err(|e| {
        std::io::Error::other(format!("remove_config_host_entries task panicked: {e}"))
    })??;

    // Verified gone: forget the durable record so a future restart does not
    // keep attributing lines this configuration no longer writes.
    forget_ssl_id_written(&https_id, mode).await;
    forget_ssl_id_written(&https_local_id, mode).await;

    Ok(())
}

pub fn remove_all_host_entries() -> std::io::Result<()> {
    HOSTFILE_MANAGER.remove_all_host_entries()
}

pub async fn add_ssl_host_entry(
    config_id: &str, alias: &str, _https_port: u16, mode: DatabaseMode,
) -> std::io::Result<()> {
    let https_id = format!("{config_id}-https");
    let https_local_id = format!("{config_id}-https-local");

    // Persisted before the write: a crash between the write landing and
    // this record would otherwise lose attribution of the lines it is
    // about to add.
    persist_ssl_id_written(&https_id, mode).await;
    persist_ssl_id_written(&https_local_id, mode).await;

    let config_id_owned = config_id.to_string();
    let alias_owned = alias.to_string();
    let result = tokio::task::spawn_blocking(move || {
        HOSTFILE_MANAGER.add_ssl_host_entry(&config_id_owned, &alias_owned)
    })
    .await
    .map_err(|e| std::io::Error::other(format!("add_ssl_host_entry task panicked: {e}")))?;

    match result {
        Ok(_) => Ok(()),
        Err(failure) => {
            settle_ssl_write_failure(&https_id, &https_local_id, failure.left_on_disk, mode).await;
            Err(failure.error)
        }
    }
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
    fn history_handed_to_the_helper_never_claims_a_line_a_running_forward_needs() {
        let shared = HostEntry {
            ip: "127.0.0.1".parse().unwrap(),
            hostname: "shared.local".to_owned(),
        };
        // This run handed the alias to the helper for configuration 7, and an
        // older helper left it unmarked. Configuration 9 is still forwarding
        // on the same alias, so it is not stranded on 7's behalf.
        let section = vec![kftray_commons::utils::hostsfile::SectionEntry {
            ip: shared.ip,
            hostname: shared.hostname.clone(),
            owner: None,
        }];
        let mut history: HashSet<(String, HostEntry)> = HashSet::new();
        history.insert(("7".to_owned(), shared.clone()));
        let handed = attributable_lines(&history, &[], &[]);
        assert_eq!(
            DirectHostfileManager::stranded_in_helper_section(&section, &["7"], &handed),
            vec!["7"]
        );
        let handed = attributable_lines(&history, &[], std::slice::from_ref(&shared));
        assert!(
            DirectHostfileManager::stranded_in_helper_section(&section, &["7"], &handed).is_empty()
        );
    }

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
        let ssl_written = HashSet::from(["41-https".to_owned(), "41-https-local".to_owned()]);
        let entries = config_host_entries(41, Some(&config), &ssl_written);
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
        assert!(config_host_entries(41, Some(&plain), &ssl_written).is_empty());
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
    fn https_aliases_are_claimed_only_for_ids_this_run_actually_wrote() {
        use kftray_commons::models::config_model::Config;

        // A TCP configuration whose SSL was never enabled: the helper's
        // `add_ssl_host_entry` was never called for it, so nothing recorded
        // "44-https" as written. An unrelated unmarked line that happens to
        // share the alias must not be attributed to it.
        let config = Config {
            id: Some(44),
            alias: Some("plain.local".to_owned()),
            protocol: "tcp".to_owned(),
            ..Config::default()
        };
        assert!(
            config_host_entries(44, Some(&config), &HashSet::new()).is_empty(),
            "an id whose HTTPS aliases were never written must not claim any lines"
        );

        // Once this run's helper call records the id as written, the same
        // configuration claims exactly those lines.
        let written = HashSet::from(["44-https".to_owned()]);
        let entries = config_host_entries(44, Some(&config), &written);
        let ids: Vec<&str> = entries.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["44-https"]);
    }

    #[test]
    fn a_manager_without_a_helper_never_reaches_one() {
        init();
        let manager = HostfileManager::without_helper();

        assert!(
            manager.helper().is_none(),
            "the direct-fallback seam must not resolve a helper, whatever is installed on the \
             machine running the test"
        );
    }

    #[tokio::test]
    async fn a_restart_still_attributes_persisted_https_ids() {
        let _lock = kftray_commons::test_utils::MEMORY_MODE_TEST_MUTEX
            .lock()
            .await;
        let mode = DatabaseMode::Memory;

        // A prior run's `add_ssl_host_entry` persisted this id before the
        // process exited; nothing about this test's in-memory state knows
        // it.
        persist_ssl_id_written("77-https", mode).await;

        // A brand new manager, exactly what a restart leaves behind: its
        // `ssl_ids_written` is empty.
        let manager = HostfileManager::without_helper();
        let ids = manager.ssl_ids_written_including_persisted(mode).await;

        assert!(
            ids.contains("77-https"),
            "a fresh manager instance must still attribute an HTTPS id a run before a restart \
             persisted"
        );

        forget_ssl_id_written("77-https", mode).await;
    }

    #[tokio::test]
    async fn persisted_ssl_ids_survive_a_failed_second_write_whose_rollback_also_fails() {
        let _lock = kftray_commons::test_utils::MEMORY_MODE_TEST_MUTEX
            .lock()
            .await;
        let mode = DatabaseMode::Memory;

        // What `add_ssl_host_entry` does before attempting the write: both
        // ids are persisted up front, so a crash between the write and the
        // record cannot lose attribution.
        persist_ssl_id_written("81-https", mode).await;
        persist_ssl_id_written("81-https-local", mode).await;

        // The second write failed and the rollback of the first could not
        // verify the line gone: `left_on_disk` is true, so the persisted
        // record must survive for a later stop to attribute and remove the
        // leftover.
        settle_ssl_write_failure("81-https", "81-https-local", true, mode).await;

        let manager = HostfileManager::without_helper();
        let ids = manager.ssl_ids_written_including_persisted(mode).await;
        assert!(
            ids.contains("81-https") && ids.contains("81-https-local"),
            "a leftover that could not be rolled back must stay attributable so a later stop \
             can remove it"
        );

        forget_ssl_id_written("81-https", mode).await;
        forget_ssl_id_written("81-https-local", mode).await;
    }

    #[tokio::test]
    async fn persisted_ssl_ids_are_forgotten_once_a_failed_writes_rollback_is_verified() {
        let _lock = kftray_commons::test_utils::MEMORY_MODE_TEST_MUTEX
            .lock()
            .await;
        let mode = DatabaseMode::Memory;

        persist_ssl_id_written("82-https", mode).await;
        persist_ssl_id_written("82-https-local", mode).await;

        // The write failed but rollback verified nothing is left: the
        // persisted record is no longer needed.
        settle_ssl_write_failure("82-https", "82-https-local", false, mode).await;

        let manager = HostfileManager::without_helper();
        let ids = manager.ssl_ids_written_including_persisted(mode).await;
        assert!(
            !ids.contains("82-https") && !ids.contains("82-https-local"),
            "a verified rollback must not leave a durable record behind"
        );
    }

    #[test]
    fn remove_all_is_ok_when_both_sections_are_already_gone_despite_a_helper_error() {
        let errors = vec!["helper: not available".to_owned()];
        assert!(
            remove_all_verified_despite(&errors, &[], &[]),
            "the direct clear already reached both sections; a helper IPC failure alone must \
             not fail the call"
        );
    }

    #[test]
    fn remove_all_still_fails_when_a_line_actually_survives() {
        let errors = vec!["helper: not available".to_owned()];
        let leftover = vec![kftray_commons::utils::hostsfile::SectionEntry {
            ip: "127.0.0.1".parse().unwrap(),
            hostname: "still-here.local".to_owned(),
            owner: Some("7".to_owned()),
        }];
        assert!(
            !remove_all_verified_despite(&errors, &leftover, &[]),
            "a line still on disk must fail the call, not just an error being reported"
        );
        assert!(!remove_all_verified_despite(&errors, &[], &leftover));
    }
}
