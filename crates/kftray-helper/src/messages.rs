use kftray_commons::models::hostfile::HostEntry;
use serde::{
    Deserialize,
    Serialize,
};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct HelperRequest {
    pub request_id: String,
    pub app_id: String,
    pub command: RequestCommand,

    #[serde(default)]
    pub return_address: String,

    #[serde(default = "default_timestamp")]
    pub timestamp: u64,
}

fn default_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HelperResponse {
    pub request_id: String,
    pub result: RequestResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestCommand {
    Network(NetworkCommand),
    Address(AddressCommand),
    Service(ServiceCommand),
    Host(HostCommand),
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkCommand {
    Add { address: String },
    Remove { address: String },
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AddressCommand {
    Allocate { service_name: String },
    Release { address: String },
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceCommand {
    Status,
    Stop,
    Restart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HostCommand {
    Add {
        id: String,
        entry: HostEntry,
    },
    Remove {
        id: String,
    },
    /// Removes unmarked copies of the given aliases. Lines written by a helper
    /// that did not mark its lines can only be tied to a configuration by what
    /// that configuration says its aliases are, and only those are removed.
    RemoveUnowned {
        entries: Vec<HostEntry>,
    },
    /// Removes the given owners' lines from the application's own section,
    /// the one it writes without the helper. An application that wrote there
    /// and later lost write access has no other way to take its lines out.
    RemoveDirectOwned {
        ids: Vec<String>,
        /// Unmarked copies of these aliases in the helper's own section go in
        /// the same write: the owned lines being removed are the only record
        /// tying those copies to the configuration, so the two cannot be
        /// separate requests with a failure possible between them.
        #[serde(default)]
        legacy: Vec<HostEntry>,
    },
    List,
    /// Clears both hosts sections this helper writes, whoever wrote each
    /// line.
    ///
    /// Kept for a client of another version still sending it: the current
    /// client uses `RemoveDirectOwned` and `RemoveUnowned` instead, but an
    /// installed helper of this version must still answer an older one
    /// rather than reject it as unrecognized.
    RemoveAll,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RequestResult {
    Success,
    StringSuccess(String),
    ListSuccess(Vec<String>),
    AllocationsSuccess(Vec<(String, String)>),
    HostEntriesSuccess(Vec<(String, HostEntry)>),
    Error(String),
}

impl HelperRequest {
    pub fn new(app_id: String, command: RequestCommand) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            app_id,
            command,
            return_address: String::new(),
            timestamp: default_timestamp(),
        }
    }
}

impl HelperResponse {
    pub fn success(request_id: String) -> Self {
        Self {
            request_id,
            result: RequestResult::Success,
        }
    }

    pub fn string_success(request_id: String, result: String) -> Self {
        Self {
            request_id,
            result: RequestResult::StringSuccess(result),
        }
    }

    pub fn list_success(request_id: String, result: Vec<String>) -> Self {
        Self {
            request_id,
            result: RequestResult::ListSuccess(result),
        }
    }

    pub fn allocations_success(request_id: String, result: Vec<(String, String)>) -> Self {
        Self {
            request_id,
            result: RequestResult::AllocationsSuccess(result),
        }
    }

    pub fn host_entries_success(request_id: String, result: Vec<(String, HostEntry)>) -> Self {
        Self {
            request_id,
            result: RequestResult::HostEntriesSuccess(result),
        }
    }

    pub fn error(request_id: String, error: impl Into<String>) -> Self {
        Self {
            request_id,
            result: RequestResult::Error(error.into()),
        }
    }
}
