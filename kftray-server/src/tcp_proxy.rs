use log::{error, info};
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

fn relay_streams(mut read_stream: TcpStream, mut write_stream: TcpStream) -> io::Result<()> {
    let mut buffer = [0; 4096];
    loop {
        let n = read_stream.read(&mut buffer)?;
        if n == 0 {
            info!("No more data to read.");
            break;
        }
        info!("Read {} bytes from stream.", n);
        write_stream.write_all(&buffer[..n])?;
    }
    write_stream.shutdown(Shutdown::Both)?;
    Ok(())
}

pub fn start_tcp_proxy(
    target_host: &str,
    target_port: u16,
    proxy_port: u16,
    _is_running: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", proxy_port))?;
    info!("TCP Proxy started on port {}", proxy_port);

    for stream in listener.incoming() {
        let tunnel_stream = stream?;
        let target = format!("{}:{}", target_host, target_port);

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
                        relay_streams(tunnel_reader, target_writer).unwrap_or_else(|e| {
                            error!("Tunnel to Target relay failed: {}", e);
                        });
                    });

                    let target_to_client = thread::spawn(move || {
                        relay_streams(target_reader, tunnel_writer).unwrap_or_else(|e| {
                            error!("Target to Tunnel relay failed: {}", e);
                        });
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

    Ok(())
}
