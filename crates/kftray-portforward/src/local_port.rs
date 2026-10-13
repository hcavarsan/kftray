use std::io::ErrorKind;
use std::net::{
    IpAddr,
    TcpListener,
    UdpSocket,
};

use crate::kube::listener::Protocol;

const FREE_PORT_SEARCH: u16 = 100;
const FREE_PORT_OFFER: &str = ". Port ";

fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Tcp => "TCP",
        Protocol::Udp => "UDP",
    }
}

fn bind(ip: IpAddr, port: u16, protocol: Protocol) -> std::io::Result<u16> {
    match protocol {
        Protocol::Tcp => TcpListener::bind((ip, port))?.local_addr(),
        Protocol::Udp => UdpSocket::bind((ip, port))?.local_addr(),
    }
    .map(|addr| addr.port())
}

/// The first port after `busy` that binds on `ip`, or one the OS picks when
/// none of the next ports is free.
fn next_free_port(ip: IpAddr, busy: u16, protocol: Protocol) -> Option<u16> {
    (busy.saturating_add(1)..=u16::MAX)
        .take(usize::from(FREE_PORT_SEARCH))
        .find(|&port| bind(ip, port, protocol).is_ok())
        .or_else(|| bind(ip, 0, protocol).ok())
}

/// The error for a local port another process holds. It names a free port the
/// clients offer to start on instead; [`free_port_in_error`] reads it back.
pub(crate) fn port_in_use(ip: IpAddr, port: u16, protocol: Protocol) -> anyhow::Error {
    let name = protocol_name(protocol);
    match next_free_port(ip, port, protocol) {
        Some(free) => {
            anyhow::anyhow!(
                "Local {name} port {port} on {ip} is already in use{FREE_PORT_OFFER}{free} is free."
            )
        }
        None => anyhow::anyhow!("Local {name} port {port} on {ip} is already in use."),
    }
}

/// The error for a local port the operating system refuses to bind: a
/// privileged port without the privilege, or a range Windows reserves. No
/// other process holds it, so the message does not say so, but it still
/// names a port that binds for the clients to offer.
pub(crate) fn bind_denied(address: &str, port: u16, protocol: Protocol) -> anyhow::Error {
    let name = protocol_name(protocol);
    let offer = address
        .parse::<IpAddr>()
        .ok()
        .and_then(|ip| next_free_port(ip, port, protocol));
    match offer {
        Some(free) => anyhow::anyhow!(
            "Local {name} port {port} on {address} cannot be bound: access denied or the port \
             is reserved by the system{FREE_PORT_OFFER}{free} is free."
        ),
        None => anyhow::anyhow!(
            "Local {name} port {port} on {address} cannot be bound: access denied or the port \
             is reserved by the system."
        ),
    }
}

/// The error for a local address that is not configured on this machine,
/// such as a loopback alias that was never set up.
pub(crate) fn address_unavailable(address: &str, port: u16, protocol: Protocol) -> anyhow::Error {
    let name = protocol_name(protocol);
    anyhow::anyhow!(
        "Local {name} port {port} cannot be bound: address {address} is not configured on this \
         machine."
    )
}

/// Fails before any cluster call when another process already holds the local
/// port or the system refuses to bind it. Every other bind error is left to
/// the real bind, which reports it.
pub(crate) fn ensure_free(address: &str, port: u16, protocol: Protocol) -> anyhow::Result<()> {
    if port == 0 {
        return Ok(());
    }
    let Ok(ip) = address.parse::<IpAddr>() else {
        return Ok(());
    };
    match bind(ip, port, protocol) {
        Err(error) if error.kind() == ErrorKind::AddrInUse => Err(port_in_use(ip, port, protocol)),
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            Err(bind_denied(address, port, protocol))
        }
        _ => Ok(()),
    }
}

