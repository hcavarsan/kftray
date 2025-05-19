use std::io::{
    Read,
    Write,
};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{
    Duration,
    Instant,
};

use log::debug;

use crate::error::HelperError;
use crate::messages::{
    HelperRequest,
    HelperResponse,
    RequestCommand,
};

pub fn is_socket_available(socket_path: &Path) -> bool {
    if !socket_path.exists() {
        debug!("Helper socket doesn't exist at: {:?}", socket_path);
        return false;
    }

    match UnixStream::connect(socket_path) {
        Ok(_) => {
            debug!("Successfully connected to helper socket");
            true
        }
        Err(e) => {
            debug!("Socket exists but connection failed: {}", e);

            if let Err(rm_err) = std::fs::remove_file(socket_path) {
                debug!("Failed to remove stale socket: {}", rm_err);
            } else {
                debug!("Removed stale socket file");
            }

            false
        }
    }
}

pub fn send_request(
    socket_path: &Path, app_id: &str, command: RequestCommand,
) -> Result<HelperResponse, HelperError> {
    let request = HelperRequest::new(app_id.to_string(), command);

    let request_bytes = serde_json::to_vec(&request)
        .map_err(|e| HelperError::Communication(format!("Failed to serialize request: {}", e)))?;

    debug!("Connecting to helper socket at {}", socket_path.display());
    let mut stream = UnixStream::connect(socket_path).map_err(|e| {
        HelperError::Communication(format!("Failed to connect to helper socket: {}", e))
    })?;

    if let Err(e) = stream.set_nonblocking(false) {
        debug!("Failed to set blocking mode: {}", e);
    }

    if let Err(e) = stream.set_read_timeout(Some(Duration::from_secs(5))) {
        debug!("Failed to set read timeout: {}", e);
    }

    if let Err(e) = stream.set_write_timeout(Some(Duration::from_secs(5))) {
        debug!("Failed to set write timeout: {}", e);
    }

    debug!("Sending request ({} bytes)", request_bytes.len());
    match stream.write_all(&request_bytes) {
        Ok(_) => debug!("Request sent successfully"),
        Err(e) => {
            return Err(HelperError::Communication(format!(
                "Failed to write request: {}",
                e
            )))
        }
    }

    match stream.flush() {
        Ok(_) => debug!("Socket flushed successfully"),
        Err(e) => {
            return Err(HelperError::Communication(format!(
                "Failed to flush socket: {}",
                e
            )))
        }
    }

    std::thread::sleep(Duration::from_millis(200));

    read_response(stream)
}

fn read_response(mut stream: UnixStream) -> Result<HelperResponse, HelperError> {
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

        match stream.read(&mut tmp_buf) {
            Ok(0) => {
                debug!("End of stream reached (0 bytes read)");
                if buffer.is_empty() {
                    debug!("Socket closed without sending any data");
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                } else {
                    debug!("Socket closed after receiving data, breaking read loop");
                    break;
                }
            }
            Ok(n) => {
                debug!("Read {} bytes from response", n);
                buffer.extend_from_slice(&tmp_buf[..n]);

                if n < tmp_buf.len() {
                    debug!("Message appears complete (got less than buffer size)");
                    break;
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

                if !buffer.is_empty() && start_time.elapsed() > Duration::from_secs(3) {
                    debug!("We have some data and waited 3 seconds, assuming response is complete");
                    break;
                }

                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                debug!("Read interrupted, retrying...");
                continue;
            }
            Err(e) => {
                debug!("Error reading from socket: {}", e);

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

    match serde_json::from_slice::<HelperResponse>(&buffer) {
        Ok(response) => {
            debug!("Successfully parsed response: {:?}", response.result);
            Ok(response)
        }
        Err(e) => {
            debug!("Failed to parse response JSON: {}", e);
            debug!(
                "Response content (first 100 bytes): {:?}",
                String::from_utf8_lossy(&buffer[..std::cmp::min(buffer.len(), 100)])
            );
            Err(HelperError::Communication(format!(
                "Failed to parse response: {}",
                e
            )))
        }
    }
}
