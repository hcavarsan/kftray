use std::collections::HashMap;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};
use std::sync::{
    Arc,
    Mutex,
    RwLock,
};
use std::thread;
use std::time::Duration;

use kftray_commons::models::hostfile::HostEntry;
use kftray_commons::utils::hostsfile::HostsFile;
use log::{
    debug,
    error,
    info,
};

const BATCH_DELAY_MS: u64 = 100;
const MAX_WRITE_BACKOFF: Duration = Duration::from_secs(30);
const MAX_WRITE_ATTEMPTS: u32 = 8;
const KFTRAY_HOSTS_TAG: &str = "kftray-hosts";

type HostEntriesMap = HashMap<String, HostEntry>;

pub struct DirectHostfileManager {
    entries: Arc<RwLock<HostEntriesMap>>,
    needs_update: Arc<Mutex<bool>>,
    writer_running: Arc<Mutex<bool>>,
    /// Generation of the last mutation a successful write covered. The file
    /// matches memory only while it equals `generation`; comparing the two
    /// leaves no window where a mutation can be lost between a check and a
    /// store, which a separate boolean had.
    reconciled_generation: Arc<AtomicU64>,
    /// Bumped by every mutation.
    generation: Arc<AtomicU64>,
    /// Serializes the synchronous and background writes. Without it, a
    /// background write could snapshot older entries and finish after a
    /// synchronous one, recording a generation its content does not match.
    write_lock: Arc<Mutex<()>>,
    /// Aliases removed here but not yet written out.
    ///
    /// The managed section is shared with the privileged helper and carries no
    /// per-owner marker, so a write rebuilds it from what is on disk. Without
    /// this set a removal would be undone by the very write meant to apply it.
    retired: Arc<Mutex<Vec<(std::net::IpAddr, String)>>>,
}

impl DirectHostfileManager {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            needs_update: Arc::new(Mutex::new(false)),
            writer_running: Arc::new(Mutex::new(false)),
            reconciled_generation: Arc::new(AtomicU64::new(u64::MAX)),
            generation: Arc::new(AtomicU64::new(0)),
            write_lock: Arc::new(Mutex::new(())),
            retired: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_host_entry(&self, id: String, entry: HostEntry) -> std::io::Result<()> {
        debug!("Adding host entry for ID {id}: {entry:?}");

        {
            match self.entries.write() {
                Ok(mut entries) => {
                    entries.insert(id, entry);
                }
                Err(e) => {
                    error!("Failed to acquire host entries write lock: {e}");
                    return Err(std::io::Error::other(e.to_string()));
                }
            }
        }

        self.mark_dirty();
        self.ensure_writer_running();

        Ok(())
    }

    pub fn remove_host_entry(&self, id: &str) -> std::io::Result<()> {
        self.remove_host_entries(std::slice::from_ref(&id))
            .map(drop)
    }

    /// Marks the file as possibly out of step with this manager's map.
    ///
    /// The helper writes the same tagged section, so an entry it owns is
    /// invisible here. Without this, a reconciled map would report a removal as
    /// complete without even inspecting the file.
    /// Forgets ids the privileged helper already removed from the file.
    ///
    /// Their entries stay in this map otherwise, and a later retry of a write
    /// that failed earlier would put the stopped alias back on disk.
    pub fn forget_entries(&self, ids: &[&str]) {
        let Ok(mut entries) = self.entries.write() else {
            return;
        };
        for id in ids {
            entries.remove(*id);
        }
    }

    pub fn invalidate_reconciliation(&self) {
        self.reconciled_generation
            .store(u64::MAX, Ordering::Relaxed);
    }

