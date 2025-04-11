use std::sync::Arc;

use anyhow::Context;
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::UdpSocket as TokioUdpSocket;
use tokio::sync::Mutex;
use tracing::{
    error,
    info,
};

const BUFFER_SIZE: usize = 131072;

pub struct UdpForwarder;

impl UdpForwarder {
    pub async fn bind_and_forward(
        local_address: String, local_port: u16,
        upstream_conn: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    ) -> anyhow::Result<(u16, tokio::task::JoinHandle<()>)> {
        let local_udp_addr = format!("{}:{}", local_address, local_port);

        let local_udp_socket = Arc::new(
            TokioUdpSocket::bind(&local_udp_addr)
                .await
                .context("Failed to bind local UDP socket")?,
        );

        let local_port = local_udp_socket.local_addr()?.port();

        info!("Local UDP socket bound to {}", local_udp_addr);

        let (tcp_read, tcp_write) = tokio::io::split(upstream_conn);
        let tcp_read = Arc::new(Mutex::new(tcp_read));
        let tcp_write = Arc::new(Mutex::new(tcp_write));

        let local_udp_socket_read = local_udp_socket.clone();
        let local_udp_socket_write = local_udp_socket;

        let handle = tokio::spawn({
            let tcp_read = tcp_read.clone();
            let tcp_write = tcp_write.clone();
            async move {
                let mut udp_buffer = [0u8; BUFFER_SIZE];
                let mut peer: Option<std::net::SocketAddr> = None;

                loop {
                    tokio::select! {
                        result = local_udp_socket_read.recv_from(&mut udp_buffer) => {
                            match result {
                                Ok((len, src)) => {
                                    peer = Some(src);
                                    let mut writer = tcp_write.lock().await;

                                    let packet_len = (len as u32).to_be_bytes();
                                    if let Err(e) = writer.write_all(&packet_len).await {
                                        error!("Failed to write packet length to TCP stream: {:?}", e);
                                        break;
                                    }
                                    if let Err(e) = writer.write_all(&udp_buffer[..len]).await {
                                        error!("Failed to write UDP packet to TCP stream: {:?}", e);
                                        break;
                                    }
                                    if let Err(e) = writer.flush().await {
                                        error!("Failed to flush TCP stream: {:?}", e);
                                        break;
                                    }
                                },
                                Err(e) => {
                                    error!("Failed to receive from UDP socket: {:?}", e);
                                    break;
                                }
                            }
                        },
                        result = async {
                            let mut reader = tcp_read.lock().await;
                            Self::read_tcp_length_and_packet(&mut *reader).await
                        } => {
                            match result {
                                Ok(Some(packet)) => {
                                    if let Some(peer) = peer {
                                        if let Err(e) = local_udp_socket_write.send_to(&packet, &peer).await {
                                            error!("Failed to send UDP packet to peer: {:?}", e);
                                            break;
                                        }
                                    } else {
                                        error!("No UDP peer to send to");
                                        break;
                                    }
                                },
                                Ok(None) => break,
                                Err(e) => {
                                    error!("Failed to read from TCP stream: {:?}", e);
                                    break;
                                }
                            }
                        }
                    }
                }

                if let Err(e) = tcp_write.lock().await.shutdown().await {
                    error!("Error shutting down TCP writer: {:?}", e);
                }
            }
        });

        Ok((local_port, handle))
    }

    async fn read_tcp_length_and_packet(
        tcp_read: &mut (impl AsyncReadExt + Unpin),
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let mut len_bytes = [0u8; 4];

        if tcp_read.read_exact(&mut len_bytes).await.is_err() {
            return Ok(None);
        }

        let len = u32::from_be_bytes(len_bytes) as usize;

        let mut packet = vec![0u8; len];

        if tcp_read.read_exact(&mut packet).await.is_err() {
            return Ok(None);
        }

        Ok(Some(packet))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::duplex;
    use tokio::net::UdpSocket;

    use super::*;

    #[tokio::test]
    async fn test_read_tcp_length_and_packet() {
        let (mut reader, mut writer) = duplex(1024);

        let packet = b"hello world";
        let len = packet.len() as u32;
        let len_bytes = len.to_be_bytes();

        writer.write_all(&len_bytes).await.unwrap();
        writer.write_all(packet).await.unwrap();
        writer.flush().await.unwrap();

        let result = UdpForwarder::read_tcp_length_and_packet(&mut reader).await;
        assert!(result.is_ok());

        let data = result.unwrap();
        assert!(data.is_some());
        assert_eq!(data.unwrap(), packet);
    }

    #[tokio::test]
    async fn test_read_tcp_length_and_packet_empty() {
        let (mut reader, _) = duplex(0);

        let result = UdpForwarder::read_tcp_length_and_packet(&mut reader).await;
        assert!(result.is_ok());

        let data = result.unwrap();
        assert!(data.is_none());
    }

    #[tokio::test]
    async fn test_bind_and_forward_basic() {
        let (_, server_stream) = duplex(1024);

        let result =
            UdpForwarder::bind_and_forward("127.0.0.1".to_string(), 0, server_stream).await;

        assert!(result.is_ok());

        let (port, handle) = result.unwrap();
        assert!(port > 0);

        let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client_socket
            .connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();

        client_socket.send(b"hello").await.unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        handle.abort();
    }

    #[tokio::test]
    async fn test_read_tcp_length_and_packet_partial() {
        let (mut reader, mut writer) = duplex(1024);

        let packet = b"hello world";
        let len = packet.len() as u32;
        let len_bytes = len.to_be_bytes();

        writer.write_all(&len_bytes).await.unwrap();
        writer.write_all(&packet[0..5]).await.unwrap();
        writer.flush().await.unwrap();

        let read_task =
            tokio::spawn(
                async move { UdpForwarder::read_tcp_length_and_packet(&mut reader).await },
            );

        tokio::time::sleep(Duration::from_millis(100)).await;

        drop(writer);

        let result = tokio::time::timeout(Duration::from_secs(1), read_task).await;
        assert!(result.is_ok(), "Test timed out");
    }
}
