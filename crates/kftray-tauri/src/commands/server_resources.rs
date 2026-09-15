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
    /// Whether the object carries a `deletionTimestamp`. Not sent to the
    /// frontend: only used to keep a resource still finalizing out of the
    /// sibling check that decides whether a config's cluster obligation can
    /// be settled.
    #[serde(skip)]
    pub terminating: bool,
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
            list_pods_in_namespace(&client, &namespace, &config_ids, installation_id)
                .await
                .unwrap_or_default(),
        );

        let user_deployments =
            list_deployments_in_namespace(&client, &namespace, &config_ids, installation_id)
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

/// Whether an ownership label names this installation. A memory-mode run
/// extends the identity with a session identifier; its resources are still
/// this installation's to manage.
fn owned_by(owner: &str, installation_id: &str) -> bool {
    kftray_commons::utils::config_dir::owned_by_installation(owner, installation_id)
}

/// Whether a resource is this installation's to show, by label or, for
/// resources that predate the label, by name.
///
/// The label is the positive evidence: a customized manifest can carry any
/// name, and the label still says the resource is ours. Name matching only
/// covers what was created before the label existed.
fn is_ours(
    name: &str, labels: &std::collections::BTreeMap<String, String>, installation_id: &str,
) -> bool {
    match labels.get(kftray_portforward::kube::INSTALLATION_LABEL) {
        Some(owner) => owned_by(owner, installation_id),
        None => is_forward_name(name) || is_expose_name(name),
    }
}

/// Whether `name` is a relay this application could have created, by the
/// same prefix `proxy_resource_prefix` gives new resources: that helper is
/// the sole source of the naming rule, so a separately hand-rolled username
/// sanitization here can never drift from what it actually produces.
fn is_forward_name(name: &str) -> bool {
    name.starts_with(&kftray_portforward::kube::proxy_resource_prefix())
}

/// Whether `name` is an exposure this application could have created.
/// Mirrors `is_forward_name`.
fn is_expose_name(name: &str) -> bool {
    name.starts_with(&kftray_portforward::expose::kubernetes::expose_resource_prefix())
}

/// Whether a service or ingress belongs to this installation: by label, by a
/// name this application could have generated, or, because a public
/// exposure can rename its edge resource to an arbitrary subdomain, by
/// `config_id` when that id already belongs to a deployment known to be
/// ours.
fn is_ours_dependent(
    name: &str, labels: &std::collections::BTreeMap<String, String>, config_id: Option<&str>,
    deployment_config_ids: &[String], installation_id: &str,
) -> bool {
    match labels.get(kftray_portforward::kube::INSTALLATION_LABEL) {
        Some(owner) => owned_by(owner, installation_id),
        None => {
            is_forward_name(name)
                || is_expose_name(name)
                || config_id.is_some_and(|id| deployment_config_ids.contains(&id.to_string()))
        }
    }
}

/// The `config_id` used for attribution, the stop decision and the settle
/// decision is always the fetched object's own label, never the value the
/// caller passed alongside the delete request: a caller-supplied id can
/// name a different config than the object currently carries (relabeled
/// since the caller last listed it, or simply wrong), and trusting it would
/// let attribution, the stop and the settle decision reason about the
/// wrong config.
fn label_config_id(labels: &std::collections::BTreeMap<String, String>) -> Option<String> {
    labels.get("config_id").cloned()
}

/// Whether `kind` may treat `name` as attributable to this installation on
/// this delete screen. `kind` selects which attribution helper applies: a
/// pod or deployment keeps its own name, while a service or ingress can be
/// renamed to an arbitrary subdomain by a public exposure and so is also
/// checked against `deployment_config_ids`. An unlabeled resource whose
/// name matches neither the forward nor the expose prefix, and whose
/// `config_id` (if any) names no known deployment, is not attributable:
/// this is the gate that keeps a direct invoke with an arbitrary unlabeled
/// name from deleting a non-kftray object.
fn attribution_for_delete(
    kind: &str, name: &str, labels: &std::collections::BTreeMap<String, String>,
    config_id: Option<&str>, deployment_config_ids: &[String], installation_id: &str,
) -> bool {
    match kind {
        "service" | "ingress" => is_ours_dependent(
            name,
            labels,
            config_id,
            deployment_config_ids,
            installation_id,
        ),
        _ => is_ours(name, labels, installation_id),
    }
}

