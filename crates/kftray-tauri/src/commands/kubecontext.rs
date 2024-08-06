use std::collections::HashSet;

use anyhow::{
    Context,
    Result,
};
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::{
    api::core::v1::{
        Namespace,
        Service,
    },
    apimachinery::pkg::util::intstr::IntOrString,
};
use kftray_commons::utils::config_dir::get_default_kubeconfig_path;
use kftray_portforward::client::create_client_with_specific_context;
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
    config::Kubeconfig,
    ResourceExt,
};
use log::info;

#[tauri::command]
pub async fn list_pods(
    context_name: &str, namespace: &str, kubeconfig: Option<String>,
) -> Result<Vec<PodInfo>, String> {
    if namespace.trim().is_empty() {
        return Err("Namespace parameter cannot be empty".to_string());
    }

    let client = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| {
            format!(
                "Failed to create client for context '{}': {}",
                context_name, err
            )
        })?;

    let api: Api<Pod> = Api::namespaced(client, namespace);

    let pod_list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let mut unique_labels = HashSet::new();

    for pod in pod_list {
        if let Some(labels) = &pod.meta().labels {
            for (key, value) in labels {
                unique_labels.insert(format!("{}={}", key, value));
            }
        }
    }

    let label_infos = unique_labels
        .into_iter()
        .map(|label_str| PodInfo {
            labels_str: label_str,
        })
        .collect();

    Ok(label_infos)
}

#[tauri::command]
pub async fn list_kube_contexts(
    kubeconfig: Option<String>,
) -> Result<Vec<KubeContextInfo>, String> {
    info!("list_kube_contexts {}", kubeconfig.as_deref().unwrap_or(""));

    let kubeconfig = if let Some(path) = &kubeconfig {
        if path == "default" {
            let default_path = get_default_kubeconfig_path()
                .context("Couldn't get default kubeconfig path")
                .map_err(|err| err.to_string())?;

            info!(
                "Reading kubeconfig from default location: {:?}",
                default_path
            );

            Kubeconfig::read_from(default_path.to_str().unwrap())
                .context("Failed to read kubeconfig from default path")
                .map_err(|err| err.to_string())?
        } else {
            info!("Reading kubeconfig from specified path: {}", path);

            Kubeconfig::read_from(path)
                .context("Failed to read kubeconfig from specified path")
                .map_err(|err| err.to_string())?
        }
    } else {
        let default_path = get_default_kubeconfig_path()
            .context("Couldn't get default kubeconfig path")
            .map_err(|err| err.to_string())?;

        info!(
            "Reading kubeconfig from default location: {:?}",
            default_path
        );

        Kubeconfig::read_from(default_path.to_str().unwrap())
            .context("Failed to read kubeconfig from default path")
            .map_err(|err| err.to_string())?
    };

    Ok(kubeconfig
        .contexts
        .into_iter()
        .map(|c| KubeContextInfo { name: c.name })
        .collect())
}

#[tauri::command]

pub async fn list_namespaces(
    context_name: &str, kubeconfig: Option<String>,
) -> Result<Vec<KubeNamespaceInfo>, String> {
    let client = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| {
            format!(
                "Failed to create client for context '{}': {}",
                context_name, err
            )
        })?;

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

    let client = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| {
            format!(
                "Failed to create client for context '{}': {}",
                context_name, err
            )
        })?;

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
    let client = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| {
            format!(
                "Failed to create client for context '{}': {}",
                context_name, err
            )
        })?;

    let api_svc: Api<Service> = Api::namespaced(client.clone(), namespace);
    let api_pod: Api<Pod> = Api::namespaced(client.clone(), namespace);

    // Try to get the service first
    match api_svc.get(service_name).await {
        Ok(service) => {
            let mut service_port_infos = Vec::new();

            if let Some(spec) = service.spec {
                if let Some(service_ports) = spec.ports {
                    for sp in service_ports {
                        if let Some(IntOrString::String(ref name)) = sp.target_port {
                            // Construct a selector string from the pod's labels, if available.
                            let selector_string =
                                spec.selector.as_ref().map_or_else(String::new, |s| {
                                    s.iter()
                                        .map(|(key, value)| format!("{}={}", key, value))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                });

                            // Attempt to list pods using the constructed label selector.
                            let pods = api_pod
                                .list(&ListParams::default().labels(selector_string.as_str()))
                                .await
                                .map_err(|e| format!("Failed to list pods: {}", e))?;

                            // Iterate through the list of pods to find a matching container port
                            // name.
                            'port_search: for pod in pods {
                                if let Some(spec) = &pod.spec {
                                    for container in &spec.containers {
                                        if let Some(ports) = &container.ports {
                                            for cp in ports {
                                                // Match the port name and add the port info to the
                                                // service_port_infos vector if found.
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
            // If service is not found, treat service_name as a label and search for
            // matching pods
            let pods = api_pod
                .list(&ListParams::default().labels(service_name))
                .await
                .map_err(|e| format!("Failed to list pods: {}", e))?;

            let mut pod_port_infos = Vec::new();

            for pod in pods {
                if let Some(spec) = pod.spec {
                    for container in spec.containers {
                        if let Some(ports) = container.ports {
                            for cp in ports {
                                pod_port_infos.push(KubeServicePortInfo {
                                    name: cp.name.clone(),
                                    port: Some(IntOrString::Int(cp.container_port)),
                                });
                            }
                        }
                    }
                }
            }

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
