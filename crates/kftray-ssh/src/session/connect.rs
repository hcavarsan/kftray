use std::sync::{
    Arc,
    Mutex,
    PoisonError,
};

use russh::client::{
    self,
    Handle,
};
use russh::keys::PrivateKeyWithHashAlg;
use tokio::sync::oneshot;
use tokio::time::timeout;

use super::Stage;
use super::handler::ClientHandler;
use crate::consts::{
    AUTH_TIMEOUT,
    HANDSHAKE_TIMEOUT,
    KEEPALIVE_INTERVAL,
    KEEPALIVE_MAX,
};
use crate::error::SshError;
use crate::host_keys::HostKeyStore;
use crate::key::KftrayKey;
use crate::route::{
    Credentials,
    SshRoute,
};

/// A logged-in session and a signal that fires when it ends.
pub(crate) type Established = (Handle<ClientHandler>, oneshot::Receiver<()>);

pub(super) fn protocol_error(route: &str, reason: impl std::fmt::Display) -> SshError {
    SshError::Protocol {
        route: route.to_owned(),
        reason: reason.to_string(),
    }
}

/// transport -> SSH handshake (host key check included) -> authentication.
pub(crate) async fn establish(
    route: &SshRoute, key: &KftrayKey, store: &Arc<dyn HostKeyStore>,
    on_stage: &mut (dyn FnMut(Stage) + Send),
) -> Result<Established, SshError> {
    let label = route.label.as_str();

    on_stage(Stage::Transport);
    let stream = route.transport.connect().await?;

    on_stage(Stage::Handshake);
    let verdict = Arc::new(Mutex::new(None));
    let (closed_tx, mut closed_rx) = oneshot::channel();
    let handler = ClientHandler::new(
        label.to_owned(),
        Arc::clone(&route.host_keys),
        Arc::clone(store),
        Arc::clone(&verdict),
        closed_tx,
    );
    let config = client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVE_MAX,
        inactivity_timeout: None,
        ..Default::default()
    };
    let mut handle = match timeout(
        HANDSHAKE_TIMEOUT,
        client::connect_stream(Arc::new(config), stream, handler),
    )
    .await
    {
        Ok(Ok(handle)) => handle,
        Ok(Err(e)) => {
            let rejected = verdict
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            return Err(rejected.unwrap_or_else(|| protocol_error(label, e)));
        }
        Err(_) => {
            return Err(SshError::Timeout {
                route: label.to_owned(),
                stage: Stage::Handshake,
            });
        }
    };

    on_stage(Stage::Auth);
    let credentials = route.auth.credentials(key).await?;
    let user = credentials.user().to_owned();
    let authenticated = timeout(AUTH_TIMEOUT, async {
        match credentials {
            Credentials::PublicKey { user } => {
                handle
                    .authenticate_publickey(user, PrivateKeyWithHashAlg::new(key.private(), None))
                    .await
            }
            Credentials::Certificate { user, cert } => {
                handle
                    .authenticate_openssh_cert(user, key.private(), cert)
                    .await
            }
        }
    })
    .await;
    match authenticated {
        Ok(Ok(result)) if result.success() => {}
        Ok(Ok(_)) => {
            // russh reports a session that died mid-authentication as a plain
            // failure, which must not look like a rejected login.
            let ended = closed_rx.try_recv() != Err(oneshot::error::TryRecvError::Empty);
            return Err(if ended {
                SshError::Transport {
                    route: label.to_owned(),
                    reason: "connection closed during authentication".to_owned(),
                }
            } else {
                SshError::AuthRejected {
                    route: label.to_owned(),
                    user,
                }
            });
        }
        Ok(Err(e)) => return Err(protocol_error(label, e)),
        Err(_) => {
            return Err(SshError::Timeout {
                route: label.to_owned(),
                stage: Stage::Auth,
            });
        }
    }

    log::info!("[ssh {label}] connected as {user}");
    Ok((handle, closed_rx))
}

#[cfg(test)]
mod tests {
    use std::time::{
        SystemTime,
        UNIX_EPOCH,
    };

    use async_trait::async_trait;
    use russh::keys::ssh_key::certificate::{
        Builder,
        CertType,
    };
    use russh::keys::{
        Algorithm,
        PrivateKey,
    };

    use super::*;
    use crate::route::{
        Authenticator,
        Credentials,
        RouteKey,
    };
    use crate::session::SessionState;
    use crate::test_support::{
        Fixture,
        USER,
        route,
    };
    use crate::testing::MockSshdOptions;

    /// Signs kftray's key for `principal` the way a certificate authority would.
    struct CertAuth {
        ca: PrivateKey,
        principal: &'static str,
    }

