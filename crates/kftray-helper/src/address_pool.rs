use std::{
    collections::HashMap,
    fs,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use serde::{
    Deserialize,
    Serialize,
};
use tokio::sync::RwLock;

use crate::error::HelperError;

const MAX_ALLOCATION_AGE: Duration = Duration::from_secs(3600 * 24 * 7);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AddressAllocation {
    service_name: String,
    last_refreshed: SystemTime,
    /// The pid that requested this address, from peer credentials on the
    /// connection that allocated or last refreshed it. Absent for an
    /// allocation a helper from before this field existed persisted.
    #[serde(default)]
    owner_pid: Option<u32>,
}

/// Whether `pid` names a process still running.
///
/// A dead recorded owner is no owner at all: the process that could have
/// disputed a release is gone, so there is nothing left to protect.
#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    // Signal 0 sends nothing; it only validates the pid. `ESRCH` means the
    // process is gone. Any other failure (`EPERM`, owned by someone else)
    // still means it exists.
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(windows)]
fn is_pid_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(_) => false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AddressPoolStorage {
    allocations: HashMap<String, AddressAllocation>,
}

pub struct AddressPoolManager {
    allocations: Arc<RwLock<HashMap<String, AddressAllocation>>>,
    storage_path: PathBuf,
}

impl AddressPoolManager {
    pub fn new() -> Result<Self, HelperError> {
        let storage_path = Self::get_storage_path()?;
        let allocations = Self::load_allocations(&storage_path)?;

        let manager = Self {
            allocations: Arc::new(RwLock::new(allocations)),
            storage_path,
        };

        Ok(manager)
    }

    /// A manager over `storage_path` instead of the real `~/.kftray`
    /// location, so a test's allocations never touch the machine's shared
    /// address pool file.
    #[cfg(test)]
    fn for_test(storage_path: PathBuf) -> Self {
        Self {
            allocations: Arc::new(RwLock::new(HashMap::new())),
            storage_path,
        }
    }

    fn get_storage_path() -> Result<PathBuf, HelperError> {
        let dirs = dirs::home_dir()
            .ok_or_else(|| HelperError::AddressPool("Could not determine home directory".into()))?;

        let config_dir = dirs.join(".kftray");
        fs::create_dir_all(&config_dir).map_err(|e| {
            HelperError::AddressPool(format!("Could not create config directory: {e}"))
        })?;

        Ok(config_dir.join("address_pool.json"))
    }

    fn load_allocations(path: &Path) -> Result<HashMap<String, AddressAllocation>, HelperError> {
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(path).map_err(|e| {
            HelperError::AddressPool(format!("Could not read address pool storage: {e}"))
        })?;

        let storage: AddressPoolStorage = serde_json::from_str(&content).map_err(|e| {
            HelperError::AddressPool(format!("Could not parse address pool storage: {e}"))
        })?;

        Ok(storage.allocations)
    }

    async fn save_allocations(&self) -> Result<(), HelperError> {
        let allocations = self.allocations.read().await.clone();
        let storage = AddressPoolStorage { allocations };

        let content = serde_json::to_string_pretty(&storage).map_err(|e| {
            HelperError::AddressPool(format!("Could not serialize address pool storage: {e}"))
        })?;

        fs::write(&self.storage_path, content).map_err(|e| {
            HelperError::AddressPool(format!("Could not write address pool storage: {e}"))
        })?;

        Ok(())
    }

    /// `caller_pid` becomes the recorded owner of a newly allocated address,
    /// and is refreshed on the owner's own re-allocation of an address it
    /// already holds.
    pub async fn allocate_address(
        &self, service_name: &str, caller_pid: Option<u32>,
    ) -> Result<String, HelperError> {
        self.cleanup_stale_allocations().await?;

        let mut allocations = self.allocations.write().await;

        let existing_addr = allocations
            .iter()
            .find(|(_, alloc)| alloc.service_name == service_name)
            .map(|(addr, _)| addr.clone());

        if let Some(addr) = existing_addr {
            if let Some(alloc) = allocations.get(&addr).cloned() {
                let mut updated_alloc = alloc;
                updated_alloc.last_refreshed = SystemTime::now();
                updated_alloc.owner_pid = caller_pid;
                allocations.insert(addr.clone(), updated_alloc);
            }

            drop(allocations);
            self.save_allocations().await?;

            return Ok(addr);
        }

        let address = self.find_next_available_address(&allocations)?;

        let allocation = AddressAllocation {
            service_name: service_name.to_string(),
            last_refreshed: SystemTime::now(),
            owner_pid: caller_pid,
        };

        allocations.insert(address.clone(), allocation);

        drop(allocations);
        self.save_allocations().await?;

        Ok(address)
    }

