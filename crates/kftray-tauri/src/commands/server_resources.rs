use std::collections::HashMap;

use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::Pod,
    core::v1::Service,
    networking::v1::Ingress,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_portforward::kube::client::create_client_with_specific_context;
use kube::api::{
    Api,
    DeleteParams,
    ListParams,
};
use kube::{
    Client,
    ResourceExt,
};
use log::{
    error,
    info,
};
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerResource {
    pub resource_type: String,
    pub name: String,
    pub namespace: String,
    pub config_id: Option<String>,
    pub is_orphaned: bool,
    pub age: String,
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NamespaceGroup {
    pub namespace: String,
    pub resources: Vec<ServerResource>,
}

#[tauri::command]
pub async fn list_all_kftray_resources(
    context_name: &str, kubeconfig: Option<String>,
) -> Result<Vec<NamespaceGroup>, String> {
    info!(
        "Listing all kftray-server resources for context: {}",
        context_name
    );

    let connection = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| format!("Failed to create client for context '{context_name}': {err}"))?;

    let client = connection.client;
    let installation_id = kftray_commons::utils::config_dir::installation_id().await?;

    let username = whoami::username()
        .unwrap_or_else(|_| "unknown".to_string())
        .to_lowercase();
    let clean_username: String = username
        .chars()
        .filter(|c: &char| c.is_alphanumeric())
        .collect();

    info!(
        "Filtering kftray resources for user: {} in context: {}",
        clean_username, context_name
    );

    let configs = kftray_commons::config::get_configs()
        .await
        .unwrap_or_default();

    let context_configs: Vec<_> = configs
        .iter()
        .filter(|c| {
            c.context
                .as_ref()
                .map(|ctx| ctx == context_name)
                .unwrap_or(false)
        })
        .collect();

    let config_ids: Vec<String> = context_configs
        .iter()
        .filter_map(|c| c.id.map(|id| id.to_string()))
        .collect();

    let namespaces: Vec<String> = context_configs
        .iter()
        .map(|c| c.namespace.as_str())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    info!(
        "Checking {} unique namespaces from configs in context {}",
        namespaces.len(),
        context_name
    );

    let mut namespace_resources: HashMap<String, Vec<ServerResource>> = HashMap::new();

    for namespace in namespaces {
        let mut resources = Vec::new();

        resources.extend(
            list_pods_in_namespace(
                &client,
                &namespace,
                &clean_username,
                &config_ids,
                installation_id,
            )
            .await
            .unwrap_or_default(),
        );

        let user_deployments = list_deployments_in_namespace(
            &client,
            &namespace,
            &clean_username,
            &config_ids,
            installation_id,
        )
        .await
        .unwrap_or_default();

        let deployment_config_ids: Vec<String> = user_deployments
            .iter()
            .filter_map(|d| d.config_id.clone())
            .collect();

        resources.extend(user_deployments);

        resources.extend(
            list_services_in_namespace(
                &client,
                &namespace,
                &deployment_config_ids,
                &config_ids,
                installation_id,
            )
            .await
            .unwrap_or_default(),
        );
        resources.extend(
            list_ingresses_in_namespace(
                &client,
                &namespace,
                &deployment_config_ids,
                &config_ids,
                installation_id,
            )
            .await
            .unwrap_or_default(),
        );

        if !resources.is_empty() {
            namespace_resources.insert(namespace, resources);
        }
    }

    let mut namespace_groups: Vec<NamespaceGroup> = namespace_resources
        .into_iter()
        .map(|(namespace, resources)| NamespaceGroup {
            namespace,
            resources,
        })
        .collect();

    namespace_groups.sort_by(|a, b| a.namespace.cmp(&b.namespace));

    info!(
        "Found {} namespaces with kftray resources",
        namespace_groups.len()
    );

    Ok(namespace_groups)
}

