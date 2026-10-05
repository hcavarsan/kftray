use kftray_helper::HelperClient;
use kftray_helper::client::socket_comm;
use kftray_helper::communication::get_default_socket_path;
use kftray_helper::messages::{
    AddressCommand,
    HostCommand,
    RequestCommand,
    RequestResult,
};
use log::{
    info,
    warn,
};
use serde::Serialize;

const APP_ID: &str = "com.kftray.app";

#[derive(Serialize)]
pub struct HelperAddress {
    service: String,
    address: String,
}

#[derive(Serialize)]
pub struct HelperHostEntry {
    id: String,
    ip: String,
    hostname: String,
}

/// What the helper is and what it currently holds on this machine.
///
/// The lists are `None` when they could not be read from a running helper,
/// so "nothing allocated" is never shown for "could not ask".
#[derive(Serialize)]
pub struct HelperStatus {
    running: bool,
    error: Option<String>,
    addresses: Option<Vec<HelperAddress>>,
    host_entries: Option<Vec<HelperHostEntry>>,
}

/// Queries the helper over its socket without `HelperClient::send_request`,
/// which installs the helper first when it is missing: checking status must
/// never trigger an elevation prompt.
#[tauri::command]
pub async fn get_helper_status() -> Result<HelperStatus, String> {
    tokio::task::spawn_blocking(read_helper_status)
        .await
        .map_err(|e| e.to_string())?
}

fn read_helper_status() -> Result<HelperStatus, String> {
    let socket_path = get_default_socket_path().map_err(|e| e.to_string())?;
    let mut status = HelperStatus {
        running: false,
        error: None,
        addresses: None,
        host_entries: None,
    };

    if !socket_comm::is_socket_available(&socket_path) {
        return Ok(status);
    }

    match socket_comm::send_request(&socket_path, APP_ID, RequestCommand::Ping) {
        Ok(response) if matches!(&response.result, RequestResult::StringSuccess(s) if s == "pong") =>
        {
            status.running = true;
        }
        Ok(response) => {
            status.error = Some(format!("Unexpected ping response: {:?}", response.result));
            return Ok(status);
        }
        Err(e) => {
            status.error = Some(e.to_string());
            return Ok(status);
        }
    }

    let list = |command| match socket_comm::send_request(&socket_path, APP_ID, command) {
        Ok(response) => Some(response.result),
        Err(e) => {
            warn!("Failed to read helper state: {e}");
            None
        }
    };

    status.addresses = match list(RequestCommand::Address(AddressCommand::List)) {
        Some(RequestResult::AllocationsSuccess(allocations)) => Some(
            allocations
                .into_iter()
                .map(|(service, address)| HelperAddress { service, address })
                .collect(),
        ),
        _ => None,
    };

    status.host_entries = match list(RequestCommand::Host(HostCommand::List)) {
        Some(RequestResult::HostEntriesSuccess(entries)) => Some(
            entries
                .into_iter()
                .map(|(id, entry)| HelperHostEntry {
                    id,
                    ip: entry.ip.to_string(),
                    hostname: entry.hostname,
                })
                .collect(),
        ),
        _ => None,
    };

    Ok(status)
}

#[tauri::command]
pub async fn install_helper() -> Result<bool, String> {
    info!("Installing helper sidecar");
    let client = HelperClient::new(APP_ID.to_string()).map_err(|e| e.to_string())?;

    client
        .ensure_helper_installed()
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn remove_helper() -> Result<bool, String> {
    info!("Removing helper sidecar");
    let client = HelperClient::new(APP_ID.to_string()).map_err(|e| e.to_string())?;

    client
        .ensure_helper_uninstalled()
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn allocate_local_address_cmd(service_name: String) -> Result<String, String> {
    info!("Allocating local address for service: {service_name}");
    let client = HelperClient::new(APP_ID.to_string()).map_err(|e| e.to_string())?;

    match client.allocate_local_address(service_name) {
        Ok(address) => {
            info!("Successfully allocated address: {address}");
            Ok(address)
        }
        Err(e) => {
            warn!("Failed to allocate address: {e}");
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn release_local_address_cmd(address: String) -> Result<(), String> {
    info!("Releasing local address: {address}");
    let client = HelperClient::new(APP_ID.to_string()).map_err(|e| e.to_string())?;

    client.release_local_address(address).map_err(|e| {
        warn!("Failed to release address: {e}");
        e.to_string()
    })
}
