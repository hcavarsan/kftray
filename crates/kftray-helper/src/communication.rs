#[cfg(unix)]
use std::os::unix::net::{
    UnixListener,
    UnixStream,
};
use std::{
    fs,
    io::{
        Read,
        Write,
    },
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use kftray_commons::utils::config_dir;
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

use crate::{
    address_pool::AddressPoolManager,
    auth::{
        validate_peer_credentials,
        validate_request,
    },
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
    if let Ok(socket_path) = std::env::var("SOCKET_PATH") {
        if !socket_path.is_empty() {
            println!("Using explicit SOCKET_PATH from environment: {socket_path}");
            return Some(PathBuf::from(socket_path).parent()?.to_path_buf());
        }
    }

    if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
        if !config_dir.is_empty() {
            println!("Using KFTRAY_CONFIG from environment: {config_dir}");
            return Some(PathBuf::from(config_dir));
        }
    }

    if let Ok(config_dir) = std::env::var("CONFIG_DIR") {
        if !config_dir.is_empty() {
            println!("Using CONFIG_DIR from environment: {config_dir}");
            return Some(PathBuf::from(config_dir));
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            if let Ok(user) = std::env::var("USER") {
                if user != "root" {
                    println!("Using home directory for user '{user}': {home}");
                    let mut path = PathBuf::from(home);
                    path.push(".kftray");
                    return Some(path);
                }
            }

            if home.starts_with("/Users/") || home.starts_with("/home/") {
                let mut path = PathBuf::from(home);
                path.push(".kftray");
                println!("Using user's home directory: {}", path.display());
                return Some(path);
            }

            if let Ok(sudo_user) = std::env::var("SUDO_USER") {
                if !sudo_user.is_empty() && sudo_user != "root" {
                    println!("Using SUDO_USER's home directory for: {sudo_user}");
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
        }
    }

    None
}

pub fn get_default_socket_path() -> Result<PathBuf, HelperError> {
    #[cfg(target_os = "macos")]
    {
        println!("Getting socket path from user config directory");

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
                    println!("WARNING: Could not get config directory, falling back to /tmp");
                    PathBuf::from(format!("/tmp/{SOCKET_FILENAME}"))
                }
            }
        };

        if let Some(parent) = socket_path.parent() {
            if !parent.exists() {
                println!("Creating socket parent directory: {parent:?}");
                if let Err(e) = std::fs::create_dir_all(parent) {
                    println!("Failed to create socket directory: {e}");
                    return Err(HelperError::Communication(format!(
                        "Failed to create socket directory: {parent:?}, error: {e}"
                    )));
                }

                #[cfg(unix)]
                if is_running_as_root() {
                    if let (Ok(user_id), Ok(group_id)) =
                        (std::env::var("SUDO_UID"), std::env::var("SUDO_GID"))
                    {
                        println!("Fixing directory ownership for socket directory");
                        if let Err(e) = std::process::Command::new("chown")
                            .arg(format!("{user_id}:{group_id}"))
                            .arg(parent.as_os_str())
                            .status()
                        {
                            println!("Failed to fix directory ownership: {e}");
                        }
                    }
                }
            }

            match parent.metadata() {
                Ok(_) => {
                    println!("Socket parent directory exists at: {}", parent.display());
                }
                Err(e) => {
                    println!("Warning: Couldn't check socket parent directory: {e}");
                }
            }
        }

        println!("Using socket path: {}", socket_path.display());
        Ok(socket_path)
    }

    #[cfg(target_os = "linux")]
    {
        let socket_path = if let Ok(path) = std::env::var("SOCKET_PATH") {
            if !path.is_empty() {
                println!("Using explicit SOCKET_PATH from environment: {}", path);
                PathBuf::from(path)
            } else {
                find_linux_socket_path()
            }
        } else if let Ok(config_dir) = std::env::var("KFTRAY_CONFIG") {
            if !config_dir.is_empty() {
                println!("Using KFTRAY_CONFIG from environment: {}", config_dir);
                let mut path = PathBuf::from(config_dir);
                path.push(SOCKET_FILENAME);
                path
            } else {
                find_linux_socket_path()
            }
        } else if let Ok(config_dir) = std::env::var("CONFIG_DIR") {
            if !config_dir.is_empty() {
                println!("Using CONFIG_DIR from environment: {}", config_dir);
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
            if let Ok(user) = std::env::var("USER") {
                if user != "root" {
                    if let Ok(home) = std::env::var("HOME") {
                        if !home.is_empty()
                            && (home.starts_with("/home/") || home.starts_with("/Users/"))
                        {
                            println!("Using home directory for user '{}': {}", user, home);
                            let mut path = PathBuf::from(home);
                            path.push(".kftray");
                            path.push(SOCKET_FILENAME);
                            return path;
                        }
                    }
                }
            }

            if is_running_as_root() {
                if let Ok(sudo_user) = std::env::var("SUDO_USER") {
                    if !sudo_user.is_empty() && sudo_user != "root" {
                        println!("Using SUDO_USER's home directory for: {}", sudo_user);
                        let user_home = format!("/home/{}", sudo_user);
                        let mut path = PathBuf::from(user_home);
                        path.push(".kftray");
                        path.push(SOCKET_FILENAME);
                        return path;
                    }
                }
            }

            if let Ok(home) = std::env::var("HOME") {
                if !home.is_empty() && (home.starts_with("/home/") || home.starts_with("/Users/")) {
                    println!("Using home directory: {}", home);
                    let mut path = PathBuf::from(home);
                    path.push(".kftray");
                    path.push(SOCKET_FILENAME);
                    return path;
                }
            }

            match config_dir::get_config_dir() {
                Ok(mut path) => {
                    path.push(SOCKET_FILENAME);
                    path
                }
                Err(_) => PathBuf::from(format!("/tmp/{}", SOCKET_FILENAME)),
            }
        }

        if let Some(parent) = socket_path.parent() {
            if !parent.exists() {
                println!("Creating socket parent directory: {:?}", parent);
                if let Err(e) = std::fs::create_dir_all(parent) {
                    println!("Failed to create socket directory: {}", e);
                    return Err(HelperError::Communication(format!(
                        "Failed to create socket directory: {:?}, error: {}",
                        parent, e
                    )));
                }

                if is_running_as_root() {
                    if let (Ok(user_id), Ok(group_id)) =
                        (std::env::var("SUDO_UID"), std::env::var("SUDO_GID"))
                    {
                        println!("Fixing directory ownership for socket directory");
                        if let Err(e) = std::process::Command::new("chown")
                            .arg(format!("{}:{}", user_id, group_id))
                            .arg(parent.as_os_str())
                            .status()
                        {
                            println!("Failed to fix directory ownership: {}", e);
                        }
                    }
                }
            }
        }

        println!("Using socket path: {}", socket_path.display());
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
            HelperError::Communication(format!("Failed to remove existing socket: {e}"))
        })?;
    }

    if let Some(parent) = socket_path.parent() {
        if !parent.exists() {
            println!("Creating socket directory: {}", parent.display());
            fs::create_dir_all(parent).map_err(|e| {
                HelperError::Communication(format!("Failed to create socket directory: {e}"))
            })?;
        }

        #[cfg(unix)]
        {
            if is_running_as_root() {
                println!("Running as root, proceeding with socket creation");
            } else if !parent.exists() {
                println!("Creating socket directory: {}", parent.display());
                if let Err(e) = fs::create_dir_all(parent) {
                    println!("Warning: Couldn't create socket directory: {e}");
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
            println!("Warning: Failed to set socket permissions: {e}");
        } else {
            println!("Set socket permissions to 666 (rw-rw-rw-)");
        }

        if is_running_as_root() {
            if let (Ok(user_id), Ok(group_id)) =
                (std::env::var("SUDO_UID"), std::env::var("SUDO_GID"))
            {
                println!("Fixing socket ownership for user access");
                if let Err(e) = std::process::Command::new("chown")
                    .arg(format!("{user_id}:{group_id}"))
                    .arg(&socket_path)
                    .status()
                {
                    println!("Warning: Failed to fix socket ownership: {e}");
                } else {
                    println!("Set socket ownership to {user_id}:{group_id}");
                }
            }
        }

        if socket_path.exists() {
            println!("Socket file exists at: {}", socket_path.display());
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
        println!("Listening on Unix socket: {socket_path_clone:?}");

        listener.set_nonblocking(true).map_err(|e| {
            HelperError::Communication(format!("Failed to set non-blocking mode: {e}"))
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
                            eprintln!("Error handling connection: {e}");
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
            "Listener task panicked: {e}"
        ))),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn handle_connection(
    mut stream: UnixStream, pool_manager: Arc<AddressPoolManager>,
    network_manager: Arc<NetworkConfigManager>,
) -> Result<(), HelperError> {
    println!("New connection received");

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if let Err(e) = validate_peer_credentials(&stream) {
            println!("Peer credential validation failed: {e}");
            return Err(e);
        }
        println!("Peer credentials validated successfully");
    }

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| HelperError::Communication(format!("Failed to set socket timeout: {e}")))?;

    let mut buffer = Vec::new();
    let mut tmp_buf = [0u8; 4096];

    loop {
        match stream.read(&mut tmp_buf) {
            Ok(0) => {
                println!("Client closed connection (0 bytes read)");
                break;
            }
            Ok(n) => {
                println!("Read {n} bytes from client");
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
                println!("Error reading from client: {e}");
                return Err(HelperError::Communication(format!(
                    "Failed to read from socket: {e}"
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
                println!("Read {n} bytes from client after wait");
                buffer.extend_from_slice(&tmp_buf[..n]);
            }
            Err(e) => {
                println!("Error reading more data: {e}");
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

            if let Err(e) = validate_request(&req) {
                println!("Request validation failed: {e}");
                return Err(e);
            }
            println!("Request validation passed");

            req
        }
        Err(e) => {
            println!("Failed to parse request: {e}");
            return Err(HelperError::Communication(format!(
                "Failed to parse request: {e}"
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
            println!("Failed to serialize response: {e}");
            return Err(HelperError::Communication(format!(
                "Failed to serialize response: {e}"
            )));
        }
    };

    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| {
            println!("Failed to set write timeout: {e}");
            HelperError::Communication(format!("Failed to set socket write timeout: {e}"))
        })?;

    println!(
        "Writing response directly to client socket ({} bytes)",
        response_bytes.len()
    );
    match stream.write_all(&response_bytes) {
        Ok(_) => println!("Response written successfully"),
        Err(e) => {
            println!("Failed to write response: {e}");
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                println!("Client disconnected (broken pipe), ignoring error");
                return Ok(());
            }
            return Err(HelperError::Communication(format!(
                "Failed to write response: {e}"
            )));
        }
    }

    println!("Flushing socket output");
    match stream.flush() {
        Ok(_) => println!("Response flushed successfully"),
        Err(e) => {
            println!("Failed to flush response: {e}");
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                println!("Client disconnected (broken pipe), ignoring error");
                return Ok(());
            }
            return Err(HelperError::Communication(format!(
                "Failed to flush response: {e}"
            )));
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(100));

    println!("Connection handled successfully");
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
) -> Result<(), HelperError> {
    println!(
        "Starting Windows named pipe server on: {}",
        _pipe_path.display()
    );

    let pipe_name = _pipe_path.to_string_lossy();

    let pool_manager = Arc::new(_pool_manager);
    let network_manager = Arc::new(_network_manager);

    let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    let pipe_name_clone = pipe_name.to_string();
    let listener_task = task::spawn(async move {
        println!("Listening on Windows named pipe: {}", pipe_name_clone);

        loop {
            let pipe = match create_secure_pipe(&pipe_name_clone) {
                Ok(pipe) => pipe,
                Err(e) => {
                    eprintln!("Failed to create named pipe: {}", e);
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    continue;
                }
            };

            println!("Named pipe created, waiting for connection");

            match pipe.connect().await {
                Ok(()) => {
                    println!("Client connected to named pipe");
                    let pool_manager_clone = Arc::clone(&pool_manager);
                    let network_manager_clone = Arc::clone(&network_manager);

                    task::spawn(async move {
                        if let Err(e) = handle_windows_connection(
                            pipe,
                            pool_manager_clone,
                            network_manager_clone,
                        )
                        .await
                        {
                            eprintln!("Error handling Windows pipe connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Failed to connect client to named pipe: {}", e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }

            if shutdown_rx.try_recv().is_ok() {
                println!("Shutting down Windows named pipe server");
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
    network_manager: Arc<NetworkConfigManager>,
) -> Result<(), HelperError> {
    use tokio::io::{
        AsyncReadExt,
        AsyncWriteExt,
    };
    println!("New connection received on Windows named pipe");

    let mut buffer = Vec::new();
    let mut tmp_buf = [0u8; 4096];

    let mut total_read = 0;
    let timeout = Duration::from_secs(30);
    let start_time = std::time::Instant::now();

    loop {
        if start_time.elapsed() > timeout {
            println!(
                "Read operation timed out after {} seconds",
                timeout.as_secs()
            );
            break;
        }

        match tokio::time::timeout(Duration::from_secs(5), pipe.read(&mut tmp_buf)).await {
            Ok(read_result) => match read_result {
                Ok(0) => {
                    println!("Client closed connection (0 bytes read)");
                    break;
                }
                Ok(n) => {
                    println!("Read {} bytes from client", n);
                    buffer.extend_from_slice(&tmp_buf[..n]);
                    total_read += n;

                    if n < tmp_buf.len() {
                        println!("Read less than buffer size, assuming message is complete");
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    println!("Pipe would block, waiting briefly");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    println!("Pipe read interrupted, continuing");
                    continue;
                }
                Err(e) => {
                    println!("Error reading from client: {}", e);
                    if total_read > 0 {
                        println!(
                            "Have partial data ({} bytes), continuing with processing",
                            total_read
                        );
                        break;
                    }
                    return Err(HelperError::Communication(format!(
                        "Failed to read from pipe: {}",
                        e
                    )));
                }
            },
            Err(_) => {
                println!("Read operation timed out");
                if total_read > 0 {
                    println!(
                        "Have partial data ({} bytes), continuing with processing",
                        total_read
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    if buffer.is_empty() {
        println!("Empty request received, will wait for more data");
        tokio::time::sleep(Duration::from_millis(500)).await;

        match tokio::time::timeout(Duration::from_secs(5), pipe.read(&mut tmp_buf)).await {
            Ok(result) => match result {
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
            },
            Err(_) => {
                println!("Additional read timed out, closing connection");
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

            if let Err(e) = validate_request(&req) {
                println!("Request validation failed: {}", e);
                return Err(e);
            }
            println!("Request validation passed");

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

    println!(
        "Writing response directly to client pipe ({} bytes)",
        response_bytes.len()
    );

    match pipe.write_all(&response_bytes).await {
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

    println!("Flushing pipe output");
    match pipe.flush().await {
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

    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("Connection handled successfully");
    Ok(())
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
                println!("Processing Add request for address: {address}");
                match network_manager.add_loopback_address(&address).await {
                    Ok(_) => {
                        println!("Add request successful for address: {address}");
                        Ok(HelperResponse::success(request_id))
                    }
                    Err(e) => {
                        println!("Add request failed for address {address}: {e}");
                        Ok(HelperResponse::error(request_id, format!("Error: {e}")))
                    }
                }
            }
            NetworkCommand::Remove { address } => {
                println!("Processing Remove request for address: {address}");

                let result = match network_manager.remove_loopback_address(&address).await {
                    Ok(_) => {
                        println!("Remove request successful for address: {address}");
                        println!("Successfully removed loopback address: {address}");
                        HelperResponse::success(request_id.clone())
                    }
                    Err(e) => {
                        println!("Remove request failed for address {address}: {e}");

                        if e.to_string().contains("not found")
                            || e.to_string().contains("No such process")
                        {
                            println!("Address already removed, considering operation successful");
                            HelperResponse::success(request_id.clone())
                        } else {
                            println!("Returning error response for failed removal");
                            HelperResponse::error(request_id.clone(), format!("Error: {e}"))
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
                        println!("Failed to serialize response: {e}");
                        Ok(HelperResponse::error(
                            request_id.clone(),
                            format!("Error serializing response: {e}"),
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
                        println!("List request failed: {e}");
                        Ok(HelperResponse::list_success(request_id, vec![]))
                    }
                }
            }
        },
        RequestCommand::Address(cmd) => match cmd {
            AddressCommand::Allocate { service_name } => {
                println!("Processing Allocate request for service: {service_name}");
                match pool_manager.allocate_address(&service_name).await {
                    Ok(address) => {
                        println!(
                            "Allocate request successful for service {service_name}: {address}"
                        );
                        Ok(HelperResponse::string_success(request_id, address))
                    }
                    Err(e) => {
                        println!("Allocate request failed for service {service_name}: {e}");

                        Ok(HelperResponse::string_success(
                            request_id,
                            "127.0.0.1".to_string(),
                        ))
                    }
                }
            }
            AddressCommand::Release { address } => {
                println!("Processing Release request for address: {address}");
                match pool_manager.release_address(&address).await {
                    Ok(_) => {
                        println!("Release request successful for address: {address}");
                        Ok(HelperResponse::success(request_id))
                    }
                    Err(e) => {
                        println!("Release request failed for address {address}: {e}");

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
                        println!("List request failed: {e}");

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
