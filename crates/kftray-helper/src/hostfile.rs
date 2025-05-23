use std::collections::HashMap;
use std::sync::{
    Arc,
    Mutex,
    RwLock,
};
use std::thread;
use std::time::Duration;

use hostsfile::HostsBuilder;
use kftray_commons::models::hostfile::HostEntry;
use log::{
    debug,
    error,
    info,
};
use thiserror::Error;

const BATCH_DELAY_MS: u64 = 100;
const KFTRAY_HOSTS_TAG: &str = "kftray-hosts";

#[derive(Error, Debug)]
pub enum HostfileError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Lock error: {0}")]
    Lock(String),
}

type HostEntriesMap = HashMap<String, HostEntry>;

pub struct HostfileManager {
    entries: Arc<RwLock<HostEntriesMap>>,
    needs_update: Arc<Mutex<bool>>,
    writer_running: Arc<Mutex<bool>>,
}

impl HostfileManager {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            needs_update: Arc::new(Mutex::new(false)),
            writer_running: Arc::new(Mutex::new(false)),
        }
    }

    pub fn add_entry(&self, id: String, entry: HostEntry) -> Result<(), HostfileError> {
        debug!("Adding host entry for ID {id}: {entry:?}");

        {
            match self.entries.write() {
                Ok(mut entries) => {
                    entries.insert(id, entry);
                }
                Err(e) => {
                    error!("Failed to acquire host entries write lock: {e}");
                    return Err(HostfileError::Lock(e.to_string()));
                }
            }
        }

        {
            let mut needs_update = self.needs_update.lock().unwrap_or_else(|e| {
                error!("Failed to acquire needs_update lock: {e}");
                e.into_inner()
            });
            *needs_update = true;
        }

        self.ensure_writer_running();

        Ok(())
    }

    pub fn remove_entry(&self, id: &str) -> Result<(), HostfileError> {
        debug!("Removing host entry for ID {id}");

        {
            match self.entries.write() {
                Ok(mut entries) => {
                    entries.remove(id);
                }
                Err(e) => {
                    error!("Failed to acquire host entries write lock: {e}");
                    return Err(HostfileError::Lock(e.to_string()));
                }
            }
        }

        {
            let mut needs_update = self.needs_update.lock().unwrap_or_else(|e| {
                error!("Failed to acquire needs_update lock: {e}");
                e.into_inner()
            });
            *needs_update = true;
        }

        self.ensure_writer_running();

        Ok(())
    }

    pub fn remove_all_entries(&self) -> Result<(), HostfileError> {
        info!("Removing all host entries");

        {
            match self.entries.write() {
                Ok(mut entries) => {
                    entries.clear();
                }
                Err(e) => {
                    error!("Failed to acquire host entries write lock: {e}");
                    return Err(HostfileError::Lock(e.to_string()));
                }
            }
        }

        self.update_hosts_file()
    }

    pub fn list_entries(&self) -> Result<Vec<(String, HostEntry)>, HostfileError> {
        match self.entries.read() {
            Ok(entries) => Ok(entries
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()),
            Err(e) => {
                error!("Failed to acquire host entries read lock: {e}");
                Err(HostfileError::Lock(e.to_string()))
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

            let entries = self.entries.clone();
            let needs_update = self.needs_update.clone();
            let writer_running = self.writer_running.clone();

            thread::spawn(move || {
                Self::batch_writer_loop(entries, needs_update, writer_running);
            });
        }
    }

    fn batch_writer_loop(
        entries: Arc<RwLock<HostEntriesMap>>, needs_update: Arc<Mutex<bool>>,
        writer_running: Arc<Mutex<bool>>,
    ) {
        loop {
            thread::sleep(Duration::from_millis(BATCH_DELAY_MS));

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

            if should_update {
                if let Err(e) = Self::update_hosts_file_static(&entries) {
                    error!("Failed to write hosts file in background writer: {e}");
                }
            } else {
                let pending = {
                    *needs_update.lock().unwrap_or_else(|e| {
                        error!("Failed to check for pending updates: {e}");
                        e.into_inner()
                    })
                };

                if !pending {
                    break;
                }
            }
        }

        let mut writer_running = writer_running.lock().unwrap_or_else(|e| {
            error!("Failed to acquire writer_running lock when exiting: {e}");
            e.into_inner()
        });
        *writer_running = false;
    }

    fn update_hosts_file(&self) -> Result<(), HostfileError> {
        Self::update_hosts_file_static(&self.entries)
    }

    fn update_hosts_file_static(
        entries: &Arc<RwLock<HostEntriesMap>>,
    ) -> Result<(), HostfileError> {
        let entries_snapshot = match entries.read() {
            Ok(entries) => entries.clone(),
            Err(e) => {
                error!("Failed to acquire host entries read lock: {e}");
                return Err(HostfileError::Lock(e.to_string()));
            }
        };

        let mut hosts_builder = HostsBuilder::new(KFTRAY_HOSTS_TAG);

        for (id, entry) in &entries_snapshot {
            debug!("Adding entry for ID {id} to hosts file: {entry:?}");
            hosts_builder.add_hostname(entry.ip, &entry.hostname);
        }

        match hosts_builder.write() {
            Ok(_) => {
                debug!(
                    "Successfully wrote {} entries to hosts file",
                    entries_snapshot.len()
                );
                Ok(())
            }
            Err(e) => {
                error!("Failed to write to hosts file: {e}");
                Err(HostfileError::Io(std::io::Error::other(e)))
            }
        }
    }
}

impl Default for HostfileManager {
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
    fn test_add_and_remove_host_entry() {
        init();
        let manager = HostfileManager::new();
        let _ = manager.remove_all_entries();

        let id = "test-id-1".to_string();
        let entry = get_test_entry();

        let result = manager.add_entry(id.clone(), entry.clone());
        assert!(result.is_ok());

        {
            let mut writer_running = manager.writer_running.lock().unwrap();
            *writer_running = false;
        }

        let entries = manager.list_entries().unwrap();
        assert!(
            entries.iter().any(|(k, v)| k == &id && v == &entry),
            "Entry should be in the list after add_entry"
        );

        let result = manager.remove_entry(&id);
        assert!(result.is_ok());

        {
            let mut writer_running = manager.writer_running.lock().unwrap();
            *writer_running = false;
        }

        let entries = manager.list_entries().unwrap();
        assert!(
            !entries.iter().any(|(k, _)| k == &id),
            "Entry should not be in the list after remove_entry"
        );
    }
}
