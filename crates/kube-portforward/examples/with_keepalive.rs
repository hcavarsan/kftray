use std::time::Duration;

use kube_portforward::{
    Client,
    RecoverySignal,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let kube_client = kube::Client::try_default().await?;
    let cluster_url: http::Uri = "https://kubernetes.default.svc".parse()?;

    let client = Client::new(kube_client, cluster_url);
    let session = client
        .session("default", "nginx", 80)
        .keepalive(Duration::from_secs(5), Duration::from_secs(15))
        .on_recovery(|signal: RecoverySignal| {
            eprintln!("recovery: {signal:?}");
        })
        .open()
        .await?;

    let _stream = session.connect().await?;
    tokio::time::sleep(Duration::from_secs(30)).await;
    session.close().await?;
    Ok(())
}
