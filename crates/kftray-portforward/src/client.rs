use anyhow::{
    Context,
    Result,
};
use hyper_util::rt::TokioExecutor;
use kftray_commons::config_dir::get_default_kubeconfig_path;
use kube::{
    client::ConfigExt,
    config::{
        Config,
        KubeConfigOptions,
        Kubeconfig,
    },
    Client,
};
use log::info;
use tower::ServiceBuilder;

pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>, context_name: &str,
) -> Result<Client> {
    // Determine the kubeconfig based on the input
    let kubeconfig = if let Some(path) = kubeconfig {
        if path == "default" {
            let default_path = get_default_kubeconfig_path()?;

            info!(
                "Reading kubeconfig from default location: {:?}",
                default_path
            );

            Kubeconfig::read_from(default_path)
                .context("Failed to read kubeconfig from default location")?
        } else {
            // Otherwise, try to read the kubeconfig from the specified path
            info!("Reading kubeconfig from specified path: {}", path);

            Kubeconfig::read_from(path).context("Failed to read kubeconfig from specified path")?
        }
    } else {
        // If no kubeconfig is specified, read the default kubeconfig
        let default_path = get_default_kubeconfig_path()?;

        info!(
            "Reading kubeconfig from default location: {:?}",
            default_path
        );

        Kubeconfig::read_from(default_path)
            .context("Failed to read kubeconfig from default location")?
    };

    let config = Config::from_custom_kubeconfig(
        kubeconfig,
        &KubeConfigOptions {
            context: Some(context_name.to_owned()),
            ..Default::default()
        },
    )
    .await
    .context("Failed to create configuration from kubeconfig")?;

    let https_connector = config
        .rustls_https_connector()
        .context("Failed to create Rustls HTTPS connector")?;

    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .option_layer(config.auth_layer()?)
        .service(
            hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(https_connector),
        );

    let client = Client::new(service, config.default_namespace);

    Ok(client)
}
