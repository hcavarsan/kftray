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

    pub async fn allocate_address(&self, service_name: &str) -> Result<String, HelperError> {
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
        };

        allocations.insert(address.clone(), allocation);

        drop(allocations);
        self.save_allocations().await?;

        Ok(address)
    }

    pub async fn release_address(&self, address: &str) -> Result<(), HelperError> {
        let mut allocations = self.allocations.write().await;

        if allocations.remove(address).is_none() {
            return Err(HelperError::AddressPool(format!(
                "Address {address} is not allocated"
            )));
        }

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
