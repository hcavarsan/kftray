use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::LazyLock;
use std::sync::{
    Mutex,
    RwLock,
};
use std::thread;
use std::time::Duration;

use ::hostsfile::HostsBuilder;
use log::{
    debug,
    error,
    info,
};

const BATCH_DELAY_MS: u64 = 100;

const KFTRAY_HOSTS_TAG: &str = "kftray-hosts";

#[derive(Debug, Clone)]
pub struct HostEntry {
    pub ip: IpAddr,
    pub hostname: String,
}

type HostEntriesMap = HashMap<String, HostEntry>;

static HOST_ENTRIES: LazyLock<RwLock<HostEntriesMap>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

static NEEDS_UPDATE: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

static WRITER_RUNNING: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

pub fn add_host_entry(id: String, entry: HostEntry) -> std::io::Result<()> {
    debug!("Adding host entry for ID {}: {:?}", id, entry);

    {
        match HOST_ENTRIES.write() {
            Ok(mut entries) => {
                entries.insert(id, entry);
            }
            Err(e) => {
                error!("Failed to acquire host entries write lock: {}", e);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ));
            }
        }
    }

    {
        let mut needs_update = NEEDS_UPDATE.lock().unwrap_or_else(|e| {
            error!("Failed to acquire needs_update lock: {}", e);
            e.into_inner()
        });
        *needs_update = true;
    }

    ensure_writer_running();

    Ok(())
}

pub fn remove_host_entry(id: &str) -> std::io::Result<()> {
    debug!("Removing host entry for ID {}", id);

    {
        match HOST_ENTRIES.write() {
            Ok(mut entries) => {
                entries.remove(id);
            }
            Err(e) => {
                error!("Failed to acquire host entries write lock: {}", e);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ));
            }
        }
    }

    {
        let mut needs_update = NEEDS_UPDATE.lock().unwrap_or_else(|e| {
            error!("Failed to acquire needs_update lock: {}", e);
            e.into_inner()
        });
        *needs_update = true;
    }

    ensure_writer_running();

    Ok(())
}

pub fn remove_all_host_entries() -> std::io::Result<()> {
    info!("Removing all host entries");

    {
        match HOST_ENTRIES.write() {
            Ok(mut entries) => {
                entries.clear();
            }
            Err(e) => {
                error!("Failed to acquire host entries write lock: {}", e);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ));
            }
        }
    }

    update_hosts_file()
}

fn ensure_writer_running() {
    let mut writer_running = WRITER_RUNNING.lock().unwrap_or_else(|e| {
        error!("Failed to acquire writer_running lock: {}", e);
        e.into_inner()
    });

    if !*writer_running {
        *writer_running = true;

        thread::spawn(|| {
            batch_writer_loop();
        });
    }
}

fn batch_writer_loop() {
    loop {
        thread::sleep(Duration::from_millis(BATCH_DELAY_MS));

        let needs_update = {
            let mut update_flag = NEEDS_UPDATE.lock().unwrap_or_else(|e| {
                error!("Failed to acquire needs_update lock in writer loop: {}", e);
                e.into_inner()
            });

            if *update_flag {
                *update_flag = false;
                true
            } else {
                false
            }
        };

        if needs_update {
            if let Err(e) = update_hosts_file() {
                error!("Failed to write hosts file in background writer: {}", e);
            }
        } else {
            let pending = {
                NEEDS_UPDATE.lock().unwrap_or_else(|e| {
                    error!("Failed to check for pending updates: {}", e);
                    e.into_inner()
                })
            };

            if !*pending {
                break;
            }
        }
    }

    let mut writer_running = WRITER_RUNNING.lock().unwrap_or_else(|e| {
        error!("Failed to acquire writer_running lock when exiting: {}", e);
        e.into_inner()
    });
    *writer_running = false;
}

fn update_hosts_file() -> std::io::Result<()> {
    let entries_snapshot = match HOST_ENTRIES.read() {
        Ok(entries) => entries.clone(),
        Err(e) => {
            error!("Failed to acquire host entries read lock: {}", e);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ));
        }
    };

    let mut hosts_builder = HostsBuilder::new(KFTRAY_HOSTS_TAG);

    for (id, entry) in &entries_snapshot {
        debug!("Adding entry for ID {} to hosts file: {:?}", id, entry);
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
            error!("Failed to write to hosts file: {}", e);
            Err(std::io::Error::new(std::io::ErrorKind::Other, e))
        }
    }
}
