#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{
    Duration,
    Instant,
};

use log::debug;
#[cfg(windows)]
use tokio::io::AsyncWriteExt;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;

use crate::error::HelperError;
use crate::messages::{
    HelperRequest,
    HelperResponse,
    RequestCommand,
};

/// Mirrors the server's request size cap: a response this large before it
/// parses means something is wrong, not that more reading will fix it.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Outcome of trying to parse one `HelperResponse` out of a buffer that may
/// still be growing.
enum FramingOutcome {
    /// A complete response, followed by nothing but whitespace.
    Complete(HelperResponse),
    /// Not enough bytes yet to tell whether this is a response.
    Incomplete,
    /// A complete value followed by other bytes, or JSON that no amount of
    /// further reading would make valid.
    Invalid,
}

/// Looks for exactly one `HelperResponse` at the start of `buffer`, the
/// same way the server's `parse_framed_request` looks for a request:
/// a `StreamDeserializer` rather than parsing the whole buffer as one
/// value, so trailing bytes are diagnosed instead of looping until the
/// 30s timeout, and a growing buffer is walked once per call instead of
/// re-parsed from byte zero after every read.
fn parse_framed_response(buffer: &[u8]) -> FramingOutcome {
    let mut stream = serde_json::Deserializer::from_slice(buffer).into_iter::<HelperResponse>();
    match stream.next() {
        Some(Ok(response)) => {
            let trailing = &buffer[stream.byte_offset()..];
            if trailing.iter().all(u8::is_ascii_whitespace) {
                FramingOutcome::Complete(response)
            } else {
                FramingOutcome::Invalid
            }
        }
        Some(Err(e)) if e.is_eof() => FramingOutcome::Incomplete,
        Some(Err(_)) => FramingOutcome::Invalid,
        None => FramingOutcome::Incomplete,
    }
}

pub fn is_socket_available(socket_path: &Path) -> bool {
    #[cfg(unix)]
    {
        if !socket_path.exists() {
            debug!("Helper socket doesn't exist at: {socket_path:?}");
            return false;
        }

        try_connect_socket(socket_path)
    }

    #[cfg(windows)]
    {
        let pipe_name = socket_path.to_string_lossy();
        debug!("Checking if Windows pipe is available: {}", pipe_name);
        match ClientOptions::new().open(pipe_name.as_ref()) {
            Ok(_) => {
                debug!("Successfully connected to Windows pipe");
                true
            }
            Err(e) => {
                debug!("Failed to connect to Windows pipe: {}", e);
                false
            }
        }
    }
}

#[cfg(unix)]
fn try_connect_socket(socket_path: &Path) -> bool {
    debug!("Socket exists at: {}", socket_path.display());

    match UnixStream::connect(socket_path) {
        Ok(_) => {
            debug!("Successfully connected to helper socket at {socket_path:?}");
            true
        }
        Err(e) => {
            debug!("Socket exists at {socket_path:?} but connection failed: {e}");

            if e.kind() == std::io::ErrorKind::PermissionDenied
                || e.kind() == std::io::ErrorKind::ConnectionRefused
            {
                debug!("Detected stale or inaccessible socket");

                if let Some(parent) = socket_path.parent() {
                    if is_directory_writable(parent) {
                        debug!("Removing stale socket from writable directory");
                        if let Err(rm_err) = std::fs::remove_file(socket_path) {
                            debug!("Failed to remove stale socket: {rm_err}");
                        } else {
                            debug!("Removed stale socket file");
                        }
                    } else {
                        debug!("Cannot remove stale socket - parent directory not writable");
                    }
                }
            }

            false
        }
    }
}

#[cfg(unix)]
fn is_directory_writable(path: &Path) -> bool {
    let test_file_path = path.join(".kftray_write_test");
    let write_result = std::fs::File::create(&test_file_path);

    if test_file_path.exists() {
        let _ = std::fs::remove_file(&test_file_path);
    }

    write_result.is_ok()
}

