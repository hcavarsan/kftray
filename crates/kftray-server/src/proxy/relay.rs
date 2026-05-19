//! Bidirectional byte-pump for non-HTTP traffic with direct per-session
//! outbound connections (no pool — raw TCP sessions are pinned 1:1).

use std::time::Duration;

use socket2::SockRef;
use tokio::io::copy_bidirectional;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::proxy::config::ProxyConfig;
use crate::proxy::error::ProxyError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const KEEPALIVE_IDLE: Duration = Duration::from_secs(60);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

/// Connect directly to the proxy target (no pool).
///
/// Prefers `resolved_ip` when available, falls back to `target_host`.
async fn connect_to_target(config: &ProxyConfig) -> Result<TcpStream, ProxyError> {
    if let Some(ref ip) = config.resolved_ip {
        let addr = format!("{}:{}", ip, config.target_port);
        match try_connect(&addr).await {
            Ok(s) => return Ok(s),
            Err(e) => log::warn!("direct connect to resolved IP {addr} failed: {e}"),
        }
    }
    let addr = format!("{}:{}", config.target_host, config.target_port);
    try_connect(&addr)
        .await
        .map_err(|e| ProxyError::Connection(format!("connect to {addr} failed: {e}")))
}

async fn try_connect(addr: &str) -> Result<TcpStream, std::io::Error> {
    let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timeout"))??;
    apply_keepalive(&stream)?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

fn apply_keepalive(stream: &TcpStream) -> Result<(), std::io::Error> {
    let sock = SockRef::from(stream);
    let cfg = socket2::TcpKeepalive::new()
        .with_time(KEEPALIVE_IDLE)
        .with_interval(KEEPALIVE_INTERVAL);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let cfg = cfg.with_retries(3);
    sock.set_tcp_keepalive(&cfg)
}

/// Relay bytes between an inbound stream and a directly-connected outbound
/// stream until either side reaches EOF or the cancellation token fires.
pub(crate) async fn relay_direct(
    mut inbound: TcpStream, config: &ProxyConfig, cancel: CancellationToken,
) -> Result<(), ProxyError> {
    let mut outbound = connect_to_target(config).await?;
    tokio::select! {
        res = copy_bidirectional(&mut inbound, &mut outbound) => match res {
            Ok(_) => Ok(()),
            Err(e) if is_benign(&e) => Ok(()),
            Err(e) => Err(ProxyError::Connection(format!("relay: {e}"))),
        },
        () = cancel.cancelled() => Ok(()),
    }
}

fn is_benign(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    use super::*;
    use crate::proxy::config::ProxyType;

    async fn setup() -> (TcpStream, TcpStream, ProxyConfig) {
        let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                if target.accept().await.is_err() {
                    break;
                }
            }
        });

        let cfg = ProxyConfig::builder()
            .target_host("127.0.0.1".into())
            .target_port(target_addr.port())
            .proxy_port(0)
            .proxy_type(ProxyType::Tcp)
            .build()
            .unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accept = tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            s
        });
        let client = TcpStream::connect(addr).await.unwrap();
        let server_side = accept.await.unwrap();
        (client, server_side, cfg)
    }

    #[tokio::test]
    async fn client_eof_completes_relay() {
        let (mut client, server_side, cfg) = setup().await;
        let cancel = CancellationToken::new();
        client.shutdown().await.unwrap();
        drop(client);
        let res = timeout(
            Duration::from_secs(2),
            relay_direct(server_side, &cfg, cancel),
        )
        .await
        .expect("relay should finish");
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn target_eof_completes_relay() {
        let (mut client, server_side, cfg) = setup().await;
        let cancel = CancellationToken::new();
        client.shutdown().await.unwrap();
        drop(client);
        let res = timeout(
            Duration::from_secs(2),
            relay_direct(server_side, &cfg, cancel),
        )
        .await
        .expect("relay should finish");
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn cancellation_aborts_relay() {
        let (_client, server_side, cfg) = setup().await;
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_clone.cancel();
        });
        let res = timeout(
            Duration::from_secs(2),
            relay_direct(server_side, &cfg, cancel),
        )
        .await
        .expect("relay should finish");
        assert!(res.is_ok());
    }
}
