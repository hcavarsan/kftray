use std::path::{
    Path,
    PathBuf,
};

use anyhow::{
    Context,
    Result,
    anyhow,
};
use futures::future::try_join_all;
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{
    Api,
    PostParams,
};
use kube::config::{
    KubeConfigOptions,
    Kubeconfig,
};
use kube::{
    Client,
    Config,
};
use rustls::crypto::CryptoProvider;
use testcontainers_modules::k3s::{
    K3s,
    KUBE_SECURE_PORT,
};
use testcontainers_modules::testcontainers::core::{
    CmdWaitFor,
    ExecCommand,
};
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::{
    ContainerAsync,
    ImageExt,
};

use crate::harness::env::CLUSTER_READY;
use crate::harness::wait::poll;
use crate::harness::workload::{
    NAMESPACE,
    Workload,
};

const K3S_TAG: &str = "v1.33.4-k3s1";
const K3S_KUBECONFIG: &str = "/etc/rancher/k3s/k3s.yaml";

pub struct Cluster {
    _container: ContainerAsync<K3s>,
    pub kubeconfig: PathBuf,
}

impl Cluster {
    pub async fn start(workloads: &[Workload], out_dir: &Path) -> Result<Self> {
        if CryptoProvider::get_default().is_none() {
            rustls::crypto::ring::default_provider()
                .install_default()
                .map_err(|_| anyhow!("install the rustls crypto provider"))?;
        }
        std::fs::create_dir_all(out_dir)
            .with_context(|| format!("create {}", out_dir.display()))?;
        log::info!("starting k3s {K3S_TAG}");
        let container = K3s::default()
            .with_tag(K3S_TAG)
            .with_privileged(true)
            .with_userns_mode("host")
            .start()
            .await
            .context("start k3s; docker must be running and allow privileged containers")?;

        let host = container.get_host().await.context("read the docker host")?;
        let port = container
            .get_host_port_ipv4(KUBE_SECURE_PORT)
            .await
            .context("read the mapped k3s API port")?;
        let raw = container
            .exec(
                ExecCommand::new(["cat", K3S_KUBECONFIG])
                    .with_cmd_ready_condition(CmdWaitFor::exit_code(0)),
            )
            .await
            .context("read the kubeconfig k3s wrote")?
            .stdout_to_vec()
            .await
            .context("collect the k3s kubeconfig")?;
        let raw = String::from_utf8(raw).context("decode the k3s kubeconfig")?;
        let mut kubeconfig = Kubeconfig::from_yaml(&raw).context("parse the k3s kubeconfig")?;
        for named in &mut kubeconfig.clusters {
            if let Some(cluster) = named.cluster.as_mut() {
                cluster.server = Some(format!("https://{host}:{port}"));
            }
        }
        let path = out_dir.join("kubeconfig.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&kubeconfig)?)
            .with_context(|| format!("write {}", path.display()))?;

        let config = Config::from_custom_kubeconfig(kubeconfig, &KubeConfigOptions::default())
            .await
            .context("load the rewritten kubeconfig")?;
        let client = Client::try_from(config).context("build the kube client")?;
        wait_api_ready(&client).await?;
        create_namespace(&client, NAMESPACE).await?;
        try_join_all(workloads.iter().map(|w| w.apply(&client, NAMESPACE))).await?;
        try_join_all(
            workloads
                .iter()
                .map(|w| w.wait_ready(&client, NAMESPACE, CLUSTER_READY)),
        )
        .await?;
        log::info!("k3s ready, kubeconfig at {}", path.display());

        Ok(Self {
            _container: container,
            kubeconfig: path,
        })
    }
}

async fn wait_api_ready(client: &Client) -> Result<()> {
    poll(CLUSTER_READY, || async {
        client.apiserver_version().await.ok()
    })
    .await
    .with_context(|| format!("the k3s API did not answer within {CLUSTER_READY:?}"))?;
    Ok(())
}

async fn create_namespace(client: &Client, name: &str) -> Result<()> {
    let namespace = Namespace {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            ..ObjectMeta::default()
        },
        ..Namespace::default()
    };
    Api::<Namespace>::all(client.clone())
        .create(&PostParams::default(), &namespace)
        .await
        .with_context(|| format!("create namespace {name}"))?;
    Ok(())
}
