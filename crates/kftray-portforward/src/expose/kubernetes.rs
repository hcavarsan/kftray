use std::collections::HashMap;
use std::time::{
    Duration,
    SystemTime,
    UNIX_EPOCH,
};

use futures::TryStreamExt;
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{
        Container,
        Pod,
        Probe,
        Service,
        TCPSocketAction,
    },
    networking::v1::Ingress,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
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
use kube_runtime::WatchStreamExt;
use log::{
    debug,
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
) -> Result<ExposeResources, ExposeCreateError> {
    let config_id_str = config
        .id
        .map_or_else(|| "default".to_string(), |id| id.to_string());

    let existing = check_existing_resources(&client, &config.namespace, &config_id_str).await;

    if let Some(resources) = existing {
        info!(
            "Resources already exist for config {}: {:?}. Cleaning up before recreating",
            config_id_str, resources
        );
        // Reported rather than ignored: a pre-existing resource this attempt
        // could not remove stays its responsibility, so it must not later claim
        // a complete rollback and let the cleanup record be dropped.
        delete_expose_resources(
            client.clone(),
            &config.namespace,
            &config_id_str,
            config.exposure_type.as_deref() == Some("public"),
        )
        .await
        .map_err(ExposeCreateError::from)?;

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

    // Names this attempt created, so rollback deletes exactly those. Deleting
    // by label would also remove resources another instance created for the
    // same config id after our existence check.
    let mut created: Vec<CreatedResource> = Vec::new();
    let result: Result<ExposeResources, ExposeCreateError> = async {
        created.push(
            create_deployment(
                &client,
                &config.namespace,
                &deployment_name,
                &config_id_str,
                config,
            )
            .await?,
        );

        let pod_name = wait_for_pod_ready(&client, &config.namespace, &config_id_str).await?;

        let pod_ip = get_pod_ip(&client, &config.namespace, &pod_name).await?;

        let local_port = config.local_port.unwrap_or(8080);
        created.push(
            create_service(
                &client,
                &config.namespace,
                &service_name,
                &config_id_str,
                local_port,
            )
            .await?,
        );

        let ingress_created = if config.exposure_type.as_deref() == Some("public") {
            created.push(
                create_ingress(
                    &client,
                    &config.namespace,
                    &ingress_name,
                    &service_name,
                    config,
                )
                .await?,
            );
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
            owned: std::mem::take(&mut created),
        })
    }
    .await;
    if let Err(error) = result {
        return match delete_created_resources(&client, &config.namespace, &created).await {
            Ok(()) => Err(ExposeCreateError {
                rolled_back: true,
                ..error
            }),
            Err(cleanup_error) => Err(ExposeCreateError {
                message: format!("{}; cleanup failed: {cleanup_error}", error.message),
                ambiguous: error.ambiguous,
                rolled_back: false,
            }),
        };
    }
    result
}

/// Labels every exposure resource must carry for cleanup to find it, applied
/// programmatically so a customized template cannot omit them.
async fn tag_expose_ownership(
    labels: &mut Option<std::collections::BTreeMap<String, String>>, config_id: &str,
) -> Result<(), String> {
    let labels = labels.get_or_insert_with(std::collections::BTreeMap::new);
    labels.insert("app".to_owned(), "kftray-expose".to_owned());
    labels.insert("config_id".to_owned(), config_id.to_owned());
    labels.insert(
        crate::kube::proxy::INSTALLATION_LABEL.to_owned(),
        kftray_commons::utils::config_dir::installation_id()
            .await?
            .to_owned(),
    );
    Ok(())
}

/// Whether a label is one this installation injects to claim its resources.
fn is_ownership_label(key: &str) -> bool {
    matches!(key, "app" | "config_id") || key == crate::kube::proxy::INSTALLATION_LABEL
}

/// Selector matching only the exposure resources this installation created.
///
/// Configuration ids come from a local database, so `config_id` alone also
/// matches another installation's exposure in the same namespace.
pub async fn expose_owner_selector(config_id: &str) -> Result<String, String> {
    Ok(format!(
        "app=kftray-expose,config_id={config_id},{}={}",
        crate::kube::proxy::INSTALLATION_LABEL,
        kftray_commons::utils::config_dir::installation_id().await?
    ))
}

