use log::{debug, error, info};
use std::io::{ErrorKind, Read, Result as IoResult, Write};
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::{atomic::AtomicBool, Arc};
use std::thread;

fn relay_streams<R: Read, W: Write>(mut read_stream: R, mut write_stream: W) -> IoResult<()> {
    let mut buffer = [0; 65536];
    loop {
        match read_stream.read(&mut buffer) {
            Ok(0) => {
                debug!("No more bytes to read; end of stream.");
                break;
            }
            Ok(n) => {
                debug!("Read {} bytes, now writing them.", n);
                if let Err(e) = write_stream.write_all(&buffer[..n]) {
                    if e.kind() != ErrorKind::BrokenPipe {
                        error!("Write error with kind != BrokenPipe: {}", e);
                        return Err(e);
                    }
                    debug!("BrokenPipe detected when attempting to write; terminating.");
                    break;
                }
                debug!("Successfully wrote {} bytes.", n);
            }
            Err(e) => {
                error!("Read error: {}", e);
                return Err(e);
            }
        }
    }
    debug!("Finished relay between streams.");
    Ok(())
}

pub fn start_tcp_proxy(
    target_host: &str,
    target_port: u16,
    proxy_port: u16,
    _is_running: Arc<AtomicBool>,
) -> IoResult<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", proxy_port))?;
    info!("TCP Proxy started on port {}", proxy_port);

    for stream in listener.incoming() {
        match stream {
            Ok(tunnel_stream) => {
                let target = format!("{}:{}", target_host, target_port);
                debug!("Incoming connection established.");

                thread::spawn(move || {
                    match TcpStream::connect(&target) {
                        Ok(target_stream) => {
                            info!("Connected to target {}", target);

                            let tunnel_reader = tunnel_stream
                                .try_clone()
                                .expect("Failed to clone tunnel_stream for reading");
                            let tunnel_writer = tunnel_stream;
                            let target_reader = target_stream
                                .try_clone()
                                .expect("Failed to clone target_stream for reading");
                            let target_writer = target_stream;

                            let client_to_target = thread::spawn(move || {
                                if let Err(e) = relay_streams(tunnel_reader, target_writer) {
                                    error!("Tunnel to Target relay failed: {}", e);
                                }
                            });

                            let target_to_client = thread::spawn(move || {
                                if let Err(e) = relay_streams(target_reader, tunnel_writer) {
                                    error!("Target to Tunnel relay failed: {}", e);
                                }
                            });

                            let _ = client_to_target.join();
                            let _ = target_to_client.join();
                        }
                        Err(e) => {
                            error!("Failed to connect to target: {}", e);
                        }
                    };
                });
            }
            Err(e) => error!("Failure when accepting incoming connection: {}", e),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    use mockall::predicate::*;

    mock! {
        pub TcpStream {}

        impl Read for TcpStream {
            fn read(&mut self, buf: &mut [u8]) -> IoResult<usize>;
        }

        impl Write for TcpStream {
            fn write(&mut self, buf: &[u8]) -> IoResult<usize>;
            fn flush(&mut self) -> IoResult<()>;
        }
    }

    #[test]
    fn test_relay_streams_with_mocks() {
        let mut read_mock = MockTcpStream::new();
        let mut write_mock = MockTcpStream::new();

        read_mock
            .expect_read()
            .returning(|buf| {
                let data = b"Hello, world!";
                let n = std::cmp::min(buf.len(), data.len());
                buf[..n].copy_from_slice(&data[..n]);
                Ok(n)
            })
            .times(1);

        write_mock
            .expect_write()
            .with(eq(b"Hello, world!".as_ref()))
            .returning(|data| Ok(data.len()))
            .times(1);

        read_mock.expect_read().returning(|_buf| Ok(0)).times(1);

        let result = relay_streams(&mut read_mock, &mut write_mock);
        assert!(result.is_ok());
    }
}
