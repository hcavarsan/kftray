use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::thread;

fn handle_tcp_to_udp(tcp_stream: TcpStream, udp_socket: Arc<UdpSocket>) -> io::Result<()> {
    let mut tcp_stream = tcp_stream;
    let mut buffer = [0u8; 4096];
    loop {
        let size = tcp_stream.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        udp_socket.send(&buffer[..size])?;
    }
    Ok(())
}

fn handle_udp_to_tcp(udp_socket: Arc<UdpSocket>, tcp_stream: TcpStream) -> io::Result<()> {
    let mut tcp_stream = tcp_stream;
    let mut buffer = [0u8; 65507];
    loop {
        let (size, _) = udp_socket.recv_from(&mut buffer)?;
        tcp_stream.write_all(&buffer[..size])?;
    }
}

pub fn start_udp_over_tcp_proxy(target_host: &str, target_port: u16, proxy_port: u16, _is_running: Arc<AtomicBool>) -> std::io::Result<()> {
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", proxy_port))?;

    for stream in tcp_listener.incoming() {
        let tcp_stream = stream?;
        let target_addr = format!("{}:{}", target_host, target_port);

        let udp_socket = UdpSocket::bind("0.0.0.0:0")?;
        udp_socket.connect(&target_addr)?;

        let udp_write_socket = Arc::new(udp_socket);
        let udp_read_socket = Arc::clone(&udp_write_socket);

        let tcp_writer = tcp_stream.try_clone()?;

        thread::spawn(move || {
            if let Err(e) = handle_tcp_to_udp(tcp_stream, udp_write_socket) {
                eprintln!("Failed to handle TCP to UDP: {}", e);
            }
        });

        thread::spawn(move || {
            if let Err(e) = handle_udp_to_tcp(udp_read_socket, tcp_writer) {
                eprintln!("Failed to handle UDP to TCP: {}", e);
            }
        });
    }

    Ok(())
}