pub fn send_request(
    socket_path: &Path, app_id: &str, command: RequestCommand,
) -> Result<HelperResponse, HelperError> {
    let request = HelperRequest::new(app_id.to_string(), command);

    debug!("Using socket path: {}", socket_path.display());

    let request_bytes = serde_json::to_vec(&request)
        .map_err(|e| HelperError::Communication(format!("Failed to serialize request: {e}")))?;

    #[cfg(unix)]
    {
        debug!("Connecting to Unix socket at {}", socket_path.display());
        let mut stream = UnixStream::connect(socket_path).map_err(|e| {
            HelperError::Communication(format!("Failed to connect to Unix socket: {e}"))
        })?;

        if let Err(e) = stream.set_nonblocking(false) {
            debug!("Failed to set blocking mode: {e}");
        }

        if let Err(e) = stream.set_read_timeout(Some(Duration::from_secs(5))) {
            debug!("Failed to set read timeout: {e}");
        }

        if let Err(e) = stream.set_write_timeout(Some(Duration::from_secs(5))) {
            debug!("Failed to set write timeout: {e}");
        }

        debug!("Sending request ({} bytes)", request_bytes.len());
        match io::Write::write_all(&mut stream, &request_bytes) {
            Ok(_) => debug!("Request sent successfully"),
            Err(e) => {
                return Err(HelperError::Communication(format!(
                    "Failed to write request: {e}"
                )));
            }
        }

        match io::Write::flush(&mut stream) {
            Ok(_) => debug!("Socket flushed successfully"),
            Err(e) => {
                return Err(HelperError::Communication(format!(
                    "Failed to flush socket: {e}"
                )));
            }
        }

        std::thread::sleep(Duration::from_millis(200));

        read_unix_response(stream)
    }

    #[cfg(windows)]
    {
        debug!("Connecting to Windows pipe at {}", socket_path.display());
        let pipe_name = socket_path.to_string_lossy();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                HelperError::Communication(format!("Failed to create tokio runtime: {}", e))
            })?;

        rt.block_on(async {
            let mut pipe = ClientOptions::new().open(pipe_name.as_ref()).map_err(|e| {
                HelperError::Communication(format!("Failed to connect to Windows pipe: {}", e))
            })?;

            debug!("Sending request ({} bytes)", request_bytes.len());
            match pipe.write_all(&request_bytes).await {
                Ok(_) => debug!("Request sent successfully"),
                Err(e) => {
                    return Err(HelperError::Communication(format!(
                        "Failed to write request: {}",
                        e
                    )));
                }
            }

            match pipe.flush().await {
                Ok(_) => debug!("Pipe flushed successfully"),
                Err(e) => {
                    return Err(HelperError::Communication(format!(
                        "Failed to flush pipe: {}",
                        e
                    )));
                }
            }

            tokio::time::sleep(Duration::from_millis(200)).await;

            read_windows_response(&mut pipe).await
        })
    }
}

