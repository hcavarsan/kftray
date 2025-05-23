use log::debug;

use super::helper_client::HelperClient;
use crate::error::HelperError;
use crate::messages::{
    NetworkCommand,
    RequestCommand,
    RequestResult,
    ServiceCommand,
};

impl HelperClient {
    pub fn ping(&self) -> Result<bool, HelperError> {
        match self.send_request(RequestCommand::Ping) {
            Ok(response) => match response.result {
                RequestResult::StringSuccess(s) if s == "pong" => Ok(true),
                _ => Ok(false),
            },
            Err(_) => Ok(false),
        }
    }

    pub fn add_loopback_address(&self, address: &str) -> Result<(), HelperError> {
        let command = RequestCommand::Network(NetworkCommand::Add {
            address: address.to_string(),
        });

        let response = self.send_request(command)?;

        match response.result {
            RequestResult::Success => Ok(()),
            RequestResult::Error(e) => Err(HelperError::NetworkConfig(e)),
            _ => Err(HelperError::Communication(
                "Unexpected response type".into(),
            )),
        }
    }

    pub fn remove_loopback_address(&self, address: &str) -> Result<(), HelperError> {
        debug!("Removing loopback address: {address}");
        let command = RequestCommand::Network(NetworkCommand::Remove {
            address: address.to_string(),
        });

        let mut last_error = None;
        for attempt in 1..=3 {
            debug!("Attempt {attempt} to remove loopback address: {address}");
            match self.send_request(command.clone()) {
                Ok(response) => match response.result {
                    RequestResult::Success => {
                        debug!("Successfully removed loopback address: {address}");
                        return Ok(());
                    }
                    RequestResult::Error(e) => {
                        if e.contains("not found") || e.contains("No such process") {
                            debug!("Address {address} is already removed");
                            return Ok(());
                        }
                        last_error = Some(HelperError::NetworkConfig(e));
                    }
                    _ => {
                        last_error = Some(HelperError::Communication(
                            "Unexpected response type".into(),
                        ));
                    }
                },
                Err(e) => {
                    debug!("Error removing loopback address (attempt {attempt}): {e}");
                    last_error = Some(e);

                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            HelperError::Communication(format!("Failed to remove loopback address: {address}"))
        }))
    }

    pub fn stop_service(&self) -> Result<(), HelperError> {
        let command = RequestCommand::Service(ServiceCommand::Stop);

        let response = self.send_request(command)?;

        match response.result {
            RequestResult::Success => Ok(()),
            RequestResult::Error(e) => Err(HelperError::PlatformService(e)),
            _ => Err(HelperError::Communication(
                "Unexpected response type".into(),
            )),
        }
    }
}
