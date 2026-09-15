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

/// No legitimate request comes close to this size; a client still growing
/// the buffer past it before a `HelperRequest` parses is a mistake or an
/// attack, never something worth reading further.
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

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

    // BSD/macOS accepted sockets inherit O_NONBLOCK from the listening
    // socket: left as is, the read timeout set below is silently ignored
    // and every `WouldBlock` in the read loop below is mistaken for a real
    // timeout instead of "no data yet".
    stream.set_nonblocking(false).map_err(|e| {
        HelperError::Communication(format!(
            "Failed to clear non-blocking mode on accepted socket: {e}"
        ))
    })?;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let caller_pid = match validate_peer_credentials(&stream) {
        Ok(pid) => {
            debug!("Peer credentials validated successfully");
            pid
        }
        Err(e) => {
            error!("Peer credential validation failed: {e}");
            return Err(e);
        }
    };

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| HelperError::Communication(format!("Failed to set socket timeout: {e}")))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| {
            warn!("Failed to set write timeout: {e}");
            HelperError::Communication(format!("Failed to set socket write timeout: {e}"))
        })?;

    // Blocks the calling thread until a full request is framed or the read
    // times out; run on the blocking pool instead of the async task itself,
    // now that the task is a plain `spawn` rather than a `spawn_blocking`
    // wrapping the whole connection -- otherwise every concurrent
    // connection would still park a blocking-pool thread for its request's
    // full read, the same nesting this split is meant to avoid.
    let (mut stream, request) = task::spawn_blocking(move || {
        let request = read_request(&mut stream);
        (stream, request)
    })
    .await
    .map_err(|e| HelperError::Communication(format!("Connection task panicked: {e}")))?;
    let request = match request? {
        Some(request) => request,
        None => return Ok(()),
    };

    if let Err(e) = validate_request(&request) {
        error!("Request validation failed: {e}");
        return Err(e);
    }
    debug!("Request validation passed");

    debug!("Processing request...");
    let response = process_request(
        request,
        pool_manager,
        network_manager,
        hostfile_manager,
        caller_pid,
    )
    .await?;
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
    // The write, flush, and the settle delay below all block the calling
    // thread; run them on the blocking pool for the same reason the read
    // above does.
    task::spawn_blocking(move || -> Result<(), HelperError> {
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
        Ok(())
    })
    .await
    .map_err(|e| HelperError::Communication(format!("Connection task panicked: {e}")))??;

    info!("Connection handled successfully");
    Ok(())
}

/// Reads one `HelperRequest` off `stream`, capping the buffer so a client
/// that never completes valid JSON cannot grow it without bound.
///
/// Returns `Ok(None)` once the read loop already answered the client with
/// a parse-error reply (the size cap, malformed JSON, or a stalled read),
/// or gave up without one (an EOF with data already buffered would only
/// hit a broken pipe), and there is nothing left to process. Returns
/// `Ok(Some(request))` on a successfully parsed request.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_request(stream: &mut UnixStream) -> Result<Option<HelperRequest>, HelperError> {
    let mut buffer = Vec::new();
    let mut tmp_buf = [0u8; 4096];
    let mut waited_for_first_byte = false;
    let timeout = Duration::from_secs(30);
    let start_time = std::time::Instant::now();

    // Completeness is judged by whether the buffer parses, not by the size
    // of a single read: a message can arrive in reads of any size, and a
    // large batch legitimately spans more than one.
    loop {
        if start_time.elapsed() > timeout {
            warn!(
                "Read operation timed out after {} seconds",
                timeout.as_secs()
            );
            respond_with_parse_error(stream, &buffer)?;
            return Ok(None);
        }

        match stream.read(&mut tmp_buf) {
            Ok(0) => {
                if buffer.is_empty() {
                    info!("Client closed connection (0 bytes read)");
                } else {
                    // The peer already closed its side after sending an
                    // incomplete request: replying would only hit a broken
                    // pipe, so there is nothing left worth answering.
                    debug!(
                        "Client closed connection after sending {} bytes, skipping reply",
                        buffer.len()
                    );
                }
                return Ok(None);
            }
            Ok(n) => {
                debug!("Read {n} bytes from client");
                buffer.extend_from_slice(&tmp_buf[..n]);

                if buffer.len() > MAX_REQUEST_BYTES {
                    warn!("Request exceeded {MAX_REQUEST_BYTES} bytes before parsing, rejecting");
                    respond_with_parse_error(stream, &buffer)?;
                    return Ok(None);
                }

                match parse_framed_request(&buffer) {
                    FramingOutcome::Complete(req) => {
                        debug!("Request parsed successfully");
                        return Ok(Some(req));
                    }
                    FramingOutcome::Invalid => {
                        respond_with_parse_error(stream, &buffer)?;
                        return Ok(None);
                    }
                    FramingOutcome::Incomplete => {}
                }
            }
            // With a socket read timeout set (SO_RCVTIMEO), a timed-out read
            // surfaces as WouldBlock rather than TimedOut on macOS, so an idle
            // client must fail fast the same way on both: one grace wait for a
            // slow client's first byte, then the same fail-fast the Windows
            // path uses.
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if buffer.is_empty() && !waited_for_first_byte {
                    waited_for_first_byte = true;
                    debug!("No data yet, waiting briefly for a slow client");
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                debug!("Socket read timed out, ending read loop");
                respond_with_parse_error(stream, &buffer)?;
                return Ok(None);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                debug!("Socket read interrupted, continuing");
            }
            Err(e) => {
                error!("Error reading from client: {e}");
                return Err(HelperError::Communication(format!(
                    "Failed to read from socket: {e}"
                )));
            }
        }
    }
}

