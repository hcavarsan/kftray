use std::{
    io,
    net::{
        IpAddr,
        SocketAddr,
    },
    os::fd::OwnedFd,
};

use log::{
    error,
    info,
};
use socket2::{
    Domain,
    Protocol,
    Socket,
    Type,
};

use crate::messages::{
    HelperResponse,
    SocketCommand,
    SocketProtocol,
};

const TCP_BACKLOG: i32 = 1024;
const FIRST_UNPRIVILEGED_PORT: u16 = 1024;

pub(crate) fn validate_bind_request(address: &str, port: u16) -> Result<SocketAddr, String> {
    let ip: IpAddr = address
        .parse()
        .map_err(|_| format!("Invalid address: {address}"))?;
    if !ip.is_loopback() {
        return Err(format!("Address is not a loopback address: {address}"));
    }
    if port == 0 || port >= FIRST_UNPRIVILEGED_PORT {
        return Err(format!("Port {port} is not a privileged port"));
    }
    Ok(SocketAddr::new(ip, port))
}

fn bind_socket(addr: SocketAddr, protocol: SocketProtocol) -> io::Result<OwnedFd> {
    let socket = match protocol {
        SocketProtocol::Tcp => {
            let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
            socket.set_reuse_address(true)?;
            socket.bind(&addr.into())?;
            socket.listen(TCP_BACKLOG)?;
            socket
        }
        SocketProtocol::Udp => {
            let socket = Socket::new(Domain::for_address(addr), Type::DGRAM, Some(Protocol::UDP))?;
            socket.bind(&addr.into())?;
            socket
        }
    };
    Ok(socket.into())
}

pub(crate) fn handle_socket_command(
    command: &SocketCommand, request_id: String,
) -> (HelperResponse, Option<OwnedFd>) {
    let SocketCommand::BindPrivileged {
        address,
        port,
        protocol,
    } = command;

    let addr = match validate_bind_request(address, *port) {
        Ok(addr) => addr,
        Err(message) => {
            error!("Rejected privileged bind request: {message}");
            return (HelperResponse::error(request_id, message), None);
        }
    };

    match bind_socket(addr, *protocol) {
        Ok(fd) => {
            info!("Bound privileged {protocol:?} socket on {addr}");
            (HelperResponse::success(request_id), Some(fd))
        }
        Err(e) => {
            error!("Privileged {protocol:?} bind on {addr} failed: {e}");
            let errno = e.raw_os_error().unwrap_or(libc::EACCES);
            (HelperResponse::bind_error(request_id, errno), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{
            TcpListener,
            UdpSocket,
        },
        os::{
            fd::AsFd,
            unix::net::UnixStream,
        },
    };

    use super::*;
    use crate::{
        fd_passing,
        messages::RequestResult,
    };

    #[test]
    fn loopback_privileged_ports_are_accepted() {
        for (address, port) in [
            ("127.0.0.1", 1),
            ("127.0.0.2", 80),
            ("127.0.0.3", 443),
            ("127.255.255.254", 1023),
            ("::1", 80),
        ] {
            let addr = validate_bind_request(address, port).unwrap();
            assert_eq!(addr, SocketAddr::new(address.parse().unwrap(), port));
        }
    }

    #[test]
    fn anything_but_a_privileged_loopback_request_is_rejected() {
        for (address, port) in [
            ("0.0.0.0", 80),
            ("192.168.1.10", 80),
            ("10.0.0.1", 443),
            ("::", 80),
            ("localhost", 80),
            ("", 80),
            ("127.0.0.1", 0),
            ("127.0.0.1", 1024),
            ("127.0.0.1", 8080),
            ("127.0.0.2", 65535),
        ] {
            assert!(
                validate_bind_request(address, port).is_err(),
                "{address}:{port} must be rejected"
            );
        }
    }

    #[test]
    fn a_rejected_request_answers_with_an_error_and_no_descriptor() {
        let command = SocketCommand::BindPrivileged {
            address: "192.168.1.10".into(),
            port: 80,
            protocol: SocketProtocol::Tcp,
        };

        let (response, fd) = handle_socket_command(&command, "req-1".into());

        assert!(fd.is_none());
        assert_eq!(response.request_id, "req-1");
        assert!(matches!(response.result, RequestResult::Error(_)));
    }

    #[test]
    fn tcp_sockets_listen_and_udp_sockets_are_bound() {
        let tcp = bind_socket("127.0.0.1:0".parse().unwrap(), SocketProtocol::Tcp).unwrap();
        let tcp = TcpListener::from(tcp);
        let addr = tcp.local_addr().unwrap();
        std::net::TcpStream::connect(addr).expect("the socket is already listening");

        let udp = bind_socket("127.0.0.1:0".parse().unwrap(), SocketProtocol::Udp).unwrap();
        let udp = UdpSocket::from(udp);
        let target = udp.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender.send_to(b"x", target).unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(udp.recv(&mut buf).unwrap(), 1);
    }

    #[test]
    fn a_taken_port_reports_the_os_error_not_a_descriptor() {
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let err = bind_socket(taken.local_addr().unwrap(), SocketProtocol::Tcp).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AddrInUse);
    }

    fn receive(client: &UnixStream, total: usize) -> (Vec<u8>, Vec<OwnedFd>) {
        let mut received = Vec::new();
        let mut fds = Vec::new();
        let mut buf = [0u8; 4096];
        while received.len() < total {
            let n = fd_passing::recv_with_fds(client, &mut buf, &mut fds).unwrap();
            received.extend_from_slice(&buf[..n]);
        }
        (received, fds)
    }

    #[test]
    fn a_bound_socket_reaches_the_peer_with_the_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let expected = listener.local_addr().unwrap();
        let (server, client) = UnixStream::pair().unwrap();
        let bytes = serde_json::to_vec(&HelperResponse::success("req-1".into())).unwrap();

        fd_passing::send_with_fd(&server, &bytes, listener.as_fd()).unwrap();
        drop(listener);

        let (received, mut fds) = receive(&client, bytes.len());

        let response: HelperResponse = serde_json::from_slice(&received).unwrap();
        assert!(matches!(response.result, RequestResult::Success));
        assert_eq!(fds.len(), 1);
        let adopted = TcpListener::from(fds.remove(0));
        assert_eq!(adopted.local_addr().unwrap(), expected);
    }

    #[test]
    fn a_response_larger_than_the_socket_buffer_arrives_whole() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let expected = listener.local_addr().unwrap();
        let (server, client) = UnixStream::pair().unwrap();
        let mut bytes = serde_json::to_vec(&HelperResponse::success("req-1".into())).unwrap();
        bytes.resize(bytes.len() + 1024 * 1024, b' ');
        let total = bytes.len();

        let sender = std::thread::spawn(move || {
            fd_passing::send_with_fd(&server, &bytes, listener.as_fd()).unwrap();
        });

        let (received, mut fds) = receive(&client, total);
        sender.join().unwrap();

        assert_eq!(received.len(), total);
        assert_eq!(fds.len(), 1);
        let adopted = TcpListener::from(fds.remove(0));
        assert_eq!(adopted.local_addr().unwrap(), expected);
    }
}
