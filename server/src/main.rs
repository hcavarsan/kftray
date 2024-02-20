mod http_proxy;
mod tcp_proxy;
mod udp_over_tcp_proxy;

use std::env;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() {
    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_signal = is_running.clone();

    ctrlc::set_handler(move || {
        is_running_signal.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    let target_host = env::var("REMOTE_ADDRESS").unwrap_or_else(|_| {
        eprintln!("REMOTE_ADDRESS not set.");
        process::exit(1);
    });

    let target_port: u16 = env::var("REMOTE_PORT").unwrap_or_else(|_| {
        eprintln!("REMOTE_PORT not set.");
        process::exit(1);
    })
    .parse()
    .unwrap_or_else(|_| {
        eprintln!("REMOTE_PORT must be a valid port number.");
        process::exit(1);
    });

    let proxy_port: u16 = env::var("LOCAL_PORT").unwrap_or_else(|_| {
        eprintln!("LOCAL_PORT not set.");
        process::exit(1);
    })
    .parse()
    .unwrap_or_else(|_| {
        eprintln!("LOCAL_PORT must be a valid port number.");
        process::exit(1);
    });

    let proxy_type = env::var("PROXY_TYPE").unwrap_or_else(|_| {
        eprintln!("PROXY_TYPE not set.");
        process::exit(1);
    });

    match proxy_type.as_str() {
        "tcp" => {
            if let Err(e) =
                tcp_proxy::start_tcp_proxy(&target_host, target_port, proxy_port, Arc::clone(&is_running))
            {
                eprintln!("TCP Proxy failed with error: {}", e);
            }
        }
        "udp" => {
            if let Err(e) =
                udp_over_tcp_proxy::start_udp_over_tcp_proxy(&target_host, target_port, proxy_port, Arc::clone(&is_running))
            {
                eprintln!("UDP over TCP Proxy failed with error: {}", e);
            }
        }
        "http" => {
            if let Err(e) =
                http_proxy::start_http_proxy(&target_host, target_port, proxy_port, Arc::clone(&is_running))
            {
                eprintln!("HTTP Proxy failed with error: {}", e);
            }
        }
        _ => {
            eprintln!("Unsupported PROXY_TYPE: {}", proxy_type);
            process::exit(1);
        }
    }


    while is_running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    println!("Exiting...");
}