async fn list_pods_in_namespace(
    client: &Client, namespace: &str, config_ids: &[String], installation_id: &str,
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

            if !is_ours(&pod_name, pod.labels(), installation_id) {
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
                terminating: pod.metadata.deletion_timestamp.is_some(),
            })
        })
        .collect())
}

async fn list_deployments_in_namespace(
    client: &Client, namespace: &str, config_ids: &[String], installation_id: &str,
) -> Result<Vec<ServerResource>, String> {
    let deployments_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    // Selects on `config_id` existing rather than any specific label value: a
    // relay Deployment carries its own hashed name as `app`, not a shared
    // value, so listing only `app=kftray-expose` hid every proxy relay from
    // the screen that exists to remove them by hand. Both proxy and expose
    // manifests always carry `config_id`, so this stays cheap without
    // needing every Deployment in the namespace.
    let deployments = deployments_api
        .list(&ListParams::default().labels("config_id"))
        .await
        .map_err(|e| format!("Failed to list deployments: {e}"))?;

    Ok(deployments
        .items
        .into_iter()
        .filter(|deployment| {
            let name = deployment.name_any();
            is_ours(&name, deployment.labels(), installation_id)
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
                terminating: deployment.metadata.deletion_timestamp.is_some(),
            }
        })
        .collect())
}

async fn list_services_in_namespace(
    client: &Client, namespace: &str, deployment_config_ids: &[String], config_ids: &[String],
    installation_id: &str,
) -> Result<Vec<ServerResource>, String> {
    let services_api: Api<Service> = Api::namespaced(client.clone(), namespace);

    // Same treatment as deployments: a proxy or expose Service does not
    // carry `app=kftray-expose`, only `config_id`, so filtering on the app
    // label hid it from the screen that exists to remove it by hand.
    let services = services_api
        .list(&ListParams::default().labels("config_id"))
        .await
        .map_err(|e| format!("Failed to list services: {e}"))?;

    Ok(services
        .items
        .into_iter()
        .filter_map(|service| {
            let name = service.name_any();
            let config_id = service.labels().get("config_id").map(|s| s.to_string());
            if !is_ours_dependent(
                &name,
                service.labels(),
                config_id.as_deref(),
                deployment_config_ids,
                installation_id,
            ) {
                return None;
            }

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
                name,
                namespace: namespace.to_string(),
                config_id,
                is_orphaned,
                age,
                status: cluster_ip,
                terminating: service.metadata.deletion_timestamp.is_some(),
            })
        })
        .collect())
}

