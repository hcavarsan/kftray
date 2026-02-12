use std::collections::HashMap;
use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{
        Pod,
        Service,
    },
    networking::v1::Ingress,
};
use kftray_commons::models::config_model::Config;
use kube::api::{
    DeleteParams,
    ListParams,
    PostParams,
};
use kube::{
    Api,
    Client,
};
use kube_runtime::wait::conditions;
use log::{
    debug,
    error,
    info,
};

use crate::expose::{
    models::ExposeResources,
    templates,
};

/// Extracts the first part of a domain name (before the first dot) to use as a
/// DNS-1035 compliant name For example: "testelocal.ideia.totvs.io" ->
/// "testelocal"
fn extract_subdomain(domain: &str) -> String {
    domain.split('.').next().unwrap_or(domain).to_string()
}

pub async fn create_expose_resources(
    client: Client, config: &Config,
) -> Result<ExposeResources, String> {
    let config_id_str = config
        .id
        .map_or_else(|| "default".to_string(), |id| id.to_string());

    let existing = check_existing_resources(&client, &config.namespace, &config_id_str).await;

    if let Some(resources) = existing {
        info!(
            "Resources already exist for config {}: {:?}. Cleaning up before recreating",
            config_id_str, resources
        );
        let _ = delete_expose_resources(client.clone(), &config.namespace, &config_id_str).await;

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();

    let random_string: String = {
        use rand::RngExt;
        let mut rng = rand::rng();
        (0..6)
            .map(|_| {
                let idx = rng.random_range(0..26);
                (b'a' + idx) as char
            })
            .collect()
    };

    let username = whoami::username()
        .unwrap_or_else(|_| "unknown".to_string())
        .to_lowercase();
    let clean_username: String = username
        .chars()
        .filter(|c: &char| c.is_alphanumeric())
        .collect();

    let deployment_name = format!(
        "kftray-expose-{}-{}-{}",
        clean_username, timestamp, random_string
    );

    // For public exposure, use the first part of the domain (before the first dot)
    // as the service/ingress name For example: "testelocal.ideia.totvs.io"
    // becomes "testelocal" This ensures DNS-1035 compliance for Kubernetes
    // resource names
    let service_name = if config.exposure_type.as_deref() == Some("public") {
        config
            .alias
            .as_ref()
            .map(|alias| extract_subdomain(alias))
            .unwrap_or_else(|| deployment_name.clone())
    } else {
        config
            .alias
            .clone()
            .unwrap_or_else(|| deployment_name.clone())
    };

    let ingress_name = if config.exposure_type.as_deref() == Some("public") {
        config
            .alias
            .as_ref()
            .map(|alias| extract_subdomain(alias))
            .unwrap_or_else(|| deployment_name.clone())
    } else {
        config
            .alias
            .clone()
            .unwrap_or_else(|| deployment_name.clone())
    };

    create_deployment(
        &client,
        &config.namespace,
        &deployment_name,
        &config_id_str,
        config,
    )
    .await?;

    let pod_name = wait_for_pod_ready(&client, &config.namespace, &config_id_str).await?;

    let pod_ip = get_pod_ip(&client, &config.namespace, &pod_name).await?;

    let local_port = config.local_port.unwrap_or(8080);
    create_service(
        &client,
        &config.namespace,
        &service_name,
        &config_id_str,
        local_port,
    )
    .await?;

    let ingress_created = if config.exposure_type.as_deref() == Some("public") {
        create_ingress(
            &client,
            &config.namespace,
            &ingress_name,
            &service_name,
            config,
        )
        .await?;
        true
    } else {
        false
    };

    Ok(ExposeResources {
        deployment_name: deployment_name.clone(),
        service_name: service_name.clone(),
        ingress_name: if ingress_created {
            Some(ingress_name)
        } else {
            None
        },
        pod_ip,
        pod_name,
    })
}

async fn create_deployment(
    client: &Client, namespace: &str, deployment_name: &str, config_id: &str, config: &Config,
) -> Result<(), String> {
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    let local_port = config.local_port.unwrap_or(8080).to_string();

    let mut values = HashMap::new();
    values.insert("deployment_name", deployment_name.to_string());
    values.insert("namespace", namespace.to_string());
    values.insert("config_id", config_id.to_string());
    values.insert("local_port", local_port);

    let template = templates::load_deployment_template()?;
    let rendered = templates::render_template(&template, &values);

    let deployment: Deployment = serde_json::from_str(&rendered)
        .map_err(|e| format!("Failed to parse deployment: {}", e))?;

    deployments
        .create(&PostParams::default(), &deployment)
        .await
        .map_err(|e| format!("Failed to create deployment: {}", e))?;

    info!("Deployment created successfully");
    Ok(())
}

async fn wait_for_pod_ready(
    client: &Client, namespace: &str, config_id: &str,
) -> Result<String, String> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let lp = ListParams::default().labels(&format!("app=kftray-expose,config_id={}", config_id));

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let pod_list = pods
        .list(&lp)
        .await
        .map_err(|e| format!("Failed to list pods: {}", e))?;

    let pod = pod_list.items.first().ok_or("No pod found")?;

    let pod_name = pod.metadata.name.clone().ok_or("Pod has no name")?;

    kube_runtime::wait::await_condition(pods.clone(), &pod_name, conditions::is_pod_running())
        .await
        .map_err(|e| format!("Pod not ready: {}", e))?;

    info!("Pod ready: {}", pod_name);
    Ok(pod_name)
}

