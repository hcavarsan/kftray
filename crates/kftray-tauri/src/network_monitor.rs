use std::time::Duration;

use kftray_portforward::port_forward::CANCEL_NOTIFIER;
use log::info;
use tokio::time::{
    sleep,
    timeout,
};

pub async fn start_network_monitor() {
    info!("Starting network monitor");

    let mut was_network_up = check_network().await;

    loop {
        sleep(Duration::from_secs(5)).await;

        let is_network_up = check_network().await;

        if !was_network_up && is_network_up {
            info!("Network reconnected - likely wake from sleep");
            handle_reconnect().await;
        } else if was_network_up && !is_network_up {
            info!("Network disconnected - possibly entering sleep");
        }

        was_network_up = is_network_up;
    }
}

async fn check_network() -> bool {
    let connectivity_check = tokio::spawn(async {
        if let Ok(socket) = tokio::net::TcpStream::connect("8.8.8.8:53").await {
            drop(socket);
            true
        } else {
            false
        }
    });

    match timeout(Duration::from_secs(1), connectivity_check).await {
        Ok(Ok(result)) => result,
        _ => false,
    }
}

async fn handle_reconnect() {
    info!("Triggering port forward reconnection after network change");

    CANCEL_NOTIFIER.notify_waiters();

    info!("Network change handled - UI will reconnect active connections");
}