/// A creation failure, and whether the object may exist despite it.
pub struct ExposeCreateError {
    pub message: String,
    /// The API server never gave a definitive answer, so it may still be
    /// persisting an object this attempt cannot name. The cleanup record must
    /// stay uncertain.
    pub ambiguous: bool,
    /// Everything this attempt created was deleted again, so nothing of its own
    /// is left for a later cleanup pass to find.
    pub rolled_back: bool,
}

impl From<String> for ExposeCreateError {
    fn from(message: String) -> Self {
        Self {
            message,
            ambiguous: false,
            rolled_back: false,
        }
    }
}

impl From<&str> for ExposeCreateError {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}

/// Classifies a failed create. Only a definitive rejection proves the object
/// was not created; a transport failure or a server-side timeout can be
/// returned while the request is still being applied.
fn classify_create_error(what: &str, error: &kube::Error) -> ExposeCreateError {
    let ambiguous = match error {
        kube::Error::Api(response) => {
            matches!(response.code, 408 | 429 | 500 | 502 | 503 | 504)
        }
        _ => true,
    };
    ExposeCreateError {
        message: format!("Failed to create {what}: {error}"),
        ambiguous,
        rolled_back: false,
    }
}

/// A resource this attempt created, identified by the name and UID the API
/// server returned rather than the name the template asked for.
#[derive(Debug)]
pub struct CreatedResource {
    kind: ResourceKind,
    name: String,
    uid: Option<String>,
}

#[derive(Debug)]
pub enum ResourceKind {
    Deployment,
    Service,
    Ingress,
}

fn created_from<K: kube::Resource>(kind: ResourceKind, created: &K) -> CreatedResource {
    CreatedResource {
        kind,
        name: created.meta().name.clone().unwrap_or_default(),
        uid: created.meta().uid.clone(),
    }
}

