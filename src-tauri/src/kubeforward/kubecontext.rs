use anyhow::{Context, Result};
use k8s_openapi::{
    api::core::v1::{Namespace, Service},
    apimachinery::pkg::util::intstr::IntOrString,
};
use kube::{
    api::{Api, ListParams},
    client::ConfigExt,
    config::{Config, KubeConfigOptions, Kubeconfig},
    Client, ResourceExt,
};
use tower::ServiceBuilder;

use crate::{
    kubeforward::vx::Pod,
    models::kube::{KubeContextInfo, KubeNamespaceInfo, KubeServiceInfo, KubeServicePortInfo},
};
pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>,
    context_name: &str,
) -> Result<Client> {
    println!(
        "create_client_with_specific_context {}",
        kubeconfig.as_deref().unwrap_or("")
    );
    println!("create_client_with_specific_context {}", context_name);

    // Determine the kubeconfig based on the input
    let kubeconfig = if let Some(path) = kubeconfig {
        if path == "default" {
            // If the path explicitly mentions "default", read the default kubeconfig
            Kubeconfig::read().context("Failed to read kubeconfig from default location")?
        } else {
            // Otherwise, try to read the kubeconfig from the specified path
            Kubeconfig::read_from(path).context("Failed to read kubeconfig from specified path")?
        }
    } else {
        // If no kubeconfig is specified, read the default kubeconfig
        Kubeconfig::read().context("Failed to read kubeconfig from default location")?
    };
    println!("create_client_with_specific_context2 {:?}", kubeconfig);

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
        .service(hyper::Client::builder().build(https_connector));

    let client = Client::new(service, config.default_namespace);

    Ok(client)
}

#[tauri::command]
pub async fn list_kube_contexts(
    kubeconfig: Option<String>,
) -> Result<Vec<KubeContextInfo>, String> {
    let kubeconfig_path = kubeconfig.as_deref().unwrap_or("");
    println!("list_kube_contexts {}", kubeconfig_path);
    let kubeconfig = match &kubeconfig {
        Some(path) => Kubeconfig::read_from(path),
        None => Kubeconfig::read(),
    }
    .map_err(|e| e.to_string())?;
    Ok(kubeconfig
        .contexts
        .into_iter()
        .map(|c| KubeContextInfo { name: c.name })
        .collect())
}

#[tauri::command]
pub async fn list_namespaces(
    context_name: &str,
    kubeconfig: Option<String>,
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
    context_name: &str,
    namespace: &str,
    kubeconfig: Option<String>,
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
pub async fn list_service_ports(
    context_name: &str,
    namespace: &str,
    service_name: &str,
    kubeconfig: Option<String>,
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
    let service = api_svc.get(service_name).await.map_err(|e| e.to_string())?;

    let api_pod: Api<Pod> = Api::namespaced(client, namespace);

    let mut service_port_infos = Vec::new();

    if let Some(spec) = service.spec {
        if let Some(service_ports) = spec.ports {
            for sp in service_ports {
                if let Some(IntOrString::String(ref name)) = sp.target_port {
                    // Construct a selector string from the pod's labels, if available.
                    let selector_string = spec.selector.as_ref().map_or_else(String::new, |s| {
                        s.iter()
                            .map(|(key, value)| format!("{}={}", key, value))
                            .collect::<Vec<_>>()
                            .join(", ")
                    });

                    // Attempt to list pods using the constructed label selector.
                    let pods = api_pod
                        .list(&ListParams::default().labels(selector_string.as_str())) // Correctly pass a &str
                        .await
                        .map_err(|e| format!("Failed to list pods: {}", e))?;

                    // Iterate through the list of pods to find a matching container port name.
                    'port_search: for pod in pods {
                        if let Some(spec) = &pod.spec {
                            for container in &spec.containers {
                                if let Some(ports) = &container.ports {
                                    for cp in ports {
                                        // Match the port name and add the port info to the service_port_infos vector if found.
                                        if cp.name.as_deref() == Some(name) {
                                            service_port_infos.push(KubeServicePortInfo {
                                                name: cp.name.clone(),
                                                port: Some(IntOrString::Int(cp.container_port)),
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
