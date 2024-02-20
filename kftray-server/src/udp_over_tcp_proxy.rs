use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

fn handle_tcp_to_udp(
    tcp_stream: TcpStream,
    udp_socket: Arc<UdpSocket>,
    is_running: Arc<AtomicBool>,
) -> io::Result<()> {
    let mut tcp_stream = tcp_stream;
    let mut buffer = [0u8; 4096];
    while is_running.load(Ordering::SeqCst) {
        match tcp_stream.read(&mut buffer) {
            Ok(0) => break, // Updated line
            Ok(size) => {
                udp_socket.send(&buffer[..size])?;
            }
            Err(e) => {
                eprintln!("TCP to UDP read error: {}", e);
                break;
            }
        }
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
                if let Ok(mut stream) = tcp_stream.lock() {
                    stream.write_all(&buffer[..size])?;
                }
            }
            Err(e) => {
                eprintln!("UDP to TCP recv error: {}", e);
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
            break;
        }

        let tcp_stream = match stream_result {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("Failed to accept incoming connection: {}", e);
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
                eprintln!("Failed to handle TCP to UDP: {}", e);
            }
        });

        thread::spawn(move || {
            if let Err(e) =
                handle_udp_to_tcp(udp_read_socket, tcp_writer, is_running_for_udp_to_tcp)
            {
                eprintln!("Failed to handle UDP to TCP: {}", e);
            }
        });
    }

    Ok(())
}
