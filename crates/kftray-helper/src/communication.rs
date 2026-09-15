#[cfg(unix)]
use std::os::unix::net::{
    UnixListener,
    UnixStream,
};
#[cfg(unix)]
use std::{
    fs,
    io::{
        Read,
        Write,
    },
};
use std::{
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

#[cfg(unix)]
use kftray_commons::utils::config_dir;
use log::{
    debug,
    error,
    info,
    warn,
};
#[cfg(windows)]
use tokio::net::windows::named_pipe::{
    NamedPipeServer,
    PipeMode,
    ServerOptions,
};
use tokio::{
    sync::mpsc,
    task,
};

#[cfg(unix)]
use crate::auth::validate_peer_credentials;
use crate::{
    address_pool::AddressPoolManager,
    auth::validate_request,
    error::HelperError,
    hostfile::HostfileManager,
    messages::{
        AddressCommand,
        HelperRequest,
        HelperResponse,
        HostCommand,
        NetworkCommand,
        RequestCommand,
        ServiceCommand,
    },
    network::NetworkConfigManager,
};

#[cfg(target_os = "linux")]
pub const SOCKET_FILENAME: &str = "kftray-helper.sock";

#[cfg(target_os = "macos")]
pub const SOCKET_FILENAME: &str = "com.hcavarsan.kftray.helper.sock";

#[cfg(target_os = "windows")]
pub const DEFAULT_NAMED_PIPE: &str = r"\\.\pipe\kftray-helper";

#[cfg(unix)]
fn is_running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(target_os = "macos")]
fn get_user_config_dir() -> Option<PathBuf> {
    if let Ok(socket_path) = std::env::var("SOCKET_PATH")
        && !socket_path.is_empty()
    {
        info!("Using explicit SOCKET_PATH from environment: {socket_path}");
        return Some(PathBuf::from(socket_path).parent()?.to_path_buf());
    }

    if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG")
        && !config_dir.is_empty()
    {
        info!("Using KFTRAY_CONFIG from environment: {config_dir}");
        return Some(PathBuf::from(config_dir));
    }

    if let Ok(config_dir) = std::env::var("CONFIG_DIR")
        && !config_dir.is_empty()
    {
        info!("Using CONFIG_DIR from environment: {config_dir}");
        return Some(PathBuf::from(config_dir));
    }

    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        if let Ok(user) = std::env::var("USER")
            && user != "root"
        {
            info!("Using home directory for user '{user}': {home}");
            let mut path = PathBuf::from(home);
            path.push(".kftray");
            return Some(path);
        }

        if home.starts_with("/Users/") || home.starts_with("/home/") {
            let mut path = PathBuf::from(home);
            path.push(".kftray");
            info!("Using user's home directory: {}", path.display());
            return Some(path);
        }

        if let Ok(sudo_user) = std::env::var("SUDO_USER")
            && !sudo_user.is_empty()
            && sudo_user != "root"
        {
            info!("Using SUDO_USER's home directory for: {sudo_user}");
            let user_home = if cfg!(target_os = "macos") {
                format!("/Users/{sudo_user}")
            } else {
                format!("/home/{sudo_user}")
            };
            let mut path = PathBuf::from(user_home);
            path.push(".kftray");
            return Some(path);
        }
    }

    None
}