/// Outcome of trying to parse one `HelperRequest` out of a buffer that may
/// still be growing.
#[derive(Debug)]
enum FramingOutcome {
    /// A complete request, followed by nothing but whitespace.
    Complete(HelperRequest),
    /// Not enough bytes yet to tell whether this is a request.
    Incomplete,
    /// A complete value followed by other bytes, or JSON that no amount of
    /// further reading would make valid.
    Invalid,
}

/// Looks for exactly one `HelperRequest` at the start of `buffer`, using a
/// `StreamDeserializer` instead of parsing the whole buffer as one value so
/// trailing bytes are diagnosed instead of making the request unparseable
/// forever as the client keeps appending to it.
fn parse_framed_request(buffer: &[u8]) -> FramingOutcome {
    let mut stream = serde_json::Deserializer::from_slice(buffer).into_iter::<HelperRequest>();
    match stream.next() {
        Some(Ok(request)) => {
            let trailing = &buffer[stream.byte_offset()..];
            if trailing.iter().all(u8::is_ascii_whitespace) {
                FramingOutcome::Complete(request)
            } else {
                FramingOutcome::Invalid
            }
        }
        Some(Err(e)) if e.is_eof() => FramingOutcome::Incomplete,
        Some(Err(_)) => FramingOutcome::Invalid,
        None => FramingOutcome::Incomplete,
    }
}

