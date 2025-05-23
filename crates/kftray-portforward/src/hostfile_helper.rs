use std::path::PathBuf;

use kftray_commons::models::hostfile::HostEntry;
use kftray_helper::HelperError;
use log::{
    debug,
    error,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HostfileHelperError {
    #[error("Helper error: {0}")]
    Helper(#[from] HelperError),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

pub struct HostfileHelperClient {
    app_id: String,
    socket_path: PathBuf,
}

impl HostfileHelperClient {
    pub fn new() -> Result<Self, HostfileHelperError> {
        debug!("Creating new HostfileHelperClient");

        let socket_path = kftray_helper::communication::get_default_socket_path()
            .map_err(HostfileHelperError::Helper)?;

        Ok(Self {
            app_id: "com.kftray.app".to_string(),
            socket_path,
        })
    }

    pub fn add_host_entry(&self, id: String, entry: HostEntry) -> Result<(), HostfileHelperError> {
        debug!("Adding host entry via helper for ID {}: {:?}", id, entry);

        if !self.is_available() {
            return Err(HostfileHelperError::Communication(
                "Helper service is not available".to_string(),
            ));
        }

        let command = kftray_helper::messages::RequestCommand::Host(
            kftray_helper::messages::HostCommand::Add {
                id: id.clone(),
                entry,
            },
        );

        match kftray_helper::client::socket_comm::send_request(
            &self.socket_path,
            &self.app_id,
            command,
        ) {
            Ok(response) => match response.result {
                kftray_helper::messages::RequestResult::Success => {
                    debug!("Successfully added host entry for ID: {}", id);
                    Ok(())
                }
                kftray_helper::messages::RequestResult::Error(err) => {
                    error!("Helper returned error for add_host_entry: {}", err);
                    Err(HostfileHelperError::Communication(err))
                }
                _ => {
                    error!("Unexpected response type for add_host_entry");
                    Err(HostfileHelperError::InvalidResponse(
                        "Expected Success or Error response".to_string(),
                    ))
                }
            },
            Err(e) => {
                error!("Failed to send add_host_entry request to helper: {}", e);
                Err(HostfileHelperError::Helper(e))
            }
        }
    }

    pub fn remove_host_entry(&self, id: &str) -> Result<(), HostfileHelperError> {
        debug!("Removing host entry via helper for ID: {}", id);

        if !self.is_available() {
            return Err(HostfileHelperError::Communication(
                "Helper service is not available".to_string(),
            ));
        }

        let command = kftray_helper::messages::RequestCommand::Host(
            kftray_helper::messages::HostCommand::Remove { id: id.to_string() },
        );

        match kftray_helper::client::socket_comm::send_request(
            &self.socket_path,
            &self.app_id,
            command,
        ) {
            Ok(response) => match response.result {
                kftray_helper::messages::RequestResult::Success => {
                    debug!("Successfully removed host entry for ID: {}", id);
                    Ok(())
                }
                kftray_helper::messages::RequestResult::Error(err) => {
                    error!("Helper returned error for remove_host_entry: {}", err);
                    Err(HostfileHelperError::Communication(err))
                }
                _ => {
                    error!("Unexpected response type for remove_host_entry");
                    Err(HostfileHelperError::InvalidResponse(
                        "Expected Success or Error response".to_string(),
                    ))
                }
            },
            Err(e) => {
                error!("Failed to send remove_host_entry request to helper: {}", e);
                Err(HostfileHelperError::Helper(e))
            }
        }
    }

    pub fn remove_all_host_entries(&self) -> Result<(), HostfileHelperError> {
        debug!("Removing all host entries via helper");

        if !self.is_available() {
            return Err(HostfileHelperError::Communication(
                "Helper service is not available".to_string(),
            ));
        }

        let command = kftray_helper::messages::RequestCommand::Host(
            kftray_helper::messages::HostCommand::RemoveAll,
        );

        match kftray_helper::client::socket_comm::send_request(
            &self.socket_path,
            &self.app_id,
            command,
        ) {
            Ok(response) => match response.result {
                kftray_helper::messages::RequestResult::Success => {
                    debug!("Successfully removed all host entries");
                    Ok(())
                }
                kftray_helper::messages::RequestResult::Error(err) => {
                    error!("Helper returned error for remove_all_host_entries: {}", err);
                    Err(HostfileHelperError::Communication(err))
                }
                _ => {
                    error!("Unexpected response type for remove_all_host_entries");
                    Err(HostfileHelperError::InvalidResponse(
                        "Expected Success or Error response".to_string(),
                    ))
                }
            },
            Err(e) => {
                error!(
                    "Failed to send remove_all_host_entries request to helper: {}",
                    e
                );
                Err(HostfileHelperError::Helper(e))
            }
        }
    }

    pub fn list_host_entries(&self) -> Result<Vec<(String, HostEntry)>, HostfileHelperError> {
        debug!("Listing host entries via helper");

        // Check availability first - don't try to install automatically
        if !self.is_available() {
            return Err(HostfileHelperError::Communication(
                "Helper service is not available".to_string(),
            ));
        }

        let command = kftray_helper::messages::RequestCommand::Host(
            kftray_helper::messages::HostCommand::List,
        );

        match kftray_helper::client::socket_comm::send_request(
            &self.socket_path,
            &self.app_id,
            command,
        ) {
            Ok(response) => match response.result {
                kftray_helper::messages::RequestResult::HostEntriesSuccess(entries) => {
                    debug!("Successfully listed {} host entries", entries.len());
                    Ok(entries)
                }
                kftray_helper::messages::RequestResult::Error(err) => {
                    error!("Helper returned error for list_host_entries: {}", err);
                    Err(HostfileHelperError::Communication(err))
                }
                _ => {
                    error!("Unexpected response type for list_host_entries");
                    Err(HostfileHelperError::InvalidResponse(
                        "Expected HostEntriesSuccess or Error response".to_string(),
                    ))
                }
            },
            Err(e) => {
                error!("Failed to send list_host_entries request to helper: {}", e);
                Err(HostfileHelperError::Helper(e))
            }
        }
    }

    pub fn is_available(&self) -> bool {
        debug!("Checking if helper service is available");

        kftray_helper::client::socket_comm::is_socket_available(&self.socket_path)
    }
}

impl Default for HostfileHelperClient {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            panic!("Failed to create default HostfileHelperClient: {:?}", e);
        })
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn test_client_creation() {
        let result = HostfileHelperClient::new();

        match result {
            Ok(_) => println!("Helper client created successfully"),
            Err(e) => println!("Helper client creation failed (expected in tests): {}", e),
        }
    }

    #[test]
    fn test_client_availability_check() {
        if let Ok(client) = HostfileHelperClient::new() {
            let _is_available = client.is_available();
            println!("Helper client availability check completed");
        } else {
            println!("Helper client creation failed (expected in tests without running service)");
        }
    }
}
