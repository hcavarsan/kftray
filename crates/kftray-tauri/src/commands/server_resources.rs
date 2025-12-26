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

    let (client, _, _) = create_client_with_specific_context(kubeconfig, Some(context_name))
        .await
        .map_err(|err| format!("Failed to create client for context '{context_name}': {err}"))?;

    let client =
        client.ok_or_else(|| format!("Client not created for context '{context_name}'"))?;

    let username = whoami::username().to_lowercase();
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
            list_pods_in_namespace(&client, &namespace, &clean_username, &config_ids)
                .await
                .unwrap_or_default(),
        );

        let user_deployments =
            list_deployments_in_namespace(&client, &namespace, &clean_username, &config_ids)
                .await
                .unwrap_or_default();

        let deployment_config_ids: Vec<String> = user_deployments
            .iter()
            .filter_map(|d| d.config_id.clone())
            .collect();

        resources.extend(user_deployments);

        resources.extend(
            list_services_in_namespace(&client, &namespace, &deployment_config_ids, &config_ids)
                .await
                .unwrap_or_default(),
        );
        resources.extend(
            list_ingresses_in_namespace(&client, &namespace, &deployment_config_ids, &config_ids)
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

async fn list_pods_in_namespace(
    client: &Client, namespace: &str, username: &str, config_ids: &[String],
) -> Result<Vec<ServerResource>, String> {
    let pods_api: Api<Pod> = Api::namespaced(client.clone(), namespace);

    let pods = pods_api
        .list(&ListParams::default())
        .await
        .map_err(|e| format!("Failed to list pods: {e}"))?;

    let user_prefix_forward = format!("kftray-forward-{}", username);
    let user_prefix_expose = format!("kftray-expose-{}", username);

    Ok(pods
        .items
        .into_iter()
        .filter_map(|pod| {
            let pod_name = pod.name_any();

            if !pod_name.starts_with(&user_prefix_forward)
                && !pod_name.starts_with(&user_prefix_expose)
            {
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
    client: &Client, namespace: &str, username: &str, config_ids: &[String],
) -> Result<Vec<ServerResource>, String> {
    let deployments_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let lp = ListParams::default().labels("app=kftray-expose");

    let deployments = deployments_api
        .list(&lp)
        .await
        .map_err(|e| format!("Failed to list deployments: {e}"))?;

    let user_prefix = format!("kftray-expose-{}-", username);

    Ok(deployments
        .items
        .into_iter()
        .filter(|deployment| deployment.name_any().starts_with(&user_prefix))
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
            let config_id = service.labels().get("config_id").map(|s| s.to_string());

            if let Some(ref id) = config_id {
                if !deployment_config_ids.contains(id) {
                    return None;
                }
            } else {
                return None;
            }

            let is_orphaned = config_id
                .as_ref()
                .map(|id| !config_ids.contains(id))
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
            let config_id = ingress.labels().get("config_id").map(|s| s.to_string());

            if let Some(ref id) = config_id {
                if !deployment_config_ids.contains(id) {
                    return None;
                }
            } else {
                return None;
            }

            let is_orphaned = config_id
                .as_ref()
                .map(|id| !config_ids.contains(id))
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
    let created: k8s_openapi::chrono::DateTime<k8s_openapi::chrono::Utc> = creation_timestamp.0;
    let now = k8s_openapi::chrono::Utc::now();
    let duration = now.signed_duration_since(created);

    if duration.num_days() > 0 {
        format!("{}d", duration.num_days())
    } else if duration.num_hours() > 0 {
        format!("{}h", duration.num_hours())
    } else if duration.num_minutes() > 0 {
        format!("{}m", duration.num_minutes())
    } else {
        format!("{}s", duration.num_seconds())
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

    let (client, _, _) = create_client_with_specific_context(kubeconfig, Some(context_name))
        .await
        .map_err(|err| format!("Failed to create client for context '{context_name}': {err}"))?;

    let client =
        client.ok_or_else(|| format!("Client not created for context '{context_name}'"))?;

    let delete_params = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::default()
    };

    match resource_type {
        "pod" => {
            let api: Api<Pod> = Api::namespaced(client, namespace);
            api.delete(resource_name, &delete_params)
                .await
                .map_err(|e| format!("Failed to delete pod: {e}"))?;
        }
        "deployment" => {
            let api: Api<Deployment> = Api::namespaced(client, namespace);
            api.delete(resource_name, &delete_params)
                .await
                .map_err(|e| format!("Failed to delete deployment: {e}"))?;
        }
        "service" => {
            let api: Api<Service> = Api::namespaced(client, namespace);
            api.delete(resource_name, &delete_params)
                .await
                .map_err(|e| format!("Failed to delete service: {e}"))?;
        }
        "ingress" => {
            let api: Api<Ingress> = Api::namespaced(client, namespace);
            api.delete(resource_name, &delete_params)
                .await
                .map_err(|e| format!("Failed to delete ingress: {e}"))?;
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
