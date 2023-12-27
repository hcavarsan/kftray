use anyhow::Result;
use k8s_openapi::api::core::v1::{Namespace, Service};
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
    pub port: u16,
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

    let api: Api<Service> = Api::namespaced(client, namespace);
    let svc = api.get(service_name).await.map_err(|e| e.to_string())?;
    let service_ports = svc.spec.map_or(vec![], |spec| {
        spec.ports
            .unwrap_or_default()
            .into_iter()
            .map(|p| KubeServicePortInfo {
                name: p.name,
                port: p.port as u16,
            })
            .collect()
    });

    Ok(service_ports)
}