    /// Removes several ids and reconciles them with one write.
    ///
    /// The file is rewritten from the whole remaining map, so a single
    /// successful write covers every id in the batch.
    ///
    /// Returns whether any of the ids was known here: a caller that reached
    /// this through a failing helper needs to tell "already gone" apart from
    /// "this manager never owned it".
    pub fn remove_host_entries(&self, ids: &[&str]) -> std::io::Result<bool> {
        debug!("Removing host entries for IDs {ids:?}");

        let existed = match self.entries.write() {
            Ok(mut entries) => {
                // Collected first so every id is removed, not just the ones
                // before the first hit.
                let removed: Vec<HostEntry> =
                    ids.iter().filter_map(|id| entries.remove(*id)).collect();
                let existed = !removed.is_empty();
                // Remembered until a write lands: the section is rebuilt from
                // what is on disk, so without this the alias just removed would
                // be written straight back.
                self.retired
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .extend(removed.into_iter().map(|entry| (entry.ip, entry.hostname)));
                // Counted while the entries lock is held: a concurrent removal
                // of the same id would otherwise see the entry already gone and
                // the generation still reconciled, and report success without
                // writing anything.
                if existed {
                    self.generation.fetch_add(1, Ordering::Relaxed);
                }
                existed
            }
            Err(e) => {
                error!("Failed to acquire host entries write lock: {e}");
                return Err(std::io::Error::other(e.to_string()));
            }
        };

        if !self.removal_needs_write(existed) {
            return Ok(existed);
        }

        self.mark_dirty();
        // Written here rather than left to the background writer: a removal is
        // part of stopping a forward, and the caller can only keep it tracked
        // for retry if it learns the alias is still on disk.
        match Self::write_snapshot(
            &self.entries,
            &self.generation,
            &self.reconciled_generation,
            &self.write_lock,
            &self.retired,
        ) {
            // A successful write rewrote the whole managed section from this
            // map, so every id in the batch is off disk whether or not this
            // manager still had it: an earlier failed write can have dropped it
            // from memory already.
            Ok(_) => Ok(true),
            Err((_, error)) => {
                self.mark_dirty();
                self.ensure_writer_running();
                Err(error)
            }
        }
    }