async fn get_pod_ip(client: &Client, namespace: &str, pod_name: &str) -> Result<String, String> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pod = pods
        .get(pod_name)
        .await
        .map_err(|e| format!("Failed to get pod: {}", e))?;

    let pod_ip = pod.status.and_then(|s| s.pod_ip).ok_or("Pod has no IP")?;

    Ok(pod_ip)
}

async fn create_service(
    client: &Client, namespace: &str, service_name: &str, config_id: &str, local_port: u16,
) -> Result<(), String> {
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);

    let mut values = HashMap::new();
    values.insert("service_name", service_name.to_string());
    values.insert("namespace", namespace.to_string());
    values.insert("config_id", config_id.to_string());
    values.insert("local_port", local_port.to_string());

    let template = templates::load_service_template()?;
    let rendered = templates::render_template(&template, &values);

    let service: Service =
        serde_json::from_str(&rendered).map_err(|e| format!("Failed to parse service: {}", e))?;

    services
        .create(&PostParams::default(), &service)
        .await
        .map_err(|e| format!("Failed to create service: {}", e))?;

    info!("Service created successfully");
    Ok(())
}

async fn create_ingress(
    client: &Client, namespace: &str, ingress_name: &str, service_name: &str, config: &Config,
) -> Result<(), String> {
    let ingresses: Api<Ingress> = Api::namespaced(client.clone(), namespace);

    let domain = config
        .alias
        .as_ref()
        .ok_or("Domain not configured for public exposure (set alias field)")?;

    let config_id_str = config
        .id
        .map_or_else(|| "default".to_string(), |id| id.to_string());

    let local_port = config.local_port.unwrap_or(8080);

    let cert_manager_enabled = config.cert_manager_enabled.unwrap_or(false);
    let annotations = templates::build_ingress_annotations(
        cert_manager_enabled,
        config.cert_issuer.as_deref(),
        config.cert_issuer_kind.as_deref(),
        config.ingress_annotations.as_deref(),
    );
    let tls = templates::build_tls_section(cert_manager_enabled, domain, &config_id_str);
    let ingress_class_name = templates::build_ingress_class_name(config.ingress_class.as_deref());

    let mut values = HashMap::new();
    values.insert("ingress_name", ingress_name.to_string());
    values.insert("namespace", namespace.to_string());
    values.insert("service_name", service_name.to_string());
    values.insert("domain", domain.clone());
    values.insert("config_id", config_id_str.clone());
    values.insert("local_port", local_port.to_string());
    values.insert("annotations", annotations);
    values.insert("tls", tls);
    values.insert("ingress_class_name", ingress_class_name);

    let template = templates::load_ingress_template()?;
    let rendered = templates::render_template(&template, &values);

    let ingress: Ingress =
        serde_json::from_str(&rendered).map_err(|e| format!("Failed to parse ingress: {}", e))?;

    ingresses
        .create(&PostParams::default(), &ingress)
        .await
        .map_err(|e| format!("Failed to create ingress: {}", e))?;

    info!("Created ingress");
    Ok(())
}

