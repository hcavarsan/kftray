use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::thread;

fn handle_client(mut client_stream: TcpStream, target_addr: String) -> std::io::Result<()> {
    let mut server_stream = TcpStream::connect(&target_addr)?;

    let mut client_read_stream = client_stream.try_clone()?;
    let mut server_write_stream = server_stream.try_clone()?;

    let client_to_server = thread::spawn(move || -> std::io::Result<()> {
        copy_stream(&mut client_read_stream, &mut server_write_stream)
    });

    let server_to_client = thread::spawn(move || -> std::io::Result<()> {
        copy_stream(&mut server_stream, &mut client_stream)
    });

    client_to_server.join().unwrap()?;
    server_to_client.join().unwrap()?;

    Ok(())
}

fn copy_stream(read_stream: &mut TcpStream, write_stream: &mut TcpStream) -> std::io::Result<()> {
    let mut buffer = [0; 4096];
    loop {
        let count = read_stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }

        write_stream.write_all(&buffer[..count])?;
    }

    Ok(())
}

pub fn start_http_proxy(target_host: &str, target_port: u16, proxy_port: u16, _is_running: Arc<AtomicBool>) -> std::io::Result<()> {
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", proxy_port))?;
    let target_addr = format!("{}:{}", target_host, target_port);

    for stream in tcp_listener.incoming() {
        let client_stream = stream?;
        let target_addr = target_addr.clone();

        thread::spawn(move || {
            if let Err(e) = handle_client(client_stream, target_addr) {
                eprintln!("Error relaying HTTP connection: {}", e);
            }
        });
    }

    Ok(())
}
