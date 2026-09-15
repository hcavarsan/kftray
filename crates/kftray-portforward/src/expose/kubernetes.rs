use std::collections::HashMap;
use std::time::{
    Duration,
    SystemTime,
    UNIX_EPOCH,
};

use futures::StreamExt;
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
use kftray_commons::utils::db_mode::DatabaseMode;
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
    warn,
};
use tokio_util::sync::CancellationToken;

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

/// Name prefix shared by every expose resource this user creates.
///
/// The username is reduced to at most 16 ASCII alphanumeric characters,
/// mirroring [`crate::kube::proxy::proxy_resource_prefix`]: the full name is
/// also used as an `app` label value, which Kubernetes caps at 63 characters
/// and restricts to ASCII, so a long or non-ASCII username would make every
/// create request fail.
pub fn expose_resource_prefix() -> String {
    let username: String = whoami::username()
        .unwrap_or_else(|_| "unknown".to_string())
        .to_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(16)
        .collect();
    format!("kftray-expose-{username}-")
}

pub async fn create_expose_resources(
    connection: &crate::kube::client::KubeConnection, config: &Config, mode: DatabaseMode,
    cancellation: Option<&CancellationToken>,
) -> Result<ExposeResources, ExposeCreateError> {
    let client = connection.client.clone();
    let location = ExposeLocation::resolve(&connection.cluster_url, &config.namespace);
    let config_id_str = config
        .id
        .map_or_else(|| "default".to_string(), |id| id.to_string());

    // Every start begins with the same cleanup a stop performs, whether or not
    // a Deployment is visible. Looking only for a Deployment misses the case
    // that matters most: a partial cleanup that removed it but left the
    // Service and a public Ingress, which would then front the tunnel this
    // attempt creates even if it is now configured as private. The cleanup
    // consults the ingress history, verifies that everything is gone, and
    // reports resources from an earlier version it cannot attribute, so it
    // must succeed before anything is created. A failure keeps the leftovers
    // this attempt's responsibility rather than letting the cleanup record be
    // dropped.
    // Bounded as a whole: the client carries no per-request timeout, and a
    // stalled request here would otherwise hold the start, and the batch
    // collecting its result, until the user cancelled it. The cleanup record
    // this attempt is responsible for is untouched by the deadline. Comfortably
    // above `ROLLBACK_DELETION_TIMEOUT`: waiting for earlier resources to
    // disappear is only part of this deadline, alongside the deletes and
    // listings around it.
    const PRE_START_CLEANUP_TIMEOUT: Duration = Duration::from_secs(60);
    let cleanup_timeout = async {
        tokio::time::timeout(
            PRE_START_CLEANUP_TIMEOUT,
            delete_expose_resources(
                client.clone(),
                &config.namespace,
                &config_id_str,
                config.exposure_type.as_deref() == Some("public"),
                &location,
                mode,
            ),
        )
        .await
        .unwrap_or_else(|_| {
            Err(format!(
                "Timed out after {PRE_START_CLEANUP_TIMEOUT:?} cleaning up the earlier exposure \
                 for config {config_id_str}"
            ))
        })
    };
    // Nothing has been created yet, so cancelling this wait is a clean unwind
    // rather than a rollback: a stop must not be held behind the full cleanup
    // budget on top of the readiness budget below.
    match cancellation {
        Some(token) => tokio::select! {
            biased;
            _ = token.cancelled() => Err(format!(
                "Expose startup cancelled for config {config_id_str} before cleaning up the \
                 earlier exposure"
            )),
            result = cleanup_timeout => result,
        },
        None => cleanup_timeout.await,
    }
    .map_err(ExposeCreateError::from)?;

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

    let deployment_name = format!(
        "{}{}-{}",
        expose_resource_prefix(),
        timestamp,
        random_string
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
        let (deployment, relay) = create_deployment(
            &client,
            &config.namespace,
            &deployment_name,
            &config_id_str,
            config,
            mode,
        )
        .await?;
        created.push(deployment);

        // Cancellable, unlike the create above: the deployment is already in
        // `created` for UID-scoped rollback, so abandoning the wait here does
        // not lose track of anything, and a stop must not be held behind the
        // full readiness budget.
        let pod_name = wait_for_pod_ready(
            &client,
            &config.namespace,
            &config_id_str,
            mode,
            &relay,
            cancellation,
        )
        .await?;

        let pod_ip = get_pod_ip(&client, &config.namespace, &pod_name).await?;

        let local_port = config.local_port.unwrap_or(8080);
        created.push(
            create_service(
                &client,
                &config.namespace,
                &service_name,
                &config_id_str,
                local_port,
                mode,
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
                    &location,
                    mode,
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
            websocket_port: relay.websocket_port,
            owned: std::mem::take(&mut created),
        })
    }
    .await;
    if let Err(error) = result {
        let (cleaned, message) = rollback_created_resources(
            &client,
            &config.namespace,
            &created,
            &config_id_str,
            &location,
            mode,
            error.message,
        )
        .await;
        return Err(ExposeCreateError {
            message,
            ambiguous: error.ambiguous,
            rolled_back: cleaned,
        });
    }
    result
}

