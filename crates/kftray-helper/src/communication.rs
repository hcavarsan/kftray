use std::{
    fs,
    io::{
        Read,
        Write,
    },
    os::unix::net::{
        UnixListener,
        UnixStream,
    },
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use tokio::{
    sync::mpsc,
    task,
};

use crate::{
    address_pool::AddressPoolManager,
    error::HelperError,
    messages::{
        AddressCommand,
        HelperRequest,
        HelperResponse,
        NetworkCommand,
        RequestCommand,
        ServiceCommand,
    },
    network::NetworkConfigManager,
};

#[cfg(any(target_os = "macos", target_os = "linux"))]
const DEFAULT_SOCKET_PATH_STR: &str = "/tmp/kftray-helper.sock";

#[cfg(target_os = "windows")]
const DEFAULT_NAMED_PIPE: &str = r"\\.\pipe\kftray-helper";

pub fn get_default_socket_path() -> Result<PathBuf, HelperError> {
    #[cfg(target_os = "macos")]
    {
        let socket_path = PathBuf::from(DEFAULT_SOCKET_PATH_STR);

        if let Some(parent) = socket_path.parent() {
            if !parent.exists() {
                println!(
                    "Warning: Socket parent directory doesn't exist: {:?}",
                    parent
                );
                return Err(HelperError::Communication(format!(
                    "Socket parent directory doesn't exist: {:?}",
                    parent
                )));
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                match parent.metadata() {
                    Ok(meta) => {
                        let perms = meta.permissions().mode();
                        if perms & 0o777 != 0o777 {
                            println!("Warning: Socket parent directory has restrictive permissions: {:o}", perms);
                        }
                    }
                    Err(e) => {
                        println!(
                            "Warning: Couldn't check socket parent directory permissions: {}",
                            e
                        );
                    }
                }
            }
        }

        println!("Using socket path: {}", socket_path.display());
        Ok(socket_path)
    }

    #[cfg(target_os = "linux")]
    {
        Ok(PathBuf::from(DEFAULT_SOCKET_PATH_STR))
    }

    #[cfg(target_os = "windows")]
    {
        Ok(PathBuf::from(DEFAULT_NAMED_PIPE))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(HelperError::UnsupportedPlatform)
    }
}

pub async fn start_communication_server(
    socket_path: PathBuf, pool_manager: AddressPoolManager, network_manager: NetworkConfigManager,
) -> Result<(), HelperError> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        start_unix_socket_server(socket_path, pool_manager, network_manager).await
    }

    #[cfg(target_os = "windows")]
    {
        start_named_pipe_server(socket_path, pool_manager, network_manager).await
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(HelperError::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn start_unix_socket_server(
    socket_path: PathBuf, pool_manager: AddressPoolManager, network_manager: NetworkConfigManager,
) -> Result<(), HelperError> {
    println!("Starting Unix socket server on: {}", socket_path.display());

    if socket_path.exists() {
        println!("Removing existing socket file");
        fs::remove_file(&socket_path).map_err(|e| {
            HelperError::Communication(format!("Failed to remove existing socket: {}", e))
        })?;
    }

    if let Some(parent) = socket_path.parent() {
        if !parent.exists() {
            println!("Creating socket directory: {}", parent.display());
            fs::create_dir_all(parent).map_err(|e| {
                HelperError::Communication(format!("Failed to create socket directory: {}", e))
            })?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            println!("Attempting to set socket directory permissions");

            if let Err(e) = fs::set_permissions(parent, fs::Permissions::from_mode(0o777)) {
                println!(
                    "Warning: Couldn't set directory permissions (continuing anyway): {}",
                    e
                );
            } else {
                println!("Successfully set socket directory permissions");
            }
        }
    }

    let listener = UnixListener::bind(&socket_path)
        .map_err(|e| HelperError::Communication(format!("Failed to bind Unix socket: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        println!("Attempting to set socket permissions");

        if let Err(e) = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o777)) {
            println!(
                "Warning: Couldn't set socket permissions (continuing anyway): {}",
                e
            );
        } else {
            println!("Successfully set socket permissions");
        }
    }

    println!(
        "Unix socket bound successfully at: {}",
        socket_path.display()
    );

    let pool_manager = Arc::new(pool_manager);
    let network_manager = Arc::new(network_manager);

    let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    let socket_path_clone = socket_path.clone();
    let listener_task = task::spawn(async move {
        println!("Listening on Unix socket: {:?}", socket_path_clone);

        listener.set_nonblocking(true).map_err(|e| {
            HelperError::Communication(format!("Failed to set non-blocking mode: {}", e))
        })?;

        loop {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    let pool_manager = Arc::clone(&pool_manager);
                    let network_manager = Arc::clone(&network_manager);

                    task::spawn(async move {
                        if let Err(e) =
                            handle_connection(stream, pool_manager, network_manager).await
                        {
                            eprintln!("Error handling connection: {}", e);
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => {
                    return Err(HelperError::Communication(format!(
                        "Error accepting connection: {}",
                        e
                    )));
                }
            }

            if shutdown_rx.try_recv().is_ok() {
                println!("Shutting down Unix socket server");
                break;
            }
        }

        if socket_path_clone.exists() {
            fs::remove_file(&socket_path_clone).ok();
        }

        Ok::<(), HelperError>(())
    });

    match listener_task.await {
        Ok(result) => result,
        Err(e) => Err(HelperError::Communication(format!(
            "Listener task panicked: {}",
            e
        ))),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn handle_connection(
    mut stream: UnixStream, pool_manager: Arc<AddressPoolManager>,
    network_manager: Arc<NetworkConfigManager>,
) -> Result<(), HelperError> {
    println!("New connection received");

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| HelperError::Communication(format!("Failed to set socket timeout: {}", e)))?;

    let mut buffer = Vec::new();
    let mut tmp_buf = [0u8; 4096];

    loop {
        match stream.read(&mut tmp_buf) {
            Ok(0) => {
                println!("Client closed connection (0 bytes read)");
                break;
            }
            Ok(n) => {
                println!("Read {} bytes from client", n);
                buffer.extend_from_slice(&tmp_buf[..n]);

                if n < tmp_buf.len() {
                    println!("Read less than buffer size, assuming message is complete");
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                println!("Socket would block, waiting briefly");
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                println!("Socket read interrupted, continuing");
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                println!("Socket read timed out, ending read loop");
                break;
            }
            Err(e) => {
                println!("Error reading from client: {}", e);
                return Err(HelperError::Communication(format!(
                    "Failed to read from socket: {}",
                    e
                )));
            }
        }
    }

    if buffer.is_empty() {
        println!("Empty request received, will wait for more data");
        std::thread::sleep(std::time::Duration::from_millis(500));

        match stream.read(&mut tmp_buf) {
            Ok(0) => {
                println!("Client still sent 0 bytes, closing connection");
                return Ok(());
            }
            Ok(n) => {
                println!("Read {} bytes from client after wait", n);
                buffer.extend_from_slice(&tmp_buf[..n]);
            }
            Err(e) => {
                println!("Error reading more data: {}", e);
                return Ok(());
            }
        }

        if buffer.is_empty() {
            println!("Request is still empty after retry, cannot process");
            return Ok(());
        }
    }

    let request = match serde_json::from_slice::<HelperRequest>(&buffer) {
        Ok(req) => {
            println!("Request parsed successfully");
            req
        }
        Err(e) => {
            println!("Failed to parse request: {}", e);
            return Err(HelperError::Communication(format!(
                "Failed to parse request: {}",
                e
            )));
        }
    };

    println!("Processing request...");
    let response = process_request(request, pool_manager, network_manager).await?;
    println!("Request processed successfully");

    let response_bytes = match serde_json::to_vec(&response) {
        Ok(bytes) => {
            println!(
                "Response serialized ({} bytes), result={:?}",
                bytes.len(),
                response.result
            );
            bytes
        }
        Err(e) => {
            println!("Failed to serialize response: {}", e);
            return Err(HelperError::Communication(format!(
                "Failed to serialize response: {}",
                e
            )));
        }
    };

    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| {
            println!("Failed to set write timeout: {}", e);
            HelperError::Communication(format!("Failed to set socket write timeout: {}", e))
        })?;

    println!(
        "Writing response directly to client socket ({} bytes)",
        response_bytes.len()
    );
    match stream.write_all(&response_bytes) {
        Ok(_) => println!("Response written successfully"),
        Err(e) => {
            println!("Failed to write response: {}", e);
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                println!("Client disconnected (broken pipe), ignoring error");
                return Ok(());
            }
            return Err(HelperError::Communication(format!(
                "Failed to write response: {}",
                e
            )));
        }
    }

    println!("Flushing socket output");
    match stream.flush() {
        Ok(_) => println!("Response flushed successfully"),
        Err(e) => {
            println!("Failed to flush response: {}", e);
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                println!("Client disconnected (broken pipe), ignoring error");
                return Ok(());
            }
            return Err(HelperError::Communication(format!(
                "Failed to flush response: {}",
                e
            )));
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(100));

    println!("Connection handled successfully");
    Ok(())
}

#[cfg(target_os = "windows")]
async fn start_named_pipe_server(
    pipe_path: PathBuf, pool_manager: AddressPoolManager, network_manager: NetworkConfigManager,
) -> Result<(), HelperError> {
    Err(HelperError::Communication(
        "Windows named pipe implementation not yet available".into(),
    ))
}

async fn process_request(
    request: HelperRequest, pool_manager: Arc<AddressPoolManager>,
    network_manager: Arc<NetworkConfigManager>,
) -> Result<HelperResponse, HelperError> {
    let request_id = request.request_id.clone();

    println!("Processing request: {:?}", request.command);

    match request.command {
        RequestCommand::Network(cmd) => match cmd {
            NetworkCommand::Add { address } => {
                println!("Processing Add request for address: {}", address);
                match network_manager.add_loopback_address(&address).await {
                    Ok(_) => {
                        println!("Add request successful for address: {}", address);
                        Ok(HelperResponse::success(request_id))
                    }
                    Err(e) => {
                        println!("Add request failed for address {}: {}", address, e);
                        Ok(HelperResponse::error(request_id, format!("Error: {}", e)))
                    }
                }
            }
            NetworkCommand::Remove { address } => {
                println!("Processing Remove request for address: {}", address);

                let result = match network_manager.remove_loopback_address(&address).await {
                    Ok(_) => {
                        println!("Remove request successful for address: {}", address);
                        println!("Successfully removed loopback address: {}", address);
                        HelperResponse::success(request_id.clone())
                    }
                    Err(e) => {
                        println!("Remove request failed for address {}: {}", address, e);

                        if e.to_string().contains("not found")
                            || e.to_string().contains("No such process")
                        {
                            println!("Address already removed, considering operation successful");
                            HelperResponse::success(request_id.clone())
                        } else {
                            println!("Returning error response for failed removal");
                            HelperResponse::error(request_id.clone(), format!("Error: {}", e))
                        }
                    }
                };

                println!(
                    "Prepared response for address removal: result={:?}",
                    result.result
                );

                match serde_json::to_vec(&result) {
                    Ok(bytes) => {
                        println!("Serialized response: {} bytes", bytes.len());
                        Ok(result)
                    }
                    Err(e) => {
                        println!("Failed to serialize response: {}", e);
                        Ok(HelperResponse::error(
                            request_id.clone(),
                            format!("Error serializing response: {}", e),
                        ))
                    }
                }
            }
            NetworkCommand::List => {
                println!("Processing List request");
                match network_manager.list_loopback_addresses().await {
                    Ok(addresses) => {
                        println!(
                            "List request successful, found {} addresses",
                            addresses.len()
                        );
                        Ok(HelperResponse::list_success(request_id, addresses))
                    }
                    Err(e) => {
                        println!("List request failed: {}", e);
                        Ok(HelperResponse::list_success(request_id, vec![]))
                    }
                }
            }
        },
        RequestCommand::Address(cmd) => match cmd {
            AddressCommand::Allocate { service_name } => {
                println!("Processing Allocate request for service: {}", service_name);
                match pool_manager.allocate_address(&service_name).await {
                    Ok(address) => {
                        println!(
                            "Allocate request successful for service {}: {}",
                            service_name, address
                        );
                        Ok(HelperResponse::string_success(request_id, address))
                    }
                    Err(e) => {
                        println!(
                            "Allocate request failed for service {}: {}",
                            service_name, e
                        );

                        Ok(HelperResponse::string_success(
                            request_id,
                            "127.0.0.1".to_string(),
                        ))
                    }
                }
            }
            AddressCommand::Release { address } => {
                println!("Processing Release request for address: {}", address);
                match pool_manager.release_address(&address).await {
                    Ok(_) => {
                        println!("Release request successful for address: {}", address);
                        Ok(HelperResponse::success(request_id))
                    }
                    Err(e) => {
                        println!("Release request failed for address {}: {}", address, e);

                        Ok(HelperResponse::success(request_id))
                    }
                }
            }
            AddressCommand::List => {
                println!("Processing List request");
                match pool_manager.list_allocations().await {
                    Ok(allocations) => {
                        println!(
                            "List request successful, found {} allocations",
                            allocations.len()
                        );
                        Ok(HelperResponse::allocations_success(request_id, allocations))
                    }
                    Err(e) => {
                        println!("List request failed: {}", e);

                        Ok(HelperResponse::allocations_success(request_id, vec![]))
                    }
                }
            }
        },
        RequestCommand::Service(cmd) => match cmd {
            ServiceCommand::Status => {
                println!("Processing Status request");

                Ok(HelperResponse::string_success(request_id, "running".into()))
            }
            ServiceCommand::Stop => {
                println!("Processing Stop request");
                Ok(HelperResponse::success(request_id))
            }
            ServiceCommand::Restart => {
                println!("Processing Restart request");
                Ok(HelperResponse::success(request_id))
            }
        },
        RequestCommand::Ping => {
            println!("Processing Ping request");
            Ok(HelperResponse::string_success(request_id, "pong".into()))
        }
    }
}
