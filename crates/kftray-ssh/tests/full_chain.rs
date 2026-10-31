#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(all(feature = "testing", feature = "provider-gcp"))]

use std::net::SocketAddr;
use std::sync::Arc;

use kftray_ssh::gcp::bastion_route;
use kftray_ssh::russh::keys::{
    Algorithm,
    PrivateKey,
    PublicKey,
};
use kftray_ssh::testing::gcp::{
    FAKE_FINGERPRINT,
    FakeGcp,
    FakeGcpConfig,
};
use kftray_ssh::testing::{
    MemoryHostKeyStore,
    MockSshd,
    MockSshdOptions,
    route_conformance,
};
use kftray_ssh::{
    SshRoute,
    SshRuntime,
    StreamOpener,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::sync::OnceCell;

fn config(mock: &MockSshd, authorized: &PublicKey, user_ca: Option<PrivateKey>) -> FakeGcpConfig {
    FakeGcpConfig {
        project: "kftray-test".into(),
        zone: "us-central1-b".into(),
        instance: "jumper".into(),
        instance_id: "1234567890".into(),
        email: "dev@example.com".into(),
        posix_user: "dev_example_com".into(),
        authorized_key: authorized.to_openssh().unwrap(),
        ssh_upstream: mock.addr(),
        token: "test-token".into(),
        oslogin_certificates: user_ca.is_some(),
        guest_host_key: None,
        user_ca,
    }
}

/// Builds the route through `bastion_route` against a fake started on the
/// first call, in front of the conformance sshd.
async fn route(
    fake: &OnceCell<FakeGcp>, mock: &MockSshd, user_ca: Option<&PrivateKey>,
) -> SshRoute {
    let fake = fake
        .get_or_init(|| async {
            let key = mock.authorized_keys().into_iter().next().unwrap();
            if let Some(ca) = user_ca {
                mock.trust_user_ca(ca.public_key().clone());
            }
            FakeGcp::start(config(mock, &key, user_ca.cloned()))
                .await
                .unwrap()
        })
        .await;
    bastion_route(&fake.context().unwrap(), &fake.target())
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn bastion_route_should_pass_conformance_with_an_imported_key() {
    let fake = OnceCell::new();
    route_conformance(async |mock: &MockSshd| route(&fake, mock, None).await).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bastion_route_should_pass_conformance_with_os_login_certificates() {
    let ca = PrivateKey::random(
        &mut kftray_ssh::russh::keys::key::safe_rng(),
        Algorithm::Ed25519,
    )
    .unwrap();
    let fake = OnceCell::new();
    route_conformance(async |mock: &MockSshd| route(&fake, mock, Some(&ca)).await).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bastion_route_should_renew_the_existing_key_and_carry_bytes_through_iap() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SshRuntime::new(dir.path(), Arc::new(MemoryHostKeyStore::default())).unwrap();
    let mock = MockSshd::start(MockSshdOptions {
        authorized: vec![runtime.key().public()],
        forwarding: true,
        ..MockSshdOptions::default()
    })
    .await
    .unwrap();
    let fake = FakeGcp::start(config(&mock, &runtime.key().public(), None))
        .await
        .unwrap();
    let route = bastion_route(&fake.context().unwrap(), &fake.target())
        .await
        .unwrap();

    let lease = runtime.acquire(route, &mut |_| {}).await.unwrap();
    let echo = mock.echo_addr();
    let mut stream = lease
        .opener(echo.ip().to_string(), echo.port())
        .open(SocketAddr::from(([127, 0, 0, 1], 40_000)))
        .await
        .unwrap();
    let payload: Vec<u8> = (0..64u8).collect();
    stream.write_all(&payload).await.unwrap();
    let mut received = vec![0u8; payload.len()];
    stream.read_exact(&mut received).await.unwrap();
    assert_eq!(received, payload);

    let requests = fake.received_requests().await;
    let patches = requests
        .iter()
        .filter(|r| {
            r.method.as_str() == "PATCH"
                && r.url
                    .path()
                    .ends_with(&format!("/sshPublicKeys/{FAKE_FINGERPRINT}"))
                && r.url.query() == Some("updateMask=expirationTimeUsec")
        })
        .count();
    let imports = requests
        .iter()
        .filter(|r| r.url.path().ends_with(":importSshPublicKey"))
        .count();
    assert_eq!((patches, imports), (1, 0));
}
