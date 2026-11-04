use std::io;
use std::net::SocketAddr;
#[cfg(unix)]
use std::os::fd::OwnedFd;

#[cfg(unix)]
use kftray_helper::{
    client::privileged_bind::{
        BindPrivilegedError,
        bind_privileged,
    },
    messages::SocketProtocol,
};
use tokio::net::{
    TcpListener,
    UdpSocket,
};
#[cfg(unix)]
use tracing::debug;

#[cfg(unix)]
const APP_ID: &str = "com.kftray.app";
const FIRST_UNPRIVILEGED_PORT: u16 = 1024;

pub(crate) async fn bind_tcp_listener(addr: SocketAddr) -> io::Result<TcpListener> {
    let denied = match TcpListener::bind(addr).await {
        Err(error) if helper_may_help(&error, addr) => error,
        result => return result,
    };

    #[cfg(unix)]
    {
        let fd = bind_through_helper(addr, SocketProtocol::Tcp, denied).await?;
        adopt_tcp(fd, addr)
    }
    #[cfg(not(unix))]
    {
        Err(denied)
    }
}

pub(crate) async fn bind_udp_socket(addr: SocketAddr) -> io::Result<UdpSocket> {
    let denied = match UdpSocket::bind(addr).await {
        Err(error) if helper_may_help(&error, addr) => error,
        result => return result,
    };

    #[cfg(unix)]
    {
        let fd = bind_through_helper(addr, SocketProtocol::Udp, denied).await?;
        adopt_udp(fd, addr)
    }
    #[cfg(not(unix))]
    {
        Err(denied)
    }
}

fn helper_may_help(error: &io::Error, addr: SocketAddr) -> bool {
    cfg!(unix)
        && error.kind() == io::ErrorKind::PermissionDenied
        && addr.port() < FIRST_UNPRIVILEGED_PORT
        && addr.ip().is_loopback()
}

#[cfg(unix)]
async fn bind_through_helper(
    addr: SocketAddr, protocol: SocketProtocol, denied: io::Error,
) -> io::Result<OwnedFd> {
    let outcome = tokio::task::spawn_blocking(move || {
        let socket_path = kftray_helper::communication::get_default_socket_path()
            .map_err(|e| BindPrivilegedError::Unavailable(e.to_string()))?;
        bind_privileged(&socket_path, APP_ID, addr.ip(), addr.port(), protocol)
    })
    .await;

    match outcome {
        Ok(Ok(fd)) => Ok(fd),
        Ok(Err(BindPrivilegedError::Os(error))) => Err(error),
        Ok(Err(error)) => {
            debug!("Helper cannot bind {addr}: {error}");
            Err(denied)
        }
        Err(error) => {
            debug!("Helper bind task for {addr} failed: {error}");
            Err(denied)
        }
    }
}

#[cfg(unix)]
fn ensure_bound_to(local: io::Result<SocketAddr>, requested: SocketAddr) -> io::Result<()> {
    let local = local?;
    if local == requested {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "the helper bound {local} instead of {requested}"
        )))
    }
}

#[cfg(unix)]
fn adopt_tcp(fd: OwnedFd, addr: SocketAddr) -> io::Result<TcpListener> {
    let listener = std::net::TcpListener::from(fd);
    listener.set_nonblocking(true)?;
    ensure_bound_to(listener.local_addr(), addr)?;
    TcpListener::from_std(listener)
}

#[cfg(unix)]
fn adopt_udp(fd: OwnedFd, addr: SocketAddr) -> io::Result<UdpSocket> {
    let socket = std::net::UdpSocket::from(fd);
    socket.set_nonblocking(true)?;
    ensure_bound_to(socket.local_addr(), addr)?;
    UdpSocket::from_std(socket)
}