    pub fn remove_all_host_entries(&self) -> std::io::Result<()> {
        info!("Removing all host entries");

        {
            match self.entries.write() {
                Ok(mut entries) => {
                    let had_entries = !entries.is_empty();
                    entries.clear();
                    // Counted here so a concurrent removal cannot observe the
                    // cleared map with a still-reconciled generation and report
                    // success without writing.
                    if had_entries {
                        self.generation.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(e) => {
                    error!("Failed to acquire host entries write lock: {e}");
                    return Err(std::io::Error::other(e.to_string()));
                }
            }
        }

        // Counted as a mutation like any other, so a background write that
        // snapshotted the old entries cannot claim to have reconciled this one.
        self.mark_dirty();
        match Self::write_snapshot(
            &self.entries,
            &self.generation,
            &self.reconciled_generation,
            &self.write_lock,
            &self.retired,
        ) {
            Ok(_) => Ok(()),
            Err((_, error)) => {
                self.mark_dirty();
                self.ensure_writer_running();
                Err(error)
            }
        }
    }

    /// Marks the hosts file as no longer matching the in-memory entries. Until
    /// a write succeeds, a removal cannot be short-circuited: the file may
    /// still carry an entry this process already forgot.
    /// Whether removing an entry still requires touching the file.
    ///
    /// An id this process never had can still be on disk when an earlier write
    /// never landed, so a removal is only a no-op while memory and file agree.
    /// A file with no managed section has nothing to remove, which matters
    /// because most stops involve no alias at all and the file usually needs
    /// elevated privileges to write.
    fn removal_needs_write(&self, existed: bool) -> bool {
        // Taken under the write lock so the answer cannot be overtaken by a
        // snapshot write that is still landing, and an unreadable file counts
        // as "might still hold entries" rather than as nothing to do.
        let _writing = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());

        removal_needs_write(existed, self.is_reconciled(), || {
            HostsFile::new(KFTRAY_HOSTS_TAG)
                .section_exists()
                .unwrap_or(true)
        })
    }

    fn is_reconciled(&self) -> bool {
        self.reconciled_generation.load(Ordering::Relaxed)
            == self.generation.load(Ordering::Relaxed)
    }

    fn mark_dirty(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        let mut needs_update = self.needs_update.lock().unwrap_or_else(|e| {
            error!("Failed to acquire needs_update lock: {e}");
            e.into_inner()
        });
        *needs_update = true;
    }

    pub fn list_host_entries(&self) -> std::io::Result<Vec<(String, HostEntry)>> {
        match self.entries.read() {
            Ok(entries) => Ok(entries
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()),
            Err(e) => {
                error!("Failed to acquire host entries read lock: {e}");
                Err(std::io::Error::other(e.to_string()))
            }
        }
    }

    fn ensure_writer_running(&self) {
        let mut writer_running = self.writer_running.lock().unwrap_or_else(|e| {
            error!("Failed to acquire writer_running lock: {e}");
            e.into_inner()
        });

        if !*writer_running {
            *writer_running = true;
            Self::spawn_writer(
                self.entries.clone(),
                self.needs_update.clone(),
                self.writer_running.clone(),
                self.reconciled_generation.clone(),
                self.generation.clone(),
                self.write_lock.clone(),
                self.retired.clone(),
            );
        }
    }

    /// Starts a writer thread. The caller owns setting `writer_running`.
    fn spawn_writer(
        entries: Arc<RwLock<HostEntriesMap>>, needs_update: Arc<Mutex<bool>>,
        writer_running: Arc<Mutex<bool>>, reconciled_generation: Arc<AtomicU64>,
        generation: Arc<AtomicU64>, write_lock: Arc<Mutex<()>>,
        retired: Arc<Mutex<Vec<(std::net::IpAddr, String)>>>,
    ) {
        thread::spawn(move || {
            Self::batch_writer_loop(
                entries,
                needs_update,
                writer_running,
                reconciled_generation,
                generation,
                write_lock,
                retired,
            );
        });
    }

    fn batch_writer_loop(
        entries: Arc<RwLock<HostEntriesMap>>, needs_update: Arc<Mutex<bool>>,
        writer_running: Arc<Mutex<bool>>, reconciled_generation: Arc<AtomicU64>,
        generation: Arc<AtomicU64>, write_lock: Arc<Mutex<()>>,
        retired: Arc<Mutex<Vec<(std::net::IpAddr, String)>>>,
    ) {
        let mut backoff = Duration::from_millis(BATCH_DELAY_MS);
        let mut failures = 0u32;
        let mut exhausted_at = None;

        loop {
            thread::sleep(backoff);

            let should_update = {
                let mut update_flag = needs_update.lock().unwrap_or_else(|e| {
                    error!("Failed to acquire needs_update lock in writer loop: {e}");
                    e.into_inner()
                });

                if *update_flag {
                    *update_flag = false;
                    true
                } else {
                    false
                }
            };

            if !should_update {
                let pending = {
                    *needs_update.lock().unwrap_or_else(|e| {
                        error!("Failed to check for pending updates: {e}");
                        e.into_inner()
                    })
                };

                if !pending {
                    break;
                }
                continue;
            }

            match Self::write_snapshot(
                &entries,
                &generation,
                &reconciled_generation,
                &write_lock,
                &retired,
            ) {
                Ok(_) => {
                    backoff = Duration::from_millis(BATCH_DELAY_MS);
                    failures = 0;
                }
                Err((attempted, e)) => {
                    failures += 1;
                    // The change is still pending either way, so a later
                    // add or removal can start a fresh writer once whatever
                    // blocked the write clears.
                    let mut update_flag = needs_update.lock().unwrap_or_else(|e| {
                        error!("Failed to re-arm the pending hosts write: {e}");
                        e.into_inner()
                    });
                    *update_flag = true;
                    drop(update_flag);
                    // A persistent error is usually missing permissions on the
                    // hosts file. Retry with backoff, then stop rather than
                    // spin a thread logging forever.
                    if failures >= MAX_WRITE_ATTEMPTS {
                        error!(
                            "Giving up on the hosts file after {failures} attempts, last error: {e}"
                        );
                        // The generation this write covered, not whatever
                        // landed since: a mutation that arrived during the
                        // final attempt must still earn a replacement writer.
                        exhausted_at = Some(attempted);
                        break;
                    }
                    error!("Failed to write hosts file in background writer: {e}");
                    backoff = (backoff * 2).min(MAX_WRITE_BACKOFF);
                }
            }
        }

        // The flag is only cleared when no replacement is started, so exactly
        // one writer exists at a time. Re-checking for work under the lock also
        // closes the window where a mutation would see the flag still set,
        // start nothing, and stay pending forever. After giving up, only a
        // mutation newer than the one that failed earns a replacement, so a
        // persistent permission error cannot spin.
        let mut running = writer_running.lock().unwrap_or_else(|e| {
            error!("Failed to acquire writer_running lock when exiting: {e}");
            e.into_inner()
        });
        let pending = *needs_update.lock().unwrap_or_else(|e| e.into_inner());
        let superseded =
            exhausted_at.is_none_or(|failed| generation.load(Ordering::Relaxed) != failed);
        if pending && superseded {
            drop(running);
            Self::spawn_writer(
                entries,
                needs_update,
                Arc::clone(&writer_running),
                reconciled_generation,
                generation,
                write_lock,
                retired,
            );
        } else {
            *running = false;
        }
    }

    /// Writes the current entries and records which mutation the write covered.
    /// The generation is read inside the write lock so the record always
    /// matches the content that reached disk.
    fn write_snapshot(
        entries: &Arc<RwLock<HostEntriesMap>>, generation: &Arc<AtomicU64>,
        reconciled_generation: &Arc<AtomicU64>, write_lock: &Arc<Mutex<()>>,
        retired: &Arc<Mutex<Vec<(std::net::IpAddr, String)>>>,
    ) -> Result<u64, (u64, std::io::Error)> {
        let _writing = write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let started = generation.load(Ordering::Relaxed);
        let dropping = retired.lock().unwrap_or_else(|e| e.into_inner()).clone();
        match Self::update_hosts_file_static(entries, &dropping) {
            Ok(()) => {
                // Retained until the write lands: an alias removed here is only
                // off disk once the section has been rewritten without it.
                let mut retired = retired.lock().unwrap_or_else(|e| e.into_inner());
                retired.retain(|entry| !dropping.contains(entry));
                // Stored under the write lock, and compared against the
                // generation read under it too, so an invalidation that lands
                // while this write is in flight is never marked reconciled.
                reconciled_generation.store(started, Ordering::Relaxed);
                Ok(started)
            }
            Err(error) => Err((started, error)),
        }
    }

    fn update_hosts_file_static(
        entries: &Arc<RwLock<HostEntriesMap>>, dropping: &[(std::net::IpAddr, String)],
    ) -> std::io::Result<()> {
        let entries_snapshot = match entries.read() {
            Ok(entries) => entries.clone(),
            Err(e) => {
                error!("Failed to acquire host entries read lock: {e}");
                return Err(std::io::Error::other(e.to_string()));
            }
        };

        let mut hosts_file = HostsFile::new(KFTRAY_HOSTS_TAG);

        // Rebuilt from what is on disk: the privileged helper writes into the
        // same section, and its aliases carry no marker, so writing only this
        // manager's map would delete them.
        let owned: std::collections::HashSet<(std::net::IpAddr, &str)> = entries_snapshot
            .values()
            .map(|entry| (entry.ip, entry.hostname.as_str()))
            .collect();
        match hosts_file.read_section() {
            Ok(existing) => {
                for (ip, hostnames) in existing {
                    for hostname in hostnames {
                        if owned.contains(&(ip, hostname.as_str()))
                            || dropping.iter().any(|(retired_ip, retired_hostname)| {
                                *retired_ip == ip && *retired_hostname == hostname
                            })
                        {
                            continue;
                        }
                        hosts_file.add_entry(ip, hostname);
                    }
                }
            }
            // Treated as an empty section: the write below still has to happen,
            // and a section it could not read is one it cannot preserve.
            Err(error) => log::warn!("Could not read the existing hosts section: {error}"),
        }

        for (id, entry) in &entries_snapshot {
            debug!("Adding entry for ID {id} to hosts file: {entry:?}");
            hosts_file.add_entry(entry.ip, &entry.hostname);
        }

        match hosts_file.write() {
            Ok(_) => {
                debug!(
                    "Successfully wrote {} entries to hosts file",
                    entries_snapshot.len()
                );
                Ok(())
            }
            Err(e) => {
                error!("Failed to write to hosts file: {e}");
                Err(std::io::Error::other(e))
            }
        }
    }
}

/// Whether removing an entry still requires touching the file.
///
/// An id this process never had can still be on disk when an earlier write
/// never landed, so a removal is only a no-op while memory and file agree, or
/// while the file carries no managed section at all. That last case matters
/// because most stops involve no alias and the file usually needs elevated
/// privileges to write.
fn removal_needs_write(
    existed: bool, reconciled: bool, section_on_disk: impl FnOnce() -> bool,
) -> bool {
    existed || (!reconciled && section_on_disk())
}

impl Default for DirectHostfileManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::net::{
        IpAddr,
        Ipv4Addr,
    };
    use std::sync::Once;

