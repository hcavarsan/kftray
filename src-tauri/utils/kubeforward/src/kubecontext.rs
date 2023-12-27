use anyhow::Result;
use k8s_openapi::{
    api::core::v1::{Namespace, Service},
    apimachinery::pkg::util::intstr::IntOrString,
};

use crate::vx::Pod;

use kube::{
    api::{Api, ListParams},
    config::{Config, KubeConfigOptions, Kubeconfig},
    Client, ResourceExt,
};
use serde::Serialize;

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

#[derive(Serialize)]
pub struct KubeContextInfo {
    pub name: String,
}

#[derive(Serialize)]
pub struct KubeNamespaceInfo {
    pub name: String,
}

#[derive(Serialize)]
pub struct KubeServiceInfo {
    pub name: String,
}

#[derive(Serialize)]
pub struct KubeServicePortInfo {
    pub name: Option<String>,
    pub port: Option<IntOrString>,
}

#[tauri::command]
pub async fn list_kube_contexts() -> Result<Vec<KubeContextInfo>, String> {
    let kubeconfig = Kubeconfig::read().map_err(|e| e.to_string())?;
    Ok(kubeconfig
        .contexts
        .into_iter()
        .map(|c| KubeContextInfo { name: c.name })
        .collect())
}

#[tauri::command]
pub async fn list_namespaces(context_name: &str) -> Result<Vec<KubeNamespaceInfo>, String> {
    let client = create_client_with_specific_context(context_name)
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
    context_name: &str,
    namespace: &str,
) -> Result<Vec<KubeServiceInfo>, String> {
    let client = create_client_with_specific_context(context_name)
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
pub async fn list_service_ports(
    context_name: &str,
    namespace: &str,
    service_name: &str,
) -> Result<Vec<KubeServicePortInfo>, String> {
    let client = create_client_with_specific_context(context_name)
        .await
        .map_err(|err| {
            format!(
                "Failed to create client for context '{}': {}",
                context_name, err
            )
        })?;

    let api_svc: Api<Service> = Api::namespaced(client.clone(), namespace);
    let service = api_svc.get(service_name).await.map_err(|e| e.to_string())?;

    let api_pod: Api<Pod> = Api::namespaced(client, namespace);

    let mut service_port_infos = Vec::new();

    if let Some(spec) = service.spec {
        if let Some(service_ports) = spec.ports {
            for sp in service_ports {
                if let Some(IntOrString::String(ref name)) = sp.target_port {
                    let selector_string = spec.selector.as_ref().map_or_else(
                        || String::new(),
                        |s| {
                            s.iter()
                                .map(|(key, value)| format!("{}={}", key, value))
                                .collect::<Vec<_>>()
                                .join(", ")
                        },
                    );

                    let pods = api_pod
                        .list(&ListParams::default().labels(&selector_string))
                        .await
                        .map_err(|e| format!("Failed to list pods: {}", e))?;

                    'port_search: for pod in pods {
                        if let Some(spec) = &pod.spec {
                            for container in &spec.containers {
                                if let Some(ports) = &container.ports {
                                    for cp in ports {
                                        // Match the port name
                                        if cp.name.as_deref() == Some(name) {
                                            service_port_infos.push(KubeServicePortInfo {
                                                name: cp.name.clone(),
                                                port: Some(IntOrString::Int(
                                                    cp.container_port as i32,
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
        return Err(format!(
            "No ports found for service '{}' in namespace '{}'",
            service_name, namespace
        ));
    }

    Ok(service_port_infos)
}