/// The request id an otherwise-unparseable buffer still carries, if it was
/// at least valid JSON with that field: lets the client correlate the
/// error with its own request instead of getting an unaddressed one.
fn parse_error_request_id(buffer: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(buffer)
        .ok()
        .and_then(|value| value.get("request_id")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The `(request_id, message)` a parse-error reply carries, shared by both
/// platforms' `respond_with_parse_error`: only how the bytes reach the
/// client differs (sync `Write` vs async `AsyncWriteExt`), not what the
/// reply says.
fn parse_error_reply(buffer: &[u8]) -> (String, String) {
    let message = serde_json::from_slice::<HelperRequest>(buffer)
        .err()
        .map(|e| format!("Failed to parse request: {e}"))
        .unwrap_or_else(|| "Incomplete request".to_string());
    (parse_error_request_id(buffer), message)
}

/// Writes an error response for a request that could not be parsed, so an
/// old or misbehaving client fails fast instead of waiting out its timeout.
///
/// The caller only reaches this once its own parse of `buffer` as a
/// `HelperRequest` already failed, so re-parsing it here is always the
/// error case; kept as a fallback rather than assumed, so a change to the
/// caller cannot silently turn this into a report of success.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn respond_with_parse_error(stream: &mut UnixStream, buffer: &[u8]) -> Result<(), HelperError> {
    let (request_id, message) = parse_error_reply(buffer);
    error!("{message}");

    let bytes = serde_json::to_vec(&HelperResponse::error(request_id, message)).map_err(|e| {
        warn!("Failed to serialize parse-error response: {e}");
        HelperError::Communication(format!("Failed to serialize parse-error response: {e}"))
    })?;
    if let Err(e) = stream.write_all(&bytes) {
        if matches!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
        ) {
            info!("Client disconnected before parse-error reply could be sent, ignoring: {e}");
            return Ok(());
        }
        warn!("Failed to write parse-error response: {e}");
        return Err(HelperError::Communication(format!(
            "Failed to write parse-error response: {e}"
        )));
    }
    if let Err(e) = stream.flush() {
        if matches!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
        ) {
            info!("Client disconnected before parse-error reply could be flushed, ignoring: {e}");
            return Ok(());
        }
        warn!("Failed to flush parse-error response: {e}");
        return Err(HelperError::Communication(format!(
            "Failed to flush parse-error response: {e}"
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn create_secure_pipe(pipe_name: &str) -> Result<NamedPipeServer, std::io::Error> {
    use windows::Win32::Foundation::{
        HLOCAL,
        LocalFree,
    };
    use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows::Win32::Security::{
        PSECURITY_DESCRIPTOR,
        SECURITY_ATTRIBUTES,
    };
    use windows::core::HSTRING;

    let sddl = HSTRING::from(crate::win_identity::pipe_security_descriptor_sddl());
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, 1, &mut descriptor, None)
    }
    .map_err(|e| std::io::Error::other(format!("Failed to build pipe security descriptor: {e}")))?;

    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };

    // Safety: `attrs` outlives the call and `lpSecurityDescriptor` points at
    // a descriptor that stays valid (and is freed) for the same span.
    let result = unsafe {
        ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .access_inbound(true)
            .access_outbound(true)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                pipe_name,
                (&mut attrs as *mut SECURITY_ATTRIBUTES).cast(),
            )
    };

    unsafe {
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }

    result
}

