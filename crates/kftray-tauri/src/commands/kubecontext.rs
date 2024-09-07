use std::collections::HashSet;

use anyhow::Result;
use k8s_openapi::api::core::v1::{
    Namespace,
    Pod,
    Service,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kftray_commons::config_model::Config;
use kftray_portforward::client::create_client_with_specific_context;
use kftray_portforward::core::retrieve_service_configs;
use kftray_portforward::models::kube::{
    KubeContextInfo,
    KubeNamespaceInfo,
    KubeServiceInfo,
    KubeServicePortInfo,
    PodInfo,
};
use kube::Resource;
use kube::{
    api::{
        Api,
        ListParams,
    },
    ResourceExt,
};
use log::info;

#[tauri::command]
pub async fn list_kube_contexts(
    kubeconfig: Option<String>,
) -> Result<Vec<KubeContextInfo>, String> {
    info!("list_kube_contexts {}", kubeconfig.as_deref().unwrap_or(""));

    let (_, kubeconfig, contexts) = create_client_with_specific_context(kubeconfig, None)
        .await
        .map_err(|err| format!("Failed to create client: {}", err))?;

    if let Some(kubeconfig) = kubeconfig {
        let contexts: Vec<KubeContextInfo> = kubeconfig
            .contexts
            .into_iter()
            .map(|c| KubeContextInfo { name: c.name })
            .collect();

        Ok(contexts)
    } else if !contexts.is_empty() {
        let context_infos: Vec<KubeContextInfo> = contexts
            .into_iter()
            .map(|name| KubeContextInfo { name })
            .collect();

        Ok(context_infos)
    } else {
        Err("Failed to retrieve kubeconfig".to_string())
    }
}

#[tauri::command]
pub async fn list_pods(
    context_name: &str, namespace: &str, kubeconfig: Option<String>,
) -> Result<Vec<PodInfo>, String> {
    if namespace.trim().is_empty() {
        return Err("Namespace parameter cannot be empty".to_string());
    }

    let (client, _, _) = create_client_with_specific_context(kubeconfig, Some(context_name))
        .await
        .map_err(|err| {
            format!(
                "Failed to create client for context '{}': {}",
                context_name, err
            )
        })?;

    let client =
        client.ok_or_else(|| format!("Client not created for context '{}'", context_name))?;
    let api: Api<Pod> = Api::namespaced(client, namespace);

    let pod_list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let unique_labels: HashSet<String> = pod_list
        .iter()
        .filter_map(|pod| pod.meta().labels.as_ref())
        .flat_map(|labels| {
            labels
                .iter()
                .map(|(key, value)| format!("{}={}", key, value))
        })
        .collect();

    let label_infos = unique_labels
        .into_iter()
        .map(|label_str| PodInfo {
            labels_str: label_str,
        })
        .collect();

    Ok(label_infos)
}

#[tauri::command]
pub async fn list_namespaces(
    context_name: &str, kubeconfig: Option<String>,
) -> Result<Vec<KubeNamespaceInfo>, String> {
    let (client, _, _) = create_client_with_specific_context(kubeconfig, Some(context_name))
        .await
        .map_err(|err| {
            format!(
                "Failed to create client for context '{}': {}",
                context_name, err
            )
        })?;

    let client =
        client.ok_or_else(|| format!("Client not created for context '{}'", context_name))?;
    let api: Api<Namespace> = Api::all(client);

    let ns_list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|ns| KubeNamespaceInfo {
            name: ns.name_any(),
        })
        .collect();

    Ok(ns_list)
}

