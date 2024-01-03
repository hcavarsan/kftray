use std::env;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::thread;

fn handle_client(mut stream: TcpStream, remote_addr: String) -> io::Result<()> {
    let mut remote_stream = TcpStream::connect(remote_addr)?;

    let mut local_to_remote = stream.try_clone()?;
    let mut remote_to_local = remote_stream.try_clone()?;

    let client_to_server = thread::spawn(move || {
        io::copy(&mut local_to_remote, &mut remote_stream)
    });

    let server_to_client = thread::spawn(move || {
        io::copy(&mut remote_to_local, &mut stream)
    });

    client_to_server.join().unwrap()?;
    server_to_client.join().unwrap()?;
    Ok(())
}

fn start_tcp_proxy(local_port: u16, remote_addr: String) -> io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", local_port))?;
    println!("Listening for TCP connections on 0.0.0.0:{}", local_port);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let remote_addr = remote_addr.clone();
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, remote_addr) {
                        eprintln!("Failed to handle client: {:?}", e);
                    }
                });
            }
            Err(e) => eprintln!("Connection failed: {:?}", e),
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let local_port: u16 = env::var("LOCAL_PORT").unwrap().parse().unwrap();
    let remote_port: u16 = env::var("REMOTE_PORT").unwrap().parse().unwrap();
    let remote_address: String = env::var("REMOTE_ADDRESS").unwrap();
    let tunneled_type: String = env::var("TUNNELED_TYPE").unwrap().to_lowercase();

    let remote_addr = format!("{}:{}", remote_address, remote_port);

    match tunneled_type.as_str() {
        "tcp" => start_tcp_proxy(local_port, remote_addr),
        _ => {
            eprintln!("Unsupported TUNNELED_TYPE: {}", tunneled_type);
            Err(io::Error::new(io::ErrorKind::Other, "Unsupported TUNNELED_TYPE"))
        }
    }
}
