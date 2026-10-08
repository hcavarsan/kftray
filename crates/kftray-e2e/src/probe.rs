use std::cell::RefCell;
use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Duration;

use anyhow::{
    Context,
    Result,
    ensure,
};
use tokio::net::{
    TcpStream,
    UdpSocket,
};
use tokio::time::timeout;

use crate::fixtures::Expect;
use crate::harness::env::POLL_INTERVAL;
use crate::harness::wait::poll;

impl Expect {
    pub async fn verify(&self, port: u16, limit: Duration) -> Result<()> {
        match self {
            Self::Http { body } => {
                let got = http_get(port, limit).await?;
                ensure!(
                    got.trim() == *body,
                    "port {port} answered {got:?}, expected {body:?}"
                );
                Ok(())
            }
            Self::Udp { send, reply } => {
                let got = udp_exchange(port, send, limit).await?;
                ensure!(
                    got == *reply,
                    "port {port} replied {:?}, expected {:?}",
                    String::from_utf8_lossy(&got),
                    String::from_utf8_lossy(reply)
                );
                Ok(())
            }
        }
    }
}

pub async fn wait_closed(port: u16, limit: Duration) -> Result<()> {
    poll(limit, || async {
        TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
            .then_some(())
    })
    .await
    .with_context(|| format!("port {port} still accepts connections after {limit:?}"))
}

pub async fn read_written_file(path: &Path, limit: Duration) -> Result<String> {
    poll(limit, || async {
        tokio::fs::read_to_string(path)
            .await
            .ok()
            .filter(|content| !content.is_empty())
    })
    .await
    .with_context(|| format!("{} was not written within {limit:?}", path.display()))
}

async fn http_get(port: u16, limit: Duration) -> Result<String> {
    let client = reqwest::Client::new();
    let url = format!("http://{}:{port}/", Ipv4Addr::LOCALHOST);
    let last_error = RefCell::new(String::from("no attempt finished"));
    poll(limit, || async {
        let outcome = match client.get(&url).send().await {
            Ok(response) => response.text().await,
            Err(error) => Err(error),
        };
        outcome
            .map_err(|error| *last_error.borrow_mut() = format!("{error:#}"))
            .ok()
    })
    .await
    .with_context(|| {
        format!(
            "port {port} gave no HTTP answer within {limit:?}; last error: {}",
            last_error.borrow()
        )
    })
}

async fn udp_exchange(port: u16, payload: &[u8], limit: Duration) -> Result<Vec<u8>> {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .context("bind a local UDP socket")?;
    socket
        .connect((Ipv4Addr::LOCALHOST, port))
        .await
        .with_context(|| format!("connect a UDP socket to port {port}"))?;
    let last_error = RefCell::new(String::from("no attempt finished"));
    poll(limit, || async {
        let mut buffer = [0; 1024];
        let received = match socket.send(payload).await {
            Ok(_) => timeout(POLL_INTERVAL, socket.recv(&mut buffer))
                .await
                .unwrap_or_else(|_| Err(std::io::ErrorKind::TimedOut.into())),
            Err(error) => Err(error),
        };
        received
            .map(|length| buffer[..length].to_vec())
            .map_err(|error| *last_error.borrow_mut() = error.to_string())
            .ok()
    })
    .await
    .with_context(|| {
        format!(
            "port {port} gave no UDP reply within {limit:?}; last error: {}",
            last_error.borrow()
        )
    })
}
