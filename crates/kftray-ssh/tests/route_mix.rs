#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(all(feature = "testing", feature = "provider-gcp"))]

use std::sync::Arc;

use kftray_ssh::gcp::bastion_route;
use kftray_ssh::testing::gcp::{
    FakeGcp,
    FakeGcpConfig,
};
use kftray_ssh::testing::{
    DuplexTransport,
    MockSshd,
    route_conformance,
};
use tokio::sync::OnceCell;

/// OS Login auth and guest-attribute host keys from the gcp provider over a
/// transport that is not IAP: provider pieces combine outside their route.
#[tokio::test(flavor = "multi_thread")]
async fn gcp_auth_and_host_keys_should_pass_conformance_over_another_transport() {
    let fake = OnceCell::new();
    route_conformance(async |mock: &MockSshd| {
        let fake = fake
            .get_or_init(|| async {
                let key = mock.authorized_keys().into_iter().next().unwrap();
                FakeGcp::start(FakeGcpConfig {
                    project: "kftray-test".into(),
                    zone: "us-central1-b".into(),
                    instance: "jumper".into(),
                    instance_id: "1234567890".into(),
                    email: "dev@example.com".into(),
                    posix_user: "dev_example_com".into(),
                    authorized_key: key.to_openssh().unwrap(),
                    ssh_upstream: mock.addr(),
                    token: "test-token".into(),
                    oslogin_certificates: false,
                    guest_host_key: Some(mock.host_key()),
                    user_ca: None,
                })
                .await
                .unwrap()
            })
            .await;
        let mut route = bastion_route(&fake.context().unwrap(), &fake.target())
            .await
            .unwrap();
        route.transport = Arc::new(DuplexTransport::new(mock));
        route
    })
    .await;
}