async fn check_existing_resources(
    client: &Client, namespace: &str, config_id: &str,
) -> Option<Vec<String>> {
    let label_selector = format!("app=kftray-expose,config_id={}", config_id);

    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployment_lp = ListParams::default().labels(&label_selector);

    match deployments.list(&deployment_lp).await {
        Ok(deployment_list) if !deployment_list.items.is_empty() => {
            let names: Vec<String> = deployment_list
                .items
                .iter()
                .filter_map(|d| d.metadata.name.clone())
                .collect();
            debug!(
                "Found existing deployments for config {}: {:?}",
                config_id, names
            );
            Some(names)
        }
        _ => None,
    }
}

pub async fn delete_expose_resources(
    client: Client, namespace: &str, config_id_label: &str,
) -> Result<(), String> {
    let label_selector = format!("app=kftray-expose,config_id={}", config_id_label);
    let lp = ListParams::default().labels(&label_selector);

    info!(
        "Deleting expose resources for config_id label '{}'",
        config_id_label
    );

    delete_ingresses(&client, namespace, &lp).await?;
    delete_services(&client, namespace, &lp).await?;
    delete_deployments(&client, namespace, &lp).await?;

    info!(
        "Successfully deleted expose resources for config_id label '{}'",
        config_id_label
    );
    Ok(())
}

async fn delete_ingresses(client: &Client, namespace: &str, lp: &ListParams) -> Result<(), String> {
    let api: Api<Ingress> = Api::namespaced(client.clone(), namespace);

    let items = match api.list(lp).await {
        Ok(list) => list,
        Err(e) => {
            info!("No ingresses to delete or error listing: {}", e);
            return Ok(());
        }
    };

    if items.items.is_empty() {
        debug!("No ingresses found to delete");
        return Ok(());
    }

    for ingress in items.items {
        if let Some(name) = &ingress.metadata.name {
            info!("Deleting ingress: {}", name);
            match api.delete(name, &DeleteParams::default()).await {
                Ok(_) => info!("Ingress {} deleted successfully", name),
                Err(e) => error!("Failed to delete ingress {}: {}", name, e),
            }
        }
    }
    Ok(())
}

async fn delete_services(client: &Client, namespace: &str, lp: &ListParams) -> Result<(), String> {
    let api: Api<Service> = Api::namespaced(client.clone(), namespace);

    let items = match api.list(lp).await {
        Ok(list) => list,
        Err(e) => {
            info!("No services to delete or error listing: {}", e);
            return Ok(());
        }
    };

    if items.items.is_empty() {
        debug!("No services found to delete");
        return Ok(());
    }

    for service in items.items {
        if let Some(name) = &service.metadata.name {
            info!("Deleting service: {}", name);
            match api.delete(name, &DeleteParams::default()).await {
                Ok(_) => info!("Service {} deleted successfully", name),
                Err(e) => error!("Failed to delete service {}: {}", name, e),
            }
        }
    }
    Ok(())
}

async fn delete_deployments(
    client: &Client, namespace: &str, lp: &ListParams,
) -> Result<(), String> {
    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    let items = match api.list(lp).await {
        Ok(list) => list,
        Err(e) => {
            info!("No deployments to delete or error listing: {}", e);
            return Ok(());
        }
    };

    if items.items.is_empty() {
        debug!("No deployments found to delete");
        return Ok(());
    }

    for deployment in items.items {
        if let Some(name) = &deployment.metadata.name {
            info!("Deleting deployment: {}", name);
            match api.delete(name, &DeleteParams::default()).await {
                Ok(_) => info!("Deployment {} deleted successfully", name),
                Err(e) => error!("Failed to delete deployment {}: {}", name, e),
            }
        }
    }
    Ok(())
}
