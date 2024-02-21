mod http_proxy;
mod tcp_proxy;
mod udp_over_tcp_proxy;

use log::{error, info, warn};
use std::env;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    env_logger::init();

    let shutdown_notify = Arc::new(Notify::new());
    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_signal = is_running.clone();

    ctrlc::set_handler(move || {
        info!("Ctrl-C handler triggered");
        is_running_signal.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    let target_host = env::var("REMOTE_ADDRESS").unwrap_or_else(|_| {
        error!("REMOTE_ADDRESS not set.");
        exit(1);
    });

    let target_port: u16 = env::var("REMOTE_PORT")
        .unwrap_or_else(|_| {
            error!("REMOTE_PORT not set.");
            exit(1);
        })
        .parse()
        .unwrap_or_else(|_| {
            error!("REMOTE_PORT must be a valid port number.");
            exit(1);
        });

    let proxy_port: u16 = env::var("LOCAL_PORT")
        .unwrap_or_else(|_| {
            error!("LOCAL_PORT not set.");
            exit(1);
        })
        .parse()
        .unwrap_or_else(|_| {
            error!("LOCAL_PORT must be a valid port number.");
            exit(1);
        });

    let proxy_type = env::var("PROXY_TYPE").unwrap_or_else(|_| {
        error!("PROXY_TYPE not set.");
        exit(1);
    });

    match proxy_type.as_str() {
        "tcp" => {
            info!("Starting TCP proxy...");
            if let Err(e) = tcp_proxy::start_tcp_proxy(
                &target_host,
                target_port,
                proxy_port,
                Arc::clone(&is_running),
            ) {
                error!("TCP Proxy failed with error: {}", e);
            }
        }
        "udp" => {
            info!("Starting UDP over TCP proxy...");
            if let Err(e) = udp_over_tcp_proxy::start_udp_over_tcp_proxy(
                &target_host,
                target_port,
                proxy_port,
                Arc::clone(&is_running),
            ) {
                error!("UDP over TCP Proxy failed with error: {}", e);
            }
        }
        "http" => {
            info!("Starting HTTP proxy...");
            if let Err(e) = http_proxy::start_http_proxy(
                &target_host,
                target_port,
                proxy_port,
                Arc::clone(&is_running),
                Arc::clone(&shutdown_notify),
            )
            .await
            {
                error!("Failed to start the HTTP proxy: {}", e);
            }
        }
        _ => {
            error!("Unsupported PROXY_TYPE: {}", proxy_type);
            exit(1);
        }
    }

    info!("Proxy is up and running.");

    while is_running.load(Ordering::SeqCst) {
        info!("Waiting for shutdown signal...");
        shutdown_notify.notified().await;

        info!("Shutdown signal received...");
        sleep(Duration::from_secs(1)).await;
    }

    warn!("Shutdown initiated...");
    info!("Exiting...");
}