/// Deletes exactly the resources one creation attempt made, newest first.
pub async fn delete_created_resources(
    client: &Client, namespace: &str, created: &[CreatedResource],
) -> Result<(), String> {
    let mut errors = Vec::new();
    for resource in created.iter().rev() {
        // The UID precondition keeps rollback from deleting a replacement
        // another instance created under the same name.
        let dp = DeleteParams {
            preconditions: resource.uid.clone().map(|uid| kube::api::Preconditions {
                uid: Some(uid),
                resource_version: None,
            }),
            // Foreground propagation keeps the Deployment until its ReplicaSets
            // and Pods are gone, so waiting for it to disappear also proves the
            // relay containers stopped.
            propagation_policy: matches!(resource.kind, ResourceKind::Deployment)
                .then_some(kube::api::PropagationPolicy::Foreground),
            ..DeleteParams::default()
        };
        let name = &resource.name;
        let deleted = match resource.kind {
            ResourceKind::Deployment => {
                let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
                api.delete(name, &dp).await.map(|_| ())
            }
            ResourceKind::Service => {
                let api: Api<Service> = Api::namespaced(client.clone(), namespace);
                api.delete(name, &dp).await.map(|_| ())
            }
            ResourceKind::Ingress => {
                let api: Api<Ingress> = Api::namespaced(client.clone(), namespace);
                api.delete(name, &dp).await.map(|_| ())
            }
        };
        if let Err(error) = deleted
            && !matches!(&error, kube::Error::Api(response) if response.code == 404)
        {
            // A precondition conflict means the name now holds a different
            // object. Only the one this attempt created is its responsibility,
            // so the check below decides whether anything is actually left.
            let conflict = matches!(&error, kube::Error::Api(response) if response.code == 409);
            if !conflict || still_present(client, namespace, resource).await? {
                errors.push(error.to_string());
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    // An accepted DELETE only starts deletion: a finalizer can keep the object,
    // and its containers, running. Rollback is only complete once the recorded
    // objects are gone, or replaced by something this attempt does not own.
    let deadline = tokio::time::Instant::now() + ROLLBACK_DELETION_TIMEOUT;
    loop {
        let mut remaining = Vec::new();
        for resource in created {
            if still_present(client, namespace, resource).await? {
                remaining.push(resource.name.clone());
            }
        }
        if remaining.is_empty() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!(
                "Resources are still terminating after {ROLLBACK_DELETION_TIMEOUT:?}: {}",
                remaining.join(", ")
            ));
        }
        tokio::time::sleep(ROLLBACK_DELETION_POLL).await;
    }
}

const ROLLBACK_DELETION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const ROLLBACK_DELETION_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// Whether the exact object this attempt created still exists. A different UID
/// under the same name belongs to someone else.
async fn still_present(
    client: &Client, namespace: &str, resource: &CreatedResource,
) -> Result<bool, String> {
    let uid = match resource.kind {
        ResourceKind::Deployment => {
            let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
            api.get_opt(&resource.name)
                .await
                .map(|found| found.and_then(|item| item.metadata.uid))
        }
        ResourceKind::Service => {
            let api: Api<Service> = Api::namespaced(client.clone(), namespace);
            api.get_opt(&resource.name)
                .await
                .map(|found| found.and_then(|item| item.metadata.uid))
        }
        ResourceKind::Ingress => {
            let api: Api<Ingress> = Api::namespaced(client.clone(), namespace);
            api.get_opt(&resource.name)
                .await
                .map(|found| found.and_then(|item| item.metadata.uid))
        }
    }
    .map_err(|error| error.to_string())?;

    Ok(match (uid, resource.uid.as_deref()) {
        (None, _) => false,
        (Some(found), Some(created)) => found == created,
        (Some(_), None) => true,
    })
}

async fn create_deployment(
    client: &Client, namespace: &str, deployment_name: &str, config_id: &str, config: &Config,
) -> Result<CreatedResource, ExposeCreateError> {
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    let local_port = config.local_port.unwrap_or(8080).to_string();

    let mut values = HashMap::new();
    values.insert("deployment_name", deployment_name.to_string());
    values.insert("namespace", namespace.to_string());
    values.insert("config_id", config_id.to_string());
    values.insert("local_port", local_port);

    let template = templates::load_deployment_template()?;
    let rendered = templates::render_template(&template, &values);

    let mut deployment: Deployment = serde_json::from_str(&rendered)
        .map_err(|e| format!("Failed to parse deployment: {}", e))?;
    // Tagged so cleanup can tell this installation's exposure apart from
    // another one using the same, locally assigned, configuration id.
    tag_expose_ownership(&mut deployment.metadata.labels, config_id).await?;
    if let Some(spec) = deployment.spec.as_mut() {
        tag_expose_ownership(
            &mut spec.template.metadata.get_or_insert_default().labels,
            config_id,
        )
        .await?;
        // Every injected label goes into the selector, not just the
        // installation one: tagging the template overwrites whatever `app` a
        // customized manifest used, and a selector left on the old value would
        // no longer match its own pods, which Kubernetes rejects outright.
        let ownership: Vec<(String, String)> = spec
            .template
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.labels.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|(key, _)| is_ownership_label(key))
            .collect();
        spec.selector
            .match_labels
            .get_or_insert_with(std::collections::BTreeMap::new)
            .extend(ownership);
    }
    let spec = deployment
        .spec
        .as_mut()
        .and_then(|deployment| deployment.template.spec.as_mut())
        .ok_or("Expose deployment must contain a pod specification")?;
    // Probes are only injected into a container that identifies itself as the
    // relay. Falling back to the first container would put relay probes on an
    // unrelated sidecar in a customized template and restart it forever.
    let relay = spec.containers.iter_mut().find(|container| {
        container.name == "kftray-server"
            || container.env.as_ref().is_some_and(|env| {
                env.iter().any(|value| {
                    value.name == "PROXY_TYPE" && value.value.as_deref() == Some("reverse_http")
                })
            })
    });
    if let Some(container) = relay {
        if let Some(websocket_port) = container_env_port(container, "WEBSOCKET_PORT", 9999) {
            container.startup_probe.get_or_insert_with(|| Probe {
                tcp_socket: Some(TCPSocketAction {
                    port: IntOrString::Int(websocket_port),
                    ..Default::default()
                }),
                period_seconds: Some(1),
                timeout_seconds: Some(1),
                failure_threshold: Some(30),
                ..Default::default()
            });
        }
        if let Some(http_port) = container_env_port(container, "HTTP_PORT", 8080) {
            container.readiness_probe.get_or_insert_with(|| Probe {
                tcp_socket: Some(TCPSocketAction {
                    port: IntOrString::Int(http_port),
                    ..Default::default()
                }),
                period_seconds: Some(1),
                timeout_seconds: Some(1),
                failure_threshold: Some(3),
                ..Default::default()
            });
        }
    }

    let created = deployments
        .create(&PostParams::default(), &deployment)
        .await
        .map_err(|e| classify_create_error("deployment", &e))?;
    let created = created_from(ResourceKind::Deployment, &created);

    info!("Deployment created successfully");
    Ok(created)
}