#[cfg(unix)]
fn read_unix_response(mut stream: UnixStream) -> Result<HelperResponse, HelperError> {
    let start_time = Instant::now();
    let timeout = Duration::from_secs(30);
    let mut buffer = Vec::new();
    let mut tmp_buf = [0u8; 4096];

    debug!(
        "Starting response read with timeout of {} seconds",
        timeout.as_secs()
    );

    loop {
        if start_time.elapsed() > timeout {
            debug!("Request timed out after {} seconds", timeout.as_secs());
            return Err(HelperError::Communication(format!(
                "Timed out waiting for response after {} seconds",
                timeout.as_secs()
            )));
        }

        match io::Read::read(&mut stream, &mut tmp_buf) {
            Ok(0) => {
                debug!("End of stream reached (0 bytes read)");
                if buffer.is_empty() {
                    debug!("Helper closed the connection before sending any data");
                    return Err(HelperError::Communication(
                        "Helper closed the connection before sending a response".into(),
                    ));
                }
                debug!("Socket closed after receiving data, breaking read loop");
                break;
            }
            Ok(n) => {
                debug!("Read {n} bytes from response");
                buffer.extend_from_slice(&tmp_buf[..n]);

                if buffer.len() > MAX_RESPONSE_BYTES {
                    return Err(HelperError::Communication(format!(
                        "Response exceeded {MAX_RESPONSE_BYTES} bytes before parsing, aborting"
                    )));
                }

                match parse_framed_response(&buffer) {
                    FramingOutcome::Complete(response) => {
                        debug!("Response appears complete");
                        return Ok(response);
                    }
                    FramingOutcome::Invalid => {
                        debug!(
                            "Response is malformed or has trailing bytes, failing fast instead \
                             of waiting out the timeout"
                        );
                        return Err(HelperError::Communication(
                            "Received a malformed response".into(),
                        ));
                    }
                    FramingOutcome::Incomplete => {}
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if buffer.is_empty() {
                    debug!("No data received yet, waiting...");
                } else {
                    debug!(
                        "Partial data received ({} bytes), waiting for more...",
                        buffer.len()
                    );
                }

                debug!("Time elapsed: {:?}", start_time.elapsed());

                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                debug!("Read interrupted, retrying...");
                continue;
            }
            Err(e) => {
                debug!("Error reading from socket: {e}");

                if !buffer.is_empty() {
                    debug!(
                        "Got error but have some data ({} bytes), attempting to parse",
                        buffer.len()
                    );
                    break;
                }

                return Err(HelperError::Communication(format!(
                    "Failed to read response: {e}"
                )));
            }
        }
    }

    debug!("Finished reading response, total {} bytes", buffer.len());

    if buffer.is_empty() {
        debug!("Empty response buffer after read loop");
        return Err(HelperError::Communication("Empty response received".into()));
    }

    match parse_framed_response(&buffer) {
        FramingOutcome::Complete(response) => {
            debug!("Successfully parsed response: {:?}", response.result);
            Ok(response)
        }
        _ => {
            debug!("Failed to parse response JSON");
            debug!(
                "Response content (first 100 bytes): {:?}",
                String::from_utf8_lossy(&buffer[..std::cmp::min(buffer.len(), 100)])
            );
            Err(HelperError::Communication(
                "Failed to parse response".into(),
            ))
        }
    }
}

#[cfg(windows)]
async fn read_windows_response<T: tokio::io::AsyncRead + Unpin>(
    pipe: &mut T,
) -> Result<HelperResponse, HelperError> {
    use tokio::io::AsyncReadExt;

    let start_time = Instant::now();
    let timeout = Duration::from_secs(30);
    let mut buffer = Vec::new();
    let mut tmp_buf = [0u8; 4096];

    debug!(
        "Starting Windows pipe response read with timeout of {} seconds",
        timeout.as_secs()
    );

    loop {
        if start_time.elapsed() > timeout {
            debug!("Request timed out after {} seconds", timeout.as_secs());
            return Err(HelperError::Communication(format!(
                "Timed out waiting for response after {} seconds",
                timeout.as_secs()
            )));
        }

        let read_future = pipe.read(&mut tmp_buf);
        let read_result = match tokio::time::timeout(Duration::from_secs(5), read_future).await {
            Ok(result) => result,
            Err(_) => {
                debug!("Read operation timed out, checking buffer state");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };

        match read_result {
            Ok(0) => {
                debug!("End of pipe reached (0 bytes read)");
                if buffer.is_empty() {
                    debug!("Helper closed the connection before sending any data");
                    return Err(HelperError::Communication(
                        "Helper closed the connection before sending a response".into(),
                    ));
                }
                debug!("Pipe closed after receiving data, breaking read loop");
                break;
            }
            Ok(n) => {
                debug!("Read {} bytes from pipe response", n);
                buffer.extend_from_slice(&tmp_buf[..n]);

                if buffer.len() > MAX_RESPONSE_BYTES {
                    return Err(HelperError::Communication(format!(
                        "Response exceeded {MAX_RESPONSE_BYTES} bytes before parsing, aborting"
                    )));
                }

                match parse_framed_response(&buffer) {
                    FramingOutcome::Complete(response) => {
                        debug!("Response appears complete");
                        return Ok(response);
                    }
                    FramingOutcome::Invalid => {
                        debug!(
                            "Response is malformed or has trailing bytes, failing fast instead \
                             of waiting out the timeout"
                        );
                        return Err(HelperError::Communication(
                            "Received a malformed response".into(),
                        ));
                    }
                    FramingOutcome::Incomplete => {}
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if buffer.is_empty() {
                    debug!("No data received yet, waiting...");
                } else {
                    debug!(
                        "Partial data received ({} bytes), waiting for more...",
                        buffer.len()
                    );
                }

                debug!("Time elapsed: {:?}", start_time.elapsed());

                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                debug!("Read interrupted, retrying...");
                continue;
            }
            Err(e) => {
                debug!("Error reading from pipe: {}", e);

                if !buffer.is_empty() {
                    debug!(
                        "Got error but have some data ({} bytes), attempting to parse",
                        buffer.len()
                    );
                    break;
                }

                return Err(HelperError::Communication(format!(
                    "Failed to read response: {}",
                    e
                )));
            }
        }
    }

    debug!("Finished reading response, total {} bytes", buffer.len());

    if buffer.is_empty() {
        debug!("Empty response buffer after read loop");
        return Err(HelperError::Communication("Empty response received".into()));
    }

    match parse_framed_response(&buffer) {
        FramingOutcome::Complete(response) => {
            debug!("Successfully parsed response: {:?}", response.result);
            Ok(response)
        }
        _ => {
            debug!("Failed to parse response JSON");
            debug!(
                "Response content (first 100 bytes): {:?}",
                String::from_utf8_lossy(&buffer[..std::cmp::min(buffer.len(), 100)])
            );
            Err(HelperError::Communication(
                "Failed to parse response".into(),
            ))
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::net::UnixListener;

    use super::*;

    #[test]
    fn eof_before_any_response_fails_fast_instead_of_waiting_out_the_timeout() {
        let socket_path = std::env::temp_dir().join(format!(
            "kftray-helper-eof-test-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).unwrap();

        // An already-installed helper that predates a request it cannot
        // parse: it accepts the connection and closes it without writing
        // anything back.
        let server = std::thread::spawn(move || {
            let (_conn, _) = listener.accept().unwrap();
        });

        let stream = UnixStream::connect(&socket_path).unwrap();
        let start = Instant::now();
        let result = read_unix_response(stream);
        let elapsed = start.elapsed();

        server.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);

        assert!(
            result.is_err(),
            "a connection closed before any response must be an error"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "an old helper that never responds must fail fast instead of waiting out the 30s \
             response timeout, took {elapsed:?}"
        );
    }

    #[test]
    fn an_oversized_response_is_rejected_instead_of_read_forever() {
        let socket_path = std::env::temp_dir().join(format!(
            "kftray-cap-test-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).unwrap();

        // Five times the cap: an unbounded reader would drain this over a
        // local socket without ever blocking the writer; a capped reader
        // stops well short, so the writer fills the kernel send buffer and
        // times out before sending it all.
        let target = MAX_RESPONSE_BYTES * 5;
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            conn.set_write_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            // Never valid JSON on its own: keeps the read loop going until
            // the cap trips instead of a parse succeeding early.
            let chunk = vec![b'a'; 65536];
            let mut written = 0usize;
            while written < target {
                if io::Write::write_all(&mut conn, &chunk).is_err() {
                    break;
                }
                written += chunk.len();
            }
            written
        });

        let stream = UnixStream::connect(&socket_path).unwrap();
        let start = Instant::now();
        let result = read_unix_response(stream);
        let elapsed = start.elapsed();
        let written = server.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);

        assert!(
            result.is_err(),
            "a response past the size cap must error out, not be read forever: {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "the size cap must trip almost immediately once exceeded, not wait out the old \
             3-second partial-data heuristic: took {elapsed:?}"
        );
        assert!(
            written < target,
            "the server side must block on a full send buffer once the cap stops the client \
             consuming, not finish sending all {target} bytes unchecked: sent {written}"
        );
    }

    #[test]
    fn trailing_garbage_after_a_complete_response_fails_fast() {
        let socket_path = std::env::temp_dir().join(format!(
            "kft-trail-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).unwrap();

        // The connection is kept open well past when the client must have
        // already returned: this proves the trailing bytes are diagnosed
        // from the buffer itself, not merely detected once EOF finally
        // arrives after nothing else in `buffer` ever parses.
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut bytes =
                serde_json::to_vec(&HelperResponse::success("req-1".to_string())).unwrap();
            bytes.extend_from_slice(b"garbage-after-response");
            io::Write::write_all(&mut conn, &bytes).unwrap();
            std::thread::sleep(Duration::from_secs(4));
        });

        let stream = UnixStream::connect(&socket_path).unwrap();
        let start = Instant::now();
        let result = read_unix_response(stream);
        let elapsed = start.elapsed();

        server.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);

        assert!(
            result.is_err(),
            "a response followed by trailing bytes must be diagnosed as malformed: {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "trailing bytes must be diagnosed from the buffer itself and fail fast, not wait \
             for EOF or the 30s response timeout, took {elapsed:?}"
        );
    }
}
