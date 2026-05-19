use kube_portforward::Client;
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let kube_client = kube::Client::try_default().await?;
    let cluster_url: http::Uri = "https://kubernetes.default.svc".parse()?;

    let client = Client::new(kube_client, cluster_url);
    let session = client.session("default", "nginx", 80).open().await?;
    println!("negotiated subprotocol: {:?}", session.protocol());

    let mut stream = session.connect().await?;
    stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    println!("got {n} bytes back");

    session.close().await?;
    Ok(())
}