/// Resolves the port an injected probe should target.
///
/// Returns `None` only when the port is genuinely unresolved: guessing the
/// default would probe the wrong port and fail a healthy deployment. An
/// explicit `env` entry wins, matching Kubernetes' own precedence over
/// `envFrom`.
fn container_env_port(container: &Container, name: &str, default: i32) -> Option<i32> {
    if let Some(variable) = container
        .env
        .as_ref()
        .and_then(|env| env.iter().find(|variable| variable.name == name))
    {
        return variable
            .value
            .as_deref()
            .and_then(|value| value.parse().ok());
    }
    if container
        .env_from
        .as_ref()
        .is_some_and(|sources| !sources.is_empty())
    {
        return None;
    }
    Some(default)
}

async fn wait_for_pod_ready(
    client: &Client, namespace: &str, config_id: &str,
) -> Result<String, String> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let watcher = kube_runtime::watcher(
        pods,
        kube_runtime::watcher::Config::default().labels(&expose_owner_selector(config_id).await?),
    )
    .applied_objects();
    futures::pin_mut!(watcher);
    tokio::time::timeout(Duration::from_secs(120), async {
        while let Some(pod) = watcher
            .try_next()
            .await
            .map_err(|error| error.to_string())?
        {
            if pod.metadata.deletion_timestamp.is_none()
                && pod.status.as_ref().is_some_and(|status| {
                    status.phase.as_deref() == Some("Running")
                        && status.conditions.as_ref().is_some_and(|conditions| {
                            conditions.iter().any(|condition| {
                                condition.type_ == "Ready" && condition.status == "True"
                            })
                        })
                })
            {
                return pod
                    .metadata
                    .name
                    .ok_or_else(|| "Pod has no name".to_string());
            }
        }
        Err("Expose pod watch ended before readiness".to_string())
    })
    .await
    .map_err(|_| "Timed out waiting for expose pod readiness".to_string())?
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
) -> Result<CreatedResource, ExposeCreateError> {
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);

    let mut values = HashMap::new();
    values.insert("service_name", service_name.to_string());
    values.insert("namespace", namespace.to_string());
    values.insert("config_id", config_id.to_string());
    values.insert("local_port", local_port.to_string());

    let template = templates::load_service_template()?;
    let rendered = templates::render_template(&template, &values);

    let mut service: Service =
        serde_json::from_str(&rendered).map_err(|e| format!("Failed to parse service: {}", e))?;
    tag_expose_ownership(&mut service.metadata.labels, config_id).await?;
    // Without this the Service would also select another installation's relay
    // pods and send its HTTP traffic to the wrong local service.
    // Only an existing selector is narrowed. Giving a selectorless Service one
    // would change it from manually managed endpoints to routing at every pod
    // this installation runs.
    if let Some(selector) = service
        .spec
        .as_mut()
        .and_then(|spec| spec.selector.as_mut())
        .filter(|selector| !selector.is_empty())
    {
        // Reconciled with the labels the pod template carries, for the same
        // reason as the Deployment selector: a customized `app` value is
        // overwritten there, and a Service still selecting the old one would
        // route to no pods at all.
        let mut ownership = None;
        tag_expose_ownership(&mut ownership, config_id).await?;
        selector.extend(ownership.unwrap_or_default());
    }

    let created = services
        .create(&PostParams::default(), &service)
        .await
        .map_err(|e| classify_create_error("service", &e))?;
    let created = created_from(ResourceKind::Service, &created);

    info!("Service created successfully");
    Ok(created)
}