async fn list_ingresses_in_namespace(
    client: &Client, namespace: &str, deployment_config_ids: &[String], config_ids: &[String],
    installation_id: &str,
) -> Result<Vec<ServerResource>, String> {
    let ingresses_api: Api<Ingress> = Api::namespaced(client.clone(), namespace);

    // Same treatment as deployments and services: a proxy or expose Ingress
    // does not carry `app=kftray-expose`, only `config_id`.
    let ingresses = ingresses_api
        .list(&ListParams::default().labels("config_id"))
        .await
        .map_err(|e| format!("Failed to list ingresses: {e}"))?;

    Ok(ingresses
        .items
        .into_iter()
        .filter_map(|ingress| {
            let name = ingress.name_any();
            let config_id = ingress.labels().get("config_id").map(|s| s.to_string());
            if !is_ours_dependent(
                &name,
                ingress.labels(),
                config_id.as_deref(),
                deployment_config_ids,
                installation_id,
            ) {
                return None;
            }

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
                name,
                namespace: namespace.to_string(),
                config_id,
                is_orphaned,
                age,
                status: hosts,
                terminating: ingress.metadata.deletion_timestamp.is_some(),
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

/// Every namespace a config in `context_name` currently uses, deduplicated.
///
/// A config's cluster resources normally live in its own namespace; if the
/// config was later edited to point at a different one, its old resources
/// are left behind there. Checking every namespace the context's configs
/// use, not just the namespace a particular resource happens to be in, is
/// what finds those leftovers.
async fn namespaces_for_context(context_name: &str) -> Vec<String> {
    kftray_commons::config::get_configs()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.context.as_deref() == Some(context_name))
        .map(|c| c.namespace)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
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

    let connection = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| format!("Failed to create client for context '{context_name}': {err}"))?;

    let destination = kftray_portforward::kube::client::cluster_identity(&connection.cluster_url);
    let client = connection.client;
    let installation_id = kftray_commons::utils::config_dir::installation_id().await?;

    let delete_params = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::default()
    };

    // Fetched once and reused for the ownership gate, the stop decision and
    // the delete precondition: refetching between them let an unrelated
    // status write on the object turn a legitimate delete into a spurious
    // 409, and reading it twice asked the API server for the same object for
    // no reason. A resource labelled with another installation's id, or one
    // with no label that cannot be attributed either way, is left alone
    // entirely: neither stopped nor deleted.
    struct DeleteScope<'a> {
        params: &'a DeleteParams,
        installation_id: &'a str,
        namespace: &'a str,
        context_name: &'a str,
        destination: &'a str,
        client: &'a Client,
    }

    /// Whether any resource carrying `config_id` and this installation's
    /// label still exists anywhere in the context, checked across every
    /// namespace the context's configs currently use plus the just-deleted
    /// resource's own namespace, and every kind the screen lists. A
    /// config's resources normally live in its own namespace, but one
    /// edited to point elsewhere after creation leaves old resources behind
    /// in the previous namespace, so every namespace is checked rather than
    /// only the just-deleted resource's own; that namespace is still always
    /// included because a config whose row was itself removed, or whose
    /// current namespace no longer matches, would otherwise never be
    /// checked there. A single resource just deleted by hand may be only
    /// one of several this installation created for the same config (a
    /// proxy's relay Deployment, Service and Pod, say), so the cluster
    /// obligation is only settled once none of them remain; a sibling still
    /// carrying a `deletionTimestamp` is already on its way out and does
    /// not count, or a just-deleted object stuck finalizing would block
    /// settling indefinitely.
    async fn any_sibling_resources_remain(
        client: &Client, context_name: &str, just_deleted_namespace: &str, id: i64,
        installation_id: &str,
    ) -> Result<bool, String> {
        let target = id.to_string();
        let matches = |resources: &[ServerResource]| {
            resources.iter().any(|resource| {
                !resource.terminating && resource.config_id.as_deref() == Some(target.as_str())
            })
        };

        let mut namespaces = namespaces_for_context(context_name).await;
        if !namespaces.iter().any(|ns| ns == just_deleted_namespace) {
            namespaces.push(just_deleted_namespace.to_string());
        }

        for namespace in namespaces {
            let pods = list_pods_in_namespace(client, &namespace, &[], installation_id).await?;
            if matches(&pods) {
                return Ok(true);
            }

            let deployments =
                list_deployments_in_namespace(client, &namespace, &[], installation_id).await?;
            if matches(&deployments) {
                return Ok(true);
            }
            let deployment_config_ids: Vec<String> = deployments
                .iter()
                .filter_map(|d| d.config_id.clone())
                .collect();

            let services = list_services_in_namespace(
                client,
                &namespace,
                &deployment_config_ids,
                &[],
                installation_id,
            )
            .await?;
            if matches(&services) {
                return Ok(true);
            }

            let ingresses = list_ingresses_in_namespace(
                client,
                &namespace,
                &deployment_config_ids,
                &[],
                installation_id,
            )
            .await?;
            if matches(&ingresses) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn delete_kube_resource<K>(
        api: Api<K>, name: &str, kind: &str, scope: &DeleteScope<'_>,
    ) -> Result<(), String>
    where
        K: Clone + serde::de::DeserializeOwned + std::fmt::Debug + kube::Resource<DynamicType = ()>,
    {
        let DeleteScope {
            params,
            installation_id,
            namespace,
            context_name,
            destination,
            client,
        } = *scope;

        let object = match api.get_opt(name).await {
            Ok(Some(object)) => object,
            Ok(None) => return Ok(()),
            Err(e) => return Err(format!("Failed to read {kind}: {e}")),
        };
        let labels = object.meta().labels.clone().unwrap_or_default();
        let object_config_id = label_config_id(&labels);
        let deployment_config_ids: Vec<String> = if matches!(kind, "service" | "ingress") {
            list_deployments_in_namespace(client, namespace, &[], installation_id)
                .await?
                .into_iter()
                .filter_map(|d| d.config_id)
                .collect()
        } else {
            Vec::new()
        };
        let attributed = attribution_for_delete(
            kind,
            name,
            &labels,
            object_config_id.as_deref(),
            &deployment_config_ids,
            installation_id,
        );
        if !attributed {
            return Err(
                if labels.contains_key(kftray_portforward::kube::INSTALLATION_LABEL) {
                    format!(
                        "{kind} {name} belongs to another kftray installation and was left alone"
                    )
                } else {
                    format!("{kind} {name} is not a kftray-managed resource and was left alone")
                },
            );
        }

        // The configuration id on the object only names a row in the file
        // database that created it. A memory-mode session's relay, or a
        // legacy resource with no label, can carry the same numeric id as
        // an unrelated file-mode config; the local stop is only attempted
        // when the label names this exact file-mode installation, so those
        // other cases fall straight through to the delete below.
        let is_exact_installation_owner = labels
            .get(kftray_portforward::kube::INSTALLATION_LABEL)
            .is_some_and(|owner| owner == installation_id);

        if is_exact_installation_owner
            && let Some(config_id_str) = object_config_id.as_ref()
            && let Ok(id) = config_id_str.parse::<i64>()
            && let Ok(config) = kftray_commons::config::get_config(id).await
        {
            info!("Config found, stopping port-forward before deleting resource");

            let workload_type = config.workload_type.as_deref().unwrap_or("");

            match workload_type {
                "proxy" => {
                    let _ = kftray_portforward::stop_proxy_forward(id, namespace, name.to_string())
                        .await;
                }
                "expose" => {
                    let _ = kftray_portforward::stop_expose(id, DatabaseMode::File).await;
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

        // Deleted only if it is still the object identified by this uid: the
        // name can be reused between the fetch above and this delete, and
        // the precondition makes the server refuse in that case. Unlike the
        // uid, the resource version is expected to change on any unrelated
        // status write between the fetch and the delete, which made the
        // precondition fail with 409 far more than the reused-name case it
        // was meant to catch.
        let params = DeleteParams {
            preconditions: Some(kube::api::Preconditions {
                uid: object.meta().uid.clone(),
                resource_version: None,
            }),
            ..params.clone()
        };
        let result = match api.delete(name, &params).await {
            Ok(_) => Ok(()),
            Err(kube::Error::Api(response)) if response.code == 404 => Ok(()),
            Err(kube::Error::Api(response)) if response.code == 409 => Err(format!(
                "{kind} {name} changed while it was being checked and was left alone; refresh and \
                 try again"
            )),
            Err(e) => Err(format!("Failed to delete {kind}: {e}")),
        };

        // A labelled resource this installation just deleted by hand may be
        // only one of several sharing this config_id (a proxy's relay
        // Deployment, Service and Pod, say); settle the recorded cluster
        // obligation only once none of them remain, so stop-all and
        // delete-if-idle stop refusing the row exactly when the cluster
        // footprint is actually gone. A no-op when there is no matching
        // record, e.g. the obligation was already settled or never existed.
        if result.is_ok()
            && is_exact_installation_owner
            && let Some(config_id_str) = object_config_id.as_ref()
            && let Ok(id) = config_id_str.parse::<i64>()
        {
            match any_sibling_resources_remain(client, context_name, namespace, id, installation_id)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    if let Err(e) =
                        kftray_portforward::kube::settle_cluster_obligation(id, destination).await
                    {
                        error!(
                            "Failed to settle cluster obligation for config {id} after manual \
                             delete: {e}"
                        );
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to check for remaining resources for config {id} before \
                         settling its cluster obligation: {e}"
                    );
                }
            }
        }

        result
    }

    let scope = DeleteScope {
        params: &delete_params,
        installation_id,
        namespace,
        context_name,
        destination: &destination,
        client: &client,
    };

    match resource_type {
        "pod" => {
            delete_kube_resource(
                Api::<Pod>::namespaced(client.clone(), namespace),
                resource_name,
                "pod",
                &scope,
            )
            .await?
        }
        "deployment" => {
            delete_kube_resource(
                Api::<Deployment>::namespaced(client.clone(), namespace),
                resource_name,
                "deployment",
                &scope,
            )
            .await?
        }
        "service" => {
            delete_kube_resource(
                Api::<Service>::namespaced(client.clone(), namespace),
                resource_name,
                "service",
                &scope,
            )
            .await?
        }
        "ingress" => {
            delete_kube_resource(
                Api::<Ingress>::namespaced(client.clone(), namespace),
                resource_name,
                "ingress",
                &scope,
            )
            .await?
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An unlabeled name this installation never created must not be
    /// deletable: pre-fix, `delete_kube_resource` gated on `belongs_here`,
    /// which treated a missing label as owned regardless of the name, so a
    /// direct invoke naming an arbitrary unlabeled pod or deployment would
    /// pass the gate and delete a non-kftray object.
    #[test]
    fn an_unlabeled_pod_or_deployment_name_never_created_by_this_installation_is_not_attributable()
    {
        let labels = std::collections::BTreeMap::new();

        for kind in ["pod", "deployment"] {
            assert!(
                !attribution_for_delete(
                    kind,
                    "nginx-unrelated-deployment",
                    &labels,
                    None,
                    &[],
                    "test-installation-id",
                ),
                "an unlabeled {kind} whose name matches neither the forward nor the expose \
                 prefix must not be attributed to this installation"
            );
        }
    }

    /// The service/ingress branch: same unattributable-name guarantee, plus
    /// a `config_id` that names no known deployment must not attribute it
    /// either.
    #[test]
    fn an_unlabeled_service_or_ingress_name_never_created_by_this_installation_is_not_attributable()
    {
        let labels = std::collections::BTreeMap::new();

        for kind in ["service", "ingress"] {
            assert!(
                !attribution_for_delete(
                    kind,
                    "nginx-unrelated-service",
                    &labels,
                    Some("42"),
                    &["7".to_string(), "13".to_string()],
                    "test-installation-id",
                ),
                "an unlabeled {kind} whose name and config_id match nothing this installation \
                 created must not be attributed to it"
            );
        }
    }

    /// Sanity check on the positive side: a labeled resource owned by this
    /// installation stays attributable regardless of its name, for every
    /// resource kind the delete screen handles.
    #[test]
    fn a_labeled_resource_owned_by_this_installation_is_attributable_for_every_kind() {
        let mut labels = std::collections::BTreeMap::new();
        labels.insert(
            kftray_portforward::kube::INSTALLATION_LABEL.to_string(),
            "test-installation-id".to_string(),
        );

        for kind in ["pod", "deployment", "service", "ingress"] {
            assert!(
                attribution_for_delete(
                    kind,
                    "arbitrary-name",
                    &labels,
                    None,
                    &[],
                    "test-installation-id",
                ),
                "a {kind} labeled with this installation's id must be attributable regardless \
                 of its name"
            );
        }
    }

    /// The `config_id` fed into attribution, stop and settle decisions must
    /// come from the fetched object's own label, never a value supplied
    /// alongside the request: `label_config_id` only ever reads the label
    /// map.
    #[test]
    fn label_config_id_reads_only_the_objects_own_label() {
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("config_id".to_string(), "7".to_string());
        assert_eq!(label_config_id(&labels), Some("7".to_string()));

        let unlabeled = std::collections::BTreeMap::new();
        assert_eq!(label_config_id(&unlabeled), None);
    }

    /// Regression for the caller-supplied `config_id` path: a value naming
    /// no known deployment must not be attributable even though some
    /// `config_id` was supplied, while the object's own label, read via
    /// `label_config_id`, does attribute it. Attribution must be driven by
    /// the label, not by whatever id happened to arrive with the request.
    #[test]
    fn attribution_uses_the_objects_own_config_id_label_not_a_caller_supplied_one() {
        let labels = std::collections::BTreeMap::new();
        let mut object_labels = std::collections::BTreeMap::new();
        object_labels.insert("config_id".to_string(), "7".to_string());
        let deployment_config_ids = vec!["7".to_string()];

        assert!(
            attribution_for_delete(
                "service",
                "renamed-subdomain",
                &labels,
                label_config_id(&object_labels).as_deref(),
                &deployment_config_ids,
                "test-installation-id",
            ),
            "a config_id read from the object's own label naming a known deployment must \
             attribute it"
        );

        assert!(
            !attribution_for_delete(
                "service",
                "renamed-subdomain",
                &labels,
                Some("99"),
                &deployment_config_ids,
                "test-installation-id",
            ),
            "a config_id naming no known deployment must not attribute the resource, even \
             though some config_id was supplied"
        );
    }
}
