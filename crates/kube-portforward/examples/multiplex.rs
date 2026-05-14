use std::sync::Arc;

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
    let session = Arc::new(
        client
            .session("default", "nginx", 80)
            .capacity(10)
            .open()
            .await?,
    );

    let mut handles = Vec::new();
    for i in 0..10 {
        let s = Arc::clone(&session);
        handles.push(tokio::spawn(async move {
            let mut stream = s.connect().await?;
            stream
                .write_all(format!("GET /?q={i} HTTP/1.0\r\n\r\n").as_bytes())
                .await?;
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(n)
        }));
    }

    for h in handles {
        match h.await? {
            Ok(n) => println!("stream got {n} bytes"),
            Err(e) => eprintln!("stream failed: {e}"),
        }
    }

    Arc::try_unwrap(session)
        .ok()
        .expect("sessions still shared")
        .close()
        .await?;
    Ok(())
}
