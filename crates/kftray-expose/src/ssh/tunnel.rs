use std::fs;
use std::net::{
    IpAddr,
    Ipv4Addr,
    SocketAddr,
};

use async_ssh2_lite::{
    AsyncListener,
    AsyncSession,
    SessionConfiguration,
    TokioTcpStream,
};
use async_trait::async_trait;
use log::{
    debug,
    info,
};

use crate::{
    config::TunnelConfig,
    error::*,
    ssh::connection::ConnectionHandler,
};

#[async_trait]
pub trait TunnelService {
    async fn connect(&mut self) -> TunnelResult<()>;
    async fn setup_forward(&mut self) -> TunnelResult<()>;
    async fn run(&mut self) -> TunnelResult<()>;
}

pub struct SshTunnel {
    config: TunnelConfig,
    session: Option<AsyncSession<TokioTcpStream>>,
    remote_listener: Option<AsyncListener<TokioTcpStream>>,
}

impl SshTunnel {
    pub fn new(config: TunnelConfig) -> Self {
        Self {
            config,
            session: None,
            remote_listener: None,
        }
    }

    async fn create_session(&self) -> TunnelResult<AsyncSession<TokioTcpStream>> {
        let mut session_config = SessionConfiguration::new();
        session_config.set_keepalive(true, 30);

        let stream = TokioTcpStream::connect(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            self.config.local_port,
        ))
        .await?;

        AsyncSession::new(stream, Some(session_config)).map_err(|e| TunnelError::Ssh(e.to_string()))
    }

    async fn authenticate_session(
        session: &mut AsyncSession<TokioTcpStream>, key_path: &std::path::Path,
    ) -> TunnelResult<()> {
        let key_contents = fs::read_to_string(key_path)?;

        match session
            .userauth_pubkey_file("root", None, key_path, None)
            .await
        {
            Ok(_) => {
                info!("Public key authentication successful");
                Ok(())
            }
            Err(e) => {
                debug!("Falling back to memory-based authentication: {}", e);
                session
                    .userauth_pubkey_memory("root", None, &key_contents, None)
                    .await
                    .map_err(|e| TunnelError::Ssh(format!("Authentication failed: {}", e)))
            }
        }?;

        if !session.authenticated() {
            return Err(TunnelError::Other(anyhow::anyhow!(
                "SSH authentication failed"
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl TunnelService for SshTunnel {
    async fn connect(&mut self) -> TunnelResult<()> {
        let mut session = self.create_session().await?;
        session.handshake().await?;
        Self::authenticate_session(&mut session, &self.config.ssh_key_path).await?;
        self.session = Some(session);
        Ok(())
    }

    async fn setup_forward(&mut self) -> TunnelResult<()> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| TunnelError::Other(anyhow::anyhow!("Session not initialized")))?;

        let (remote_listener, actual_port) = session
            .channel_forward_listen(self.config.remote_port, None, None)
            .await?;

        info!("Remote port forwarding established on port {}", actual_port);
        self.remote_listener = Some(remote_listener);
        Ok(())
    }

    async fn run(&mut self) -> TunnelResult<()> {
        if let Some(ref mut listener) = self.remote_listener {
            let handler = ConnectionHandler::new(self.config.clone());
            handler.handle_connections(listener).await?;
        }
        Ok(())
    }
}