/// Whether a resource may be shown and deleted from this screen.
///
/// A name prefix is not proof of ownership: it truncates the username, so two
/// users, or two installations of one user, can produce the same one. A
/// resource labelled with another installation's id is theirs and is hidden.
/// One with no such label predates the label and cannot be attributed, and
/// this screen is exactly where those are meant to be removed by hand.
fn belongs_here(
    labels: &std::collections::BTreeMap<String, String>, installation_id: &str,
) -> bool {
    labels
        .get(kftray_portforward::kube::INSTALLATION_LABEL)
        .is_none_or(|owner| owned_by(owner, installation_id))
}

/// Whether an ownership label names this installation. A memory-mode run
/// extends the identity with a process suffix; its resources are still this
/// installation's to manage.
fn owned_by(owner: &str, installation_id: &str) -> bool {
    owner == installation_id
        || owner
            .strip_prefix(installation_id)
            .is_some_and(|rest| rest.starts_with("-m"))
}

/// Whether a resource is this installation's to show, by label or, for
/// resources that predate the label, by name.
///
/// The label is the positive evidence: a customized manifest can carry any
/// name, and the username the prefix was derived from can change, and the
/// label still says the resource is ours. Name matching only covers what was
/// created before the label existed.
fn is_ours(
    name: &str, labels: &std::collections::BTreeMap<String, String>, username: &str,
    installation_id: &str,
) -> bool {
    match labels.get(kftray_portforward::kube::INSTALLATION_LABEL) {
        Some(owner) => owned_by(owner, installation_id),
        None => {
            is_forward_name(name, username)
                || name.starts_with(&format!("kftray-expose-{username}"))
        }
    }
}

/// Whether `name` is a relay this application could have created for the
/// current user, under the current naming rule or the one before it.
///
/// The current prefix truncates the username to keep the value valid as a
/// label; relays from before that carry the full sanitized username and
/// would otherwise vanish from the one screen meant to remove them by hand.
fn is_forward_name(name: &str, username: &str) -> bool {
    name.starts_with(&kftray_portforward::kube::proxy_resource_prefix())
        || name.starts_with(&format!("kftray-forward-{username}-"))
}

async fn list_pods_in_namespace(
    client: &Client, namespace: &str, username: &str, config_ids: &[String], installation_id: &str,
) -> Result<Vec<ServerResource>, String> {
    let pods_api: Api<Pod> = Api::namespaced(client.clone(), namespace);

    let pods = pods_api
        .list(&ListParams::default())
        .await
        .map_err(|e| format!("Failed to list pods: {e}"))?;

    Ok(pods
        .items
        .into_iter()
        .filter_map(|pod| {
            let pod_name = pod.name_any();

            if !is_ours(&pod_name, pod.labels(), username, installation_id) {
                return None;
            }

            let config_id = pod.labels().get("config_id").map(|s| s.to_string());

            let is_orphaned = config_id
                .as_ref()
                .map(|id| !config_ids.contains(id))
                .unwrap_or(true);

            let age = pod
                .metadata
                .creation_timestamp
                .as_ref()
                .map(calculate_age)
                .unwrap_or_else(|| "unknown".to_string());

            let status = pod
                .status
                .as_ref()
                .and_then(|s| s.phase.as_ref())
                .unwrap_or(&"Unknown".to_string())
                .clone();

            Some(ServerResource {
                resource_type: "pod".to_string(),
                name: pod_name,
                namespace: namespace.to_string(),
                config_id,
                is_orphaned,
                age,
                status,
            })
        })
        .collect())
}