async fn create_ingress(
    client: &Client, namespace: &str, ingress_name: &str, service_name: &str, config: &Config,
) -> Result<CreatedResource, ExposeCreateError> {
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

    let mut ingress: Ingress =
        serde_json::from_str(&rendered).map_err(|e| format!("Failed to parse ingress: {}", e))?;
    tag_expose_ownership(&mut ingress.metadata.labels, &config_id_str).await?;

    // Recorded before the request: an ingress can be created without this
    // client seeing the response, and cleanup for a configuration later
    // switched to private must not infer from its new type that no ingress
    // exists. The record survives restarts, where nothing else does.
    remember_ingress_created(&config_id_str).await;
    let created = ingresses
        .create(&PostParams::default(), &ingress)
        .await
        .map_err(|e| classify_create_error("ingress", &e))?;
    let created = created_from(ResourceKind::Ingress, &created);

    info!("Created ingress");
    Ok(created)
}

/// Key under which a configuration's ingress history is kept.
fn ingress_history_key(config_id: &str) -> String {
    format!("expose_ingress_created:{config_id}")
}

/// Records that this configuration created an ingress.
async fn remember_ingress_created(config_id: &str) {
    if let Err(error) =
        kftray_commons::utils::settings::set_setting(&ingress_history_key(config_id), "1").await
    {
        log::warn!("Failed to record the ingress history for config {config_id}: {error}");
    }
}

/// Whether this configuration ever created an ingress.
///
/// A configuration switched from public to private keeps the ingress it
/// created, so its current type is not evidence that none exists.
pub async fn ingress_was_created(config_id: &str) -> bool {
    kftray_commons::utils::settings::get_setting(&ingress_history_key(config_id))
        .await
        .ok()
        .flatten()
        .is_some()
}

/// Forgets the ingress history once cleanup has confirmed none is left.
async fn forget_ingress_history(config_id: &str) {
    if let Err(error) =
        kftray_commons::utils::settings::delete_setting(&ingress_history_key(config_id)).await
    {
        log::debug!("Failed to clear the ingress history for config {config_id}: {error}");
    }
}