    #[async_trait]
    impl Authenticator for CertAuth {
        async fn credentials(&self, key: &KftrayKey) -> Result<Credentials, SshError> {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let mut builder = Builder::new_with_random_nonce(
                &mut russh::keys::key::safe_rng(),
                key.public().key_data().clone(),
                now - 60,
                now + 3600,
            )
            .unwrap();
            builder.cert_type(CertType::User).unwrap();
            builder.key_id("kftray-test").unwrap();
            builder.valid_principal(self.principal).unwrap();
            Ok(Credentials::Certificate {
                user: USER.to_owned(),
                cert: builder.sign(&self.ca).unwrap(),
            })
        }
    }

    fn new_ca() -> PrivateKey {
        PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519).unwrap()
    }

    #[tokio::test]
    async fn establish_should_report_each_stage_and_log_in_with_the_key() {
        let fixture = Fixture::new().await;
        let mut stages = Vec::new();

        let lease = fixture
            .runtime
            .acquire(route(&fixture.mock, "key-login"), &mut |s| stages.push(s))
            .await
            .unwrap();

        assert_eq!(stages, [Stage::Transport, Stage::Handshake, Stage::Auth]);
        assert_eq!(*lease.state().borrow(), SessionState::Ready);
    }

    #[tokio::test]
    async fn establish_should_reject_a_key_the_sshd_does_not_know() {
        let fixture = Fixture::with(MockSshdOptions::default()).await;

        let err = fixture.acquire("unknown-key").await.err().unwrap();

        assert!(
            matches!(&err, SshError::AuthRejected { user, .. } if user == USER),
            "{err}"
        );
    }

    #[tokio::test]
    async fn establish_should_log_in_with_a_certificate_signed_by_the_user_ca() {
        let ca = new_ca();
        // The key itself is not authorized: only the certificate can log in.
        let fixture = Fixture::with(MockSshdOptions {
            user_ca: Some(ca.public_key().clone()),
            ..Default::default()
        })
        .await;
        let mut cert_route = route(&fixture.mock, "cert-login");
        cert_route.auth = Arc::new(CertAuth {
            ca,
            principal: USER,
        });

        let lease = fixture
            .runtime
            .acquire(cert_route, &mut |_| {})
            .await
            .unwrap();

        assert_eq!(*lease.state().borrow(), SessionState::Ready);
    }

    #[tokio::test]
    async fn establish_should_reject_a_certificate_from_another_ca() {
        let fixture = Fixture::with(MockSshdOptions {
            user_ca: Some(new_ca().public_key().clone()),
            ..Default::default()
        })
        .await;
        let mut cert_route = route(&fixture.mock, "foreign-cert");
        cert_route.auth = Arc::new(CertAuth {
            ca: new_ca(),
            principal: USER,
        });

        let err = fixture
            .runtime
            .acquire(cert_route, &mut |_| {})
            .await
            .err()
            .unwrap();

        assert!(matches!(err, SshError::AuthRejected { .. }), "{err}");
    }

    #[tokio::test]
    async fn establish_should_reject_a_certificate_for_another_principal() {
        let ca = new_ca();
        let fixture = Fixture::with(MockSshdOptions {
            user_ca: Some(ca.public_key().clone()),
            ..Default::default()
        })
        .await;
        let mut cert_route = route(&fixture.mock, "other-principal");
        cert_route.auth = Arc::new(CertAuth {
            ca,
            principal: "someone-else",
        });

        let err = fixture
            .runtime
            .acquire(cert_route, &mut |_| {})
            .await
            .err()
            .unwrap();

        assert!(matches!(err, SshError::AuthRejected { .. }), "{err}");
    }

    #[tokio::test]
    async fn establish_should_fail_with_host_key_mismatch_when_the_key_changed() {
        let fixture = Fixture::new().await;
        let first = fixture.acquire("rotating").await.unwrap();
        drop(first);
        fixture.mock.rotate_host_key();

        let err = fixture.acquire("rotating").await.err().unwrap();

        assert!(matches!(err, SshError::HostKeyMismatch { .. }), "{err}");
    }

    #[tokio::test]
    async fn establish_should_time_out_the_handshake_when_the_sshd_is_silent() {
        struct Silent;

        #[async_trait]
        impl crate::route::Transport for Silent {
            async fn connect(&self) -> Result<crate::BoxStream, SshError> {
                // The far end stays open and never speaks.
                let (client, server) = tokio::io::duplex(1024);
                std::mem::forget(server);
                Ok(Box::new(client))
            }
        }
        let fixture = Fixture::new().await;
        let mut silent = route(&fixture.mock, "silent");
        silent.transport = Arc::new(Silent);
        silent.key = RouteKey::new("test:silent");

        tokio::time::pause();
        let err = fixture
            .runtime
            .acquire(silent, &mut |_| {})
            .await
            .err()
            .unwrap();

        assert!(
            matches!(
                err,
                SshError::Timeout {
                    stage: Stage::Handshake,
                    ..
                }
            ),
            "{err}"
        );
    }
}