#[cfg(all(test, unix))]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    const LOCALHOST: Ipv4Addr = Ipv4Addr::LOCALHOST;

    fn denied(port: u16) -> bool {
        std::net::TcpListener::bind((LOCALHOST, port))
            .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied)
    }

    #[test]
    fn only_a_denied_privileged_loopback_bind_asks_the_helper() {
        let denied_error = || io::Error::from(io::ErrorKind::PermissionDenied);
        let loopback = |port| SocketAddr::from((Ipv4Addr::new(127, 0, 0, 2), port));

        assert!(helper_may_help(&denied_error(), loopback(80)));
        assert!(helper_may_help(&denied_error(), loopback(1023)));
        assert!(!helper_may_help(&denied_error(), loopback(1024)));
        assert!(!helper_may_help(
            &denied_error(),
            SocketAddr::from(([10, 0, 0, 1], 80))
        ));
        assert!(!helper_may_help(
            &io::Error::from(io::ErrorKind::AddrInUse),
            loopback(80)
        ));
    }

    #[tokio::test]
    async fn an_unprivileged_port_binds_without_the_helper() {
        let listener = bind_tcp_listener(SocketAddr::from((LOCALHOST, 0)))
            .await
            .unwrap();
        assert_ne!(listener.local_addr().unwrap().port(), 0);

        let socket = bind_udp_socket(SocketAddr::from((LOCALHOST, 0)))
            .await
            .unwrap();
        assert_ne!(socket.local_addr().unwrap().port(), 0);
    }

    #[tokio::test]
    async fn a_busy_port_stays_an_address_in_use_error() {
        let held = std::net::TcpListener::bind((LOCALHOST, 0)).unwrap();

        let error = bind_tcp_listener(held.local_addr().unwrap())
            .await
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    }

    #[tokio::test]
    async fn an_adopted_listener_accepts_connections() {
        let std_listener = std::net::TcpListener::bind((LOCALHOST, 0)).unwrap();
        let addr = std_listener.local_addr().unwrap();

        let listener = adopt_tcp(OwnedFd::from(std_listener), addr).unwrap();
        let client = tokio::net::TcpStream::connect(addr);
        let (accepted, connected) = tokio::join!(listener.accept(), client);

        accepted.unwrap();
        connected.unwrap();
    }

    #[tokio::test]
    async fn an_adopted_udp_socket_receives_datagrams() {
        let std_socket = std::net::UdpSocket::bind((LOCALHOST, 0)).unwrap();
        let addr = std_socket.local_addr().unwrap();

        let socket = adopt_udp(OwnedFd::from(std_socket), addr).unwrap();
        let sender = tokio::net::UdpSocket::bind((LOCALHOST, 0)).await.unwrap();
        sender.send_to(b"ping", addr).await.unwrap();
        let mut buf = [0u8; 4];

        assert_eq!(socket.recv(&mut buf).await.unwrap(), 4);
    }

    #[tokio::test]
    async fn a_socket_bound_elsewhere_than_requested_is_refused() {
        let std_listener = std::net::TcpListener::bind((LOCALHOST, 0)).unwrap();
        let other = SocketAddr::from((LOCALHOST, 1));

        let error = adopt_tcp(OwnedFd::from(std_listener), other).unwrap_err();

        assert!(error.to_string().contains("instead of"), "{error}");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    mod through_the_helper {
        use std::io::{
            Read,
            Write,
        };
        use std::os::unix::net::UnixListener;

        use kftray_commons::test_utils::EnvVarGuard;
        use kftray_helper::messages::{
            HelperResponse,
            RequestCommand,
            RequestResult,
            SocketCommand,
        };

        use super::*;

        fn helper_replying(
            result: Option<RequestResult>,
        ) -> (
            tempfile::TempDir,
            EnvVarGuard,
            Option<std::thread::JoinHandle<RequestCommand>>,
        ) {
            let dir = tempfile::tempdir().unwrap();
            let socket = dir
                .path()
                .join(kftray_helper::communication::SOCKET_FILENAME);
            let env = EnvVarGuard::set("SOCKET_PATH", socket.to_str().unwrap());
            let Some(result) = result else {
                return (dir, env, None);
            };
            let listener = UnixListener::bind(&socket).unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap();
                let request: kftray_helper::messages::HelperRequest =
                    serde_json::from_slice(&buf[..n]).unwrap();
                let response = HelperResponse {
                    request_id: request.request_id,
                    result,
                };
                stream
                    .write_all(&serde_json::to_vec(&response).unwrap())
                    .unwrap();
                request.command
            });
            (dir, env, Some(server))
        }

        #[tokio::test]
        async fn a_helper_that_cannot_bind_leaves_the_original_denial() {
            if !denied(80) {
                return;
            }
            let (_dir, _env, _server) = helper_replying(None);

            let error = bind_tcp_listener(SocketAddr::from((LOCALHOST, 80)))
                .await
                .unwrap_err();

            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        }

        #[tokio::test]
        async fn an_old_helper_leaves_the_original_denial() {
            if !denied(80) {
                return;
            }
            let (_dir, _env, server) = helper_replying(Some(RequestResult::Error(
                "Failed to parse request: unknown variant `Socket`".into(),
            )));

            let error = bind_udp_socket(SocketAddr::from((LOCALHOST, 80)))
                .await
                .unwrap_err();
            server.unwrap().join().unwrap();

            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        }

        #[tokio::test]
        async fn a_port_the_helper_finds_busy_is_an_address_in_use_error() {
            if !denied(80) {
                return;
            }
            let (_dir, _env, server) =
                helper_replying(Some(RequestResult::BindError(address_in_use_errno())));

            let error = bind_tcp_listener(SocketAddr::from((LOCALHOST, 80)))
                .await
                .unwrap_err();
            let command = server.unwrap().join().unwrap();

            assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
            match command {
                RequestCommand::Socket(SocketCommand::BindPrivileged {
                    address,
                    port,
                    protocol,
                }) => {
                    assert_eq!(address, "127.0.0.1");
                    assert_eq!(port, 80);
                    assert_eq!(protocol, SocketProtocol::Tcp);
                }
                other => panic!("unexpected command {other:?}"),
            }
        }

        fn address_in_use_errno() -> i32 {
            let probe = std::net::TcpListener::bind((LOCALHOST, 0)).unwrap();
            std::net::TcpListener::bind(probe.local_addr().unwrap())
                .unwrap_err()
                .raw_os_error()
                .unwrap()
        }
    }
}