/// Verifies the named pipe's client process runs as the account this
/// installation trusts (`crate::win_identity::record_authorized_user`), the
/// Windows analogue of the `SO_PEERCRED`/`LOCAL_PEERCRED` UID check
/// `validate_peer_credentials` does on Unix, and returns that client's pid.
///
/// This helper normally runs as `LocalSystem`, so there is no "current
/// process UID" to compare the peer against the way the Unix non-root
/// fallback does. Comparing against the physical console's session instead
/// (the previous approach) rejects a legitimate client under RDP or fast
/// user switching, and can trust the wrong account when no one is on the
/// console at all; the account recorded at install avoids depending on
/// which session happens to be active.
///
/// An in-place upgrade from before the SID was recorded leaves no file to
/// read: falling back to the active console session, as this helper always
/// did before, keeps that installation usable instead of rejecting every
/// client, and records the accepted client's SID once so the next
/// connection uses the strict path.
#[cfg(target_os = "windows")]
fn validate_windows_peer(pipe: &NamedPipeServer) -> Result<u32, HelperError> {
    use std::os::windows::io::AsRawHandle;

    use windows::Win32::Foundation::{
        CloseHandle,
        HANDLE,
    };
    use windows::Win32::Security::{
        EqualSid,
        GetTokenInformation,
        TOKEN_QUERY,
        TokenUser,
    };
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use windows::Win32::System::RemoteDesktop::{
        WTSGetActiveConsoleSessionId,
        WTSQueryUserToken,
    };
    use windows::Win32::System::Threading::{
        OpenProcess,
        OpenProcessToken,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use crate::win_identity::AlignedTokenUserBuf;

    struct OwnedHandle(HANDLE);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }

    fn open_token(process: HANDLE) -> windows::core::Result<OwnedHandle> {
        let mut token = HANDLE::default();
        unsafe {
            OpenProcessToken(process, TOKEN_QUERY, &mut token)?;
        }
        Ok(OwnedHandle(token))
    }

    fn token_user_sid(token: &OwnedHandle) -> windows::core::Result<AlignedTokenUserBuf> {
        let mut buf = AlignedTokenUserBuf::new();
        let mut returned = 0u32;
        unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(buf.as_mut_ptr().cast()),
                AlignedTokenUserBuf::LEN as u32,
                &mut returned,
            )?;
        }
        Ok(buf)
    }

    let auth_err = |what: &str, e: windows::core::Error| {
        HelperError::Authentication(format!("Failed to {what}: {e}"))
    };

    let pipe_handle = HANDLE(pipe.as_raw_handle());
    let mut client_pid = 0u32;
    unsafe { GetNamedPipeClientProcessId(pipe_handle, &mut client_pid) }
        .map_err(|e| auth_err("get named pipe client PID", e))?;

    let client_process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, client_pid) }
            .map_err(|e| auth_err("open client process", e))?;
    let client_process = OwnedHandle(client_process);
    let client_token =
        open_token(client_process.0).map_err(|e| auth_err("open client process token", e))?;
    let client_sid_buf =
        token_user_sid(&client_token).map_err(|e| auth_err("read client token SID", e))?;
    let client_sid = client_sid_buf.token_user().User.Sid;

    match crate::win_identity::read_authorized_user_sid() {
        Some(authorized_sid_string) => {
            let authorized_sid = crate::win_identity::parse_sid(&authorized_sid_string)?;
            unsafe { EqualSid(client_sid, authorized_sid.0) }.map_err(|_| {
                HelperError::Authentication(
                    "Named pipe client is not the authorized user".to_owned(),
                )
            })?;
        }
        None => {
            warn!(
                "No authorized user recorded for this helper installation (expected after an \
                 in-place upgrade); falling back to the active console session"
            );

            let session_id = unsafe { WTSGetActiveConsoleSessionId() };
            let mut session_token = HANDLE::default();
            unsafe { WTSQueryUserToken(session_id, &mut session_token) }
                .map_err(|e| auth_err("query the active console session", e))?;
            let session_token = OwnedHandle(session_token);
            let session_sid_buf = token_user_sid(&session_token)
                .map_err(|e| auth_err("read session token SID", e))?;
            let session_sid = session_sid_buf.token_user().User.Sid;

            unsafe { EqualSid(client_sid, session_sid) }.map_err(|_| {
                HelperError::Authentication(
                    "Named pipe client is not the active console user".to_owned(),
                )
            })?;

            if let Err(e) = crate::win_identity::record_authorized_user_sid(session_sid) {
                warn!("Could not record the authorized user for future connections: {e}");
            }
        }
    }

    Ok(client_pid)
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
    use tokio::io::AsyncWriteExt;

    info!("New connection received on Windows named pipe");

    let caller_pid = match validate_windows_peer(&pipe) {
        Ok(pid) => {
            debug!("Peer identity validated successfully");
            Some(pid)
        }
        Err(e) => {
            error!("Peer identity validation failed: {e}");
            return Err(e);
        }
    };

    let request = match read_request(&mut pipe).await? {
        Some(request) => request,
        None => return Ok(()),
    };

    if let Err(e) = validate_request(&request) {
        error!("Request validation failed: {}", e);
        return Err(e);
    }
    debug!("Request validation passed");

    debug!("Processing request...");
    let response = process_request(
        request,
        pool_manager,
        network_manager,
        hostfile_manager,
        caller_pid,
    )
    .await?;
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

