#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "iap")]

use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    AtomicUsize,
    Ordering,
};
use std::time::Duration;

use async_trait::async_trait;
use kftray_ssh::testing::iap::FakeIap;
use kftray_ssh::transport::iap::{
    IapTarget,
    IapTransport,
    TokenSource,
};
use kftray_ssh::{
    SshError,
    Transport,
};
use secrecy::SecretString;
use tokio::net::TcpListener;

const TOKEN: &str = "ya29.a0-sensitive-oauth-token";

static RECORDS: AtomicUsize = AtomicUsize::new(0);
static LEAKED: AtomicBool = AtomicBool::new(false);

struct Spy;

impl log::Log for Spy {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        RECORDS.fetch_add(1, Ordering::SeqCst);
        if record.args().to_string().contains(TOKEN) {
            LEAKED.store(true, Ordering::SeqCst);
        }
    }

    fn flush(&self) {}
}

struct Token;

#[async_trait]
impl TokenSource for Token {
    async fn access_token(&self) -> Result<SecretString, SshError> {
        Ok(SecretString::from(TOKEN))
    }
}

#[tokio::test]
async fn connect_should_not_log_the_token_when_every_level_is_enabled() {
    log::set_logger(&Spy).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fake = FakeIap::start(upstream.local_addr().unwrap(), TOKEN)
        .await
        .unwrap();
    let transport = IapTransport::new(
        IapTarget::Instance {
            project: "p".into(),
            zone: "z".into(),
            instance: "vm".into(),
            interface: "nic0".into(),
            port: 22,
        },
        fake.endpoint(),
        Arc::new(Token),
    );

    let stream = tokio::time::timeout(Duration::from_secs(30), transport.connect())
        .await
        .expect("timed out")
        .map_err(|e| e.to_string())
        .unwrap();
    drop(stream);

    assert_eq!(fake.connect_count(), 1);
    assert!(
        RECORDS.load(Ordering::SeqCst) > 0,
        "the logger saw nothing, so the check proves nothing"
    );
    assert!(
        !LEAKED.load(Ordering::SeqCst),
        "the OAuth token reached the log"
    );
}