    use super::*;

    static INIT: Once = Once::new();

    fn init() {
        INIT.call_once(|| {
            let _ = env_logger::builder().is_test(true).try_init();
        });
    }

    fn get_test_entry() -> HostEntry {
        HostEntry {
            ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)),
            hostname: "test.local".to_string(),
        }
    }

    #[test]
    fn a_helper_owned_entry_is_reported_as_unknown_here() {
        init();
        let manager = DirectHostfileManager::new();
        // The helper writes the same tagged section, so an entry it owns is
        // invisible here. Reporting a removal as done would let the caller
        // treat an alias still on disk as gone.
        manager.reconciled_generation.store(
            manager.generation.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );

        assert!(
            !manager.remove_host_entries(&["not-mine"]).unwrap(),
            "an id this manager never owned cannot be reported as removed"
        );

        manager
            .entries
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert("mine".to_owned(), get_test_entry());
        assert!(
            manager
                .entries
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key("mine")
        );

        manager.invalidate_reconciliation();
        assert!(
            !manager.is_reconciled(),
            "a helper write means this manager's map no longer describes the file"
        );
    }

    #[test]
    fn an_unwritten_change_keeps_later_removals_retrying() {
        init();
        let manager = DirectHostfileManager::new();
        manager.reconciled_generation.store(
            manager.generation.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        assert!(manager.is_reconciled());

        manager
            .entries
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert("41007".to_owned(), get_test_entry());
        manager.mark_dirty();
        assert!(
            !manager.is_reconciled(),
            "a pending change means the file no longer matches memory"
        );

        // The entry is gone from memory, but its removal was never written, so
        // the alias can still be on disk and a later removal must not
        // short-circuit. The decision is asserted directly: performing the
        // write would rewrite the system hosts file.
        assert!(
            removal_needs_write(false, false, || true),
            "an unreconciled file with a managed section must still be rewritten"
        );
        assert!(
            !removal_needs_write(false, false, || false),
            "a file with no managed section has nothing to remove"
        );
        assert!(
            !removal_needs_write(false, true, || true),
            "once the file matches memory, removing an absent id is a no-op"
        );
        assert!(
            removal_needs_write(true, true, || false),
            "removing an entry this process holds always writes"
        );
    }

    #[test]
    fn test_add_and_remove_host_entry() {
        init();
        let manager = DirectHostfileManager::new();

        let id = "test-id-1".to_string();
        let entry = get_test_entry();

        {
            let mut entries = manager.entries.write().unwrap();
            entries.clear();
            entries.insert(id.clone(), entry.clone());
        }

        let entries = manager.list_host_entries().unwrap();
        assert!(
            entries.iter().any(|(k, v)| k == &id && v == &entry),
            "Entry should be in the list after add_host_entry"
        );

        {
            let mut entries = manager.entries.write().unwrap();
            entries.remove(&id);
        }

        let entries = manager.list_host_entries().unwrap();
        assert!(
            !entries.iter().any(|(k, _)| k == &id),
            "Entry should not be in the list after remove_host_entry"
        );

        if can_write_hosts_file() {
            let manager = DirectHostfileManager::new();
            let _ = manager.remove_all_host_entries();

            let result = manager.add_host_entry(id.clone(), entry.clone());
            assert!(result.is_ok());

            {
                let mut writer_running = manager.writer_running.lock().unwrap();
                *writer_running = false;
            }

            let entries = manager.list_host_entries().unwrap();
            assert!(
                entries.iter().any(|(k, v)| k == &id && v == &entry),
                "Entry should be in the list after add_host_entry"
            );

            let result = manager.remove_host_entry(&id);
            assert!(result.is_ok());

            {
                let mut writer_running = manager.writer_running.lock().unwrap();
                *writer_running = false;
            }

            let entries = manager.list_host_entries().unwrap();
            assert!(
                !entries.iter().any(|(k, _)| k == &id),
                "Entry should not be in the list after remove_host_entry"
            );
        } else {
            println!("Skipping hosts file write test - insufficient permissions");
        }
    }

    #[test]
    fn test_remove_all_host_entries() {
        init();
        let manager = DirectHostfileManager::new();

        let id1 = "test-id-1".to_string();
        let id2 = "test-id-2".to_string();
        let entry = get_test_entry();

        {
            let mut entries = manager.entries.write().unwrap();
            entries.insert(id1.clone(), entry.clone());
            entries.insert(id2.clone(), entry.clone());
        }

        let entries = manager.list_host_entries().unwrap();
        assert!(!entries.is_empty());
        assert_eq!(entries.len(), 2);

        {
            let mut entries = manager.entries.write().unwrap();
            entries.clear();
        }

        let entries = manager.list_host_entries().unwrap();
        assert!(entries.is_empty());

        if can_write_hosts_file() {
            let manager = DirectHostfileManager::new();
            let _ = manager.add_host_entry(id1, entry.clone());
            let _ = manager.add_host_entry(id2, entry.clone());

            let result = manager.remove_all_host_entries();
            assert!(result.is_ok());

            let entries = manager.list_host_entries().unwrap();
            assert!(entries.is_empty());
        } else {
            println!("Skipping hosts file write test - insufficient permissions");
        }
    }

    fn can_write_hosts_file() -> bool {
        let test_hosts_file = HostsFile::new("test-permission-check");
        match test_hosts_file.write() {
            Ok(_) => {
                let cleanup_hosts_file = HostsFile::new("test-permission-check");
                let _ = cleanup_hosts_file.write();
                true
            }
            Err(_) => false,
        }
    }

    #[test]
    fn test_writer_flags() {
        init();
        let manager = DirectHostfileManager::new();

        {
            let mut writer_running = manager.writer_running.lock().unwrap();
            *writer_running = true;
            assert!(*writer_running);
            *writer_running = false;
            assert!(!*writer_running);
        }

        {
            let mut needs_update = manager.needs_update.lock().unwrap();
            *needs_update = true;
            assert!(*needs_update);
            *needs_update = false;
            assert!(!*needs_update);
        }
    }
}
