use std::path::PathBuf;
use std::time::Duration;

use log::{
    debug,
    error,
    info,
    warn,
};

use super::binary_finder;
use super::{
    installation,
    socket_comm,
    uninstallation,
};
use crate::communication::get_default_socket_path;
use crate::error::HelperError;
use crate::messages::{
    HelperResponse,
    RequestCommand,
};

pub struct HelperClient {
    app_id: String,
    socket_path: PathBuf,
}

impl HelperClient {
    pub fn new(app_id: String) -> Result<Self, HelperError> {
        let socket_path = get_default_socket_path()?;
        Ok(Self {
            app_id,
            socket_path,
        })
    }

    pub fn is_helper_available(&self) -> bool {
        socket_comm::is_socket_available(&self.socket_path)
    }

    pub fn ensure_helper_installed(&self) -> Result<(), HelperError> {
        if self.is_helper_available() {
            info!("Helper already available at {}", self.socket_path.display());
            return Ok(());
        }

        let helper_path = binary_finder::find_helper_binary()?;
        info!("Found helper binary at {}", helper_path.display());

        installation::install_helper(&helper_path)?;
        info!("Helper installation completed, waiting for socket to become available");

        let mut retry_count = 0;
        let max_retries = 10;
        while retry_count < max_retries {
            if self.is_helper_available() {
                info!("Helper is now available at {}", self.socket_path.display());
                return Ok(());
            }

            let wait_time = 500 * (retry_count + 1);
            debug!(
                "Helper not available yet, waiting {}ms (attempt {}/{})",
                wait_time,
                retry_count + 1,
                max_retries
            );
            std::thread::sleep(Duration::from_millis(wait_time));
            retry_count += 1;
        }

        if !self.is_helper_available() {
            error!(
                "Helper installation failed - socket not available at {}",
                self.socket_path.display()
            );
            return Err(HelperError::PlatformService(format!(
                "Helper was installed but socket is not available at {}",
                self.socket_path.display()
            )));
        }

        Ok(())
    }

    pub fn ensure_helper_uninstalled(&self) -> Result<(), HelperError> {
        if !self.is_helper_available() {
            info!("Helper is not running, nothing to uninstall");
            return Ok(());
        }

        info!("Attempting to stop the helper service...");
        let _ = self.stop_service();

        let helper_path = binary_finder::find_helper_binary()?;
        info!("Found helper binary at {}", helper_path.display());

        info!("Uninstalling helper...");
        uninstallation::uninstall_helper(&helper_path)?;

        info!("Waiting for helper to be fully uninstalled...");
        let mut retry_count = 0;
        let max_retries = 10;

        while retry_count < max_retries {
            if !self.is_helper_available() {
                info!("Confirmed helper is no longer available");
                return Ok(());
            }

            let wait_time = 500 * (retry_count + 1);
            debug!(
                "Helper still available, waiting {}ms (attempt {}/{})",
                wait_time,
                retry_count + 1,
                max_retries
            );
            std::thread::sleep(Duration::from_millis(wait_time));
            retry_count += 1;
        }

        if self.is_helper_available() {
            warn!(
                "Helper service is still responding after uninstall at {}",
                self.socket_path.display()
            );
            return Err(HelperError::PlatformService(format!(
                "Helper was uninstalled but is still available at {}",
                self.socket_path.display()
            )));
        }

        info!("Helper service successfully uninstalled");
        Ok(())
    }

    pub fn send_request(&self, command: RequestCommand) -> Result<HelperResponse, HelperError> {
        self.ensure_helper_installed()?;
        socket_comm::send_request(&self.socket_path, &self.app_id, command)
    }

    pub fn allocate_local_address(&self, service_name: String) -> Result<String, HelperError> {
        let command = RequestCommand::Address(super::super::messages::AddressCommand::Allocate {
            service_name,
        });
        let response = self.send_request(command)?;

        match response.result {
            super::super::messages::RequestResult::StringSuccess(address) => Ok(address),
            super::super::messages::RequestResult::Error(error) => {
                Err(HelperError::Communication(error))
            }
            _ => Err(HelperError::Communication(
                "Unexpected response format".to_string(),
            )),
        }
    }

    pub fn release_local_address(&self, address: String) -> Result<(), HelperError> {
        let command =
            RequestCommand::Address(super::super::messages::AddressCommand::Release { address });
        let response = self.send_request(command)?;

        match response.result {
            super::super::messages::RequestResult::Success => Ok(()),
            super::super::messages::RequestResult::Error(error) => {
                Err(HelperError::Communication(error))
            }
            _ => Err(HelperError::Communication(
                "Unexpected response format".to_string(),
            )),
        }
    }
}
