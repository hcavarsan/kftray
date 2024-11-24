use std::net::SocketAddr;
use tokio::{
    net::{TcpListener, UdpSocket},
    sync::oneshot,
    io::{AsyncReadExt, AsyncWriteExt},
};

use tokio::net::TcpStream;
use tokio::time::Duration;

pub const TEST_BUFFER_SIZE: usize = 1024;

pub struct TestServer {
    pub addr: SocketAddr,
    shutdown_tx: oneshot::Sender<()>,
}

impl TestServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
    }
}

pub async fn wait_for_port(addr: SocketAddr) -> bool {
    for _ in 0..50 {
        if let Ok(stream) = TcpStream::connect(addr).await {
            drop(stream);
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

pub async fn setup_test_tcp_echo_server() -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    if let Ok((mut socket, _)) = accept_result {
                        tokio::spawn(async move {
                            let mut buf = [0; TEST_BUFFER_SIZE];
                            while let Ok(n) = socket.read(&mut buf).await {
                                if n == 0 { break; }
                                if socket.write_all(&buf[..n]).await.is_err() {
                                    break;
                                }
                            }
                        });
                    }
                }
                _ = &mut shutdown_rx => break,
            }
        }
    });

    TestServer { addr, shutdown_tx }
}

pub async fn setup_test_udp_echo_server() -> TestServer {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    tokio::spawn(async move {
        let mut buf = vec![0; TEST_BUFFER_SIZE];
        loop {
            tokio::select! {
                result = socket.recv_from(&mut buf) => {
                    if let Ok((n, peer)) = result {
                        let _ = socket.send_to(&buf[..n], peer).await;
                    }
                }
                _ = &mut shutdown_rx => break,
            }
        }
    });

    TestServer { addr, shutdown_tx }
}
