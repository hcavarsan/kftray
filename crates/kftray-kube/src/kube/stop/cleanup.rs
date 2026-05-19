use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kftray_commons::config_model::Config;
use kftray_commons::{
    config::get_configs,
    utils::{
        config::read_configs_with_mode,
        db_mode::DatabaseMode,
    },
};
use kube::Client;
use kube::api::{
    Api,
    DeleteParams,
    ListParams,
};
use tracing::{
    info,
    warn,
};

/// Load configs from the database, logging errors and falling back to empty
/// vec.
pub(super) async fn load_configs(mode: DatabaseMode) -> Vec<Config> {
    let result = match mode {
        DatabaseMode::File => get_configs().await,
        DatabaseMode::Memory => read_configs_with_mode(mode).await,
    };
    match result {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to read configs ({mode:?}): {e}");
            vec![]
        }
    }
}

pub(crate) async fn delete_proxy_cluster_resources(
    client: Client, namespace: &str, config_id: i64,
) {
    let username = whoami::username().unwrap_or_else(|_| "unknown".to_string());
    let pod_prefix = format!("kftray-forward-{username}");
    let lp = ListParams::default().labels(&format!("config_id={config_id}"));

    // Delete pods
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    match pods.list(&lp).await {
        Ok(pod_list) => {
            for pod in pod_list.items {
                if let Some(pod_name) = pod.metadata.name
                    && pod_name.starts_with(&pod_prefix)
                {
                    let dp = DeleteParams {
                        grace_period_seconds: Some(0),
                        ..DeleteParams::default()
                    };
                    match pods.delete(&pod_name, &dp).await {
                        Ok(_) => info!("Deleted proxy pod: {pod_name}"),
                        Err(e) => warn!("Failed to delete proxy pod {pod_name}: {e}"),
                    }
                }
            }
        }
        Err(e) => warn!("Failed to list pods for cleanup (config_id={config_id}): {e}"),
    }

    let deployments: Api<Deployment> = Api::namespaced(client, namespace);
    match deployments.list(&lp).await {
        Ok(dep_list) => {
            for dep in dep_list.items {
                if let Some(dep_name) = dep.metadata.name
                    && dep_name.starts_with(&pod_prefix)
                {
                    let dp = DeleteParams {
                        grace_period_seconds: Some(0),
                        ..DeleteParams::default()
                    };
                    match deployments.delete(&dep_name, &dp).await {
                        Ok(_) => info!("Deleted proxy deployment: {dep_name}"),
                        Err(e) => warn!("Failed to delete proxy deployment {dep_name}: {e}"),
                    }
                }
            }
        }
        Err(e) => warn!("Failed to list deployments for cleanup (config_id={config_id}): {e}"),
    }
}
