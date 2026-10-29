use std::sync::{
    Arc,
    Mutex,
    PoisonError,
};

use russh::client::{
    DisconnectReason,
    Handler,
};
use russh::keys::PublicKeyOrCertificate;
use tokio::sync::oneshot;

use crate::error::SshError;
use crate::host_keys::{
    HostKeyStore,
    verify,
};
use crate::route::HostKeySource;

/// Why the host key check failed, filled by the handler and read by
/// `establish` once russh reports the failed handshake.
pub(super) type Verdict = Arc<Mutex<Option<SshError>>>;

pub(crate) struct ClientHandler {
    label: String,
    source: Arc<dyn HostKeySource>,
    store: Arc<dyn HostKeyStore>,
    verdict: Verdict,
    closed: Option<oneshot::Sender<()>>,
}

impl ClientHandler {
    pub(super) fn new(
        label: String, source: Arc<dyn HostKeySource>, store: Arc<dyn HostKeyStore>,
        verdict: Verdict, closed: oneshot::Sender<()>,
    ) -> Self {
        Self {
            label,
            source,
            store,
            verdict,
            closed: Some(closed),
        }
    }
}

impl Handler for ClientHandler {
    // russh would only report `UnknownKey`; the typed reason travels in `verdict`.
    type Error = russh::Error;

    async fn check_server_key(
        &mut self, key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        match verify(&key.public_key(), &self.label, &*self.source, &*self.store).await {
            Ok(()) => Ok(true),
            Err(e) => {
                *self.verdict.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
                Ok(false)
            }
        }
    }

    // Called once, when the session ends for any reason after the key exchange.
    async fn disconnected(
        &mut self, reason: DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        if let Some(closed) = self.closed.take() {
            let _ = closed.send(());
        }
        match reason {
            DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            DisconnectReason::Error(e) => Err(e),
        }
    }
}
