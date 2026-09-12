use std::sync::Arc;

use anyhow::Context;
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::UdpSocket as TokioUdpSocket;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
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
        cancellation_token: CancellationToken,
    ) -> anyhow::Result<(
        u16,
        impl Future<Output = anyhow::Result<()>> + Send + 'static,
    )> {
        let local_udp_addr = format!("{local_address}:{local_port}");

        let local_udp_socket = Arc::new(
            TokioUdpSocket::bind(&local_udp_addr)
                .await
                .context("Failed to bind local UDP socket")?,
        );

        let local_port = local_udp_socket.local_addr()?.port();

        info!("Local UDP socket bound to {}", local_udp_addr);

        let (mut tcp_read, tcp_write) = tokio::io::split(upstream_conn);
        let tcp_write = Arc::new(Mutex::new(tcp_write));
        let peer: Arc<Mutex<Option<std::net::SocketAddr>>> = Arc::new(Mutex::new(None));

        let uplink_socket = local_udp_socket.clone();
        let uplink_writer = tcp_write.clone();
        let uplink_peer = peer.clone();

        let forward_future = async move {
            let uplink = async {
                let mut udp_buffer = vec![0u8; BUFFER_SIZE];
                let result: anyhow::Result<()> = loop {
                    let (len, src) = match uplink_socket.recv_from(&mut udp_buffer).await {
                        Ok(received) => received,
                        Err(e) => {
                            error!("Failed to receive from UDP socket: {:?}", e);
                            break Err(anyhow::anyhow!("Failed to receive from UDP socket: {e}"));
                        }
                    };
                    *uplink_peer.lock().await = Some(src);
                    let mut writer = uplink_writer.lock().await;
                    if let Err(e) = writer.write_all(&(len as u32).to_be_bytes()).await {
                        error!("Failed to write packet length to TCP stream: {:?}", e);
                        break Err(anyhow::anyhow!(
                            "Failed to write packet length to TCP stream: {e}"
                        ));
                    }
                    if let Err(e) = writer.write_all(&udp_buffer[..len]).await {
                        error!("Failed to write UDP packet to TCP stream: {:?}", e);
                        break Err(anyhow::anyhow!(
                            "Failed to write UDP packet to TCP stream: {e}"
                        ));
                    }
                    if let Err(e) = writer.flush().await {
                        error!("Failed to flush TCP stream: {:?}", e);
                        break Err(anyhow::anyhow!("Failed to flush TCP stream: {e}"));
                    }
                };
                result
            };

            let downlink = async {
                let result: anyhow::Result<()> = loop {
                    let packet = match Self::read_tcp_length_and_packet(&mut tcp_read).await {
                        Ok(Some(packet)) => packet,
                        Ok(None) => break Ok(()),
                        Err(e) => {
                            error!("Failed to read from TCP stream: {:?}", e);
                            break Err(anyhow::anyhow!("Failed to read from TCP stream: {e}"));
                        }
                    };
                    let Some(peer_addr) = *peer.lock().await else {
                        error!("No UDP peer to send to");
                        break Err(anyhow::anyhow!("No UDP peer to send to"));
                    };
                    if let Err(e) = local_udp_socket.send_to(&packet, &peer_addr).await {
                        error!("Failed to send UDP packet to peer: {:?}", e);
                        break Err(anyhow::anyhow!("Failed to send UDP packet to peer: {e}"));
                    }
                };
                result
            };

            let result = tokio::select! {
                result = uplink => result,
                result = downlink => result,
                _ = cancellation_token.cancelled() => {
                    info!("UDP forwarder cancelled, shutting down");
                    Ok(())
                }
            };

            if let Err(e) = tcp_write.lock().await.shutdown().await {
                error!("Error shutting down TCP writer: {:?}", e);
            }

            result
        };

        Ok((local_port, forward_future))
    }

    async fn read_tcp_length_and_packet(
        tcp_read: &mut (impl AsyncReadExt + Unpin),
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let mut len_bytes = [0u8; 4];

        match tcp_read.read_exact(&mut len_bytes).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let len = u32::from_be_bytes(len_bytes) as usize;
        if len > BUFFER_SIZE {
            return Err(anyhow::anyhow!(
                "Upstream announced a {len} byte datagram, above the {BUFFER_SIZE} byte limit"
            ));
        }
        let mut packet = vec![0u8; len];

        match tcp_read.read_exact(&mut packet).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        Ok(Some(packet))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::duplex;
    use tokio::net::UdpSocket;
    use tokio_util::sync::CancellationToken;

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
        let (mut upstream, server_stream) = duplex(1024);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            server_stream,
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        socket2::SockRef::from(&client)
            .set_send_buffer_size(BUFFER_SIZE)
            .unwrap();
        client.connect(("127.0.0.1", port)).await.unwrap();
        let request: Vec<_> = (0..60 * 1024).map(|index| (index % 251) as u8).collect();
        client.send(&request).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            assert_eq!(upstream.read_u32().await.unwrap(), request.len() as u32);
            let mut received_request = vec![0; request.len()];
            upstream.read_exact(&mut received_request).await.unwrap();
            assert_eq!(received_request, request);
            upstream.write_u32(8).await.unwrap();
            upstream.write_all(b"response").await.unwrap();
            let mut response = [0; 8];
            let received = client.recv(&mut response).await.unwrap();
            assert_eq!(&response[..received], b"response");
        })
        .await
        .unwrap();
        cancellation_token.cancel();
        tokio::time::timeout(Duration::from_secs(5), owner)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        UdpSocket::bind(("127.0.0.1", port)).await.unwrap();
    }

    #[tokio::test]
    async fn large_packet_survives_fragmented_tcp_reads() {
        let (mut reader, mut writer) = duplex(257);
        let payload: Vec<_> = (0..60 * 1024).map(|index| (index % 251) as u8).collect();
        let header = (payload.len() as u32).to_be_bytes();
        let send = async {
            writer.write_all(&header[..2]).await.unwrap();
            tokio::task::yield_now().await;
            writer.write_all(&header[2..]).await.unwrap();
            for chunk in payload.chunks(509) {
                writer.write_all(chunk).await.unwrap();
            }
        };
        let (_, received) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(send, UdpForwarder::read_tcp_length_and_packet(&mut reader))
        })
        .await
        .unwrap();
        assert_eq!(received.unwrap().unwrap(), payload);
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

        drop(writer);

        let result = tokio::time::timeout(Duration::from_secs(1), read_task).await;
        assert!(result.is_ok(), "Test timed out");
        let inner = result.unwrap().expect("Task should not panic");
        assert!(
            inner.is_ok(),
            "Should return Ok, got: {:?}",
            inner.as_ref().unwrap_err()
        );
        assert!(
            inner.unwrap().is_none(),
            "Partial read should return Ok(None)"
        );
    }

    #[tokio::test]
    async fn oversized_length_prefix_is_rejected() {
        let (mut reader, mut writer) = duplex(64);

        writer.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
        writer.flush().await.unwrap();

        let error = UdpForwarder::read_tcp_length_and_packet(&mut reader)
            .await
            .expect_err("an oversized length prefix must not allocate");
        assert!(error.to_string().contains("above the"), "{error}");
    }

    #[tokio::test]
    async fn upstream_frame_survives_concurrent_uplink_traffic() {
        let (mut upstream, server_stream) = duplex(4096);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            server_stream,
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(("127.0.0.1", port)).await.unwrap();

        client.send(b"hello").await.unwrap();
        let mut header = [0u8; 4];
        upstream.read_exact(&mut header).await.unwrap();
        let mut relayed = [0u8; 5];
        upstream.read_exact(&mut relayed).await.unwrap();
        assert_eq!(&relayed, b"hello");

        let response_header = 8u32.to_be_bytes();
        upstream.write_all(&response_header[..2]).await.unwrap();
        upstream.flush().await.unwrap();
        tokio::task::yield_now().await;

        client.send(b"second").await.unwrap();
        upstream.read_exact(&mut header).await.unwrap();
        let mut relayed = [0u8; 6];
        upstream.read_exact(&mut relayed).await.unwrap();
        assert_eq!(&relayed, b"second");

        upstream.write_all(&response_header[2..]).await.unwrap();
        upstream.write_all(b"response").await.unwrap();
        upstream.flush().await.unwrap();

        let mut response = [0u8; 8];
        let received = tokio::time::timeout(Duration::from_secs(5), client.recv(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response[..received], b"response");

        cancellation_token.cancel();
        tokio::time::timeout(Duration::from_secs(5), owner)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
