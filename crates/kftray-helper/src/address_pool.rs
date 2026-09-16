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
    use windows::Win32::Foundation::{
        CloseHandle,
        ERROR_INVALID_PARAMETER,
    };
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
            // No such pid: definitively gone.
            Err(e) if e.code() == ERROR_INVALID_PARAMETER.to_hresult() => false,
            // Any other failure (`ACCESS_DENIED` for a protected or
            // higher-integrity process, and the like) still means the
            // process exists, symmetric with the Unix `EPERM` handling
            // above.
            Err(_) => true,
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

                // Only take ownership when nothing else still holds it: a
                // different, live recorded owner keeps it, so a second
                // process cannot re-allocate the same service name to
                // become the recorded owner and then release out from
                // under the original caller.
                let owned_by_other_live_process =
                    updated_alloc.owner_pid.is_some_and(|owner_pid| {
                        Some(owner_pid) != caller_pid && is_pid_alive(owner_pid)
                    });
                if !owned_by_other_live_process {
                    updated_alloc.owner_pid = caller_pid;
                }

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
            return Err(HelperError::AddressNotAllocated(format!(
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

    /// Expires an allocation once it is stale by age, or once its recorded
    /// owner is no longer alive -- the latter is checked here rather than
    /// only left to `release_address`, so a process that dies without
    /// releasing its address does not pin it for up to `MAX_ALLOCATION_AGE`
    /// and narrow the window in which the OS could recycle that pid onto
    /// an unrelated live process before this runs again (it runs on every
    /// `allocate_address` call).
    async fn cleanup_stale_allocations(&self) -> Result<(), HelperError> {
        let mut allocations = self.allocations.write().await;
        let now = SystemTime::now();

        allocations.retain(|_, alloc| {
            let stale_by_age = match now.duration_since(alloc.last_refreshed) {
                Ok(duration) => duration >= MAX_ALLOCATION_AGE,
                Err(_) => false,
            };
            let owner_dead = alloc.owner_pid.is_some_and(|pid| !is_pid_alive(pid));
            !(stale_by_age || owner_dead)
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

    /// Wraps the manager with the `tempfile::TempPath` backing its storage
    /// file, so the file is removed on drop instead of leaked into the OS
    /// temp directory on every test run (and permanently on a panic).
    struct TestManager {
        inner: AddressPoolManager,
        _temp: tempfile::TempPath,
    }

    impl std::ops::Deref for TestManager {
        type Target = AddressPoolManager;
        fn deref(&self) -> &AddressPoolManager {
            &self.inner
        }
    }

    fn manager() -> TestManager {
        let temp = tempfile::Builder::new()
            .prefix("kftray-address-pool-test-")
            .suffix(".json")
            .tempfile()
            .expect("create a temp file for the test address pool")
            .into_temp_path();
        let inner = AddressPoolManager::for_test(temp.to_path_buf());
        TestManager { inner, _temp: temp }
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
        // Narrows the window for the OS to have already recycled `pid`
        // onto an unrelated live process (a busy CI runner with a low
        // `pid_max` is the realistic case): asserted here, immediately
        // after reaping, so that rare reuse fails with a clear message
        // instead of a confusing failure in whichever test called this.
        assert!(
            !is_pid_alive(pid),
            "pid {pid} was already reused by another live process right after exiting"
        );
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

    #[tokio::test]
    async fn allocate_does_not_transfer_ownership_away_from_a_live_owner() {
        let pool = manager();
        let owner_pid = std::process::id();
        let address = pool
            .allocate_address("svc-d", Some(owner_pid))
            .await
            .unwrap();

        // A different live caller re-allocating the same service name gets
        // the same address back, but must not become its recorded owner:
        // otherwise it could immediately release it out from under the
        // process that still holds it.
        let other_pid = owner_pid + 1;
        let same_address = pool
            .allocate_address("svc-d", Some(other_pid))
            .await
            .unwrap();
        assert_eq!(address, same_address);

        let err = pool
            .release_address(&address, Some(other_pid))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("owned by another live process"),
            "unexpected error: {err}"
        );

        // The original owner can still release it.
        pool.release_address(&address, Some(owner_pid))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_dead_owners_allocation_is_reclaimed_without_waiting_for_the_age_limit() {
        let pool = manager();
        let dead_owner = dead_pid();
        pool.allocate_address("svc-e", Some(dead_owner))
            .await
            .unwrap();

        // Allocating an unrelated service runs `cleanup_stale_allocations`,
        // which must free the dead owner's allocation immediately rather
        // than only after `MAX_ALLOCATION_AGE`, so a live process cannot
        // end up recycled onto that pid and block the release for up to 7
        // days. The freed address may be handed straight back out to the
        // next allocation, so the service name -- not the address string,
        // which can legitimately be reused -- is what proves the reclaim.
        pool.allocate_address("svc-f", Some(std::process::id()))
            .await
            .unwrap();

        let allocations = pool.list_allocations().await.unwrap();
        assert!(
            !allocations.iter().any(|(service, _)| service == "svc-e"),
            "the dead owner's allocation should have been reclaimed: {allocations:?}"
        );
    }
}
