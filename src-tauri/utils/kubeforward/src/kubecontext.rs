use anyhow::Result;
use kube::{
    config::{Config, KubeConfigOptions},
    Client,
};

pub async fn create_client_with_specific_context(context_name: &str) -> Result<Client> {
    let config_options = KubeConfigOptions {
        context: Some(context_name.to_owned()), // Add the context name to the options
        ..Default::default()
    };

    // Here is where you need to make the change
    let config = Config::from_kubeconfig(&config_options).await?;
    let client = Client::try_from(config)?; // use try_from instead of from
    Ok(client)
}