async fn list_deployments_in_namespace(
    client: &Client, namespace: &str, username: &str, config_ids: &[String], installation_id: &str,
) -> Result<Vec<ServerResource>, String> {
    let deployments_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    // Unfiltered by label: a relay Deployment carries its own hashed name as
    // `app`, not a shared value, so listing only `app=kftray-expose` hid every
    // proxy relay from the screen that exists to remove them by hand.
    let deployments = deployments_api
        .list(&ListParams::default())
        .await
        .map_err(|e| format!("Failed to list deployments: {e}"))?;

    Ok(deployments
        .items
        .into_iter()
        .filter(|deployment| {
            let name = deployment.name_any();
            is_ours(&name, deployment.labels(), username, installation_id)
        })
        .map(|deployment| {
            let config_id = deployment.labels().get("config_id").map(|s| s.to_string());

            let is_orphaned = config_id
                .as_ref()
                .map(|id| !config_ids.contains(id))
                .unwrap_or(true);

            let age = deployment
                .metadata
                .creation_timestamp
                .as_ref()
                .map(calculate_age)
                .unwrap_or_else(|| "unknown".to_string());

            let available_replicas = deployment
                .status
                .as_ref()
                .and_then(|s| s.available_replicas)
                .unwrap_or(0);
            let replicas = deployment
                .status
                .as_ref()
                .and_then(|s| s.replicas)
                .unwrap_or(0);

            ServerResource {
                resource_type: "deployment".to_string(),
                name: deployment.name_any(),
                namespace: namespace.to_string(),
                config_id,
                is_orphaned,
                age,
                status: format!("{}/{} replicas", available_replicas, replicas),
            }
        })
        .collect())
}

async fn list_services_in_namespace(
    client: &Client, namespace: &str, deployment_config_ids: &[String], config_ids: &[String],
    installation_id: &str,
) -> Result<Vec<ServerResource>, String> {
    let services_api: Api<Service> = Api::namespaced(client.clone(), namespace);
    let lp = ListParams::default().labels("app=kftray-expose");

    let services = services_api
        .list(&lp)
        .await
        .map_err(|e| format!("Failed to list services: {e}"))?;

    Ok(services
        .items
        .into_iter()
        .filter_map(|service| {
            if !belongs_here(service.labels(), installation_id) {
                return None;
            }
            let config_id = service.labels().get("config_id").map(|s| s.to_string());

            // A service whose deployment is gone is exactly what a partial
            // cleanup leaves behind, and what a start then refuses to run next
            // to. It is listed as orphaned so it can be removed from here.
            let is_orphaned = config_id
                .as_ref()
                .map(|id| !config_ids.contains(id) || !deployment_config_ids.contains(id))
                .unwrap_or(true);

            let age = service
                .metadata
                .creation_timestamp
                .as_ref()
                .map(calculate_age)
                .unwrap_or_else(|| "unknown".to_string());

            let cluster_ip = service
                .spec
                .as_ref()
                .and_then(|s| s.cluster_ip.as_ref())
                .unwrap_or(&"None".to_string())
                .clone();

            Some(ServerResource {
                resource_type: "service".to_string(),
                name: service.name_any(),
                namespace: namespace.to_string(),
                config_id,
                is_orphaned,
                age,
                status: cluster_ip,
            })
        })
        .collect())
}