    /// Releases `address`, refusing when a different, still-live process
    /// owns it.
    ///
    /// A recorded owner that no longer differs from `caller_pid`, has none
    /// recorded (an allocation from before ownership tracking, or one this
    /// call itself does not know the caller for), or is no longer alive is
    /// not a conflict: the address is released either way.
    pub async fn release_address(
        &self, address: &str, caller_pid: Option<u32>,
    ) -> Result<(), HelperError> {
        let mut allocations = self.allocations.write().await;

        let Some(allocation) = allocations.get(address) else {
            return Err(HelperError::AddressPool(format!(
                "Address {address} is not allocated"
            )));
        };

        if let Some(owner_pid) = allocation.owner_pid
            && Some(owner_pid) != caller_pid
            && is_pid_alive(owner_pid)
        {
            return Err(HelperError::AddressPool(format!(
                "Address {address} is owned by another live process (pid {owner_pid})"
            )));
        }

        allocations.remove(address);

        drop(allocations);
        self.save_allocations().await?;

        Ok(())
    }

    pub async fn list_allocations(&self) -> Result<Vec<(String, String)>, HelperError> {
        let allocations = self.allocations.read().await;

        let result: Vec<(String, String)> = allocations
            .iter()
            .map(|(addr, alloc)| (alloc.service_name.clone(), addr.clone()))
            .collect();

        Ok(result)
    }

    async fn cleanup_stale_allocations(&self) -> Result<(), HelperError> {
        let mut allocations = self.allocations.write().await;
        let now = SystemTime::now();

        allocations.retain(|_, alloc| match now.duration_since(alloc.last_refreshed) {
            Ok(duration) => duration < MAX_ALLOCATION_AGE,
            Err(_) => true,
        });

        drop(allocations);
        self.save_allocations().await?;

        Ok(())
    }

    fn find_next_available_address(
        &self, allocations: &HashMap<String, AddressAllocation>,
    ) -> Result<String, HelperError> {
        let mut octet = 2;
        while octet < 255 {
            let address = format!("127.0.0.{octet}");
            if !allocations.contains_key(&address) {
                return Ok(address);
            }
            octet += 1;
        }

        Err(HelperError::AddressPool(
            "No more addresses available in the pool".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> AddressPoolManager {
        let path = std::env::temp_dir().join(format!(
            "kftray-address-pool-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        AddressPoolManager::for_test(path)
    }

    /// A pid that has already exited: spawning and waiting on a child
    /// guarantees it is gone rather than relying on an arbitrary unused
    /// number that might collide with something alive.
    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" })
            .args(if cfg!(windows) {
                &["/C", "exit 0"][..]
            } else {
                &[][..]
            })
            .spawn()
            .expect("spawn a short-lived child process");
        let pid = child.id();
        child.wait().expect("wait for the child to exit");
        pid
    }

    #[tokio::test]
    async fn release_is_refused_when_the_recorded_owner_is_a_different_live_process() {
        let pool = manager();
        let owner_pid = std::process::id();
        let address = pool
            .allocate_address("svc-a", Some(owner_pid))
            .await
            .unwrap();

        let err = pool
            .release_address(&address, Some(owner_pid + 1))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("owned by another live process"),
            "unexpected error: {err}"
        );

        // The owner itself can still release it.
        pool.release_address(&address, Some(owner_pid))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn release_succeeds_when_the_recorded_owner_is_dead() {
        let pool = manager();
        let owner_pid = dead_pid();
        let address = pool
            .allocate_address("svc-b", Some(owner_pid))
            .await
            .unwrap();

        pool.release_address(&address, Some(owner_pid + 1))
            .await
            .expect("a dead recorded owner must not block release");
    }

    #[tokio::test]
    async fn release_succeeds_when_no_owner_was_recorded() {
        let pool = manager();
        let address = pool.allocate_address("svc-c", None).await.unwrap();

        pool.release_address(&address, Some(std::process::id()))
            .await
            .expect("an allocation with no recorded owner must not block release");
    }
}
