use std::time::Duration;

use tokio::task::spawn_blocking;
use tokio::time::timeout;
use tracing::{
    info,
    warn,
};

use crate::port_forward_error::PortForwardError;

/// Synchronous helper function to release address via helper service.
/// Must be called from spawn_blocking to avoid blocking the tokio runtime.
fn try_release_address_sync(address: &str) -> Result<(), PortForwardError> {
    let app_id = "com.kftray.app".to_string();

    let socket_path = kftray_helper::communication::get_default_socket_path()
        .map_err(|e| PortForwardError::AddressAllocation(e.to_string()))?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err(PortForwardError::AddressAllocation(
            "Helper service is not available".to_string(),
        ));
    }

    let command = kftray_helper::messages::RequestCommand::Address(
        kftray_helper::messages::AddressCommand::Release {
            address: address.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(response) => match response.result {
            kftray_helper::messages::RequestResult::Success => Ok(()),
            kftray_helper::messages::RequestResult::Error(error) => {
                Err(PortForwardError::AddressAllocation(error))
            }
            _ => Err(PortForwardError::AddressAllocation(
                "Unexpected response format".to_string(),
            )),
        },
        Err(e) => Err(PortForwardError::AddressAllocation(e.to_string())),
    }
}

/// Release address with timeout. Skips osascript fallback to avoid blocking on
/// user interaction. Address cleanup is not critical - addresses will be freed
/// on system restart.
pub(super) async fn release_address_with_fallback(address: &str) {
    const ADDRESS_RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

    let address_owned = address.to_string();

    // Wrap blocking helper service call in spawn_blocking with timeout
    let result = timeout(ADDRESS_RELEASE_TIMEOUT, async {
        let addr = address_owned.clone();
        spawn_blocking(move || try_release_address_sync(&addr)).await
    })
    .await;

    match result {
        Ok(Ok(Ok(_))) => {
            info!("Successfully released address via helper: {}", address);
        }
        Ok(Ok(Err(e))) => {
            // Helper service returned an error - skip fallback (osascript blocks for user
            // input)
            warn!(
                "Failed to release address {} via helper: {}. Skipping fallback to avoid blocking.",
                address, e
            );
        }
        Ok(Err(e)) => {
            // spawn_blocking panicked
            warn!(
                "Address release task panicked for {}: {}. Skipping.",
                address, e
            );
        }
        Err(_) => {
            // Timeout elapsed
            warn!(
                "Address release timed out for {} after {:?}. Skipping.",
                address, ADDRESS_RELEASE_TIMEOUT
            );
        }
    }
}