async fn list_ingresses_in_namespace(
    client: &Client, namespace: &str, deployment_config_ids: &[String], config_ids: &[String],
    installation_id: &str,
) -> Result<Vec<ServerResource>, String> {
    let ingresses_api: Api<Ingress> = Api::namespaced(client.clone(), namespace);
    let lp = ListParams::default().labels("app=kftray-expose");

    let ingresses = ingresses_api
        .list(&lp)
        .await
        .map_err(|e| format!("Failed to list ingresses: {e}"))?;

    Ok(ingresses
        .items
        .into_iter()
        .filter_map(|ingress| {
            if !belongs_here(ingress.labels(), installation_id) {
                return None;
            }
            let config_id = ingress.labels().get("config_id").map(|s| s.to_string());

            // An ingress without its deployment is the most important leftover
            // of all: it still routes a public hostname. Listed as orphaned so
            // it can be removed from here.
            let is_orphaned = config_id
                .as_ref()
                .map(|id| !config_ids.contains(id) || !deployment_config_ids.contains(id))
                .unwrap_or(true);

            let age = ingress
                .metadata
                .creation_timestamp
                .as_ref()
                .map(calculate_age)
                .unwrap_or_else(|| "unknown".to_string());

            let hosts = ingress
                .spec
                .as_ref()
                .and_then(|s| s.rules.as_ref())
                .map(|rules| {
                    rules
                        .iter()
                        .filter_map(|r| r.host.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "None".to_string());

            Some(ServerResource {
                resource_type: "ingress".to_string(),
                name: ingress.name_any(),
                namespace: namespace.to_string(),
                config_id,
                is_orphaned,
                age,
                status: hosts,
            })
        })
        .collect())
}

fn calculate_age(creation_timestamp: &Time) -> String {
    let created = creation_timestamp.0;
    let now = jiff::Timestamp::now();
    let duration = now.since(created).unwrap_or_default();

    let days = duration.get_days();
    let hours = duration.get_hours();
    let minutes = duration.get_minutes();
    let seconds = duration.get_seconds();

    if days > 0 {
        format!("{}d", days)
    } else if hours > 0 {
        format!("{}h", hours)
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        format!("{}s", seconds)
    }
}

#[tauri::command]
pub async fn delete_kftray_resource(
    context_name: &str, namespace: &str, resource_type: &str, resource_name: &str,
    config_id: Option<String>, kubeconfig: Option<String>,
) -> Result<(), String> {
    info!(
        "Deleting kftray resource: {} {} in namespace {} (config_id: {:?})",
        resource_type, resource_name, namespace, config_id
    );

    if let Some(ref config_id_str) = config_id
        && let Ok(id) = config_id_str.parse::<i64>()
    {
        let config_result = kftray_commons::config::get_config(id).await;

        if let Ok(config) = config_result {
            info!("Config found, stopping port-forward before deleting resource");

            let workload_type = config.workload_type.as_deref().unwrap_or("");

            match workload_type {
                "proxy" => {
                    let _ = kftray_portforward::stop_proxy_forward(
                        id,
                        namespace,
                        resource_name.to_string(),
                    )
                    .await;
                }
                "expose" => {
                    let _ =
                        kftray_portforward::stop_expose(id, namespace, DatabaseMode::File).await;
                }
                _ => {
                    let _ = kftray_portforward::stop_port_forward_with_mode(
                        config_id_str.clone(),
                        DatabaseMode::File,
                    )
                    .await;
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    let connection = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| format!("Failed to create client for context '{context_name}': {err}"))?;

    let client = connection.client;

    let delete_params = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::default()
    };
    let installation_id = kftray_commons::utils::config_dir::installation_id().await?;

    // Checked on the object itself, not on what the screen listed: the name
    // alone reaches another installation's resource, and deleting one by
    // mistake takes down a forward that is not ours.
    async fn delete_owned<K>(
        api: Api<K>, name: &str, params: &DeleteParams, installation_id: &str, kind: &str,
    ) -> Result<(), String>
    where
        K: Clone + serde::de::DeserializeOwned + std::fmt::Debug + kube::Resource<DynamicType = ()>,
    {
        // Already gone is the outcome this command wants: the stop that ran
        // just before can have deleted it, and so can a concurrent cleanup.
        let object = match api.get_opt(name).await {
            Ok(Some(object)) => object,
            Ok(None) => return Ok(()),
            Err(e) => return Err(format!("Failed to read {kind}: {e}")),
        };
        if !belongs_here(
            object.meta().labels.as_ref().unwrap_or(&Default::default()),
            installation_id,
        ) {
            return Err(format!(
                "{kind} {name} belongs to another kftray installation and was left alone"
            ));
        }
        // Deleted only if it is still the object that passed the check: the
        // name can be reused, or the labels changed, between the read and the
        // delete, and the preconditions make the server refuse in that case.
        let params = DeleteParams {
            preconditions: Some(kube::api::Preconditions {
                uid: object.meta().uid.clone(),
                resource_version: object.meta().resource_version.clone(),
            }),
            ..params.clone()
        };
        match api.delete(name, &params).await {
            Ok(_) => Ok(()),
            Err(kube::Error::Api(response)) if response.code == 404 => Ok(()),
            Err(kube::Error::Api(response)) if response.code == 409 => Err(format!(
                "{kind} {name} changed while it was being checked and was left alone; refresh and \
                 try again"
            )),
            Err(e) => Err(format!("Failed to delete {kind}: {e}")),
        }
    }

    match resource_type {
        "pod" => {
            let api: Api<Pod> = Api::namespaced(client, namespace);
            delete_owned(api, resource_name, &delete_params, installation_id, "pod").await?;
        }
        "deployment" => {
            let api: Api<Deployment> = Api::namespaced(client, namespace);
            delete_owned(
                api,
                resource_name,
                &delete_params,
                installation_id,
                "deployment",
            )
            .await?;
        }
        "service" => {
            let api: Api<Service> = Api::namespaced(client, namespace);
            delete_owned(
                api,
                resource_name,
                &delete_params,
                installation_id,
                "service",
            )
            .await?;
        }
        "ingress" => {
            let api: Api<Ingress> = Api::namespaced(client, namespace);
            delete_owned(
                api,
                resource_name,
                &delete_params,
                installation_id,
                "ingress",
            )
            .await?;
        }
        _ => {
            return Err(format!("Unsupported resource type: {}", resource_type));
        }
    }

    info!(
        "Successfully deleted {} {} in namespace {}",
        resource_type, resource_name, namespace
    );

    Ok(())
}

#[tauri::command]
pub async fn cleanup_all_kftray_resources(
    context_name: &str, kubeconfig: Option<String>,
) -> Result<String, String> {
    info!(
        "Cleaning up all kftray resources for context: {}",
        context_name
    );

    let namespace_groups = list_all_kftray_resources(context_name, kubeconfig.clone()).await?;

    let mut deleted_count = 0;
    let mut error_count = 0;

    for group in namespace_groups {
        for resource in group.resources {
            match delete_kftray_resource(
                context_name,
                &resource.namespace,
                &resource.resource_type,
                &resource.name,
                resource.config_id.clone(),
                kubeconfig.clone(),
            )
            .await
            {
                Ok(_) => deleted_count += 1,
                Err(e) => {
                    error!("Failed to delete resource {}: {}", resource.name, e);
                    error_count += 1;
                }
            }
        }
    }

    let message = if error_count > 0 {
        format!(
            "Deleted {} resources with {} errors",
            deleted_count, error_count
        )
    } else {
        format!("Successfully deleted {} resources", deleted_count)
    };

    info!("{}", message);

    Ok(message)
}

#[tauri::command]
pub async fn cleanup_orphaned_kftray_resources(
    context_name: &str, kubeconfig: Option<String>,
) -> Result<String, String> {
    info!(
        "Cleaning up orphaned kftray resources for context: {}",
        context_name
    );

    let namespace_groups = list_all_kftray_resources(context_name, kubeconfig.clone()).await?;

    let mut deleted_count = 0;
    let mut error_count = 0;

    for group in namespace_groups {
        for resource in group.resources {
            if !resource.is_orphaned {
                continue;
            }

            match delete_kftray_resource(
                context_name,
                &resource.namespace,
                &resource.resource_type,
                &resource.name,
                resource.config_id.clone(),
                kubeconfig.clone(),
            )
            .await
            {
                Ok(_) => deleted_count += 1,
                Err(e) => {
                    error!(
                        "Failed to delete orphaned resource {}: {}",
                        resource.name, e
                    );
                    error_count += 1;
                }
            }
        }
    }

    let message = if error_count > 0 {
        format!(
            "Deleted {} orphaned resources with {} errors",
            deleted_count, error_count
        )
    } else {
        format!("Successfully deleted {} orphaned resources", deleted_count)
    };

    info!("{}", message);

    Ok(message)
}