async fn check_existing_resources(
    client: &Client, namespace: &str, config_id: &str,
) -> Option<Vec<String>> {
    let label_selector = expose_owner_selector(config_id).await.ok()?;

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

/// Names the items of a list, prefixed with their kind.
fn named_items<T: kube::Resource + Clone>(
    list: &kube::core::ObjectList<T>, kind: &str,
) -> Vec<String> {
    list.items
        .iter()
        .filter_map(|item| item.meta().name.clone())
        .map(|name| format!("{kind}/{name}"))
        .collect()
}

pub async fn delete_expose_resources(
    client: Client, namespace: &str, config_id_label: &str, ingress_possible: bool,
) -> Result<(), String> {
    // A configuration switched from public to private still owns the ingress it
    // created, and its role may not allow listing ingresses. Inferring absence
    // from the new type would leave that ingress serving the new tunnel
    // publicly, so history decides here, not the current configuration.
    let ingress_possible = ingress_possible || ingress_was_created(config_id_label).await;
    let lp = ListParams::default().labels(&expose_owner_selector(config_id_label).await?);

    info!(
        "Deleting expose resources for config_id label '{}'",
        config_id_label
    );

    let (ingresses, services, deployments) = tokio::join!(
        delete_ingresses(&client, namespace, &lp, ingress_possible),
        delete_services(&client, namespace, &lp),
        delete_deployments(&client, namespace, &lp)
    );
    let mut errors: Vec<String> = [ingresses, services, deployments]
        .into_iter()
        .filter_map(Result::err)
        .collect();

    // An accepted DELETE only starts deletion: a finalizer can keep the object,
    // and its containers, running. Anything still present keeps the
    // configuration tracked for a later retry.
    if errors.is_empty()
        && let Err(error) = wait_until_gone(&client, namespace, &lp, ingress_possible).await
    {
        errors.push(error);
    }

    // Exposures created before the installation label existed cannot be
    // attributed: configuration ids are local, so another installation in the
    // same namespace can have the same one. They are reported so an empty owned
    // list is not mistaken for confirmed cleanup.
    let unlabelled = ListParams::default().labels(&format!(
        "app=kftray-expose,config_id={config_id_label},!{}",
        crate::kube::proxy::INSTALLATION_LABEL
    ));
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let ingresses: Api<Ingress> = Api::namespaced(client.clone(), namespace);
    let mut leftovers: Vec<String> = Vec::new();
    // Every kind an exposure creates: a partial cleanup can take the Deployment
    // and leave the Service or Ingress serving traffic.
    match deployments.list(&unlabelled).await {
        Ok(list) => leftovers.extend(named_items(&list, "deployment")),
        // Collected rather than returned: this check runs after the deletions,
        // and its failure must not hide what they reported.
        Err(error) => errors.push(format!(
            "Failed to list earlier exposure deployments: {error}"
        )),
    }
    match services.list(&unlabelled).await {
        Ok(list) => leftovers.extend(named_items(&list, "service")),
        Err(error) => errors.push(format!("Failed to list earlier exposure services: {error}")),
    }
    match ingresses.list(&unlabelled).await {
        Ok(list) => leftovers.extend(named_items(&list, "ingress")),
        // A private exposure never creates an ingress and its role may not
        // allow listing them, so a refusal is not evidence of leftovers.
        Err(kube::Error::Api(response)) if response.code == 403 && !ingress_possible => {}
        Err(error) => errors.push(format!(
            "Failed to list earlier exposure ingresses: {error}"
        )),
    }
    if !leftovers.is_empty() {
        errors.push(format!(
            "Exposure resources from an earlier version are still running and cannot be \
             attributed to this installation: {}. Remove them from the server resources screen.",
            leftovers.join(", ")
        ));
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    // Nothing is left, so the history that forced the ingress checks above has
    // served its purpose.
    forget_ingress_history(config_id_label).await;
    info!(
        "Successfully deleted expose resources for config_id label '{}'",
        config_id_label
    );
    Ok(())
}

async fn delete_ingresses(
    client: &Client, namespace: &str, lp: &ListParams, ingress_possible: bool,
) -> Result<(), String> {
    let api: Api<Ingress> = Api::namespaced(client.clone(), namespace);

    let items = match api.list(lp).await {
        Ok(list) => list,
        // Only tolerated when no Ingress can exist: a private exposure never
        // creates one, so a role scoped to Deployments, Services and Pods is
        // legitimate. For a public exposure the same response would hide an
        // ingress that is still serving traffic.
        Err(kube::Error::Api(response))
            if !ingress_possible && (response.code == 403 || response.code == 404) =>
        {
            debug!("Skipping ingress cleanup: {}", response.message);
            return Ok(());
        }
        Err(e) => {
            return Err(format!("Failed to list expose ingresses: {e}"));
        }
    };

    if items.items.is_empty() {
        debug!("No ingresses found to delete");
        return Ok(());
    }

    let mut errors = Vec::new();
    for ingress in items.items {
        if let Some(name) = &ingress.metadata.name {
            info!("Deleting ingress: {}", name);
            match api.delete(name, &DeleteParams::default()).await {
                Ok(_) => info!("Ingress {} deleted successfully", name),
                Err(e) if matches!(&e, kube::Error::Api(response) if response.code == 404) => {}
                Err(e) => errors.push(format!("Failed to delete ingress {name}: {e}")),
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

async fn delete_services(client: &Client, namespace: &str, lp: &ListParams) -> Result<(), String> {
    let api: Api<Service> = Api::namespaced(client.clone(), namespace);

    let items = match api.list(lp).await {
        Ok(list) => list,
        Err(e) => {
            return Err(format!("Failed to list expose services: {e}"));
        }
    };

    if items.items.is_empty() {
        debug!("No services found to delete");
        return Ok(());
    }

    let mut errors = Vec::new();
    for service in items.items {
        if let Some(name) = &service.metadata.name {
            info!("Deleting service: {}", name);
            match api.delete(name, &DeleteParams::default()).await {
                Ok(_) => info!("Service {} deleted successfully", name),
                Err(e) if matches!(&e, kube::Error::Api(response) if response.code == 404) => {}
                Err(e) => errors.push(format!("Failed to delete service {name}: {e}")),
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Waits for every resource matching `lp` to disappear.
async fn wait_until_gone(
    client: &Client, namespace: &str, lp: &ListParams, ingress_possible: bool,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + ROLLBACK_DELETION_TIMEOUT;
    loop {
        let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
        let services: Api<Service> = Api::namespaced(client.clone(), namespace);
        let ingresses: Api<Ingress> = Api::namespaced(client.clone(), namespace);
        let mut remaining = names_matching(&deployments, lp)
            .await
            .map_err(|error| error.to_string())?;
        remaining.extend(
            names_matching(&services, lp)
                .await
                .map_err(|error| error.to_string())?,
        );
        // A private exposure never creates an Ingress, so a role without
        // permission to list them is legitimate. Only that refusal is
        // tolerated: a timeout or server error says nothing about whether an
        // ingress from an earlier public exposure is still serving traffic.
        match names_matching(&ingresses, lp).await {
            Ok(names) => remaining.extend(names),
            Err(kube::Error::Api(response))
                if !ingress_possible && matches!(response.code, 403 | 404) =>
            {
                debug!("Skipping ingress verification: {}", response.message);
            }
            Err(error) => return Err(error.to_string()),
        }
        if remaining.is_empty() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!(
                "Exposure resources are still terminating after {ROLLBACK_DELETION_TIMEOUT:?}: {}",
                remaining.join(", ")
            ));
        }
        tokio::time::sleep(ROLLBACK_DELETION_POLL).await;
    }
}

async fn names_matching<K>(api: &Api<K>, lp: &ListParams) -> Result<Vec<String>, kube::Error>
where
    K: Clone + serde::de::DeserializeOwned + std::fmt::Debug + kube::Resource,
{
    Ok(api
        .list(lp)
        .await?
        .items
        .iter()
        .filter_map(|item| item.meta().name.clone())
        .collect())
}

async fn delete_deployments(
    client: &Client, namespace: &str, lp: &ListParams,
) -> Result<(), String> {
    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    let items = match api.list(lp).await {
        Ok(list) => list,
        Err(e) => {
            return Err(format!("Failed to list expose deployments: {e}"));
        }
    };

    if items.items.is_empty() {
        debug!("No deployments found to delete");
        return Ok(());
    }

    let mut errors = Vec::new();
    for deployment in items.items {
        if let Some(name) = &deployment.metadata.name {
            info!("Deleting deployment: {}", name);
            let dp = DeleteParams {
                // Foreground propagation keeps the Deployment until its pods
                // are gone, so waiting for it to disappear also proves the
                // relay containers stopped.
                propagation_policy: Some(kube::api::PropagationPolicy::Foreground),
                ..DeleteParams::default()
            };
            match api.delete(name, &dp).await {
                Ok(_) => info!("Deployment {} deleted successfully", name),
                Err(e) if matches!(&e, kube::Error::Api(response) if response.code == 404) => {}
                Err(e) => errors.push(format!("Failed to delete deployment {name}: {e}")),
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use http::{
        Method,
        Request,
        Response,
    };
    use kube::client::Body;
    use tower_test::mock;

    use super::*;

    fn relay_container(env: Vec<k8s_openapi::api::core::v1::EnvVar>) -> Container {
        Container {
            name: "kftray-server".to_owned(),
            env: Some(env),
            ..Default::default()
        }
    }

    #[test]
    fn probe_ports_follow_a_customized_literal_value() {
        let container = relay_container(vec![k8s_openapi::api::core::v1::EnvVar {
            name: "HTTP_PORT".to_owned(),
            value: Some("9100".to_owned()),
            ..Default::default()
        }]);
        assert_eq!(
            container_env_port(&container, "HTTP_PORT", 8080),
            Some(9100)
        );
        assert_eq!(
            container_env_port(&container, "WEBSOCKET_PORT", 9999),
            Some(9999),
            "an absent variable still means the template default"
        );
    }

    #[test]
    fn no_probe_is_injected_for_a_dynamically_supplied_port() {
        let from_config_map = relay_container(vec![k8s_openapi::api::core::v1::EnvVar {
            name: "HTTP_PORT".to_owned(),
            value_from: Some(k8s_openapi::api::core::v1::EnvVarSource {
                config_map_key_ref: Some(k8s_openapi::api::core::v1::ConfigMapKeySelector {
                    name: "ports".to_owned(),
                    key: "http".to_owned(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }]);
        assert_eq!(
            container_env_port(&from_config_map, "HTTP_PORT", 8080),
            None
        );

        let mut from_env_from = relay_container(Vec::new());
        from_env_from.env_from = Some(vec![k8s_openapi::api::core::v1::EnvFromSource {
            config_map_ref: Some(k8s_openapi::api::core::v1::ConfigMapEnvSource {
                name: "ports".to_owned(),
                ..Default::default()
            }),
            ..Default::default()
        }]);
        assert_eq!(container_env_port(&from_env_from, "HTTP_PORT", 8080), None);

        let mut explicit_alongside_env_from =
            relay_container(vec![k8s_openapi::api::core::v1::EnvVar {
                name: "HTTP_PORT".to_owned(),
                value: Some("9100".to_owned()),
                ..Default::default()
            }]);
        explicit_alongside_env_from.env_from = from_env_from.env_from.clone();
        assert_eq!(
            container_env_port(&explicit_alongside_env_from, "HTTP_PORT", 8080),
            Some(9100),
            "an explicit env entry takes precedence over envFrom, as in Kubernetes"
        );
    }

    #[tokio::test]
    async fn cleanup_attempts_all_resources_and_ignores_not_found() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let mut deleted = std::collections::HashSet::new();
            while deleted.len() < 9 {
                let (request, send) = handle.next_request().await.unwrap();
                let name = request.uri().path().rsplit('/').next().unwrap();
                let (status, body) = if request.method() == Method::GET {
                    let items: Vec<_> = ["missing", "broken", "ok"].into_iter()
                        .map(|suffix| serde_json::json!({"metadata":{"name":format!("{name}-{suffix}")}}))
                        .collect();
                    (200, serde_json::json!({"items":items}))
                } else {
                    assert_eq!(request.method(), Method::DELETE);
                    assert!(deleted.insert(name.to_string()));
                    let status = if name.ends_with("-missing") {
                        404
                    } else if name.ends_with("-broken") {
                        500
                    } else {
                        200
                    };
                    (
                        status,
                        serde_json::json!({
                            "apiVersion":"v1", "kind":"Status", "status":if status==200 {"Success"} else {"Failure"},
                            "reason":if status==404 {"NotFound"} else {"InternalError"},
                            "message":name, "code":status
                        }),
                    )
                };
                send.send_response(
                    Response::builder()
                        .status(status)
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                );
            }
            deleted
        }));
        let (result, deleted) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let result = delete_expose_resources(client, "default", "42", true).await;
            (result, server.await.unwrap())
        })
        .await
        .unwrap();
        let error = result.expect_err("failed deletions must be reported");
        for kind in ["ingresses", "services", "deployments"] {
            for suffix in ["missing", "broken", "ok"] {
                assert!(deleted.contains(&format!("{kind}-{suffix}")));
            }
            assert!(error.contains(&format!("{kind}-broken")), "{error}");
            assert!(!error.contains(&format!("{kind}-missing")), "{error}");
            assert!(!error.contains(&format!("{kind}-ok")), "{error}");
        }
    }
}
