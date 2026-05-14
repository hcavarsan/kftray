use std::time::Duration;

use kube_portforward::Client;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let kube_client = kube::Client::try_default().await?;
    let cluster_url: http::Uri = "https://kubernetes.default.svc".parse()?;
    let cancel = CancellationToken::new();

    let client = Client::new(kube_client, cluster_url);
    let session = client
        .session("default", "nginx", 80)
        .shutdown_grace(Duration::from_secs(3))
        .cancellation_token(cancel.clone())
        .open()
        .await?;

    let _stream = session.connect().await?;

    tokio::spawn({
        let cancel = cancel.clone();
        async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            cancel.cancel();
        }
    });

    cancel.cancelled().await;
    session.close().await?;
    Ok(())
}