/// Reads one `HelperRequest` off `pipe`, capping the buffer so a client
/// that never completes valid JSON cannot grow it without bound.
///
/// Returns `Ok(None)` once the read loop already answered the client with
/// a parse-error reply (the size cap, malformed JSON, or a stalled read),
/// or gave up without one (an EOF with data already buffered would only
/// hit a broken pipe), and there is nothing left to process. Returns
/// `Ok(Some(request))` on a successfully parsed request.
#[cfg(target_os = "windows")]
async fn read_request(pipe: &mut NamedPipeServer) -> Result<Option<HelperRequest>, HelperError> {
    use tokio::io::AsyncReadExt;

    let mut buffer = Vec::new();
    let mut tmp_buf = [0u8; 4096];
    let mut waited_for_first_byte = false;

    let timeout = Duration::from_secs(30);
    let start_time = std::time::Instant::now();

    // Completeness is judged by whether the buffer parses, not by the size
    // of a single read: a message can arrive in reads of any size, and a
    // large batch legitimately spans more than one.
    loop {
        if start_time.elapsed() > timeout {
            warn!(
                "Read operation timed out after {} seconds",
                timeout.as_secs()
            );
            respond_with_parse_error(pipe, &buffer).await?;
            return Ok(None);
        }

        match tokio::time::timeout(Duration::from_secs(5), pipe.read(&mut tmp_buf)).await {
            Ok(read_result) => match read_result {
                Ok(0) => {
                    if buffer.is_empty() {
                        info!("Client closed connection (0 bytes read)");
                    } else {
                        // The peer already closed its side after sending an
                        // incomplete request: replying would only hit a
                        // broken pipe, so there is nothing left worth
                        // answering.
                        debug!(
                            "Client closed connection after sending {} bytes, skipping reply",
                            buffer.len()
                        );
                    }
                    return Ok(None);
                }
                Ok(n) => {
                    debug!("Read {} bytes from client", n);
                    buffer.extend_from_slice(&tmp_buf[..n]);

                    if buffer.len() > MAX_REQUEST_BYTES {
                        warn!(
                            "Request exceeded {MAX_REQUEST_BYTES} bytes before parsing, rejecting"
                        );
                        respond_with_parse_error(pipe, &buffer).await?;
                        return Ok(None);
                    }

                    match parse_framed_request(&buffer) {
                        FramingOutcome::Complete(req) => {
                            debug!("Request parsed successfully");
                            return Ok(Some(req));
                        }
                        FramingOutcome::Invalid => {
                            respond_with_parse_error(pipe, &buffer).await?;
                            return Ok(None);
                        }
                        FramingOutcome::Incomplete => {}
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
                    respond_with_parse_error(pipe, &buffer).await?;
                    return Ok(None);
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
                respond_with_parse_error(pipe, &buffer).await?;
                return Ok(None);
            }
        }
    }
}

/// Writes an error response for a request that could not be parsed, so an
/// old or misbehaving client fails fast instead of waiting out its timeout.
///
/// The caller only reaches this once its own parse of `buffer` as a
/// `HelperRequest` already failed, so re-parsing it here is always the
/// error case; kept as a fallback rather than assumed, so a change to the
/// caller cannot silently turn this into a report of success.
#[cfg(target_os = "windows")]
async fn respond_with_parse_error(
    pipe: &mut NamedPipeServer, buffer: &[u8],
) -> Result<(), HelperError> {
    use tokio::io::AsyncWriteExt;

    let (request_id, message) = parse_error_reply(buffer);
    error!("{}", message);

    let bytes = serde_json::to_vec(&HelperResponse::error(request_id, message)).map_err(|e| {
        warn!("Failed to serialize parse-error response: {}", e);
        HelperError::Communication(format!("Failed to serialize parse-error response: {}", e))
    })?;
    if let Err(e) = pipe.write_all(&bytes).await {
        if matches!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
        ) {
            info!("Client disconnected before parse-error reply could be sent, ignoring: {e}");
            return Ok(());
        }
        warn!("Failed to write parse-error response: {}", e);
        return Err(HelperError::Communication(format!(
            "Failed to write parse-error response: {}",
            e
        )));
    }
    if let Err(e) = pipe.flush().await {
        if matches!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
        ) {
            info!("Client disconnected before parse-error reply could be flushed, ignoring: {e}");
            return Ok(());
        }
        warn!("Failed to flush parse-error response: {}", e);
        return Err(HelperError::Communication(format!(
            "Failed to flush parse-error response: {}",
            e
        )));
    }
    Ok(())
}

