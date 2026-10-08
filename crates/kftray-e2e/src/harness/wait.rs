use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Duration;

use anyhow::{
    Context,
    Result,
    bail,
};
use thirtyfour::WebDriver;
use tokio::net::TcpStream;
use tokio::time::{
    interval,
    timeout,
};

use crate::harness::env::POLL_INTERVAL;

pub async fn wait_for<F, Fut>(
    driver: &WebDriver, artifacts: &Path, what: &str, limit: Duration, mut cond: F,
) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool>>,
{
    let mut tick = interval(POLL_INTERVAL);
    let outcome = timeout(limit, async {
        loop {
            tick.tick().await;
            if cond().await? {
                return Ok::<_, anyhow::Error>(());
            }
        }
    })
    .await;
    match outcome {
        Ok(result) => result,
        Err(_) => {
            let shot = artifacts.join(format!("{}.png", slug(what)));
            driver
                .screenshot(&shot)
                .await
                .with_context(|| format!("timed out after {limit:?} waiting for {what}"))?;
            bail!(
                "timed out after {limit:?} waiting for {what}; screenshot at {}",
                shot.display()
            )
        }
    }
}

pub async fn poll<T, F, Fut>(limit: Duration, mut attempt: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let mut tick = interval(POLL_INTERVAL);
    timeout(limit, async {
        loop {
            tick.tick().await;
            if let Some(value) = attempt().await {
                return value;
            }
        }
    })
    .await
    .ok()
}

pub async fn wait_for_port(port: u16, limit: Duration) -> Result<()> {
    poll(limit, || async {
        TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .ok()
            .map(drop)
    })
    .await
    .with_context(|| format!("port {port} did not accept connections within {limit:?}"))
}

fn slug(what: &str) -> String {
    what.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}