#[tauri::command]
pub async fn list_services(
    context_name: &str, namespace: &str, kubeconfig: Option<String>,
) -> Result<Vec<KubeServiceInfo>, String> {
    if namespace.trim().is_empty() {
        return Err("Namespace parameter cannot be empty".to_string());
    }

    let (client, _, _) = create_client_with_specific_context(kubeconfig, Some(context_name))
        .await
        .map_err(|err| {
            format!(
                "Failed to create client for context '{}': {}",
                context_name, err
            )
        })?;

    let client =
        client.ok_or_else(|| format!("Client not created for context '{}'", context_name))?;
    let api: Api<Service> = Api::namespaced(client, namespace);

    let svc_list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|svc| KubeServiceInfo {
            name: svc.name_any(),
        })
        .collect();

    Ok(svc_list)
}

#[tauri::command]
pub async fn list_ports(
    context_name: &str, namespace: &str, service_name: &str, kubeconfig: Option<String>,
) -> Result<Vec<KubeServicePortInfo>, String> {
    let (client, _, _) = create_client_with_specific_context(kubeconfig, Some(context_name))
        .await
        .map_err(|err| {
            format!(
                "Failed to create client for context '{}': {}",
                context_name, err
            )
        })?;

    let client =
        client.ok_or_else(|| format!("Client not created for context '{}'", context_name))?;
    let api_svc: Api<Service> = Api::namespaced(client.clone(), namespace);
    let api_pod: Api<Pod> = Api::namespaced(client, namespace);

    match api_svc.get(service_name).await {
        Ok(service) => {
            let mut service_port_infos = Vec::new();

            if let Some(spec) = service.spec {
                if let Some(service_ports) = spec.ports {
                    for sp in service_ports {
                        if let Some(IntOrString::String(ref name)) = sp.target_port {
                            let selector_string =
                                spec.selector.as_ref().map_or_else(String::new, |s| {
                                    s.iter()
                                        .map(|(key, value)| format!("{}={}", key, value))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                });

                            let pods = api_pod
                                .list(&ListParams::default().labels(&selector_string))
                                .await
                                .map_err(|e| format!("Failed to list pods: {}", e))?;

                            'port_search: for pod in pods {
                                if let Some(spec) = &pod.spec {
                                    for container in &spec.containers {
                                        if let Some(ports) = &container.ports {
                                            for cp in ports {
                                                if cp.name.as_deref() == Some(name) {
                                                    service_port_infos.push(KubeServicePortInfo {
                                                        name: cp.name.clone(),
                                                        port: Some(IntOrString::Int(
                                                            cp.container_port,
                                                        )),
                                                    });
                                                    break 'port_search;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else if let Some(IntOrString::Int(port)) = sp.target_port {
                            service_port_infos.push(KubeServicePortInfo {
                                name: sp.name,
                                port: Some(IntOrString::Int(port)),
                            });
                        }
                    }
                }
            }

            if service_port_infos.is_empty() {
                Err(format!(
                    "No ports found for service '{}' in namespace '{}'",
                    service_name, namespace
                ))
            } else {
                Ok(service_port_infos)
            }
        }
        Err(_) => {
            let pods = api_pod
                .list(&ListParams::default().labels(service_name))
                .await
                .map_err(|e| format!("Failed to list pods: {}", e))?;

            let pod_port_infos: Vec<KubeServicePortInfo> = pods
                .iter()
                .filter_map(|pod| pod.spec.as_ref())
                .flat_map(|spec| spec.containers.iter())
                .filter_map(|container| container.ports.as_ref())
                .flat_map(|ports| ports.iter())
                .map(|cp| KubeServicePortInfo {
                    name: cp.name.clone(),
                    port: Some(IntOrString::Int(cp.container_port)),
                })
                .collect();

            if pod_port_infos.is_empty() {
                Err(format!(
                    "No ports found for label '{}' in namespace '{}'",
                    service_name, namespace
                ))
            } else {
                Ok(pod_port_infos)
            }
        }
    }
}

#[tauri::command]
pub async fn get_services_with_annotations(context_name: &str) -> Result<Vec<Config>, String> {
    info!("get_services_with_annotations for context {}", context_name);

    retrieve_service_configs(context_name).await
}
