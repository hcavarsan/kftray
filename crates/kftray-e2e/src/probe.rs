use std::cell::RefCell;
use std::net::Ipv4Addr;
use std::time::Duration;

use anyhow::{
    Context,
    Result,
    ensure,
};
use tokio::net::TcpStream;

use crate::fixtures::Expect;
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
