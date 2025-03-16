use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::LazyLock;
use std::sync::Mutex;

use ::hostsfile::HostsBuilder;
use log::{
    debug,
    error,
    info,
};

static HOSTS_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

const KFTRAY_HOSTS_TAG: &str = "kftray-hosts";

#[derive(Debug, Clone)]
pub struct HostEntry {
    pub ip: IpAddr,
    pub hostname: String,
}

type HostEntriesMap = HashMap<String, HostEntry>;

static HOST_ENTRIES: LazyLock<Mutex<HostEntriesMap>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn add_host_entry(id: String, entry: HostEntry) -> std::io::Result<()> {
    debug!("Adding host entry for ID {}: {:?}", id, entry);

    let mut entries = HOST_ENTRIES.lock().unwrap_or_else(|e| {
        error!("Failed to acquire host entries lock: {}", e);
        e.into_inner()
    });

    entries.insert(id, entry);

    update_hosts_file(&entries)
}

pub fn remove_host_entry(id: &str) -> std::io::Result<()> {
    debug!("Removing host entry for ID {}", id);

    let mut entries = HOST_ENTRIES.lock().unwrap_or_else(|e| {
        error!("Failed to acquire host entries lock: {}", e);
        e.into_inner()
    });

    entries.remove(id);

    update_hosts_file(&entries)
}

pub fn remove_all_host_entries() -> std::io::Result<()> {
    info!("Removing all host entries");

    let mut entries = HOST_ENTRIES.lock().unwrap_or_else(|e| {
        error!("Failed to acquire host entries lock: {}", e);
        e.into_inner()
    });

    entries.clear();

    update_hosts_file(&entries)
}

fn update_hosts_file(entries: &HostEntriesMap) -> std::io::Result<()> {
    let _file_lock = HOSTS_FILE_LOCK.lock().unwrap_or_else(|e| {
        error!("Failed to acquire hosts file lock: {}", e);
        e.into_inner()
    });

    let mut hosts_builder = HostsBuilder::new(KFTRAY_HOSTS_TAG);

    for (id, entry) in entries {
        debug!("Adding entry for ID {} to hosts file: {:?}", id, entry);
        hosts_builder.add_hostname(entry.ip, &entry.hostname);
    }

    match hosts_builder.write() {
        Ok(_) => {
            debug!("Successfully wrote {} entries to hosts file", entries.len());
            Ok(())
        }
        Err(e) => {
            error!("Failed to write to hosts file: {}", e);
            Err(std::io::Error::new(std::io::ErrorKind::Other, e))
        }
    }
}
