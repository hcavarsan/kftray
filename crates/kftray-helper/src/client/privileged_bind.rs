use std::{
    io,
    net::{
        IpAddr,
        Ipv4Addr,
    },
    os::fd::OwnedFd,
    path::Path,
};

use thiserror::Error;

use super::socket_comm::send_request_with_fds;
use crate::messages::{
    RequestCommand,
    RequestResult,
    SocketCommand,
    SocketProtocol,
};

#[derive(Debug, Error)]
pub enum BindPrivilegedError {
    #[error("the helper could not bind the socket: {0}")]
    Os(io::Error),
    #[error("the installed helper cannot bind privileged ports: {0}")]
    Unsupported(String),
    #[error("the helper is not available: {0}")]
    Unavailable(String),
}

pub fn bind_privileged(
    socket_path: &Path, app_id: &str, address: IpAddr, port: u16, protocol: SocketProtocol,
) -> Result<OwnedFd, BindPrivilegedError> {
    let command = RequestCommand::Socket(SocketCommand::BindPrivileged {
        address: address.to_string(),
        port,
        protocol,
    });

    let (response, fds) = send_request_with_fds(socket_path, app_id, command)
        .map_err(|e| BindPrivilegedError::Unavailable(e.to_string()))?;

    match response.result {
        RequestResult::Success => fds.into_iter().next().ok_or_else(|| {
            BindPrivilegedError::Unsupported("the helper answered without a socket".into())
        }),
        RequestResult::BindError(errno) => {
            Err(BindPrivilegedError::Os(io::Error::from_raw_os_error(errno)))
        }
        RequestResult::Error(message) => Err(BindPrivilegedError::Unsupported(message)),
        other => Err(BindPrivilegedError::Unsupported(format!(
            "unexpected helper response: {other:?}"
        ))),
    }
}

pub fn is_outdated(socket_path: &Path, app_id: &str) -> bool {
    matches!(
        bind_privileged(
            socket_path,
            app_id,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            1,
            SocketProtocol::Tcp
        ),
        Err(BindPrivilegedError::Unsupported(_))
    )
}

#[cfg(test)]
mod tests {
    use std::{
        io::Read,
        net::TcpListener,
        os::{
            fd::AsFd,
            unix::net::{
                UnixListener,
                UnixStream,
            },
        },
        path::PathBuf,
        thread::JoinHandle,
    };

    use super::*;
    use crate::{
        fd_passing::send_with_fd,
        messages::{
            HelperRequest,
            HelperResponse,
        },
    };

    const ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

    fn fake_helper(
        reply: impl FnOnce(UnixStream) + Send + 'static,
    ) -> (tempfile::TempDir, PathBuf, JoinHandle<HelperRequest>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let request: HelperRequest = serde_json::from_slice(&buf[..n]).unwrap();
            reply(stream);
            request
        });
        (dir, path, handle)
    }

    fn answer(stream: &UnixStream, result: RequestResult) {
        let response = HelperResponse {
            request_id: String::new(),
            result,
        };
        let mut stream = stream;
        io::Write::write_all(&mut stream, &serde_json::to_vec(&response).unwrap()).unwrap();
    }

    fn bind(path: &Path) -> Result<OwnedFd, BindPrivilegedError> {
        bind_privileged(path, "com.kftray.app", ADDRESS, 80, SocketProtocol::Tcp)
    }

    #[test]
    fn only_a_helper_that_cannot_parse_the_command_is_outdated() {
        let (_dir, old, helper) = fake_helper(|stream| {
            answer(
                &stream,
                RequestResult::Error("unknown variant `Socket`".into()),
            )
        });
        assert!(is_outdated(&old, "com.kftray.app"));
        helper.join().unwrap();

        let (_dir, current, helper) =
            fake_helper(|stream| answer(&stream, RequestResult::BindError(libc::EADDRINUSE)));
        assert!(!is_outdated(&current, "com.kftray.app"));
        helper.join().unwrap();

        let dir = tempfile::tempdir().unwrap();
        assert!(!is_outdated(
            &dir.path().join("missing.sock"),
            "com.kftray.app"
        ));
    }

    #[test]
    fn an_old_helper_that_cannot_parse_the_command_is_unsupported() {
        let (_dir, path, helper) = fake_helper(|stream| {
            answer(
                &stream,
                RequestResult::Error(
                    "Failed to parse request: unknown variant `Socket`, expected one of `Network`"
                        .into(),
                ),
            )
        });

        let result = bind(&path);
        helper.join().unwrap();

        assert!(matches!(result, Err(BindPrivilegedError::Unsupported(_))));
    }

    #[test]
    fn a_failed_bind_surfaces_the_os_error() {
        let (_dir, path, helper) =
            fake_helper(|stream| answer(&stream, RequestResult::BindError(libc::EADDRINUSE)));

        let result = bind(&path);
        helper.join().unwrap();

        match result {
            Err(BindPrivilegedError::Os(error)) => {
                assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
            }
            other => panic!("expected the OS error, got {other:?}"),
        }
    }

    #[test]
    fn success_without_a_socket_is_unsupported() {
        let (_dir, path, helper) = fake_helper(|stream| answer(&stream, RequestResult::Success));

        let result = bind(&path);
        helper.join().unwrap();

        assert!(matches!(result, Err(BindPrivilegedError::Unsupported(_))));
    }

    #[test]
    fn a_missing_helper_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();

        let result = bind(&dir.path().join("missing.sock"));

        assert!(matches!(result, Err(BindPrivilegedError::Unavailable(_))));
    }

    #[test]
    fn the_bound_socket_comes_back_with_the_request_the_helper_saw() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let expected = listener.local_addr().unwrap();
        let (_dir, path, helper) = fake_helper(move |stream| {
            let bytes = serde_json::to_vec(&HelperResponse::success(String::new())).unwrap();
            send_with_fd(&stream, &bytes, listener.as_fd()).unwrap();
        });

        let fd = bind(&path).expect("the helper handed over a socket");
        let request = helper.join().unwrap();

        assert_eq!(TcpListener::from(fd).local_addr().unwrap(), expected);
        assert_eq!(request.app_id, "com.kftray.app");
        match request.command {
            RequestCommand::Socket(SocketCommand::BindPrivileged {
                address,
                port,
                protocol,
            }) => {
                assert_eq!(address, "127.0.0.2");
                assert_eq!(port, 80);
                assert_eq!(protocol, SocketProtocol::Tcp);
            }
            other => panic!("unexpected command {other:?}"),
        }
    }
}
