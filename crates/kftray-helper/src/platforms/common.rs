use crate::{
    address_pool::AddressPoolManager,
    communication::{
        get_default_socket_path,
        start_communication_server,
    },
    error::HelperError,
    network::NetworkConfigManager,
};

pub(crate) async fn initialize_components(
) -> Result<(AddressPoolManager, NetworkConfigManager, std::path::PathBuf), HelperError> {
    let pool_manager = match AddressPoolManager::new() {
        Ok(mgr) => {
            println!("Successfully initialized address pool manager");
            mgr
        }
        Err(e) => {
            eprintln!("Error initializing address pool manager: {e}");
            return Err(e);
        }
    };

    let network_manager = match NetworkConfigManager::new() {
        Ok(mgr) => {
            println!("Successfully initialized network manager");
            mgr
        }
        Err(e) => {
            eprintln!("Error initializing network manager: {e}");
            return Err(e);
        }
    };

    let socket_path = match get_default_socket_path() {
        Ok(path) => {
            println!("Using socket path: {}", path.display());

            if path.exists() {
                println!("Removing existing socket file");
                let _ = std::fs::remove_file(&path);
            }

            path
        }
        Err(e) => {
            eprintln!("Error getting socket path: {e}");
            return Err(e);
        }
    };

    if let Some(parent) = socket_path.parent() {
        if !parent.exists() {
            println!("Creating socket directory: {}", parent.display());
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("Error creating socket directory: {e}");
                return Err(HelperError::PlatformService(format!(
                    "Failed to create socket directory: {e}"
                )));
            }
        }
    }

    Ok((pool_manager, network_manager, socket_path))
}

pub(crate) async fn run_communication_server(
    pool_manager: AddressPoolManager, network_manager: NetworkConfigManager,
    socket_path: std::path::PathBuf,
) -> Result<(), HelperError> {
    println!(
        "Starting communication server on socket: {}",
        socket_path.display()
    );

    let tokio_handle = tokio::runtime::Handle::current();
    println!(
        "Running with tokio runtime (worker threads: {:?})",
        tokio_handle.runtime_flavor()
    );

    let hostfile_manager = crate::hostfile::HostfileManager::new();

    match start_communication_server(
        socket_path.clone(),
        pool_manager,
        network_manager,
        hostfile_manager,
    )
    .await
    {
        Ok(_) => {
            println!("Communication server exited successfully");
            Ok(())
        }
        Err(e) => {
            eprintln!("Error in communication server: {e}");
            Err(e)
        }
    }
}