/// The free port a bind error named, so a client can offer to start on it.
pub fn free_port_in_error(message: &str) -> Option<u16> {
    let (_, rest) = message.split_once(FREE_PORT_OFFER)?;
    let (port, _) = rest.split_once(" is free")?;
    port.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    #[test]
    fn a_busy_tcp_port_names_a_free_port_that_binds() {
        let held = TcpListener::bind((LOCALHOST, 0)).unwrap();
        let busy = held.local_addr().unwrap().port();

        let error = ensure_free("127.0.0.1", busy, Protocol::Tcp)
            .unwrap_err()
            .to_string();

        assert!(
            error.starts_with(&format!(
                "Local TCP port {busy} on 127.0.0.1 is already in use."
            )),
            "{error}"
        );
        let free = free_port_in_error(&error).unwrap();
        assert_ne!(free, busy);
        TcpListener::bind((LOCALHOST, free)).unwrap();
    }

    #[test]
    fn a_denied_port_offers_a_free_port_without_blaming_another_process() {
        for protocol in [Protocol::Tcp, Protocol::Udp] {
            let denied = bind(LOCALHOST, 80, protocol)
                .is_err_and(|error| error.kind() == ErrorKind::PermissionDenied);
            if !denied {
                continue;
            }

            let message = ensure_free("127.0.0.1", 80, protocol)
                .unwrap_err()
                .to_string();

            assert!(
                message.contains("access denied or the port is reserved"),
                "{message}"
            );
            assert!(!message.contains("already in use"), "{message}");
            let free = free_port_in_error(&message).expect(&message);
            assert!(bind(LOCALHOST, free, protocol).is_ok(), "{message}");
            assert_eq!(
                kftray_telemetry::classify(&message),
                kftray_telemetry::FailureKind::BindDenied,
                "{message}"
            );
        }
    }

    #[test]
    fn a_denied_port_on_an_unparsed_address_offers_nothing() {
        let message = bind_denied("localhost", 80, Protocol::Tcp).to_string();

        assert!(free_port_in_error(&message).is_none(), "{message}");
        assert_eq!(
            kftray_telemetry::classify(&message),
            kftray_telemetry::FailureKind::BindDenied,
            "{message}"
        );
    }

    #[test]
    fn an_address_this_machine_lacks_has_its_own_message_and_kind() {
        let lacks = Ipv4Addr::new(192, 0, 2, 1);
        let error = TcpListener::bind((lacks, 8080)).err();
        if error.map(|error| error.kind()) != Some(ErrorKind::AddrNotAvailable) {
            return;
        }

        let message = address_unavailable("192.0.2.1", 8080, Protocol::Tcp).to_string();

        assert_eq!(
            message,
            "Local TCP port 8080 cannot be bound: address 192.0.2.1 is not configured on this \
             machine."
        );
        assert!(free_port_in_error(&message).is_none(), "{message}");
        assert_eq!(
            kftray_telemetry::classify(&message),
            kftray_telemetry::FailureKind::AddressAllocation,
            "{message}"
        );
    }

    #[test]
    fn a_busy_udp_port_names_a_free_udp_port() {
        let held = UdpSocket::bind((LOCALHOST, 0)).unwrap();
        let busy = held.local_addr().unwrap().port();

        let error = ensure_free("127.0.0.1", busy, Protocol::Udp)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains(&format!("Local UDP port {busy} ")),
            "{error}"
        );
        let free = free_port_in_error(&error).unwrap();
        UdpSocket::bind((LOCALHOST, free)).unwrap();
    }

    #[test]
    fn a_free_port_or_an_unparsed_address_passes_the_check() {
        let probe = TcpListener::bind((LOCALHOST, 0)).unwrap();
        let free = probe.local_addr().unwrap().port();
        drop(probe);

        assert!(ensure_free("127.0.0.1", free, Protocol::Tcp).is_ok());
        assert!(ensure_free("127.0.0.1", 0, Protocol::Tcp).is_ok());
        assert!(ensure_free("localhost", free, Protocol::Tcp).is_ok());
    }

    #[test]
    fn the_last_port_falls_back_to_a_port_the_os_picks() {
        assert!(next_free_port(LOCALHOST, u16::MAX, Protocol::Tcp).is_some());
    }

    #[test]
    fn the_free_port_is_read_back_from_a_wrapped_start_error() {
        let wrapped = "Failed to start port forward: \"Failed to start TCP port forwarding for \
                       service web: Local TCP port 8080 on 127.0.0.1 is already in use. Port \
                       8081 is free.; cleanup incomplete: x\"";

        assert_eq!(free_port_in_error(wrapped), Some(8081));
        assert_eq!(
            free_port_in_error("Local TCP port 8080 on 127.0.0.1 is already in use."),
            None
        );
        assert_eq!(
            free_port_in_error("Failed to bind TCP listener: address in use"),
            None
        );
    }
}
