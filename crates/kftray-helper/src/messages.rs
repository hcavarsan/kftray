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
    Add { id: String, entry: HostEntry },
    Remove { id: String },
    RemoveAll,
    List,
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
