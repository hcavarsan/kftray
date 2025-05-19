use std::path::PathBuf;
use std::time::Duration;

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
            return Ok(());
        }

        let helper_path = binary_finder::find_helper_binary()?;

        installation::install_helper(&helper_path)?;

        for _ in 0..5 {
            if self.is_helper_available() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        if !self.is_helper_available() {
            return Err(HelperError::PlatformService(
                "Helper was installed but is not available".into(),
            ));
        }

        Ok(())
    }

    pub fn ensure_helper_uninstalled(&self) -> Result<(), HelperError> {
        if !self.is_helper_available() {
            return Ok(());
        }

        let _ = self.stop_service();

        let helper_path = binary_finder::find_helper_binary()?;

        uninstallation::uninstall_helper(&helper_path)?;

        for _ in 0..5 {
            if !self.is_helper_available() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        if self.is_helper_available() {
            return Err(HelperError::PlatformService(
                "Helper was uninstalled but is still available".into(),
            ));
        }

        Ok(())
    }

    pub fn send_request(&self, command: RequestCommand) -> Result<HelperResponse, HelperError> {
        self.ensure_helper_installed()?;
        socket_comm::send_request(&self.socket_path, &self.app_id, command)
    }
}
