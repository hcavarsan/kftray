use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use log::{error, info};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

fn handle_tcp_to_udp(
    mut tcp_stream: TcpStream,
    udp_socket: Arc<UdpSocket>,
    is_running: Arc<AtomicBool>,
) -> io::Result<()> {
    while is_running.load(Ordering::SeqCst) {
        let size = match tcp_stream.read_u32::<BigEndian>() {
            Ok(size) => size as usize,
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                info!("TCP connection closed by client");
                return Ok(());
            }
            Err(e) => {
                error!("Failed to read from TCP: {}", e);
                return Err(e);
            }
        };

        // Read the specified number of bytes from tcp_stream
        let mut buffer = vec![0u8; size];
        tcp_stream.read_exact(&mut buffer)?;
        udp_socket.send(&buffer)?;
    }

    Ok(())
}

fn handle_udp_to_tcp(
    udp_socket: Arc<UdpSocket>,
    tcp_stream: Arc<Mutex<TcpStream>>,
    is_running: Arc<AtomicBool>,
) -> io::Result<()> {
    let mut buffer = [0u8; 65535];
    while is_running.load(Ordering::SeqCst) {
        match udp_socket.recv(&mut buffer) {
            Ok(size) => {
                let mut length_buffer = vec![];
                length_buffer.write_u32::<BigEndian>(size as u32)?;

                if let Ok(mut stream) = tcp_stream.lock() {
                    stream.write_all(&length_buffer)?;
                    stream.write_all(&buffer[..size])?;
                    stream.flush()?;
                }
            }
            Err(e) => {
                error!("UDP to TCP recv error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
pub fn start_udp_over_tcp_proxy(
    target_host: &str,
    target_port: u16,
    proxy_port: u16,
    is_running: Arc<AtomicBool>,
) -> io::Result<()> {
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", proxy_port))?;

    for stream_result in tcp_listener.incoming() {
        if !is_running.load(Ordering::SeqCst) {
            info!("Stopping UDP over TCP proxy");
            break;
        }

        let tcp_stream = match stream_result {
            Ok(stream) => stream,
            Err(e) => {
                error!("Failed to accept incoming connection: {}", e);
                continue;
            }
        };

        let target_addr = format!("{}:{}", target_host, target_port);

        let udp_socket = UdpSocket::bind("0.0.0.0:0")?;
        udp_socket.connect(&target_addr)?;

        let udp_write_socket = Arc::new(udp_socket);
        let udp_read_socket = udp_write_socket.clone();

        // Try to clone the TCP stream for the reading thread.
        let tcp_reader = tcp_stream.try_clone()?;
        // Wrap the TCP stream for the writing thread in a Mutex and Arc.
        let tcp_writer = Arc::new(Mutex::new(tcp_stream));

        let is_running_for_tcp_to_udp = Arc::clone(&is_running);
        let is_running_for_udp_to_tcp = Arc::clone(&is_running);

        thread::spawn(move || {
            if let Err(e) =
                handle_tcp_to_udp(tcp_reader, udp_write_socket, is_running_for_tcp_to_udp)
            {
                error!("Failed to handle TCP to UDP: {}", e);
            }
        });

        thread::spawn(move || {
            if let Err(e) =
                handle_udp_to_tcp(udp_read_socket, tcp_writer, is_running_for_udp_to_tcp)
            {
                error!("Failed to handle UDP to TCP: {}", e);
            }
        });
    }

    Ok(())
}