pub fn get_default_socket_path() -> Result<PathBuf, HelperError> {
    #[cfg(target_os = "macos")]
    {
        debug!("Getting socket path from user config directory");

        let socket_path = if let Some(mut user_dir) = get_user_config_dir() {
            user_dir.push(SOCKET_FILENAME);
            user_dir
        } else {
            match config_dir::get_config_dir() {
                Ok(mut path) => {
                    path.push(SOCKET_FILENAME);
                    path
                }
                Err(_) => {
                    warn!("Could not get config directory, falling back to /tmp");
                    PathBuf::from(format!("/tmp/{SOCKET_FILENAME}"))
                }
            }
        };

        if let Some(parent) = socket_path.parent()
            && !parent.exists()
        {
            info!("Creating socket parent directory: {parent:?}");
            if let Err(e) = fs::create_dir_all(parent) {
                error!("Failed to create socket directory: {e}");
                return Err(HelperError::Communication(format!(
                    "Failed to create socket directory: {parent:?}, error: {e}"
                )));
            }

            #[cfg(unix)]
            if is_running_as_root()
                && let (Ok(user_id), Ok(group_id)) =
                    (std::env::var("SUDO_UID"), std::env::var("SUDO_GID"))
            {
                info!("Fixing directory ownership for socket directory");
                if let Err(e) = std::process::Command::new("chown")
                    .arg(format!("{user_id}:{group_id}"))
                    .arg(parent.as_os_str())
                    .status()
                {
                    warn!("Failed to fix directory ownership: {e}");
                }
            }
        }

        if let Some(parent) = socket_path.parent() {
            match parent.metadata() {
                Ok(_) => {
                    info!("Socket parent directory exists at: {}", parent.display());
                }
                Err(e) => {
                    warn!("Couldn't check socket parent directory: {e}");
                }
            }
        }

        info!("Using socket path: {}", socket_path.display());
        Ok(socket_path)
    }

    #[cfg(target_os = "linux")]
    {
        let socket_path = if let Ok(path) = std::env::var("SOCKET_PATH") {
            if !path.is_empty() {
                info!("Using explicit SOCKET_PATH from environment: {}", path);
                PathBuf::from(path)
            } else {
                find_linux_socket_path()
            }
        } else if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
            if !config_dir.is_empty() {
                info!("Using KFTRAY_CONFIG from environment: {}", config_dir);
                let mut path = PathBuf::from(config_dir);
                path.push(SOCKET_FILENAME);
                path
            } else {
                find_linux_socket_path()
            }
        } else if let Ok(config_dir) = std::env::var("CONFIG_DIR") {
            if !config_dir.is_empty() {
                info!("Using CONFIG_DIR from environment: {}", config_dir);
                let mut path = PathBuf::from(config_dir);
                path.push(SOCKET_FILENAME);
                path
            } else {
                find_linux_socket_path()
            }
        } else {
            find_linux_socket_path()
        };

        fn find_linux_socket_path() -> PathBuf {
            if let Ok(user) = std::env::var("USER")
                && user != "root"
                && let Ok(home) = std::env::var("HOME")
                && !home.is_empty()
                && (home.starts_with("/home/") || home.starts_with("/Users/"))
            {
                info!("Using home directory for user '{}': {}", user, home);
                let mut path = PathBuf::from(home);
                path.push(".kftray");
                path.push(SOCKET_FILENAME);
                return path;
            }

            if is_running_as_root()
                && let Ok(sudo_user) = std::env::var("SUDO_USER")
                && !sudo_user.is_empty()
                && sudo_user != "root"
            {
                info!("Using SUDO_USER's home directory for: {}", sudo_user);
                let user_home = format!("/home/{}", sudo_user);
                let mut path = PathBuf::from(user_home);
                path.push(".kftray");
                path.push(SOCKET_FILENAME);
                return path;
            }

            if let Ok(home) = std::env::var("HOME")
                && !home.is_empty()
                && (home.starts_with("/home/") || home.starts_with("/Users/"))
            {
                info!("Using home directory: {}", home);
                let mut path = PathBuf::from(home);
                path.push(".kftray");
                path.push(SOCKET_FILENAME);
                return path;
            }

            match config_dir::get_config_dir() {
                Ok(mut path) => {
                    path.push(SOCKET_FILENAME);
                    path
                }
                Err(_) => PathBuf::from(format!("/tmp/{}", SOCKET_FILENAME)),
            }
        }

        if let Some(parent) = socket_path.parent()
            && !parent.exists()
        {
            info!("Creating socket parent directory: {:?}", parent);
            if let Err(e) = fs::create_dir_all(parent) {
                error!("Failed to create socket directory: {}", e);
                return Err(HelperError::Communication(format!(
                    "Failed to create socket directory: {:?}, error: {}",
                    parent, e
                )));
            }

            if is_running_as_root()
                && let (Ok(user_id), Ok(group_id)) =
                    (std::env::var("SUDO_UID"), std::env::var("SUDO_GID"))
            {
                info!("Fixing directory ownership for socket directory");
                if let Err(e) = std::process::Command::new("chown")
                    .arg(format!("{}:{}", user_id, group_id))
                    .arg(parent.as_os_str())
                    .status()
                {
                    warn!("Failed to fix directory ownership: {}", e);
                }
            }
        }

        info!("Using socket path: {}", socket_path.display());
        Ok(socket_path)
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
    hostfile_manager: HostfileManager,
) -> Result<(), HelperError> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        start_unix_socket_server(socket_path, pool_manager, network_manager, hostfile_manager).await
    }

    #[cfg(target_os = "windows")]
    {
        start_named_pipe_server(socket_path, pool_manager, network_manager, hostfile_manager).await
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(HelperError::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn start_unix_socket_server(
    socket_path: PathBuf, pool_manager: AddressPoolManager, network_manager: NetworkConfigManager,
    hostfile_manager: HostfileManager,
) -> Result<(), HelperError> {
    info!("Starting Unix socket server on: {}", socket_path.display());

    if socket_path.exists() {
        info!("Removing existing socket file");
        fs::remove_file(&socket_path).map_err(|e| {
            HelperError::Communication(format!("Failed to remove existing socket: {e}"))
        })?;
    }

    if let Some(parent) = socket_path.parent() {
        if !parent.exists() {
            info!("Creating socket directory: {}", parent.display());
            fs::create_dir_all(parent).map_err(|e| {
                HelperError::Communication(format!("Failed to create socket directory: {e}"))
            })?;
        }

        #[cfg(unix)]
        {
            if is_running_as_root() {
                debug!("Running as root, proceeding with socket creation");
            } else if !parent.exists() {
                info!("Creating socket directory: {}", parent.display());
                if let Err(e) = fs::create_dir_all(parent) {
                    warn!("Couldn't create socket directory: {e}");
                }
            }
        }
    }

    let listener = UnixListener::bind(&socket_path)
        .map_err(|e| HelperError::Communication(format!("Failed to bind Unix socket: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o666)) {
            warn!("Failed to set socket permissions: {e}");
        } else {
            info!("Set socket permissions to 666 (rw-rw-rw-)");
        }

        if is_running_as_root()
            && let (Ok(user_id), Ok(group_id)) =
                (std::env::var("SUDO_UID"), std::env::var("SUDO_GID"))
        {
            info!("Fixing socket ownership for user access");
            match std::process::Command::new("chown")
                .arg(format!("{user_id}:{group_id}"))
                .arg(&socket_path)
                .status()
            {
                Err(e) => {
                    warn!("Failed to fix socket ownership: {e}");
                }
                _ => {
                    info!("Set socket ownership to {user_id}:{group_id}");
                }
            }
        }

        if socket_path.exists() {
            info!("Socket file exists at: {}", socket_path.display());
        }
    }

    info!(
        "Unix socket bound successfully at: {}",
        socket_path.display()
    );

    let pool_manager = Arc::new(pool_manager);
    let network_manager = Arc::new(network_manager);
    let hostfile_manager = Arc::new(hostfile_manager);

    let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    let socket_path_clone = socket_path.clone();
    let listener_task = task::spawn(async move {
        info!("Listening on Unix socket: {socket_path_clone:?}");

        listener.set_nonblocking(true).map_err(|e| {
            HelperError::Communication(format!("Failed to set non-blocking mode: {e}"))
        })?;

        loop {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    let pool_manager = Arc::clone(&pool_manager);
                    let network_manager = Arc::clone(&network_manager);
                    let hostfile_manager = Arc::clone(&hostfile_manager);

                    task::spawn(async move {
                        if let Err(e) = handle_connection(
                            stream,
                            pool_manager,
                            network_manager,
                            hostfile_manager,
                        )
                        .await
                        {
                            error!("Error handling connection: {e}");
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => {
                    return Err(HelperError::Communication(format!(
                        "Error accepting connection: {e}"
                    )));
                }
            }

            if shutdown_rx.try_recv().is_ok() {
                info!("Shutting down Unix socket server");
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
            "Listener task panicked: {e}"
        ))),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn handle_connection(
    mut stream: UnixStream, pool_manager: Arc<AddressPoolManager>,
    network_manager: Arc<NetworkConfigManager>, hostfile_manager: Arc<HostfileManager>,
) -> Result<(), HelperError> {
    info!("New connection received");

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if let Err(e) = validate_peer_credentials(&stream) {
            error!("Peer credential validation failed: {e}");
            return Err(e);
        }
        debug!("Peer credentials validated successfully");
    }

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| HelperError::Communication(format!("Failed to set socket timeout: {e}")))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| {
            warn!("Failed to set write timeout: {e}");
            HelperError::Communication(format!("Failed to set socket write timeout: {e}"))
        })?;

    let mut buffer = Vec::new();
    let mut tmp_buf = [0u8; 4096];
    let mut waited_for_first_byte = false;

    // Completeness is judged by whether the buffer parses, not by the size
    // of a single read: a message can arrive in reads of any size, and a
    // large batch legitimately spans more than one.
    let request = loop {
        match stream.read(&mut tmp_buf) {
            Ok(0) => {
                if buffer.is_empty() {
                    info!("Client closed connection (0 bytes read)");
                    return Ok(());
                }
                debug!(
                    "Client closed connection after sending {} bytes",
                    buffer.len()
                );
                return respond_with_parse_error(&mut stream, &buffer);
            }
            Ok(n) => {
                debug!("Read {n} bytes from client");
                buffer.extend_from_slice(&tmp_buf[..n]);

                if let Ok(req) = serde_json::from_slice::<HelperRequest>(&buffer) {
                    debug!("Request parsed successfully");
                    break req;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                debug!("Socket would block, waiting briefly");
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                debug!("Socket read interrupted, continuing");
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                if buffer.is_empty() && !waited_for_first_byte {
                    waited_for_first_byte = true;
                    debug!("No data yet, waiting briefly for a slow client");
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                debug!("Socket read timed out, ending read loop");
                return respond_with_parse_error(&mut stream, &buffer);
            }
            Err(e) => {
                error!("Error reading from client: {e}");
                return Err(HelperError::Communication(format!(
                    "Failed to read from socket: {e}"
                )));
            }
        }
    };

    if let Err(e) = validate_request(&request) {
        error!("Request validation failed: {e}");
        return Err(e);
    }
    debug!("Request validation passed");

    debug!("Processing request...");
    let response =
        process_request(request, pool_manager, network_manager, hostfile_manager).await?;
    debug!("Request processed successfully");

    let response_bytes = match serde_json::to_vec(&response) {
        Ok(bytes) => {
            debug!(
                "Response serialized ({} bytes), result={:?}",
                bytes.len(),
                response.result
            );
            bytes
        }
        Err(e) => {
            error!("Failed to serialize response: {e}");
            return Err(HelperError::Communication(format!(
                "Failed to serialize response: {e}"
            )));
        }
    };

    debug!(
        "Writing response directly to client socket ({} bytes)",
        response_bytes.len()
    );
    match stream.write_all(&response_bytes) {
        Ok(_) => debug!("Response written successfully"),
        Err(e) => {
            error!("Failed to write response: {e}");
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                info!("Client disconnected (broken pipe), ignoring error");
                return Ok(());
            }
            return Err(HelperError::Communication(format!(
                "Failed to write response: {e}"
            )));
        }
    }

    debug!("Flushing socket output");
    match stream.flush() {
        Ok(_) => debug!("Response flushed successfully"),
        Err(e) => {
            error!("Failed to flush response: {e}");
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                info!("Client disconnected (broken pipe), ignoring error");
                return Ok(());
            }
            return Err(HelperError::Communication(format!(
                "Failed to flush response: {e}"
            )));
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(100));

    info!("Connection handled successfully");
    Ok(())
}

/// Writes an error response for a request that could not be parsed, so an
/// old or misbehaving client fails fast instead of waiting out its timeout.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn respond_with_parse_error(stream: &mut UnixStream, buffer: &[u8]) -> Result<(), HelperError> {
    let message = match serde_json::from_slice::<HelperRequest>(buffer) {
        Ok(_) => "Incomplete request".to_string(),
        Err(e) => format!("Failed to parse request: {e}"),
    };
    error!("{message}");

    match serde_json::to_vec(&HelperResponse::error(String::new(), message)) {
        Ok(bytes) => {
            if let Err(e) = stream.write_all(&bytes) {
                warn!("Failed to write parse-error response: {e}");
            } else if let Err(e) = stream.flush() {
                warn!("Failed to flush parse-error response: {e}");
            }
        }
        Err(e) => warn!("Failed to serialize parse-error response: {e}"),
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn create_secure_pipe(pipe_name: &str) -> Result<NamedPipeServer, std::io::Error> {
    ServerOptions::new()
        .pipe_mode(PipeMode::Byte)
        .access_inbound(true)
        .access_outbound(true)
        .create(pipe_name)
}

#[cfg(target_os = "windows")]
async fn start_named_pipe_server(
    _pipe_path: PathBuf, _pool_manager: AddressPoolManager, _network_manager: NetworkConfigManager,
    _hostfile_manager: HostfileManager,
) -> Result<(), HelperError> {
    info!(
        "Starting Windows named pipe server on: {}",
        _pipe_path.display()
    );

    let pipe_name = _pipe_path.to_string_lossy();

    let pool_manager = Arc::new(_pool_manager);
    let network_manager = Arc::new(_network_manager);
    let hostfile_manager = Arc::new(_hostfile_manager);

    let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    let pipe_name_clone = pipe_name.to_string();
    let listener_task = task::spawn(async move {
        info!("Listening on Windows named pipe: {}", pipe_name_clone);

        loop {
            let pipe = match create_secure_pipe(&pipe_name_clone) {
                Ok(pipe) => pipe,
                Err(e) => {
                    error!("Failed to create named pipe: {}", e);
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    continue;
                }
            };

            debug!("Named pipe created, waiting for connection");

            match pipe.connect().await {
                Ok(()) => {
                    info!("Client connected to named pipe");
                    let pool_manager_clone = Arc::clone(&pool_manager);
                    let network_manager_clone = Arc::clone(&network_manager);
                    let hostfile_manager_clone = Arc::clone(&hostfile_manager);

                    task::spawn(async move {
                        if let Err(e) = handle_windows_connection(
                            pipe,
                            pool_manager_clone,
                            network_manager_clone,
                            hostfile_manager_clone,
                        )
                        .await
                        {
                            error!("Error handling Windows pipe connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to connect client to named pipe: {}", e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }

            if shutdown_rx.try_recv().is_ok() {
                info!("Shutting down Windows named pipe server");
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
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

#[cfg(target_os = "windows")]
async fn handle_windows_connection(
    mut pipe: NamedPipeServer, pool_manager: Arc<AddressPoolManager>,
    network_manager: Arc<NetworkConfigManager>, hostfile_manager: Arc<HostfileManager>,
) -> Result<(), HelperError> {
    use tokio::io::{
        AsyncReadExt,
        AsyncWriteExt,
    };
    info!("New connection received on Windows named pipe");

    let mut buffer = Vec::new();
    let mut tmp_buf = [0u8; 4096];
    let mut waited_for_first_byte = false;

    let timeout = Duration::from_secs(30);
    let start_time = std::time::Instant::now();

    // Completeness is judged by whether the buffer parses, not by the size
    // of a single read: a message can arrive in reads of any size, and a
    // large batch legitimately spans more than one.
    let request = loop {
        if start_time.elapsed() > timeout {
            warn!(
                "Read operation timed out after {} seconds",
                timeout.as_secs()
            );
            return respond_with_parse_error(&mut pipe, &buffer).await;
        }

        match tokio::time::timeout(Duration::from_secs(5), pipe.read(&mut tmp_buf)).await {
            Ok(read_result) => match read_result {
                Ok(0) => {
                    if buffer.is_empty() {
                        info!("Client closed connection (0 bytes read)");
                        return Ok(());
                    }
                    debug!(
                        "Client closed connection after sending {} bytes",
                        buffer.len()
                    );
                    return respond_with_parse_error(&mut pipe, &buffer).await;
                }
                Ok(n) => {
                    debug!("Read {} bytes from client", n);
                    buffer.extend_from_slice(&tmp_buf[..n]);

                    if let Ok(req) = serde_json::from_slice::<HelperRequest>(&buffer) {
                        debug!("Request parsed successfully");
                        break req;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    debug!("Pipe would block, waiting briefly");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    debug!("Pipe read interrupted, continuing");
                }
                Err(e) => {
                    error!("Error reading from client: {}", e);
                    if buffer.is_empty() {
                        return Err(HelperError::Communication(format!(
                            "Failed to read from pipe: {}",
                            e
                        )));
                    }
                    return respond_with_parse_error(&mut pipe, &buffer).await;
                }
            },
            Err(_) => {
                debug!("Read operation timed out");
                if buffer.is_empty() && !waited_for_first_byte {
                    waited_for_first_byte = true;
                    debug!("No data yet, waiting briefly for a slow client");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                debug!("Read timed out, ending read loop");
                return respond_with_parse_error(&mut pipe, &buffer).await;
            }
        }
    };

    if let Err(e) = validate_request(&request) {
        error!("Request validation failed: {}", e);
        return Err(e);
    }
    debug!("Request validation passed");

    debug!("Processing request...");
    let response =
        process_request(request, pool_manager, network_manager, hostfile_manager).await?;
    debug!("Request processed successfully");

    let response_bytes = match serde_json::to_vec(&response) {
        Ok(bytes) => {
            debug!(
                "Response serialized ({} bytes), result={:?}",
                bytes.len(),
                response.result
            );
            bytes
        }
        Err(e) => {
            error!("Failed to serialize response: {}", e);
            return Err(HelperError::Communication(format!(
                "Failed to serialize response: {}",
                e
            )));
        }
    };

    debug!(
        "Writing response directly to client pipe ({} bytes)",
        response_bytes.len()
    );

    match pipe.write_all(&response_bytes).await {
        Ok(_) => debug!("Response written successfully"),
        Err(e) => {
            error!("Failed to write response: {}", e);
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                info!("Client disconnected (broken pipe), ignoring error");
                return Ok(());
            }
            return Err(HelperError::Communication(format!(
                "Failed to write response: {}",
                e
            )));
        }
    }

    debug!("Flushing pipe output");
    match pipe.flush().await {
        Ok(_) => debug!("Response flushed successfully"),
        Err(e) => {
            error!("Failed to flush response: {}", e);
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                info!("Client disconnected (broken pipe), ignoring error");
                return Ok(());
            }
            return Err(HelperError::Communication(format!(
                "Failed to flush response: {}",
                e
            )));
        }
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    info!("Connection handled successfully");
    Ok(())
}

/// Writes an error response for a request that could not be parsed, so an
/// old or misbehaving client fails fast instead of waiting out its timeout.
#[cfg(target_os = "windows")]
async fn respond_with_parse_error(
    pipe: &mut NamedPipeServer, buffer: &[u8],
) -> Result<(), HelperError> {
    use tokio::io::AsyncWriteExt;

    let message = match serde_json::from_slice::<HelperRequest>(buffer) {
        Ok(_) => "Incomplete request".to_string(),
        Err(e) => format!("Failed to parse request: {}", e),
    };
    error!("{}", message);

    match serde_json::to_vec(&HelperResponse::error(String::new(), message)) {
        Ok(bytes) => {
            if let Err(e) = pipe.write_all(&bytes).await {
                warn!("Failed to write parse-error response: {}", e);
            } else if let Err(e) = pipe.flush().await {
                warn!("Failed to flush parse-error response: {}", e);
            }
        }
        Err(e) => warn!("Failed to serialize parse-error response: {}", e),
    }
    Ok(())
}

async fn process_request(
    request: HelperRequest, pool_manager: Arc<AddressPoolManager>,
    network_manager: Arc<NetworkConfigManager>, hostfile_manager: Arc<HostfileManager>,
) -> Result<HelperResponse, HelperError> {
    let request_id = request.request_id.clone();

    debug!("Processing request: {:?}", request.command);

    match request.command {
        RequestCommand::Network(cmd) => match cmd {
            NetworkCommand::Add { address } => {
                debug!("Processing Add request for address: {address}");
                match network_manager.add_loopback_address(&address).await {
                    Ok(_) => {
                        info!("Add request successful for address: {address}");
                        Ok(HelperResponse::success(request_id))
                    }
                    Err(e) => {
                        error!("Add request failed for address {address}: {e}");
                        Ok(HelperResponse::error(request_id, format!("Error: {e}")))
                    }
                }
            }
            NetworkCommand::Remove { address } => {
                debug!("Processing Remove request for address: {address}");

                let result = match network_manager.remove_loopback_address(&address).await {
                    Ok(_) => {
                        info!("Remove request successful for address: {address}");
                        info!("Successfully removed loopback address: {address}");
                        HelperResponse::success(request_id.clone())
                    }
                    Err(e) => {
                        error!("Remove request failed for address {address}: {e}");

                        if e.to_string().contains("not found")
                            || e.to_string().contains("No such process")
                        {
                            info!("Address already removed, considering operation successful");
                            HelperResponse::success(request_id.clone())
                        } else {
                            debug!("Returning error response for failed removal");
                            HelperResponse::error(request_id.clone(), format!("Error: {e}"))
                        }
                    }
                };

                debug!(
                    "Prepared response for address removal: result={:?}",
                    result.result
                );

                match serde_json::to_vec(&result) {
                    Ok(bytes) => {
                        debug!("Serialized response: {} bytes", bytes.len());
                        Ok(result)
                    }
                    Err(e) => {
                        error!("Failed to serialize response: {e}");
                        Ok(HelperResponse::error(
                            request_id.clone(),
                            format!("Error serializing response: {e}"),
                        ))
                    }
                }
            }
            NetworkCommand::List => {
                debug!("Processing List request");
                match network_manager.list_loopback_addresses().await {
                    Ok(addresses) => {
                        info!(
                            "List request successful, found {} addresses",
                            addresses.len()
                        );
                        Ok(HelperResponse::list_success(request_id, addresses))
                    }
                    Err(e) => {
                        error!("List request failed: {e}");
                        Ok(HelperResponse::list_success(request_id, vec![]))
                    }
                }
            }
        },
        RequestCommand::Address(cmd) => match cmd {
            AddressCommand::Allocate { service_name } => {
                debug!("Processing Allocate request for service: {service_name}");
                match pool_manager.allocate_address(&service_name).await {
                    Ok(address) => {
                        info!(
                            "Address pool allocation successful for service {service_name}: {address}"
                        );

                        match network_manager.add_loopback_address(&address).await {
                            Ok(_) => {
                                info!(
                                    "Network interface addition successful for address: {address}"
                                );
                                Ok(HelperResponse::string_success(request_id, address))
                            }
                            Err(e) => {
                                error!(
                                    "Network interface addition failed for address {address}: {e}"
                                );
                                if let Err(release_err) =
                                    pool_manager.release_address(&address).await
                                {
                                    warn!(
                                        "Failed to release address from pool after network error: {release_err}"
                                    );
                                }
                                Ok(HelperResponse::string_success(
                                    request_id,
                                    "127.0.0.1".to_string(),
                                ))
                            }
                        }
                    }
                    Err(e) => {
                        error!("Address pool allocation failed for service {service_name}: {e}");

                        Ok(HelperResponse::string_success(
                            request_id,
                            "127.0.0.1".to_string(),
                        ))
                    }
                }
            }
            AddressCommand::Release { address } => {
                debug!("Processing Release request for address: {address}");

                let pool_result = pool_manager.release_address(&address).await;
                match pool_result {
                    Ok(_) => info!("Address pool release successful for address: {address}"),
                    Err(e) => error!("Address pool release failed for address {address}: {e}"),
                }

                let network_result = network_manager.remove_loopback_address(&address).await;
                match network_result {
                    Ok(_) => {
                        info!("Network interface removal successful for address: {address}");
                        Ok(HelperResponse::success(request_id))
                    }
                    Err(e) => {
                        error!("Network interface removal failed for address {address}: {e}");

                        if e.to_string().contains("not found")
                            || e.to_string().contains("No such process")
                        {
                            info!(
                                "Address already removed from interface, considering operation successful"
                            );
                            Ok(HelperResponse::success(request_id))
                        } else {
                            debug!("Returning error response for failed network removal");
                            Ok(HelperResponse::error(request_id, format!("Error: {e}")))
                        }
                    }
                }
            }
            AddressCommand::List => {
                debug!("Processing List request");
                match pool_manager.list_allocations().await {
                    Ok(allocations) => {
                        info!(
                            "List request successful, found {} allocations",
                            allocations.len()
                        );
                        Ok(HelperResponse::allocations_success(request_id, allocations))
                    }
                    Err(e) => {
                        error!("List request failed: {e}");

                        Ok(HelperResponse::allocations_success(request_id, vec![]))
                    }
                }
            }
        },
        RequestCommand::Service(cmd) => match cmd {
            ServiceCommand::Status => {
                debug!("Processing Status request");

                Ok(HelperResponse::string_success(request_id, "running".into()))
            }
            ServiceCommand::Stop => {
                debug!("Processing Stop request");
                info!("Received stop command, shutting down helper service");

                // Send success response before shutting down
                let response = HelperResponse::success(request_id);

                // Schedule shutdown after a short delay to allow response to be sent
                tokio::spawn(async {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    info!("Shutting down helper service");
                    std::process::exit(0);
                });

                Ok(response)
            }
            ServiceCommand::Restart => {
                debug!("Processing Restart request");
                Ok(HelperResponse::success(request_id))
            }
        },
        // Off the runtime: a hosts transaction waits on the cross-process
        // lock, up to its timeout, and a worker parked on it would delay every
        // other request the helper is serving.
        RequestCommand::Host(cmd) => {
            let hostfile_manager = Arc::clone(&hostfile_manager);
            let host_request_id = request_id.clone();
            match tokio::task::spawn_blocking(move || {
                handle_host_command(cmd, &hostfile_manager, request_id)
            })
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    error!("Hosts task failed: {error}");
                    Ok(HelperResponse::error(
                        host_request_id,
                        format!("Hosts task failed: {error}"),
                    ))
                }
            }
        }
        RequestCommand::Ping => {
            debug!("Processing Ping request");
            Ok(HelperResponse::string_success(request_id, "pong".into()))
        }
    }
}

/// Turns a hosts-manager result into a logged response: success or error,
/// never a bare `Err` that would leave the caller without a reply.
fn reply<T, E: std::fmt::Display>(
    request_id: &str, what: &str, result: Result<T, E>,
    on_success: impl FnOnce(String, T) -> HelperResponse,
) -> Result<HelperResponse, HelperError> {
    match result {
        Ok(value) => {
            info!("Host {what} request successful");
            Ok(on_success(request_id.to_string(), value))
        }
        Err(e) => {
            error!("Host {what} request failed: {e}");
            Ok(HelperResponse::error(
                request_id.to_string(),
                format!("Error: {e}"),
            ))
        }
    }
}

fn handle_host_command(
    cmd: HostCommand, hostfile_manager: &HostfileManager, request_id: String,
) -> Result<HelperResponse, HelperError> {
    match cmd {
        HostCommand::Add { id, entry } => {
            debug!("Processing Host Add request for ID: {id}");
            reply(
                &request_id,
                "Add",
                hostfile_manager.add_entry(id, entry),
                |rid, _| HelperResponse::success(rid),
            )
        }
        HostCommand::Remove { id } => {
            debug!("Processing Host Remove request for ID: {id}");
            reply(
                &request_id,
                "Remove",
                hostfile_manager.remove_entry(&id),
                |rid, _| HelperResponse::success(rid),
            )
        }
        HostCommand::RemoveUnowned { entries } => {
            debug!(
                "Processing Host RemoveUnowned request for {} entries",
                entries.len()
            );
            reply(
                &request_id,
                "RemoveUnowned",
                hostfile_manager.remove_unowned_matching(&entries),
                |rid, _| HelperResponse::success(rid),
            )
        }
        HostCommand::RemoveDirectOwned { ids, legacy } => {
            debug!("Processing Host RemoveDirectOwned request for {ids:?}");
            reply(
                &request_id,
                "RemoveDirectOwned",
                hostfile_manager.remove_direct_owned(&ids, &legacy),
                |rid, _| HelperResponse::success(rid),
            )
        }
        HostCommand::RemoveAll => {
            debug!("Processing Host RemoveAll request");
            reply(
                &request_id,
                "RemoveAll",
                hostfile_manager.remove_all_entries(),
                |rid, _| HelperResponse::success(rid),
            )
        }
        HostCommand::List => {
            debug!("Processing Host List request");
            reply(
                &request_id,
                "List",
                hostfile_manager.list_entries(),
                |rid, entries| {
                    debug!("Host List request found {} entries", entries.len());
                    HelperResponse::host_entries_success(rid, entries)
                },
            )
        }
    }
}