/// Labels every exposure resource must carry for cleanup to find it, applied
/// programmatically so a customized template cannot omit them.
async fn tag_expose_ownership(
    labels: &mut Option<std::collections::BTreeMap<String, String>>, config_id: &str,
    mode: DatabaseMode,
) -> Result<(), String> {
    let labels = labels.get_or_insert_with(std::collections::BTreeMap::new);
    labels.insert("app".to_owned(), "kftray-expose".to_owned());
    labels.insert("config_id".to_owned(), config_id.to_owned());
    labels.insert(
        crate::kube::proxy::INSTALLATION_LABEL.to_owned(),
        kftray_commons::utils::config_dir::owner_identity(mode).await?,
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
pub async fn expose_owner_selector(config_id: &str, mode: DatabaseMode) -> Result<String, String> {
    Ok(format!(
        "app=kftray-expose,config_id={config_id},{}={}",
        crate::kube::proxy::INSTALLATION_LABEL,
        kftray_commons::utils::config_dir::owner_identity(mode).await?
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

/// How long one create request may take before it is treated as unanswered.
///
/// The client carries no per-request timeout, so without this a server that
/// accepts a POST and never answers holds the start, and the batch waiting on
/// it, until the user cancels. A request that timed out may still have been
/// applied, so it is ambiguous, never a proven rejection.
const CREATE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

async fn create_bounded<K>(
    api: &Api<K>, kind: ResourceKind, object: &K,
) -> Result<CreatedResource, ExposeCreateError>
where
    K: Clone + serde::de::DeserializeOwned + std::fmt::Debug + serde::Serialize + kube::Resource,
{
    match tokio::time::timeout(
        CREATE_REQUEST_TIMEOUT,
        api.create(&PostParams::default(), object),
    )
    .await
    {
        Ok(Ok(created)) => Ok(CreatedResource {
            kind,
            name: created.meta().name.clone().unwrap_or_default(),
            uid: created.meta().uid.clone(),
        }),
        Ok(Err(error)) => Err(classify_create_error(kind, &error)),
        Err(_) => Err(ExposeCreateError {
            message: format!(
                "Timed out after {CREATE_REQUEST_TIMEOUT:?} creating the {}; the server may still \
                 be applying it",
                kind.label()
            ),
            ambiguous: true,
            rolled_back: false,
        }),
    }
}

/// Classifies a failed create. Only a definitive rejection proves the object
/// was not created; a transport failure or a server-side timeout can be
/// returned while the request is still being applied.
fn classify_create_error(kind: ResourceKind, error: &kube::Error) -> ExposeCreateError {
    let ambiguous = match error {
        kube::Error::Api(response) => {
            // 409 on create means an object by this name already exists. That
            // can be this same attempt's own request, applied after an earlier
            // response was lost to a timeout or a dropped connection: treating
            // it as a definitive rejection would clear the ingress history (or
            // disarm the cleanup guard) for an object that is still there.
            matches!(response.code, 408 | 409 | 429 | 500 | 502 | 503 | 504)
        }
        _ => true,
    };
    ExposeCreateError {
        message: format!("Failed to create {}: {error}", kind.label()),
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

#[derive(Debug, Clone, Copy)]
pub enum ResourceKind {
    Deployment,
    Service,
    Ingress,
}

impl ResourceKind {
    /// The lowercase noun used in log and error messages.
    fn label(self) -> &'static str {
        match self {
            Self::Deployment => "deployment",
            Self::Service => "service",
            Self::Ingress => "ingress",
        }
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
            // so the check below decides whether anything is actually left. A
            // read failure is treated as "unknown, assume still present"
            // rather than propagated: doing that here would discard every
            // delete error this loop already collected.
            let conflict = matches!(&error, kube::Error::Api(response) if response.code == 409);
            let still_owned = !conflict
                || match still_present(client, namespace, resource).await {
                    Ok(present) => present,
                    Err(read_error) => {
                        errors.push(format!(
                            "could not confirm whether {} still exists after a conflicting \
                             delete: {read_error}",
                            resource.name
                        ));
                        true
                    }
                };
            if still_owned {
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
    let mut read_errors: Vec<String> = Vec::new();
    loop {
        let mut remaining = Vec::new();
        for resource in created {
            match still_present(client, namespace, resource).await {
                Ok(true) => remaining.push(resource.name.clone()),
                Ok(false) => {}
                Err(error) => {
                    // Unknown: assume still present rather than declaring this
                    // resource gone on a transient read failure, and keep
                    // polling instead of aborting the rollback outright.
                    remaining.push(resource.name.clone());
                    read_errors.push(error);
                }
            }
        }
        if remaining.is_empty() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            let mut message = format!(
                "Resources are still terminating after {ROLLBACK_DELETION_TIMEOUT:?}: {}",
                remaining.join(", ")
            );
            if !read_errors.is_empty() {
                message.push_str(&format!(
                    "; also failed to confirm: {}",
                    read_errors.join("; ")
                ));
            }
            return Err(message);
        }
        tokio::time::sleep(ROLLBACK_DELETION_POLL).await;
    }
}

/// Longer than the default 30s terminationGracePeriodSeconds (the template
/// itself also sets a short one): a relay that only exits on the deadline
/// must not be routinely reported as still terminating.
const ROLLBACK_DELETION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
const ROLLBACK_DELETION_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// Deletes exactly what one creation attempt made, under one deadline, and
/// describes the outcome as one message built on `reason`.
///
/// Bounded as a whole: this runs while the lifecycle lock is held and the
/// client carries no per-request timeout, so a stalled DELETE would block
/// both this startup and the stop that follows it. Returns whether the
/// created resources were fully deleted, so a caller decides only what that
/// means for its own cleanup guard.
pub(crate) async fn rollback_created_resources(
    client: &Client, namespace: &str, created: &[CreatedResource], config_id: &str,
    location: &ExposeLocation, mode: DatabaseMode, reason: String,
) -> (bool, String) {
    // Comfortably above `ROLLBACK_DELETION_TIMEOUT`: the wait for resources to
    // disappear is only part of this deadline, alongside the delete requests
    // that precede it.
    const ROLLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    let cleaned = tokio::time::timeout(
        ROLLBACK_TIMEOUT,
        delete_created_resources(client, namespace, created),
    )
    .await;

    // Confirmed by the UID-scoped delete above, not inferred from the current
    // exposure type: the ingress history has to be cleared whenever this
    // attempt's ingress is proven gone, or a later private start needs
    // ingress-list rights it may not have just to rule this exposure out.
    if matches!(cleaned, Ok(Ok(())))
        && created
            .iter()
            .any(|r| matches!(r.kind, ResourceKind::Ingress))
    {
        forget_ingress_history(config_id, location, mode).await;
    }

    match cleaned {
        Ok(Ok(())) => (true, reason),
        Ok(Err(cleanup_error)) => (false, format!("{reason}; cleanup failed: {cleanup_error}")),
        Err(_) => (
            false,
            format!("{reason}; cleanup timed out after {ROLLBACK_TIMEOUT:?} and will be retried"),
        ),
    }
}

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

/// Which container in the pod is the relay, and whether it ended up with a
/// readiness probe (injected here, or already present in a customized
/// template). [`wait_for_pod_ready`] uses this to decide whether the pod's
/// aggregate `Ready` condition is a meaningful signal: without any
/// readiness probe on the relay container, Kubernetes still requires every
/// *other* container in the pod to report ready before the pod condition
/// flips, which has nothing to do with the relay actually listening.
#[derive(Debug)]
struct RelayProbe {
    container_name: Option<String>,
    readiness_probe_present: bool,
    /// The pod-side port the tunnel's port-forward must target. Resolved the
    /// same way the startup probe is, so a relay whose `WEBSOCKET_PORT` is
    /// customized is still reachable once the deployment is up.
    websocket_port: u16,
}

async fn create_deployment(
    client: &Client, namespace: &str, deployment_name: &str, config_id: &str, config: &Config,
    mode: DatabaseMode,
) -> Result<(CreatedResource, RelayProbe), ExposeCreateError> {
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
    tag_expose_ownership(&mut deployment.metadata.labels, config_id, mode).await?;
    if let Some(spec) = deployment.spec.as_mut() {
        tag_expose_ownership(
            &mut spec.template.metadata.get_or_insert_default().labels,
            config_id,
            mode,
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
        // Expressions are ANDed with the labels above, so one naming an
        // ownership key contradicts them: a custom template selecting
        // `app In [custom-relay]` would be rejected outright once the pod
        // template carries `app: kftray-expose`. The labels now pin those keys,
        // so the expressions for them have nothing left to say.
        if let Some(expressions) = spec.selector.match_expressions.as_mut() {
            expressions.retain(|expression| !is_ownership_label(&expression.key));
        }
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
    let Some(container) = relay else {
        return Err(
            "Expose deployment has no container named kftray-server and none with a literal \
             PROXY_TYPE=reverse_http env entry, so the relay cannot be identified"
                .into(),
        );
    };
    // Guessing 9999 here would target the wrong pod port whenever the
    // template resolves it from a source (`valueFrom`/`envFrom`); the tunnel
    // would then port-forward to a port the relay never listens on.
    let websocket_port =
        container_env_port(container, "WEBSOCKET_PORT", 9999).ok_or_else(|| {
            "WEBSOCKET_PORT is set from valueFrom/envFrom, which cannot be resolved before the \
         relay pod exists; the tunnel's target port cannot be determined"
                .to_string()
        })?;
    let websocket_port = u16::try_from(websocket_port)
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| {
            format!(
                "WEBSOCKET_PORT {websocket_port} is not a valid port (must be between 1 and \
                 65535)"
            )
        })?;
    let mut relay_probe = RelayProbe {
        container_name: Some(container.name.clone()),
        readiness_probe_present: false,
        websocket_port,
    };
    container.startup_probe.get_or_insert_with(|| Probe {
        tcp_socket: Some(TCPSocketAction {
            port: IntOrString::Int(websocket_port as i32),
            ..Default::default()
        }),
        period_seconds: Some(1),
        timeout_seconds: Some(1),
        failure_threshold: Some(30),
        ..Default::default()
    });
    if let Some(http_port) = container_env_port(container, "HTTP_PORT", 8080) {
        let http_port = u16::try_from(http_port)
            .ok()
            .filter(|port| *port != 0)
            .ok_or_else(|| {
                format!("HTTP_PORT {http_port} is not a valid port (must be between 1 and 65535)")
            })?;
        container.readiness_probe.get_or_insert_with(|| Probe {
            tcp_socket: Some(TCPSocketAction {
                port: IntOrString::Int(http_port as i32),
                ..Default::default()
            }),
            period_seconds: Some(1),
            timeout_seconds: Some(1),
            failure_threshold: Some(3),
            ..Default::default()
        });
    }
    relay_probe.readiness_probe_present = container.readiness_probe.is_some();

    let created = create_bounded(&deployments, ResourceKind::Deployment, &deployment).await?;

    info!("Deployment created successfully");
    Ok((created, relay_probe))
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

/// Whether the relay is ready to receive traffic.
///
/// With a readiness probe present on the relay container, the pod's
/// aggregate `Ready` condition already reflects it and stays the signal,
/// unchanged from before. Without one, the aggregate condition also waits on
/// every other container in the pod, which the relay's own listener has
/// nothing to do with, so `Running` plus the relay container's own
/// `started`/`ready` status is what actually says it can serve traffic.
fn relay_pod_ready(pod: &Pod, relay: &RelayProbe) -> bool {
    if pod.metadata.deletion_timestamp.is_some() {
        return false;
    }
    let Some(status) = pod.status.as_ref() else {
        return false;
    };
    if status.phase.as_deref() != Some("Running") {
        return false;
    }
    match (&relay.container_name, relay.readiness_probe_present) {
        (Some(container_name), false) => {
            status
                .container_statuses
                .as_ref()
                .is_some_and(|containers| {
                    containers.iter().any(|container| {
                        &container.name == container_name
                            && (container.ready || container.started == Some(true))
                    })
                })
        }
        _ => status.conditions.as_ref().is_some_and(|conditions| {
            conditions
                .iter()
                .any(|condition| condition.type_ == "Ready" && condition.status == "True")
        }),
    }
}

async fn wait_for_pod_ready(
    client: &Client, namespace: &str, config_id: &str, mode: DatabaseMode, relay: &RelayProbe,
    cancellation: Option<&CancellationToken>,
) -> Result<String, String> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    // Backed off and retried rather than propagated: every `watcher::Error` is
    // recoverable and leaves the stream usable, so an idle connection reset or
    // a resource-version expiry would otherwise fail a startup the watch
    // recovers from on its own. The timeout below stays the only way out.
    let watcher = kube_runtime::watcher(
        pods,
        kube_runtime::watcher::Config::default()
            .labels(&expose_owner_selector(config_id, mode).await?),
    )
    .default_backoff()
    .applied_objects();
    futures::pin_mut!(watcher);
    let watch = async {
        while let Some(event) = watcher.next().await {
            let pod = match event {
                Ok(pod) => pod,
                Err(error) => {
                    debug!("Retrying the relay pod watch: {error}");
                    continue;
                }
            };
            if relay_pod_ready(&pod, relay) {
                return pod
                    .metadata
                    .name
                    .ok_or_else(|| "Pod has no name".to_string());
            }
        }
        Err("Expose pod watch ended before readiness".to_string())
    };
    // Raced against the startup token so a stop is not held behind the full
    // readiness budget: the deployment this pod belongs to is already in the
    // caller's `created` list, so cancelling here still rolls it back by UID.
    match cancellation {
        Some(token) => tokio::select! {
            biased;
            _ = token.cancelled() => Err(format!("Expose startup cancelled for config {config_id}")),
            result = tokio::time::timeout(Duration::from_secs(120), watch) => result
                .map_err(|_| "Timed out waiting for expose pod readiness".to_string())?,
        },
        None => tokio::time::timeout(Duration::from_secs(120), watch)
            .await
            .map_err(|_| "Timed out waiting for expose pod readiness".to_string())?,
    }
}

async fn get_pod_ip(client: &Client, namespace: &str, pod_name: &str) -> Result<String, String> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pod = tokio::time::timeout(CREATE_REQUEST_TIMEOUT, pods.get(pod_name))
        .await
        .map_err(|_| format!("Timed out after {CREATE_REQUEST_TIMEOUT:?} reading the relay pod"))?
        .map_err(|e| format!("Failed to get pod: {}", e))?;

    let pod_ip = pod.status.and_then(|s| s.pod_ip).ok_or("Pod has no IP")?;

    Ok(pod_ip)
}

async fn create_service(
    client: &Client, namespace: &str, service_name: &str, config_id: &str, local_port: u16,
    mode: DatabaseMode,
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
    tag_expose_ownership(&mut service.metadata.labels, config_id, mode).await?;
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
        tag_expose_ownership(&mut ownership, config_id, mode).await?;
        selector.extend(ownership.unwrap_or_default());
    }

    let created = create_bounded(&services, ResourceKind::Service, &service).await?;

    info!("Service created successfully");
    Ok(created)
}

async fn create_ingress(
    client: &Client, namespace: &str, ingress_name: &str, service_name: &str, config: &Config,
    location: &ExposeLocation, mode: DatabaseMode,
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
    tag_expose_ownership(&mut ingress.metadata.labels, &config_id_str, mode).await?;

    // Recorded before the request: an ingress can be created without this
    // client seeing the response, and cleanup for a configuration later
    // switched to private must not infer from its new type that no ingress
    // exists. The record survives restarts, where nothing else does.
    remember_ingress_created(&config_id_str, location, mode).await?;
    match create_bounded(&ingresses, ResourceKind::Ingress, &ingress).await {
        Ok(created) => {
            info!("Created ingress");
            Ok(created)
        }
        Err(error) => {
            if !error.ambiguous {
                // A definitive rejection proves nothing was created despite
                // the record above: clear it so a later private start does
                // not need ingress-list rights to rule this exposure out.
                forget_ingress_history(&config_id_str, location, mode).await;
                return Err(error);
            }
            // An unanswered create may or may not have landed. Check for the
            // object this attempt would have named: if it is missing, the
            // history is unresolved and the original error stays ambiguous
            // for a later check to retry. If it is present but owned by
            // someone else, that is proof this attempt's create did not land
            // for it, a definitive outcome. If it is ours, adopt it so the
            // caller's rollback tracks it like any other resource this
            // attempt created, instead of leaving a history record with
            // nothing to roll back.
            match resolve_ambiguous_ingress_create(
                &ingresses,
                ingress_name,
                &config_id_str,
                location,
                mode,
            )
            .await
            {
                Ok(IngressCreateResolution::Adopted(adopted)) => Ok(adopted),
                Ok(IngressCreateResolution::StillAmbiguous) => Err(error),
                Ok(IngressCreateResolution::OwnedByOther) => Err(ExposeCreateError {
                    message: error.message,
                    ambiguous: false,
                    rolled_back: error.rolled_back,
                }),
                Err(verify_error) => {
                    warn!(
                        "Failed to verify ambiguous ingress create for config \
                         {config_id_str}: {verify_error}"
                    );
                    Err(error)
                }
            }
        }
    }
}

/// Resolves an unanswered ingress create by checking who, if anyone, owns
/// the object it would have named.
///
/// Adopted when the object exists and carries this installation's labels,
/// so the caller can treat the create as succeeded and track it for
/// rollback. OwnedByOther, and the history record cleared, only when the
/// object exists and belongs to someone else: that is proof this attempt's
/// create did not land, a definitive outcome the caller can use to settle
/// cleanup instead of leaving it ambiguous. StillAmbiguous when the object
/// is missing — eventual consistency and a stale read both look identical
/// from here — so the history stays and a later check gets another chance
/// to resolve it.
enum IngressCreateResolution {
    Adopted(CreatedResource),
    OwnedByOther,
    StillAmbiguous,
}

async fn resolve_ambiguous_ingress_create(
    ingresses: &Api<Ingress>, ingress_name: &str, config_id: &str, location: &ExposeLocation,
    mode: DatabaseMode,
) -> Result<IngressCreateResolution, String> {
    let found = ingresses
        .get_opt(ingress_name)
        .await
        .map_err(|error| error.to_string())?;
    let Some(ingress) = found else {
        return Ok(IngressCreateResolution::StillAmbiguous);
    };
    let owner_identity = kftray_commons::utils::config_dir::owner_identity(mode).await?;
    let owned = ingress.metadata.labels.as_ref().is_some_and(|labels| {
        labels.get("config_id").map(String::as_str) == Some(config_id)
            && labels
                .get(crate::kube::proxy::INSTALLATION_LABEL)
                .map(String::as_str)
                == Some(owner_identity.as_str())
    });
    if !owned {
        forget_ingress_history(config_id, location, mode).await;
        return Ok(IngressCreateResolution::OwnedByOther);
    }
    Ok(IngressCreateResolution::Adopted(CreatedResource {
        kind: ResourceKind::Ingress,
        name: ingress.metadata.name.clone().unwrap_or_default(),
        uid: ingress.metadata.uid,
    }))
}

/// Key under which a configuration's ingress history is kept.
///
/// Scoped to the destination as well as the id: one configuration can owe
/// cleanup in more than one cluster or namespace at a time, and clearing the
/// history after verifying one of them would discard the evidence for the rest.
/// The destination is the resolved API server, not the context name or the
/// kubeconfig path: `@current` moves, and a kubeconfig can change its server
/// without changing either, so those would let cleanup in one cluster erase
/// what was recorded for another.
fn ingress_history_key(config_id: &str, location: &ExposeLocation) -> String {
    let scope = crate::kube::stop::stable_digest(&[
        Some(location.cluster.as_str()),
        Some(location.namespace.as_str()),
    ]);

    format!("expose_ingress_created:{config_id}:{scope:016x}")
}

/// Where an exposure's resources live, as resolved through the connection
/// that reaches them.
#[derive(Clone)]
pub struct ExposeLocation {
    pub cluster: String,
    pub namespace: String,
}

impl ExposeLocation {
    /// Keys history by the canonical destination, not the raw `Uri`: create,
    /// rollback and cleanup must agree on the same string even when the
    /// server has a default port or an IPv6 host, which `Uri::to_string()`
    /// renders differently than `cluster_identity`.
    pub fn resolve(cluster_url: &http::Uri, namespace: &str) -> Self {
        Self {
            cluster: crate::kube::client::cluster_identity(cluster_url),
            namespace: namespace.to_owned(),
        }
    }
}

/// Records that this configuration created an ingress.
///
/// Reported rather than logged: an ingress whose history could not be written
/// is one that a later cleanup cannot know about, and creating it anyway would
/// leave it unverifiable.
async fn remember_ingress_created(
    config_id: &str, location: &ExposeLocation, mode: DatabaseMode,
) -> Result<(), String> {
    kftray_commons::utils::settings::set_setting_with_mode(
        &ingress_history_key(config_id, location),
        "1",
        mode,
    )
    .await
    .map_err(|error| {
        format!("Failed to record the ingress history for config {config_id}: {error}")
    })
}

/// Whether this configuration ever created an ingress, or whether that cannot
/// be ruled out.
///
/// A configuration switched from public to private keeps the ingress it
/// created, so its current type is not evidence that none exists. A history
/// that cannot be read is not evidence either, so it counts as possible.
pub async fn ingress_was_created(
    config_id: &str, location: &ExposeLocation, mode: DatabaseMode,
) -> bool {
    match kftray_commons::utils::settings::get_setting_with_mode(
        &ingress_history_key(config_id, location),
        mode,
    )
    .await
    {
        Ok(value) => value.is_some(),
        Err(error) => {
            log::warn!("Could not read the ingress history for config {config_id}: {error}");
            true
        }
    }
}

/// Forgets the ingress history once cleanup has confirmed none is left.
async fn forget_ingress_history(config_id: &str, location: &ExposeLocation, mode: DatabaseMode) {
    if let Err(error) = kftray_commons::utils::settings::delete_setting_with_mode(
        &ingress_history_key(config_id, location),
        mode,
    )
    .await
    {
        log::debug!("Failed to clear the ingress history for config {config_id}: {error}");
    }
}

/// Key under which a configuration is marked as possibly having exposed
/// itself publicly before ingress history was recorded.
fn legacy_exposure_key(config_id: &str, mode: DatabaseMode) -> String {
    kftray_commons::utils::settings::expose_legacy_key(config_id, mode)
}

fn mode_scope(mode: DatabaseMode) -> &'static str {
    kftray_commons::utils::settings::mode_scope(mode)
}

/// The baseline is taken when a database is initialised, before anything can
/// be inserted into it. This is the fallback for a database opened by a path
/// that skipped that, and it is a no-op everywhere else.
async fn ensure_expose_history_baseline(mode: DatabaseMode) -> Result<(), String> {
    let context = kftray_commons::utils::db_mode::DatabaseManager::get_context(mode).await?;
    kftray_commons::utils::settings::establish_expose_history_baseline(&context.pool, mode)
        .await
        .map_err(|error| error.to_string())
}

/// Key under which a destination is recorded as verified free of any ingress
/// a pre-history exposure could have left.
fn legacy_verified_key(config_id: &str, location: &ExposeLocation, mode: DatabaseMode) -> String {
    let scope = crate::kube::stop::stable_digest(&[
        Some(location.cluster.as_str()),
        Some(location.namespace.as_str()),
    ]);
    format!(
        "expose_legacy_verified:{}:{config_id}:{scope:016x}",
        mode_scope(mode)
    )
}

/// Whether an exposure from before ingress history may still have an ingress
/// nobody recorded at this destination. Unreadable state counts as possible.
///
/// The doubt is about the configuration, the verification is about one
/// destination: an exposure moved to another namespace clears nothing about
/// the one it left, so each is verified on its own.
async fn legacy_exposure_possible(
    config_id: &str, location: &ExposeLocation, mode: DatabaseMode,
) -> bool {
    use kftray_commons::utils::settings::get_setting_with_mode;

    let marked = match get_setting_with_mode(&legacy_exposure_key(config_id, mode), mode).await {
        Ok(value) => value.is_some(),
        Err(error) => {
            log::warn!("Could not read the exposure baseline for config {config_id}: {error}");
            return true;
        }
    };
    if !marked {
        return false;
    }
    match get_setting_with_mode(&legacy_verified_key(config_id, location, mode), mode).await {
        Ok(value) => value.is_none(),
        Err(error) => {
            log::warn!("Could not read the exposure verification for config {config_id}: {error}");
            true
        }
    }
}

/// A listing verified that no ingress is left at this destination, so the
/// pre-history doubt is settled there for good.
async fn record_legacy_verified(config_id: &str, location: &ExposeLocation, mode: DatabaseMode) {
    if let Err(error) = kftray_commons::utils::settings::set_setting_with_mode(
        &legacy_verified_key(config_id, location, mode),
        "1",
        mode,
    )
    .await
    {
        log::debug!("Failed to record the exposure verification for config {config_id}: {error}");
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
    location: &ExposeLocation, mode: DatabaseMode,
) -> Result<(), String> {
    // A configuration switched from public to private still owns the ingress it
    // created, and its role may not allow listing ingresses. Inferring absence
    // from the new type would leave that ingress serving the new tunnel
    // publicly, so history decides here, not the current configuration.
    // An exposure from before history was kept is treated the same way: for it
    // a missing record proves nothing, so a refusal to list ingresses cannot
    // be read as absence. That fails a private start on a role that cannot
    // list ingresses, and says so, rather than reconnecting a public ingress
    // that a partial cleanup left behind to the new tunnel.
    ensure_expose_history_baseline(mode).await?;
    let ingress_possible = ingress_possible
        || ingress_was_created(config_id_label, location, mode).await
        || legacy_exposure_possible(config_id_label, location, mode).await;
    let lp = ListParams::default().labels(&expose_owner_selector(config_id_label, mode).await?);

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
        // A private exposure never creates an ingress, its role may not allow
        // listing them, and a cluster may not serve the API at all, so neither
        // refusal is evidence of leftovers.
        Err(kube::Error::Api(response))
            if matches!(response.code, 403 | 404) && !ingress_possible => {}
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
    // served its purpose, and so has the doubt about what came before it.
    forget_ingress_history(config_id_label, location, mode).await;
    record_legacy_verified(config_id_label, location, mode).await;
    info!(
        "Successfully deleted expose resources for config_id label '{}'",
        config_id_label
    );
    Ok(())
}

/// Deletes only the object that was listed.
///
/// Names come from aliases and are reused, so another installation can replace
/// a listed object between the list and the delete. The UID precondition makes
/// the request fail rather than remove the replacement.
fn owned_delete_params(uid: Option<String>) -> DeleteParams {
    DeleteParams {
        preconditions: uid.map(|uid| kube::api::Preconditions {
            uid: Some(uid),
            resource_version: None,
        }),
        ..DeleteParams::default()
    }
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
        Err(kube::Error::Api(response)) if response.code == 403 => {
            return Err(format!(
                "Cannot verify that no public ingress is left for this configuration: listing \
                 ingresses is not allowed ({}). Grant list access on ingresses once, or remove \
                 any ingress this configuration created from the server resources screen",
                response.message
            ));
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
            match api
                .delete(name, &owned_delete_params(ingress.metadata.uid.clone()))
                .await
            {
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
            match api
                .delete(name, &owned_delete_params(service.metadata.uid.clone()))
                .await
            {
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
                ..owned_delete_params(deployment.metadata.uid.clone())
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
    async fn an_out_of_range_websocket_port_fails_the_deployment_create() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        let config_dir = tempfile::tempdir().unwrap();
        let original_config_dir = std::env::var("KFTRAY_CONFIG").ok();
        unsafe { std::env::set_var("KFTRAY_CONFIG", config_dir.path()) };

        // A customized template whose WEBSOCKET_PORT does not fit in a u16:
        // `container_env_port` still parses it as an i32, so only the u16
        // conversion before probe injection can catch it.
        let manifest_path =
            kftray_commons::utils::config_dir::get_expose_deployment_manifest_path().unwrap();
        let manifest = r#"{
  "apiVersion": "apps/v1",
  "kind": "Deployment",
  "metadata": {
    "name": "{deployment_name}",
    "namespace": "{namespace}",
    "labels": {"app": "kftray-expose", "config_id": "{config_id}"}
  },
  "spec": {
    "replicas": 1,
    "selector": {"matchLabels": {"app": "kftray-expose", "config_id": "{config_id}"}},
    "template": {
      "metadata": {"labels": {"app": "kftray-expose", "config_id": "{config_id}"}},
      "spec": {
        "terminationGracePeriodSeconds": 10,
        "containers": [{
          "name": "kftray-server",
          "image": "ghcr.io/hcavarsan/kftray-server:latest",
          "env": [
            {"name": "PROXY_TYPE", "value": "reverse_http"},
            {"name": "HTTP_PORT", "value": "8080"},
            {"name": "WEBSOCKET_PORT", "value": "70000"},
            {"name": "REMOTE_ADDRESS", "value": "localhost"},
            {"name": "REMOTE_PORT", "value": "{local_port}"},
            {"name": "LOCAL_PORT", "value": "{local_port}"},
            {"name": "RUST_LOG", "value": "DEBUG"}
          ],
          "ports": [
            {"containerPort": 8080, "name": "http"},
            {"containerPort": 9999, "name": "websocket"}
          ]
        }]
      }
    }
  }
}"#;
        std::fs::write(&manifest_path, manifest).unwrap();

        let config = Config {
            id: Some(4001),
            namespace: "default".to_owned(),
            local_port: Some(8080),
            ..Config::default()
        };

        // Never contacted: the invalid port must be rejected before any
        // request is sent.
        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");

        let result =
            create_deployment(&client, "default", "myapp-deploy", "4001", &config, mode).await;

        if let Some(original) = original_config_dir {
            unsafe { std::env::set_var("KFTRAY_CONFIG", original) };
        } else {
            unsafe { std::env::remove_var("KFTRAY_CONFIG") };
        }

        let error = result.expect_err(
            "a WEBSOCKET_PORT outside u16 range must fail the create before any request is sent",
        );
        assert!(
            error.message.contains("WEBSOCKET_PORT"),
            "the error must name the offending variable: {}",
            error.message
        );
        assert!(
            !error.ambiguous,
            "a template validation failure happens before any request, so it is a definitive \
             rejection"
        );
    }

    #[tokio::test]
    async fn an_unresolved_websocket_port_fails_the_deployment_create() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        let config_dir = tempfile::tempdir().unwrap();
        let original_config_dir = std::env::var("KFTRAY_CONFIG").ok();
        unsafe { std::env::set_var("KFTRAY_CONFIG", config_dir.path()) };

        // WEBSOCKET_PORT sourced from a ConfigMap: `container_env_port`
        // cannot resolve it, and guessing 9999 would target the wrong pod
        // port once the relay is up.
        let manifest_path =
            kftray_commons::utils::config_dir::get_expose_deployment_manifest_path().unwrap();
        let manifest = r#"{
  "apiVersion": "apps/v1",
  "kind": "Deployment",
  "metadata": {
    "name": "{deployment_name}",
    "namespace": "{namespace}",
    "labels": {"app": "kftray-expose", "config_id": "{config_id}"}
  },
  "spec": {
    "replicas": 1,
    "selector": {"matchLabels": {"app": "kftray-expose", "config_id": "{config_id}"}},
    "template": {
      "metadata": {"labels": {"app": "kftray-expose", "config_id": "{config_id}"}},
      "spec": {
        "terminationGracePeriodSeconds": 10,
        "containers": [{
          "name": "kftray-server",
          "image": "ghcr.io/hcavarsan/kftray-server:latest",
          "env": [
            {"name": "PROXY_TYPE", "value": "reverse_http"},
            {"name": "HTTP_PORT", "value": "8080"},
            {"name": "WEBSOCKET_PORT", "valueFrom": {"configMapKeyRef": {"name": "ports", "key": "ws"}}},
            {"name": "REMOTE_ADDRESS", "value": "localhost"},
            {"name": "REMOTE_PORT", "value": "{local_port}"},
            {"name": "LOCAL_PORT", "value": "{local_port}"},
            {"name": "RUST_LOG", "value": "DEBUG"}
          ],
          "ports": [
            {"containerPort": 8080, "name": "http"},
            {"containerPort": 9999, "name": "websocket"}
          ]
        }]
      }
    }
  }
}"#;
        std::fs::write(&manifest_path, manifest).unwrap();

        let config = Config {
            id: Some(4002),
            namespace: "default".to_owned(),
            local_port: Some(8080),
            ..Config::default()
        };

        // Never contacted: an unresolved port must be rejected before any
        // request is sent.
        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");

        let result =
            create_deployment(&client, "default", "myapp-deploy", "4002", &config, mode).await;

        if let Some(original) = original_config_dir {
            unsafe { std::env::set_var("KFTRAY_CONFIG", original) };
        } else {
            unsafe { std::env::remove_var("KFTRAY_CONFIG") };
        }

        let error = result.expect_err(
            "a WEBSOCKET_PORT this cannot resolve must fail the create instead of keeping 9999 \
             as the tunnel target",
        );
        assert!(
            error.message.contains("WEBSOCKET_PORT"),
            "the error must name the offending variable: {}",
            error.message
        );
        assert!(
            !error.ambiguous,
            "a template validation failure happens before any request, so it is a definitive \
             rejection"
        );
    }

    #[tokio::test]
    async fn a_zero_websocket_port_fails_the_deployment_create() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        let config_dir = tempfile::tempdir().unwrap();
        let original_config_dir = std::env::var("KFTRAY_CONFIG").ok();
        unsafe { std::env::set_var("KFTRAY_CONFIG", config_dir.path()) };

        let manifest_path =
            kftray_commons::utils::config_dir::get_expose_deployment_manifest_path().unwrap();
        let manifest = r#"{
  "apiVersion": "apps/v1",
  "kind": "Deployment",
  "metadata": {
    "name": "{deployment_name}",
    "namespace": "{namespace}",
    "labels": {"app": "kftray-expose", "config_id": "{config_id}"}
  },
  "spec": {
    "replicas": 1,
    "selector": {"matchLabels": {"app": "kftray-expose", "config_id": "{config_id}"}},
    "template": {
      "metadata": {"labels": {"app": "kftray-expose", "config_id": "{config_id}"}},
      "spec": {
        "terminationGracePeriodSeconds": 10,
        "containers": [{
          "name": "kftray-server",
          "image": "ghcr.io/hcavarsan/kftray-server:latest",
          "env": [
            {"name": "PROXY_TYPE", "value": "reverse_http"},
            {"name": "HTTP_PORT", "value": "8080"},
            {"name": "WEBSOCKET_PORT", "value": "0"},
            {"name": "REMOTE_ADDRESS", "value": "localhost"},
            {"name": "REMOTE_PORT", "value": "{local_port}"},
            {"name": "LOCAL_PORT", "value": "{local_port}"},
            {"name": "RUST_LOG", "value": "DEBUG"}
          ],
          "ports": [
            {"containerPort": 8080, "name": "http"},
            {"containerPort": 9999, "name": "websocket"}
          ]
        }]
      }
    }
  }
}"#;
        std::fs::write(&manifest_path, manifest).unwrap();

        let config = Config {
            id: Some(4003),
            namespace: "default".to_owned(),
            local_port: Some(8080),
            ..Config::default()
        };

        // Never contacted: port 0 must be rejected before any request is
        // sent.
        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");

        let result =
            create_deployment(&client, "default", "myapp-deploy", "4003", &config, mode).await;

        if let Some(original) = original_config_dir {
            unsafe { std::env::set_var("KFTRAY_CONFIG", original) };
        } else {
            unsafe { std::env::remove_var("KFTRAY_CONFIG") };
        }

        let error =
            result.expect_err("WEBSOCKET_PORT 0 is not a usable port and must fail the create");
        assert!(
            error.message.contains("WEBSOCKET_PORT"),
            "the error must name the offending variable: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn no_identifiable_relay_container_fails_the_deployment_create() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        let config_dir = tempfile::tempdir().unwrap();
        let original_config_dir = std::env::var("KFTRAY_CONFIG").ok();
        unsafe { std::env::set_var("KFTRAY_CONFIG", config_dir.path()) };

        // Neither named `kftray-server` nor carrying a literal
        // PROXY_TYPE=reverse_http entry: nothing identifies this container
        // as the relay.
        let manifest_path =
            kftray_commons::utils::config_dir::get_expose_deployment_manifest_path().unwrap();
        let manifest = r#"{
  "apiVersion": "apps/v1",
  "kind": "Deployment",
  "metadata": {
    "name": "{deployment_name}",
    "namespace": "{namespace}",
    "labels": {"app": "kftray-expose", "config_id": "{config_id}"}
  },
  "spec": {
    "replicas": 1,
    "selector": {"matchLabels": {"app": "kftray-expose", "config_id": "{config_id}"}},
    "template": {
      "metadata": {"labels": {"app": "kftray-expose", "config_id": "{config_id}"}},
      "spec": {
        "terminationGracePeriodSeconds": 10,
        "containers": [{
          "name": "custom-relay",
          "image": "ghcr.io/hcavarsan/kftray-server:latest",
          "env": [
            {"name": "REMOTE_ADDRESS", "value": "localhost"},
            {"name": "REMOTE_PORT", "value": "{local_port}"},
            {"name": "LOCAL_PORT", "value": "{local_port}"}
          ]
        }]
      }
    }
  }
}"#;
        std::fs::write(&manifest_path, manifest).unwrap();

        let config = Config {
            id: Some(4004),
            namespace: "default".to_owned(),
            local_port: Some(8080),
            ..Config::default()
        };

        // Never contacted: a create with no identifiable relay must be
        // rejected before any request is sent.
        let (mock_service, _handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");

        let result =
            create_deployment(&client, "default", "myapp-deploy", "4004", &config, mode).await;

        if let Some(original) = original_config_dir {
            unsafe { std::env::set_var("KFTRAY_CONFIG", original) };
        } else {
            unsafe { std::env::remove_var("KFTRAY_CONFIG") };
        }

        result.expect_err("a deployment with no identifiable relay container must fail the create");
    }

    #[test]
    fn expose_location_stores_the_canonical_cluster_identity() {
        // `Uri::to_string()` keeps an explicit default port and drops IPv6
        // brackets; history, rollback and cleanup all key off `location`, so
        // two connections to the same server must resolve to the same
        // string even when one Uri carries a default port and the other
        // doesn't, and an IPv6 host must stay valid (unbracketed, it reads
        // as `host:port:port` and no longer parses as a `Uri` at all).
        let with_default_port: http::Uri = "https://host:443/".parse().unwrap();
        let without_port: http::Uri = "https://host".parse().unwrap();
        assert_eq!(
            ExposeLocation::resolve(&with_default_port, "default").cluster,
            ExposeLocation::resolve(&without_port, "default").cluster,
        );
        assert_eq!(
            ExposeLocation::resolve(&with_default_port, "default").cluster,
            "https://host"
        );

        let ipv6: http::Uri = "https://[2001:db8::1]:6443/".parse().unwrap();
        assert_eq!(
            ExposeLocation::resolve(&ipv6, "default").cluster,
            "https://[2001:db8::1]:6443"
        );
    }

    #[test]
    fn pod_readiness_falls_back_to_the_relay_container_without_a_probe() {
        // No readiness probe was injected (an unresolved `HTTP_PORT`, e.g.
        // `envFrom`), so the pod's aggregate `Ready` condition would also
        // wait on unrelated containers. The old check, which only looked at
        // that aggregate condition, would have read the same pod (no
        // `conditions` at all) as never ready and stalled until the 120s
        // timeout.
        let relay = RelayProbe {
            container_name: Some("kftray-server".to_owned()),
            readiness_probe_present: false,
            websocket_port: 9999,
        };
        let ready: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "relay"},
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "name": "kftray-server", "ready": true, "started": false,
                    "restartCount": 0, "image": "relay", "imageID": "relay"
                }]
            }
        }))
        .unwrap();
        assert!(
            relay_pod_ready(&ready, &relay),
            "no readiness probe means Kubernetes marks the container ready as soon as it runs"
        );

        let not_ready: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "relay"},
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "name": "kftray-server", "ready": false, "started": false,
                    "restartCount": 0, "image": "relay", "imageID": "relay"
                }]
            }
        }))
        .unwrap();
        assert!(!relay_pod_ready(&not_ready, &relay));
    }

    #[test]
    fn pod_readiness_requires_the_aggregate_condition_when_a_probe_is_present() {
        let relay = RelayProbe {
            container_name: Some("kftray-server".to_owned()),
            readiness_probe_present: true,
            websocket_port: 9999,
        };
        // The relay container itself is ready, but a sidecar in the same pod
        // is not: with a readiness probe actually injected, the aggregate
        // condition must still gate readiness, unchanged from before this
        // fallback existed.
        let sidecar_not_ready: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "relay"},
            "status": {
                "phase": "Running",
                "conditions": [{"type": "Ready", "status": "False"}],
                "containerStatuses": [
                    {"name": "kftray-server", "ready": true, "started": true,
                     "restartCount": 0, "image": "relay", "imageID": "relay"},
                    {"name": "sidecar", "ready": false, "started": true,
                     "restartCount": 0, "image": "sidecar", "imageID": "sidecar"}
                ]
            }
        }))
        .unwrap();
        assert!(!relay_pod_ready(&sidecar_not_ready, &relay));

        let all_ready: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "relay"},
            "status": {
                "phase": "Running",
                "conditions": [{"type": "Ready", "status": "True"}],
                "containerStatuses": [
                    {"name": "kftray-server", "ready": true, "started": true,
                     "restartCount": 0, "image": "relay", "imageID": "relay"}
                ]
            }
        }))
        .unwrap();
        assert!(relay_pod_ready(&all_ready, &relay));
    }

    #[tokio::test]
    async fn cleanup_attempts_all_resources_and_ignores_not_found() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
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
            let location =
                ExposeLocation::resolve(&"http://127.0.0.1:1".parse().unwrap(), "default");
            let result = delete_expose_resources(
                client,
                "default",
                "42",
                true,
                &location,
                DatabaseMode::Memory,
            )
            .await;
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

    /// Answers every list with an empty page, except ingresses, which are
    /// refused, until `stop` closes the channel.
    fn deny_ingress_listing(
        mut handle: mock::Handle<Request<Body>, Response<Body>>,
    ) -> tokio_util::task::AbortOnDropHandle<()> {
        tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            while let Some((request, send)) = handle.next_request().await {
                assert_eq!(request.method(), Method::GET);
                let (status, body) = if request.uri().path().contains("/ingresses") {
                    (
                        403,
                        serde_json::json!({
                            "apiVersion":"v1","kind":"Status","status":"Failure",
                            "reason":"Forbidden","message":"ingresses is forbidden","code":403
                        }),
                    )
                } else {
                    (200, serde_json::json!({"items":[]}))
                };
                send.send_response(
                    Response::builder()
                        .status(status)
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                );
            }
        }))
    }

    #[tokio::test]
    async fn a_pre_history_exposure_cannot_start_privately_without_verifying_its_ingress() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        // An exposure that existed before ingress history was recorded: it
        // could have left a public ingress that a partial cleanup missed.
        let legacy = kftray_commons::utils::config::insert_config_with_mode(
            Config {
                workload_type: Some("expose".to_owned()),
                exposure_type: Some("private".to_owned()),
                alias: Some("myapp".to_owned()),
                namespace: "default".to_owned(),
                ..Config::default()
            },
            mode,
        )
        .await
        .unwrap();
        // The test database took its baseline when it was created, so this row
        // is marked the way an upgrade marks a pre-existing exposure.
        kftray_commons::utils::settings::set_setting_with_mode(
            &legacy_exposure_key(&legacy.to_string(), mode),
            "1",
            mode,
        )
        .await
        .unwrap();

        let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let _server = deny_ingress_listing(handle);
        let location = ExposeLocation::resolve(&"http://127.0.0.1:1".parse().unwrap(), "default");
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            delete_expose_resources(
                client.clone(),
                "default",
                &legacy.to_string(),
                false,
                &location,
                mode,
            ),
        )
        .await
        .unwrap()
        .expect_err("a refusal to list ingresses cannot be read as absence for this exposure");
        assert!(error.contains("Cannot verify"), "{error}");

        // Verified at one destination, the doubt is settled there and nowhere
        // else: the same refusal passes in that namespace and still fails in
        // another the exposure may have used before it moved.
        let (allowing, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let allowing = kube::Client::new(allowing, "default");
        let _permissive = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let mut handle = handle;
            while let Some((_, send)) = handle.next_request().await {
                send.send_response(
                    Response::builder()
                        .status(200)
                        .body(Body::from(
                            serde_json::to_vec(&serde_json::json!({"items":[]})).unwrap(),
                        ))
                        .unwrap(),
                );
            }
        }));
        let verified_ns =
            ExposeLocation::resolve(&"http://127.0.0.1:1".parse().unwrap(), "verified");
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            delete_expose_resources(
                allowing,
                "verified",
                &legacy.to_string(),
                false,
                &verified_ns,
                mode,
            ),
        )
        .await
        .unwrap()
        .expect("a successful listing verifies this destination");
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            delete_expose_resources(
                client.clone(),
                "verified",
                &legacy.to_string(),
                false,
                &verified_ns,
                mode,
            ),
        )
        .await
        .unwrap()
        .expect("a verified destination tolerates the refusal from then on");
        let other_ns = ExposeLocation::resolve(&"http://127.0.0.1:1".parse().unwrap(), "other");
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            delete_expose_resources(
                client.clone(),
                "other",
                &legacy.to_string(),
                false,
                &other_ns,
                mode,
            ),
        )
        .await
        .unwrap()
        .expect_err("verification in one namespace says nothing about another");

        // A configuration created after the baseline has a record for every
        // ingress it ever made, so the same refusal is fine for it.
        let fresh = kftray_commons::utils::config::insert_config_with_mode(
            Config {
                workload_type: Some("expose".to_owned()),
                exposure_type: Some("private".to_owned()),
                alias: Some("other".to_owned()),
                namespace: "default".to_owned(),
                ..Config::default()
            },
            mode,
        )
        .await
        .unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            delete_expose_resources(
                client,
                "default",
                &fresh.to_string(),
                false,
                &location,
                mode,
            ),
        )
        .await
        .unwrap()
        .expect("a private exposure with a complete history tolerates the refusal");

        for id in [legacy, fresh] {
            let _ = kftray_commons::utils::config::delete_config_with_mode(id, mode).await;
        }
    }
    #[tokio::test]
    async fn a_confirmed_ingress_rollback_clears_its_history() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        let config_id = "2002";
        let location = ExposeLocation::resolve(&"http://127.0.0.1:1".parse().unwrap(), "default");
        remember_ingress_created(config_id, &location, mode)
            .await
            .unwrap();
        assert!(ingress_was_created(config_id, &location, mode).await);

        let created = vec![CreatedResource {
            kind: ResourceKind::Ingress,
            name: "myapp".to_owned(),
            uid: Some("abc-uid".to_owned()),
        }];

        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), Method::DELETE);
            let status = serde_json::json!({
                "apiVersion":"v1","kind":"Status","status":"Success","code":200
            });
            send.send_response(
                Response::builder()
                    .status(200)
                    .body(Body::from(serde_json::to_vec(&status).unwrap()))
                    .unwrap(),
            );
            // `still_present` confirms the delete: gone means the ingress
            // history's obligation is actually settled, not just requested.
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), Method::GET);
            let not_found = serde_json::json!({
                "apiVersion":"v1","kind":"Status","status":"Failure",
                "reason":"NotFound","message":"myapp","code":404
            });
            send.send_response(
                Response::builder()
                    .status(404)
                    .body(Body::from(serde_json::to_vec(&not_found).unwrap()))
                    .unwrap(),
            );
        }));

        let (cleaned, _message) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            rollback_created_resources(
                &client,
                "default",
                &created,
                config_id,
                &location,
                mode,
                "reason".to_owned(),
            ),
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert!(cleaned, "the mocked delete and confirmation both succeeded");
        assert!(
            !ingress_was_created(config_id, &location, mode).await,
            "a confirmed UID-scoped delete proves no ingress is left, so a later private start \
             must not need ingress-list rights just to rule this exposure out"
        );
    }

    #[tokio::test]
    async fn a_409_on_ingress_create_keeps_its_history() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        let config_dir = tempfile::tempdir().unwrap();
        let original_config_dir = std::env::var("KFTRAY_CONFIG").ok();
        unsafe { std::env::set_var("KFTRAY_CONFIG", config_dir.path()) };
        kftray_commons::utils::manifests::create_expose_ingress_manifest().unwrap();

        let config_id = "3003";
        let location = ExposeLocation::resolve(&"http://127.0.0.1:1".parse().unwrap(), "default");
        let config = Config {
            id: Some(3003),
            namespace: "default".to_owned(),
            alias: Some("myapp.example.com".to_owned()),
            local_port: Some(8080),
            ..Config::default()
        };

        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), Method::POST);
            let conflict = serde_json::json!({
                "apiVersion":"v1","kind":"Status","status":"Failure",
                "reason":"AlreadyExists","message":"ingresses.networking.k8s.io \"myapp\" \
                 already exists","code":409
            });
            send.send_response(
                Response::builder()
                    .status(409)
                    .body(Body::from(serde_json::to_vec(&conflict).unwrap()))
                    .unwrap(),
            );
        }));

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            create_ingress(
                &client,
                "default",
                "myapp",
                "myapp-svc",
                &config,
                &location,
                mode,
            ),
        )
        .await
        .unwrap();
        server.await.unwrap();

        if let Some(original) = original_config_dir {
            unsafe { std::env::set_var("KFTRAY_CONFIG", original) };
        } else {
            unsafe { std::env::remove_var("KFTRAY_CONFIG") };
        }

        let error = result.expect_err("a 409 must not be reported as a created ingress");
        assert!(
            error.ambiguous,
            "an AlreadyExists response can be this attempt's own request landing late; it must \
             not be read as a definitive rejection"
        );
        assert!(
            ingress_was_created(config_id, &location, mode).await,
            "a 409 must not wipe the ingress history: the object it names may be the one this \
             attempt itself is responsible for"
        );
    }

    #[tokio::test]
    async fn an_ambiguous_ingress_create_keeps_history_after_a_notfound() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        let config_dir = tempfile::tempdir().unwrap();
        let original_config_dir = std::env::var("KFTRAY_CONFIG").ok();
        unsafe { std::env::set_var("KFTRAY_CONFIG", config_dir.path()) };
        kftray_commons::utils::manifests::create_expose_ingress_manifest().unwrap();

        let config_id = "3004";
        let location = ExposeLocation::resolve(&"http://127.0.0.1:1".parse().unwrap(), "default");
        let config = Config {
            id: Some(3004),
            namespace: "default".to_owned(),
            alias: Some("myapp2.example.com".to_owned()),
            local_port: Some(8080),
            ..Config::default()
        };

        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), Method::POST);
            let unavailable = serde_json::json!({
                "apiVersion":"v1","kind":"Status","status":"Failure",
                "reason":"ServiceUnavailable","message":"etcd timeout","code":503
            });
            send.send_response(
                Response::builder()
                    .status(503)
                    .body(Body::from(serde_json::to_vec(&unavailable).unwrap()))
                    .unwrap(),
            );

            // The ambiguous create must be resolved against the object it
            // would have named, not just its own response.
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), Method::GET);
            let not_found = serde_json::json!({
                "apiVersion":"v1","kind":"Status","status":"Failure",
                "reason":"NotFound","message":"myapp2","code":404
            });
            send.send_response(
                Response::builder()
                    .status(404)
                    .body(Body::from(serde_json::to_vec(&not_found).unwrap()))
                    .unwrap(),
            );
        }));

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            create_ingress(
                &client,
                "default",
                "myapp2",
                "myapp2-svc",
                &config,
                &location,
                mode,
            ),
        )
        .await
        .unwrap();
        server.await.unwrap();

        if let Some(original) = original_config_dir {
            unsafe { std::env::set_var("KFTRAY_CONFIG", original) };
        } else {
            unsafe { std::env::remove_var("KFTRAY_CONFIG") };
        }

        let error =
            result.expect_err("nothing was adopted, so this must still be reported as a failure");
        assert!(
            error.ambiguous,
            "the create itself was never definitively rejected"
        );
        assert!(
            ingress_was_created(config_id, &location, mode).await,
            "a NotFound after an ambiguous create is not proof nothing was created (eventual \
             consistency, a stale read), so the history must stay for a later check to resolve"
        );
    }

    #[tokio::test]
    async fn an_ambiguous_ingress_create_owned_by_another_installation_is_definitive() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        let config_dir = tempfile::tempdir().unwrap();
        let original_config_dir = std::env::var("KFTRAY_CONFIG").ok();
        unsafe { std::env::set_var("KFTRAY_CONFIG", config_dir.path()) };
        kftray_commons::utils::manifests::create_expose_ingress_manifest().unwrap();

        let config_id = "3005";
        let location = ExposeLocation::resolve(&"http://127.0.0.1:1".parse().unwrap(), "default");
        let config = Config {
            id: Some(3005),
            namespace: "default".to_owned(),
            alias: Some("myapp3.example.com".to_owned()),
            local_port: Some(8080),
            ..Config::default()
        };

        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), Method::POST);
            let unavailable = serde_json::json!({
                "apiVersion":"v1","kind":"Status","status":"Failure",
                "reason":"ServiceUnavailable","message":"etcd timeout","code":503
            });
            send.send_response(
                Response::builder()
                    .status(503)
                    .body(Body::from(serde_json::to_vec(&unavailable).unwrap()))
                    .unwrap(),
            );

            // The ambiguous create is resolved against the object it would
            // have named: this one exists, but belongs to another
            // installation's config id.
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), Method::GET);
            let owned_by_other = serde_json::json!({
                "apiVersion":"networking.k8s.io/v1","kind":"Ingress",
                "metadata":{
                    "name":"myapp3",
                    "uid":"other-owner-uid",
                    "labels":{
                        "app":"kftray-expose",
                        "config_id":"9999",
                        "installation_id":"someone-else"
                    }
                }
            });
            send.send_response(
                Response::builder()
                    .status(200)
                    .body(Body::from(serde_json::to_vec(&owned_by_other).unwrap()))
                    .unwrap(),
            );
        }));

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            create_ingress(
                &client,
                "default",
                "myapp3",
                "myapp3-svc",
                &config,
                &location,
                mode,
            ),
        )
        .await
        .unwrap();
        server.await.unwrap();

        if let Some(original) = original_config_dir {
            unsafe { std::env::set_var("KFTRAY_CONFIG", original) };
        } else {
            unsafe { std::env::remove_var("KFTRAY_CONFIG") };
        }

        let error = result.expect_err("an object owned by another installation was not adopted");
        assert!(
            !error.ambiguous,
            "an object that exists and belongs to someone else is proof this attempt's create \
             did not land for it; the guard must be able to settle instead of staying uncertain \
             forever"
        );
        assert!(
            !ingress_was_created(config_id, &location, mode).await,
            "the history is cleared once ownership is definitively resolved against someone else"
        );
    }

    #[tokio::test]
    async fn a_cancelled_readiness_wait_rolls_back_the_created_deployment() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mode = DatabaseMode::Memory;
        let config_dir = tempfile::tempdir().unwrap();
        let original_config_dir = std::env::var("KFTRAY_CONFIG").ok();
        unsafe { std::env::set_var("KFTRAY_CONFIG", config_dir.path()) };
        kftray_commons::utils::manifests::create_expose_deployment_manifest().unwrap();

        let config = Config {
            id: Some(88_801),
            workload_type: Some("expose".to_owned()),
            exposure_type: Some("private".to_owned()),
            alias: Some("myapp".to_owned()),
            namespace: "default".to_owned(),
            local_port: Some(8080),
            ..Config::default()
        };

        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(mock_service, "default");
        let connection = crate::kube::client::KubeConnection {
            client: client.clone(),
            cluster_url: "http://127.0.0.1:1".parse().unwrap(),
        };
        let cancellation = CancellationToken::new();
        let server_cancellation = cancellation.clone();
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            // Pre-start cleanup lists deployments, services and ingresses
            // several times (to decide what to delete, to confirm they are
            // gone, and to check for unattributed leftovers); nothing exists
            // yet, so every list is empty and nothing is ever deleted.
            let (_request, send) = loop {
                let (request, send) = handle.next_request().await.unwrap();
                if request.method() == Method::POST {
                    break (request, send);
                }
                assert_eq!(request.method(), Method::GET);
                send.send_response(
                    Response::builder()
                        .status(200)
                        .body(Body::from(
                            serde_json::to_vec(&serde_json::json!({"items": []})).unwrap(),
                        ))
                        .unwrap(),
                );
            };

            // The deployment create itself is never cancelled, so it is
            // answered normally. Cancelling right before the response is
            // delivered, rather than racing the client task from outside,
            // guarantees the readiness wait that follows sees it: nothing
            // observes the token between the create returning and the wait
            // starting.
            let deployment = serde_json::json!({
                "apiVersion": "apps/v1", "kind": "Deployment",
                "metadata": {"name": "relay-1", "namespace": "default", "uid": "deploy-uid"}
            });
            server_cancellation.cancel();
            send.send_response(
                Response::builder()
                    .status(201)
                    .body(Body::from(serde_json::to_vec(&deployment).unwrap()))
                    .unwrap(),
            );

            // Rollback deletes exactly the deployment this attempt created.
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), Method::DELETE);
            assert!(
                request.uri().path().ends_with("/relay-1"),
                "{}",
                request.uri()
            );
            let status = serde_json::json!({
                "apiVersion": "v1", "kind": "Status", "status": "Success", "code": 200
            });
            send.send_response(
                Response::builder()
                    .status(200)
                    .body(Body::from(serde_json::to_vec(&status).unwrap()))
                    .unwrap(),
            );

            // `still_present` confirms the delete before rollback reports it
            // complete.
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), Method::GET);
            let not_found = serde_json::json!({
                "apiVersion": "v1", "kind": "Status", "status": "Failure",
                "reason": "NotFound", "message": "relay-1", "code": 404
            });
            send.send_response(
                Response::builder()
                    .status(404)
                    .body(Body::from(serde_json::to_vec(&not_found).unwrap()))
                    .unwrap(),
            );
        }));

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            create_expose_resources(&connection, &config, mode, Some(&cancellation)),
        )
        .await
        .unwrap();
        server.await.unwrap();

        match original_config_dir {
            Some(val) => unsafe { std::env::set_var("KFTRAY_CONFIG", val) },
            None => unsafe { std::env::remove_var("KFTRAY_CONFIG") },
        }

        let error = result.expect_err("a cancelled readiness wait must not report success");
        assert!(
            error.rolled_back,
            "the deployment created before cancellation must be rolled back: {}",
            error.message
        );
        assert!(error.message.contains("cancelled"), "{}", error.message);
    }
}