async fn process_request(
    request: HelperRequest, pool_manager: Arc<AddressPoolManager>,
    network_manager: Arc<NetworkConfigManager>, hostfile_manager: Arc<HostfileManager>,
    caller_pid: Option<u32>,
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
                match pool_manager
                    .allocate_address(&service_name, caller_pid)
                    .await
                {
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
                                    pool_manager.release_address(&address, caller_pid).await
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

                // A genuine ownership conflict is returned to the caller
                // rather than only logged: `Ok` here previously meant only
                // "the network interface came off", so a refused release
                // was indistinguishable from a successful one and the
                // alias this address backs was torn down anyway. An
                // unknown address (a lost/corrupt pool file, or an entry
                // `cleanup_stale_allocations` already pruned) is not a
                // conflict: the interface alias may still be bound even
                // though the pool no longer knows about it, so treat it as
                // a no-op and fall through to the interface removal.
                match pool_manager.release_address(&address, caller_pid).await {
                    Ok(()) => {
                        info!("Address pool release successful for address: {address}");
                    }
                    Err(e @ HelperError::AddressNotAllocated(_)) => {
                        warn!(
                            "Address pool release for {address} found no allocation, proceeding to interface removal: {e}"
                        );
                    }
                    Err(e) => {
                        error!("Address pool release failed for address {address}: {e}");
                        return Ok(HelperResponse::error(request_id, format!("Error: {e}")));
                    }
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

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    use super::*;

    #[test]
    fn an_oversized_request_is_rejected_instead_of_read_forever() {
        let (mut client, mut server) = UnixStream::pair().expect("paired sockets");
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // Short, so a writer blocked on a full send buffer once the reader
        // stops consuming gives up quickly instead of hanging the test.
        client
            .set_write_timeout(Some(Duration::from_millis(300)))
            .unwrap();

        // Five times the cap: an unbounded reader would drain this over a
        // local socket without ever blocking the writer; a capped reader
        // stops well short, so the writer fills the kernel send buffer and
        // times out before sending it all.
        let target = MAX_REQUEST_BYTES * 5;
        let writer = std::thread::spawn(move || {
            // Never valid JSON on its own: keeps the read loop going until
            // the cap trips instead of a parse succeeding early.
            let chunk = vec![b'a'; 65536];
            let mut written = 0usize;
            while written < target {
                if client.write_all(&chunk).is_err() {
                    break;
                }
                written += chunk.len();
            }
            written
        });

        let result = read_request(&mut server);
        let written = writer.join().unwrap();

        assert!(
            matches!(result, Ok(None)),
            "a request past the size cap must be rejected with a reply, not read forever: \
             {result:?}"
        );
        assert!(
            written < target,
            "the writer must block on a full send buffer once the cap stops the reader \
             consuming, not finish sending all {target} bytes unchecked: sent {written}"
        );
    }
}

#[cfg(test)]
mod framing_tests {
    use super::*;

    fn sample_request_bytes() -> Vec<u8> {
        let request = HelperRequest::new("com.kftray.app".to_string(), RequestCommand::Ping);
        serde_json::to_vec(&request).unwrap()
    }

    #[test]
    fn trailing_whitespace_after_a_complete_request_is_ignored() {
        let mut buffer = sample_request_bytes();
        buffer.extend_from_slice(b"  \n\t");

        match parse_framed_request(&buffer) {
            FramingOutcome::Complete(req) => {
                assert!(matches!(req.command, RequestCommand::Ping))
            }
            other => panic!("expected a complete request, got {other:?}"),
        }
    }

    #[test]
    fn trailing_garbage_after_a_complete_request_is_invalid() {
        let mut buffer = sample_request_bytes();
        buffer.extend_from_slice(b"garbage");

        assert!(matches!(
            parse_framed_request(&buffer),
            FramingOutcome::Invalid
        ));
    }

    #[test]
    fn a_partial_request_is_incomplete() {
        let buffer = sample_request_bytes();
        let partial = &buffer[..buffer.len() - 1];

        assert!(matches!(
            parse_framed_request(partial),
            FramingOutcome::Incomplete
        ));
    }

    #[test]
    fn malformed_json_is_invalid() {
        assert!(matches!(
            parse_framed_request(b"{not json"),
            FramingOutcome::Invalid
        ));
    }
}
