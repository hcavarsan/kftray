use std::sync::Arc;

use anyhow::Result;
use dashmap::DashSet;
use futures::stream::{
    self,
    StreamExt,
};
use kftray_commons::{
    models::{
        config_model::Config,
        config_state_model::ConfigState,
        hostfile::HostEntry,
        response::CustomResponse,
    },
    utils::{
        config_state::update_config_state_with_mode,
        db_mode::DatabaseMode,
        settings::get_app_settings,
        timeout_manager::start_timeout_for_forward,
    },
};
use log::{
    debug,
    error,
    info,
    warn,
};
use once_cell::sync::Lazy;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

use crate::{
    hostsfile::{
        add_host_entry,
        add_ssl_host_entry,
    },
    kube::models::{
        Port,
        PortForward,
        Target,
        TargetSelector,
    },
    port_forward::CHILD_PROCESSES,
};

pub static STOPPED_BY_TIMEOUT: Lazy<DashSet<i64>> = Lazy::new(DashSet::new);

pub fn clear_stopped_by_timeout(config_id: i64) {
    STOPPED_BY_TIMEOUT.remove(&config_id);
}

pub fn is_stopped_by_timeout(config_id: i64) -> bool {
    STOPPED_BY_TIMEOUT.contains(&config_id)
}

pub async fn cleanup_stale_timeout_entries() {
    use kftray_commons::utils::config::get_configs;

    if let Ok(configs) = get_configs().await {
        let valid_ids: std::collections::HashSet<i64> =
            configs.iter().filter_map(|c| c.id).collect();

        STOPPED_BY_TIMEOUT.retain(|id| valid_ids.contains(id));
        debug!(
            "Cleaned up stale timeout entries, {} remaining",
            STOPPED_BY_TIMEOUT.len()
        );
    }
}

async fn handle_timeout_callback(id: i64, mode: DatabaseMode) {
    info!("User-configured timeout reached for config {id}, stopping port forward");

    STOPPED_BY_TIMEOUT.insert(id);

    if let Err(e) = crate::kube::stop::stop_port_forward_with_mode(id.to_string(), mode).await {
        error!("Failed to stop port forward {id} on timeout: {e}");
        STOPPED_BY_TIMEOUT.remove(&id);
    } else {
        info!("Port forward {id} stopped due to user-configured timeout");
    }
}

fn create_static_timeout_callback(mode: DatabaseMode) -> Arc<dyn Fn(i64) + Send + Sync> {
    Arc::new(move |id: i64| {
        tokio::spawn(async move {
            handle_timeout_callback(id, mode).await;
        });
    })
}

async fn build_tls_acceptor(
    _config: &Config, settings: &kftray_commons::models::settings_model::AppSettings,
) -> Result<tokio_rustls::TlsAcceptor> {
    crate::ssl::ensure_crypto_provider_installed();

    let cert_manager = crate::ssl::CertificateManager::new(settings)?;
    let cert_pair = cert_manager.load_global_certificate().await?;

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            cert_pair.certificate.clone(),
            cert_pair.private_key.clone_key(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create server config with certificate: {}", e))?;

    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(server_config)))
}

/// Writes the HTTPS aliases from a task that outlives the startup.
///
/// Off the runtime for the same reason as the domain alias: the write waits
/// on the cross-process hosts lock. The process is already registered when
/// this runs, so a stop can race the write; a task whose process is gone by
/// the time the lines are on disk removes them again, since the stop that
/// removed the process has already done its hosts cleanup.
async fn update_hosts_with_ssl(
    config: &Config, mode: DatabaseMode, hosts_claim: u64,
) -> Result<(), String> {
    let alias = config
        .alias
        .clone()
        .ok_or("Alias required for SSL hosts entry")?;
    let id = config.id.ok_or("Config id required for SSL hosts entry")?;
    let port = config.local_port.unwrap_or(8080);

    let (sender, receiver) = tokio::sync::oneshot::channel();
    let counted = AllocationInFlight::start();
    let snapshot = config.clone();
    tokio::spawn(async move {
        let _counted = counted;
        let written =
            tokio::task::spawn_blocking(move || add_ssl_host_entry(&id.to_string(), &alias, port))
                .await
                .map_err(|error| format!("Hosts write task failed: {error}"))
                .and_then(|result| {
                    result.map_err(|error| format!("Failed to add HTTPS hosts entries: {error}"))
                });
        let succeeded = written.is_ok();
        let _ = sender.send(written);
        if !succeeded {
            return;
        }
        // Whether the waiter is still there says nothing about ownership: the
        // process was registered before the write started and may well be
        // running. Only a process that is gone, checked under the lifecycle
        // lock so a stop or a restart cannot be halfway through, leaves these
        // lines orphaned. A retry that has since claimed this id but not yet
        // registered is caught by the claim, not by `CHILD_PROCESSES` alone:
        // registration only happens at the very end of its startup.
        let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
        let _guard = lock.lock().await;
        if !CHILD_PROCESSES.contains_key(&id)
            && crate::kube::stop::take_host_entry_claim_if_current(id, hosts_claim)
        {
            warn!("Removing HTTPS hosts entries for config {id} written after it was stopped");
            let in_use = crate::kube::stop::forwarding_configs(mode).await;
            let removed = tokio::task::spawn_blocking({
                let snapshot = snapshot.clone();
                move || crate::hostsfile::remove_config_host_entries(id, Some(&snapshot), &in_use)
            })
            .await;
            match removed {
                Ok(Ok(())) => {}
                // Recorded so a later stop retries: the stop that removed the
                // process has already run its cleanup and will not look again.
                Ok(Err(error)) => {
                    warn!("Failed to remove HTTPS hosts entries for config {id}: {error}");
                    crate::kube::stop::record_local_cleanup(id, snapshot);
                }
                Err(error) => {
                    warn!("Hosts cleanup task failed for config {id}: {error}");
                    crate::kube::stop::record_local_cleanup(id, snapshot);
                }
            }
        }
    });
    receiver
        .await
        .map_err(|_| "Hosts write ended unexpectedly".to_string())?
}

fn workload_type_description(workload_type: Option<&str>) -> &'static str {
    match workload_type {
        Some("pod") => "pod label",
        Some("proxy") => "proxy pod",
        _ => "service",
    }
}

static FALLBACK_ALLOCATION_MUTEX: Lazy<TokioMutex<()>> = Lazy::new(|| TokioMutex::new(()));

/// Releases what a startup registered outside the process before it had a
/// forwarder, and keeps the configuration recorded only if that did not
/// finish.
async fn rollback_local_resources(
    config: &Config, address: &str, reason: String, mode: DatabaseMode,
) -> String {
    let mut errors = Vec::new();
    let mut unsatisfiable = Vec::new();
    // Routed through the ownership-safe release: the helper hands the same
    // address to two configurations of one service, and removing it directly
    // would take the alias from under a forward that is still using it.
    if crate::network_utils::is_custom_loopback_address(address)
        && let Err(error) =
            crate::kube::stop::release_address_with_fallback(address, config.id, mode).await
    {
        if error.is_unsatisfiable() {
            unsatisfiable.push(error.to_string());
        } else {
            errors.push(error.to_string());
        }
    }
    let id = config.id.unwrap_or_default();
    let snapshot = config.clone();
    let in_use = crate::kube::stop::forwarding_configs(mode).await;
    let hosts = tokio::task::spawn_blocking(move || {
        crate::hostsfile::remove_config_host_entries(id, Some(&snapshot), &in_use)
    })
    .await;
    match hosts {
        Ok(Ok(())) => {}
        Ok(Err(error)) => errors.push(error.to_string()),
        Err(error) => errors.push(format!("Hosts cleanup task failed: {error}")),
    }

    if errors.is_empty() {
        if let Some(id) = config.id {
            crate::kube::stop::settle_local_cleanup(id, config);
        }
        // Releasing the address needed privileges that are not available
        // right now: reported once, but not by keeping this queued forever.
        if unsatisfiable.is_empty() {
            reason
        } else {
            format!("{reason}; {}", unsatisfiable.join("; "))
        }
    } else {
        if let Some(id) = config.id {
            crate::kube::stop::record_local_cleanup(id, config.clone());
        }
        let mut incomplete = unsatisfiable;
        incomplete.extend(errors);
        format!("{reason}; cleanup incomplete: {}", incomplete.join("; "))
    }
}

async fn rollback_startup(
    port_forward: &PortForward, config: Config, reason: String, mode: DatabaseMode,
) -> String {
    match port_forward.cleanup_resources(Some(&config), mode).await {
        Ok(()) => {
            if let Some(id) = config.id {
                crate::kube::stop::settle_local_cleanup(id, &config);
            }
            reason
        }
        Err(error) => {
            if let Some(id) = config.id {
                crate::kube::stop::record_local_cleanup(id, config);
            }
            format!("{reason}; cleanup incomplete: {error}")
        }
    }
}

/// Local setup tasks (address allocations, hosts writes) that have been
/// started but whose result has not been observed yet. A startup dropped
/// while one runs leaves the task to roll its own work back, and shutdown
/// reconciliation waits for that: nothing is in the cleanup registry until
/// the task records it.
pub(crate) static OUTSTANDING_ALLOCATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Decrements the outstanding count however the task ends.
struct AllocationInFlight;

impl AllocationInFlight {
    fn start() -> Self {
        OUTSTANDING_ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

impl Drop for AllocationInFlight {
    fn drop(&mut self) {
        OUTSTANDING_ALLOCATIONS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Writes a domain alias from a task that outlives the startup.
///
/// The write is synchronous and waits on the cross-process hosts lock, so it
/// runs off the runtime; and a startup abandoned while it waits (the
/// terminal's shutdown drain aborts forwarding tasks after its deadline)
/// would otherwise leave the line behind after every cleanup has run and
/// forgotten it. The task observes its own completion: for a startup that is
/// no longer waiting it rolls back everything that startup held locally, the
/// line it just wrote and the address allocated before it, since nothing
/// else will.
async fn add_host_entry_owned(
    id: i64, config: &Config, address: &str, entry: HostEntry, mode: DatabaseMode, hosts_claim: u64,
) -> Result<(), String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let counted = AllocationInFlight::start();
    let entry_id = id.to_string();
    let snapshot = config.clone();
    let address = address.to_owned();
    tokio::spawn(async move {
        let _counted = counted;
        let written = tokio::task::spawn_blocking({
            let entry_id = entry_id.clone();
            move || add_host_entry(entry_id, entry)
        })
        .await
        .map_err(|error| format!("Hosts write task failed: {error}"))
        .and_then(|result| result.map_err(|error| error.to_string()));
        if sender.send(written).is_err() {
            // Under the lifecycle lock: a retry of the same configuration can
            // have started once the abandoned future released it, claimed the
            // same address and written the same lines, and rolling those
            // back would tear the retry down. A retry in progress holds the
            // lock until it has registered its process, so once this task
            // has it, a registered process means the resources have an owner
            // and nothing here is abandoned any more. A retry that has since
            // claimed this id but not yet registered is caught by the claim,
            // not by `CHILD_PROCESSES` alone: registration only happens at
            // the very end of its startup.
            let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
            let _guard = lock.lock().await;
            if CHILD_PROCESSES.contains_key(&id) {
                debug!("Config {id} was restarted; leaving its local resources to the new owner");
                return;
            }
            if !crate::kube::stop::take_host_entry_claim_if_current(id, hosts_claim) {
                debug!(
                    "Config {id} was claimed by a newer attempt; leaving its local resources to \
                     the new owner"
                );
                return;
            }
            warn!(
                "{}",
                rollback_local_resources(
                    &snapshot,
                    &address,
                    format!("Startup for config {id} was abandoned during its hosts write"),
                    mode,
                )
                .await
            );
        }
    });
    receiver
        .await
        .map_err(|_| "Hosts write ended unexpectedly".to_string())?
}

/// Allocates a loopback address and returns it together with the claim that
/// keeps any release away from it until the startup registers its process.
///
/// The claim is taken by the allocating task itself, before the address is
/// handed over: between the allocation and the claim a stop of another
/// configuration sharing the same helper allocation would find neither a
/// claim nor a process and release the alias and its pool reservation.
async fn allocate_local_address_owned(
    config: &mut Config, mode: DatabaseMode,
) -> Result<(String, Option<crate::kube::stop::AddressClaim>), String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut owned = config.clone();
    // Counted before the task exists: shutdown reconciliation waits for these,
    // because an allocation still in flight has nothing in the cleanup registry
    // yet and would otherwise be abandoned.
    let counted = AllocationInFlight::start();
    tokio::spawn(async move {
        let _counted = counted;
        let Allocated {
            result,
            claim,
            acquired,
        } = allocate_and_claim(&mut owned, mode).await;
        // Recorded before the handoff, and whether or not the attempt went on
        // to succeed: a successful send does not prove the startup consumed
        // it, a failed claim or confirmation still leaves the alias bound, and
        // an address nobody recorded is an address stop-all and reconciliation
        // cannot find.
        let mut held = owned.clone();
        held.local_address = acquired.clone();
        if acquired.is_some()
            && let Some(id) = held.id
        {
            crate::kube::stop::record_local_cleanup(id, held.clone());
        }
        let failed = result.is_err();
        let handed_over = match sender.send((result, owned, claim)) {
            Ok(()) => true,
            Err((_, _, claim)) => {
                // Our own claim must not block our own release.
                drop(claim);
                false
            }
        };
        // An alias acquired by an attempt that then failed has no startup to
        // roll it back: the startup only ever sees the error. One handed to a
        // startup that is no longer waiting has nobody either.
        if let Some(address) = acquired.as_deref()
            && (failed || !handed_over)
        {
            // Released through the same path a stop uses: the helper keeps a
            // pool reservation next to the alias, and removing only the alias
            // would leave that reservation consumed by every abandoned startup.
            // The release marks the address under the entry lock and stands
            // down if any startup or running forward holds it by now, which is
            // why no owner is exempted: a retry of this same configuration is
            // not what this task is rolling back.
            warn!("Releasing address {address} allocated after startup was abandoned");
            match crate::kube::stop::release_address_with_fallback(address, None, mode).await {
                Ok(()) => {
                    if let Some(id) = held.id {
                        crate::kube::stop::settle_local_cleanup(id, &held);
                    }
                }
                Err(error) => {
                    // The record stays, so a later stop retries the release.
                    warn!("Failed to release {address} after an abandoned startup: {error}");
                }
            }
        }
    });

    let (result, owned, claim) = receiver
        .await
        .map_err(|_| "Address allocation ended unexpectedly".to_string())?;
    let address = result?;
    // Persisted only now that the result reached a startup that is still
    // current. The task keeps running when this future is abandoned, and
    // writing from there would overwrite settings edited in the meantime.
    if owned.auto_loopback_address
        && let Some(id) = owned.id
        && let Err(error) = persist_allocated_address(id, &address, mode).await
    {
        error!("Failed to save allocated address {address} for config {id}: {error}");
    }
    *config = owned;
    Ok((address, claim))
}

/// Writes only the allocated address, and only while the stored configuration
/// still asks for one.
async fn persist_allocated_address(
    id: i64, address: &str, mode: DatabaseMode,
) -> Result<(), String> {
    use kftray_commons::utils::config::AllocatedAddressWrite;
    match kftray_commons::utils::config::set_allocated_local_address(id, address, mode).await? {
        AllocatedAddressWrite::Written => {}
        AllocatedAddressWrite::NotRequested => {
            debug!("Config {id} no longer requests an allocated address; keeping its own");
        }
        AllocatedAddressWrite::RowMissing => {
            warn!("Config {id} was deleted before its allocated address {address} could be saved");
        }
    }
    Ok(())
}

/// What an allocation attempt left behind.
struct Allocated {
    result: Result<String, String>,
    claim: Option<crate::kube::stop::AddressClaim>,
    /// The custom loopback address this attempt acquired, whether or not it
    /// went on to succeed. A claim that could not be taken, a confirmation
    /// that failed, or a sequence that never settled all leave an alias behind
    /// that has to be tracked, and released, like any other.
    acquired: Option<String>,
}

/// Allocates an address and returns it held by a claim that no release can
/// get past.
///
/// The claim can only be taken once the address is known, and the helper
/// hands the same address to every configuration of one service, so between
/// the allocation and the claim a stop of a sibling configuration can complete
/// a release of exactly this address, alias and pool reservation included.
/// A check for a running release does not see one that already finished.
/// So the allocation is repeated under the claim: the helper either confirms
/// the reservation still stands, or makes a fresh one now that nothing can
/// release it, and if it hands back a different address the claim moves to
/// that one and the confirmation runs again.
async fn allocate_and_claim(owned: &mut Config, mode: DatabaseMode) -> Allocated {
    const ATTEMPTS: usize = 3;

    let mut acquired = None;
    let (mut address, provenance) = match allocate_local_address_for_config(owned, mode).await {
        Ok(allocation) => allocation,
        Err(error) => {
            return Allocated {
                result: Err(error),
                claim: None,
                acquired,
            };
        }
    };
    for _ in 0..ATTEMPTS {
        if !crate::network_utils::is_custom_loopback_address(&address) {
            return Allocated {
                result: Ok(address),
                claim: None,
                acquired,
            };
        }
        acquired = Some(address.clone());
        let Some(claim) = crate::kube::stop::AddressClaim::take(&address, owned.id) else {
            return Allocated {
                result: Err(format!(
                    "Local address {address} is still being released by an earlier stop"
                )),
                claim: None,
                acquired,
            };
        };
        if provenance != Allocation::Helper {
            return Allocated {
                result: Ok(address),
                claim: Some(claim),
                acquired,
            };
        }
        let confirmed = match allocate_local_address_for_config(owned, mode).await {
            Ok((confirmed, Allocation::Helper)) => confirmed,
            // The helper stopped answering between the two calls; the address
            // it gave first is still the one this startup holds under its
            // claim, and the fallback's fresh alias is not wanted.
            Ok((other, _)) => {
                if other != address {
                    release_stray_alias(owned, &other, mode).await;
                }
                owned.local_address = Some(address.clone());
                return Allocated {
                    result: Ok(address),
                    claim: Some(claim),
                    acquired,
                };
            }
            Err(error) => {
                return Allocated {
                    result: Err(error),
                    claim: None,
                    acquired,
                };
            }
        };
        if confirmed == address {
            return Allocated {
                result: Ok(address),
                claim: Some(claim),
                acquired,
            };
        }
        warn!("Address {address} was reassigned while it was being claimed; moving to {confirmed}");
        drop(claim);
        address = confirmed;
        // The alias the helper just handed out is bound whether or not another
        // attempt follows; an exhausted sequence must release this one, not
        // the one it replaced.
        acquired = Some(address.clone());
    }
    Allocated {
        result: Err(format!(
            "Could not settle on a local address for config {}: it kept changing under the claim",
            owned.id.unwrap_or_default()
        )),
        claim: None,
        acquired,
    }
}

/// Removes an alias the fallback allocator created while the helper's address
/// was being confirmed; nothing will ever forward on it.
///
/// A release that fails leaves the alias bound with nothing else naming it,
/// so it is recorded as this configuration's local obligation and a later
/// stop retries it.
async fn release_stray_alias(owned: &Config, address: &str, mode: DatabaseMode) {
    if let Err(error) = crate::kube::stop::release_address_with_fallback(address, None, mode).await
    {
        warn!("Failed to release stray alias {address}: {error}");
        // A privilege that is not available right now will not become
        // available by retrying automatically; the warning above is the only
        // report this gets.
        if !error.is_unsatisfiable()
            && let Some(id) = owned.id
        {
            let mut stray = owned.clone();
            stray.local_address = Some(address.to_owned());
            crate::kube::stop::record_local_cleanup(id, stray);
        }
    }
}

/// Where an allocated address came from.
///
/// Only a helper allocation is backed by a reservation a sibling's release
/// can take away, and only the helper answers the same address for the same
/// service. The fallback creates a fresh alias on every call, so an address
/// it produced is never re-requested for confirmation: doing so would strand
/// the previous alias each time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Allocation {
    Helper,
    Fallback,
    /// The configuration's own address, or the default; nothing was reserved.
    Static,
}

async fn allocate_local_address_for_config(
    config: &mut Config, mode: DatabaseMode,
) -> Result<(String, Allocation), String> {
    if !config.auto_loopback_address {
        let address = config
            .local_address
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());

        if crate::network_utils::is_custom_loopback_address(&address) {
            info!("Configuring custom loopback address: {address}");
            if let Err(config_err) = crate::network_utils::ensure_loopback_address(&address).await {
                let error_msg = config_err.to_string();
                if error_msg.contains("cancelled") || error_msg.contains("canceled") {
                    return Err(format!(
                        "Custom loopback address configuration cancelled: {error_msg}"
                    ));
                }
                warn!("Failed to configure custom loopback address {address}: {config_err}");
            }
        }

        return Ok((address, Allocation::Static));
    }

    let service_name = config
        .service
        .clone()
        .unwrap_or_else(|| format!("service-{}", config.id.unwrap_or_default()));

    let service = service_name.clone();
    let allocation = tokio::task::spawn_blocking(move || try_allocate_address(&service))
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);
    match allocation {
        Ok(allocated_address) => {
            info!("Auto-allocated address {allocated_address} for service {service_name}");
            config.local_address = Some(allocated_address.clone());

            info!(
                "Setting config.local_address to {} for config_id {}",
                allocated_address,
                config.id.unwrap_or_default()
            );
            Ok((allocated_address, Allocation::Helper))
        }
        Err(e) => {
            warn!(
                "Failed to auto-allocate address for service {service_name} via helper: {e}. Trying fallback allocation"
            );

            match try_fallback_allocate_and_save(&service_name, config, mode).await {
                Ok(allocated_address) => {
                    info!(
                        "Fallback-allocated address {allocated_address} for service {service_name}"
                    );
                    Ok((allocated_address, Allocation::Fallback))
                }
                Err(fallback_err) => {
                    if fallback_err.contains("cancelled") || fallback_err.contains("canceled") {
                        error!("Address allocation cancelled by user: {fallback_err}");
                        return Err(fallback_err);
                    }

                    warn!(
                        "Fallback allocation also failed for service {service_name}: {fallback_err}. Using default 127.0.0.1"
                    );
                    let default_address = "127.0.0.1".to_string();
                    config.local_address = Some(default_address.clone());
                    Ok((default_address, Allocation::Static))
                }
            }
        }
    }
}

fn try_allocate_address(service_name: &str) -> Result<String, String> {
    let app_id = "com.kftray.app".to_string();

    let socket_path =
        kftray_helper::communication::get_default_socket_path().map_err(|e| e.to_string())?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err("Helper service is not available".to_string());
    }

    let command = kftray_helper::messages::RequestCommand::Address(
        kftray_helper::messages::AddressCommand::Allocate {
            service_name: service_name.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(response) => match response.result {
            kftray_helper::messages::RequestResult::StringSuccess(address) => Ok(address),
            kftray_helper::messages::RequestResult::Error(error) => Err(error),
            _ => Err("Unexpected response format".to_string()),
        },
        Err(e) => Err(e.to_string()),
    }
}

async fn try_fallback_allocate_and_save(
    service_name: &str, config: &mut Config, mode: DatabaseMode,
) -> Result<String, String> {
    let _lock = FALLBACK_ALLOCATION_MUTEX.lock().await;

    debug!("Acquired fallback allocation lock for service: {service_name}");

    let allocated_addresses = get_allocated_loopback_addresses(mode).await;

    for octet in 2..255 {
        let address = format!("127.0.0.{octet}");

        if allocated_addresses.contains(&address) {
            debug!("Address {address} already allocated to another config, skipping");
            continue;
        }

        // A release that timed out can still be executing, and it would remove
        // the alias from underneath this forward.
        if crate::kube::stop::address_release_in_flight(&address) {
            debug!("Address {address} is still being released, skipping");
            continue;
        }

        if crate::network_utils::is_address_accessible(&address).await {
            debug!("Address {address} is already in use on system, skipping");
            continue;
        }

        match crate::network_utils::ensure_loopback_address(&address).await {
            Ok(_) => {
                debug!(
                    "Successfully allocated and configured fallback address: {address} for service: {service_name}"
                );

                config.local_address = Some(address.clone());
                info!(
                    "Setting config.local_address to {} (fallback) for config_id {}",
                    address,
                    config.id.unwrap_or_default()
                );

                return Ok(address);
            }
            Err(e) => {
                let error_msg = e.to_string();
                debug!("Failed to configure fallback address {address}: {error_msg}");

                if error_msg.contains("User cancelled")
                    || error_msg.contains("user cancelled")
                    || error_msg.contains("cancelled")
                    || error_msg.contains("User canceled")
                    || error_msg.contains("canceled")
                {
                    return Err(format!("Address allocation cancelled by user: {error_msg}"));
                }

                continue;
            }
        }
    }

    Err("No available addresses found in fallback allocation".to_string())
}

async fn get_allocated_loopback_addresses(mode: DatabaseMode) -> std::collections::HashSet<String> {
    use std::collections::HashSet;

    let mut allocated = HashSet::new();

    if let Ok(configs) = kftray_commons::config::get_configs_with_mode(mode).await {
        for config in configs {
            if let Some(addr) = &config.local_address
                && crate::network_utils::is_custom_loopback_address(addr)
                && config.auto_loopback_address
            {
                allocated.insert(addr.clone());
                debug!(
                    "Found allocated address {} for config {}",
                    addr,
                    config.id.unwrap_or_default()
                );
            }
        }
    }

    debug!("Currently allocated loopback addresses: {allocated:?}");
    allocated
}

pub async fn start_port_forward(
    configs: Vec<Config>, protocol: &str,
) -> Result<Vec<CustomResponse>, String> {
    start_port_forward_with_mode(configs, protocol, DatabaseMode::File, false).await
}

/// `cancellation` covers the phase after the relay is ready: loopback
/// allocation, TLS setup and stream acquisition all run while the proxy
/// lifecycle lock is held, so a stop issued during them would otherwise wait
/// for the whole startup to finish.
pub(super) async fn start_config_cancellable(
    mut config: Config, protocol: &str, mode: DatabaseMode, ssl_override: bool,
    cancellation: Option<&CancellationToken>, expected_destination: Option<String>,
) -> Result<CustomResponse, String> {
    let cancelled = || cancellation.is_some_and(CancellationToken::is_cancelled);
    let config_id = config.id.ok_or("Config has no ID")?;
    if !matches!(protocol, "tcp" | "udp") {
        return Err(format!("Unsupported protocol: {protocol}"));
    }
    if cancelled() {
        return Err(format!("Startup cancelled for config {config_id}"));
    }
    let mut allocated_claim = None;
    if config.auto_loopback_address || config.local_address.is_none() {
        const ALLOCATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

        // Bounded rather than raced against cancellation: the helper request
        // runs on a blocking task that keeps going once this future is dropped,
        // and an address it assigns afterwards would never be recorded or
        // released. Waiting for the outcome keeps that impossible while still
        // releasing the lifecycle lock in bounded time.
        match tokio::time::timeout(
            ALLOCATION_TIMEOUT,
            allocate_local_address_owned(&mut config, mode),
        )
        .await
        {
            Ok(allocated) => {
                let (_, claim) = allocated?;
                allocated_claim = claim;
            }
            Err(_) => {
                return Err(format!(
                    "Timed out allocating a local address for config {config_id}"
                ));
            }
        };
        if cancelled() {
            // Routed through the same rollback as every other exit: the
            // allocation task recorded a cleanup obligation for this address,
            // and releasing it here without withdrawing that record leaves a
            // pending target every later stop re-releases and never settles.
            let reason = format!("Startup cancelled for config {config_id}");
            return Err(match &config.local_address {
                Some(address) => {
                    let address = address.clone();
                    rollback_local_resources(&config, &address, reason, mode).await
                }
                None => reason,
            });
        }
    }
    // Claimed for the address actually chosen, whichever path chose it. An
    // allocated address arrives already claimed by the allocating task; a
    // manually configured one skips allocation and is claimed here. A release
    // that timed out is still executing and would remove the alias from
    // underneath this forward, and a check instead of a claim would pass just
    // before an abandoned allocation task started removing the same address.
    // The claim is held for the whole startup, and the registered process
    // takes over from there.
    let _address_claim = match (allocated_claim, &config.local_address) {
        (Some(claim), _) => Some(claim),
        (None, Some(address)) => match crate::kube::stop::AddressClaim::take(address, config.id) {
            Some(claim) => Some(claim),
            None => {
                return Err(format!(
                    "Local address {address} is still being released by an earlier stop"
                ));
            }
        },
        (None, None) => None,
    };
    // Claimed once for the whole startup, before any hosts entry is written,
    // so a deferred cleanup task spawned by an earlier, abandoned attempt for
    // this id can tell it has been superseded rather than deleting what this
    // attempt writes.
    let hosts_claim = crate::kube::stop::claim_host_entries(config_id);
    if let Some(config_id) = config.id {
        clear_stopped_by_timeout(config_id);
    }

    let selector = match (config.workload_type.as_deref(), config.protocol.as_str()) {
        (Some("pod"), "tcp") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
        (Some("pod"), "udp") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
        (Some("service"), "tcp") => {
            TargetSelector::ServiceName(config.service.clone().unwrap_or_default())
        }
        (Some("service"), "udp") => TargetSelector::PodLabel(format!(
            "app={},config_id={}",
            config.service.clone().unwrap_or_default(),
            config.id.unwrap_or_default()
        )),
        (Some("proxy"), "udp") => TargetSelector::PodLabel(format!(
            "app={},config_id={}",
            config.service.clone().unwrap_or_default(),
            config.id.unwrap_or_default()
        )),
        (Some("proxy"), "tcp") => TargetSelector::PodLabel(format!(
            "app={},config_id={}",
            config.service.clone().unwrap_or_default(),
            config.id.unwrap_or_default()
        )),
        _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
    };

    let remote_port = Port::from(config.remote_port.unwrap_or_default() as i32);
    let context_name = Some(config.context.clone());
    let kubeconfig = Some(config.kubeconfig.clone());
    let namespace = config.namespace.clone();
    let target = Target::new(selector, remote_port, namespace.clone());

    debug!("Remote Port: {:?}", config.remote_port);
    debug!("Local Port: {:?}", config.local_port);

    match config.workload_type.as_deref() {
        Some("pod") => info!("Attempting to forward to pod label: {:?}", config.target),
        Some("proxy") => info!("Attempting to forward to proxy pod: {:?}", config.service),
        _ => info!("Attempting to forward to service: {:?}", config.service),
    }

    let final_local_address = config
        .local_address
        .clone()
        .unwrap_or_else(|| "127.0.0.1".to_string());

    if config.domain_enabled.unwrap_or_default()
        && let Some(service_name) = &config.service
    {
        // Validated first: nothing has been created yet, so a rejected address
        // must not leave a cleanup record that would then block deleting the
        // configuration.
        let Ok(ip_addr) = final_local_address.parse::<std::net::IpAddr>() else {
            let error_message =
                format!("Invalid IP address format for domain alias: {final_local_address}");
            error!("{}", error_message);
            return Err(error_message);
        };

        // Recorded before the alias exists: from here the startup owns a hosts
        // entry, and being dropped before the process is registered would
        // otherwise leave it behind with nothing tracking it.
        if let Some(id) = config.id {
            crate::kube::stop::record_local_cleanup(id, config.clone());
        }
        let host_entry = HostEntry {
            ip: ip_addr,
            hostname: config.alias.clone().unwrap_or_default(),
        };
        let written = add_host_entry_owned(
            config.id.unwrap_or_default(),
            &config,
            &final_local_address,
            host_entry,
            mode,
            hosts_claim,
        )
        .await;
        if let Err(e) = written {
            let error_message = format!(
                "Failed to write to the hostfile for {service_name}: {e}. Domain alias feature \
                 requires hostfile access."
            );
            error!("{}", error_message);

            // Releases the address this startup may already have allocated, and
            // keeps the configuration recorded only if that did not finish.
            return Err(rollback_local_resources(
                &config,
                &final_local_address,
                error_message,
                mode,
            )
            .await);
        }
    }

    let local_address_clone = Some(final_local_address);

    let settings = get_app_settings().await.ok();
    let should_use_ssl = (settings
        .as_ref()
        .is_some_and(|settings| settings.ssl_enabled)
        || ssl_override)
        && config.alias.is_some();

    let actual_config = config.clone();

    let port_forward = PortForward::new(
        target,
        actual_config.local_port,
        local_address_clone,
        context_name.clone().flatten(),
        kubeconfig.flatten(),
        actual_config.id.unwrap_or_default(),
        actual_config.workload_type.clone().unwrap_or_default(),
    )
    .expecting_destination(expected_destination);

    let tls_acceptor = if protocol == "tcp" && should_use_ssl {
        if let Some(settings) = &settings {
            match build_tls_acceptor(&actual_config, settings).await {
                Ok(acceptor) => Some(acceptor),
                Err(e) => {
                    warn!("Failed to create TLS acceptor: {}", e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Raced against the startup token: `PortForward` builds its own token, so
    // these phases, and the connection loading and selector resolution inside
    // them, would otherwise hold the lifecycle lock while a stop waits on it.
    let forward = async {
        match protocol {
            "udp" => port_forward.clone().port_forward_udp().await,
            "tcp" => port_forward.clone().port_forward_tcp(tls_acceptor).await,
            _ => {
                error!("Unsupported protocol: {protocol}");
                Err(anyhow::anyhow!("Unsupported protocol: {}", protocol))
            }
        }
    };
    let forward_result = match cancellation {
        Some(token) => tokio::select! {
            biased;
            _ = token.cancelled() => {
                return Err(rollback_startup(&port_forward, config, format!(
                    "Startup cancelled for config {config_id}"
                ), mode).await);
            }
            forwarded = forward => forwarded,
        },
        None => forward.await,
    };

    match forward_result {
        Ok((actual_local_port, mut handle)) => {
            let protocol_upper = protocol.to_uppercase();
            info!(
                "{} port forwarding is set up on local port: {:?} for {}: {:?}",
                protocol_upper,
                actual_local_port,
                workload_type_description(config.workload_type.as_deref()),
                config.service
            );

            debug!(
                "Port forwarding established for config_id: {}",
                port_forward.config_id
            );
            debug!("Actual local port: {actual_local_port}");

            if cancelled() {
                handle.cleanup_and_abort().await;
                return Err(rollback_startup(
                    &port_forward,
                    config,
                    format!("Startup cancelled for config {config_id}"),
                    mode,
                )
                .await);
            }

            // The snapshot goes first: what this forward holds is written
            // before anything says it is running, so another process never
            // reads a running row with no record of its resources.
            let recorded =
                kftray_commons::utils::config_state::set_running_snapshot(config_id, &config, mode)
                    .await;
            let config_state = ConfigState::new(config_id, true);
            if let Err(error) = match recorded {
                Ok(()) => update_config_state_with_mode(&config_state, mode).await,
                Err(error) => Err(error),
            } {
                handle.cleanup_and_abort().await;
                kftray_commons::utils::config_state::clear_running_snapshot(config_id, mode).await;
                return Err(rollback_startup(&port_forward, config, error, mode).await);
            }

            handle.set_config(config.clone());
            CHILD_PROCESSES.insert(config_id, handle);
            // The process now owns the local resources, so the record taken
            // when the address was allocated is no longer needed. Only the
            // local obligation goes: a cluster obligation left by an earlier
            // attempt on another server is not settled by this start.
            crate::kube::stop::settle_local_cleanup(config_id, &config);
            let timeout_callback = create_static_timeout_callback(mode);

            if let Err(e) = start_timeout_for_forward(config_id, timeout_callback).await {
                error!("Failed to start timeout for config {config_id}: {e}");
            }

            if should_use_ssl
                && protocol == "tcp"
                && let Err(e) = update_hosts_with_ssl(&config, mode, hosts_claim).await
            {
                warn!("Failed to update hosts file for SSL: {}", e);
            }

            let target_name =
                config
                    .service
                    .as_deref()
                    .unwrap_or_else(|| match &port_forward.target.selector {
                        TargetSelector::ServiceName(name) | TargetSelector::PodLabel(name) => name,
                    });

            Ok(CustomResponse {
                id: config.id,
                service: target_name.to_owned(),
                namespace: namespace.clone(),
                local_port: actual_local_port,
                remote_port: config.remote_port.unwrap_or_default(),
                context: config.context.clone().unwrap_or_default(),
                protocol: config.protocol.clone(),
                stdout: {
                    let protocol_display = if should_use_ssl && protocol == "tcp" {
                        "HTTPS".to_string()
                    } else {
                        protocol.to_uppercase()
                    };
                    format!(
                        "{} forwarding from 127.0.0.1:{} -> {:?}:{}{}",
                        protocol_display,
                        actual_local_port,
                        config.remote_port.unwrap_or_default(),
                        target_name,
                        if should_use_ssl && protocol == "tcp" {
                            " (HTTP redirects to HTTPS)"
                        } else {
                            ""
                        }
                    )
                },
                stderr: String::new(),
                status: 0,
            })
        }
        Err(e) => {
            let protocol_upper = protocol.to_uppercase();
            let error_message = format!(
                "Failed to start {} port forwarding for {} {}: {}",
                protocol_upper,
                workload_type_description(config.workload_type.as_deref()),
                config.service.clone().unwrap_or_default(),
                e
            );
            error!("{}", error_message);

            Err(rollback_startup(&port_forward, config, error_message, mode).await)
        }
    }
}

pub(super) async fn start_config_locked(
    config: Config, protocol: &str, mode: DatabaseMode, ssl_override: bool,
    cancellation: Option<&CancellationToken>,
) -> Result<CustomResponse, String> {
    let id = config.id.ok_or("Config has no ID")?;
    let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
    // A stop that cancels this registration must be able to overtake a start
    // waiting on the lifecycle lock, otherwise the start acquires the lock
    // afterwards and creates a listener the stop already reported as gone.
    let result = {
        let guard = match cancellation {
            Some(token) => tokio::select! {
                biased;
                _ = token.cancelled() => None,
                guard = lock.lock() => Some(guard),
            },
            None => Some(lock.lock().await),
        };
        match guard {
            None => Err(format!("Startup cancelled for config {id}")),
            Some(guard) => {
                // Held through registration and the state write, so a delete
                // or stop in another process sees either the row untouched or
                // the forward fully registered, never the gap between.
                let shared = kftray_commons::utils::config_dir::lock_config(
                    id,
                    mode,
                    crate::kube::stop::SHARED_LOCK_WAIT,
                )
                .await;
                let result = if let Err(error) = &shared {
                    Err(error.clone())
                } else if cancellation.is_some_and(CancellationToken::is_cancelled) {
                    Err(format!("Startup cancelled for config {id}"))
                } else if CHILD_PROCESSES.contains_key(&id) {
                    Err(format!(
                        "Port forwarding is already running for config {id}"
                    ))
                } else if let Some(owner) =
                    crate::kube::stop::running_in_another_process(id, mode).await
                {
                    // Checked under the shared lock: a second process starting
                    // the same row would otherwise overwrite the recorded owner
                    // while the first listener keeps running.
                    Err(format!(
                        "Config {id} is being forwarded by another kftray process ({owner})"
                    ))
                } else if kftray_commons::utils::config::get_config_with_mode(id, mode)
                    .await
                    .is_err()
                {
                    // Re-read under the lock: deletion takes the same lock, so
                    // a start that waited on it must not forward a row that has
                    // since disappeared.
                    Err(format!("Config {id} no longer exists"))
                } else if config.workload_type.as_deref() == Some("expose") {
                    crate::expose::start_single_expose(config, mode, cancellation).await
                } else {
                    start_config_cancellable(
                        config,
                        protocol,
                        mode,
                        ssl_override,
                        cancellation,
                        None,
                    )
                    .await
                };
                drop(shared);
                drop(guard);
                result
            }
        }
    };
    drop(lock);
    crate::kube::proxy_recovery::remove_recovery_lock(id);
    result
}

/// Failure placeholder mirroring [`stop_response`](super::stop) so a batch can
/// report per-config outcomes. Returning a batch-level `Err` would discard the
/// responses of the configs that did start, including their dynamically
/// assigned local ports.
pub(super) fn start_failure_response(config: &Config, error: String) -> CustomResponse {
    CustomResponse {
        id: config.id,
        service: config.service.clone().unwrap_or_default(),
        namespace: config.namespace.clone(),
        local_port: config.local_port.unwrap_or_default(),
        remote_port: config.remote_port.unwrap_or_default(),
        context: config.context.clone().unwrap_or_default(),
        protocol: config.protocol.clone(),
        stdout: String::new(),
        status: 1,
        stderr: error,
    }
}

pub async fn start_port_forward_with_mode(
    configs: Vec<Config>, protocol: &str, mode: DatabaseMode, ssl_override: bool,
) -> Result<Vec<CustomResponse>, String> {
    // Registration is eager so a stop-all that snapshots the pending starts
    // sees the whole batch: `buffer_unordered` only polls a window, and the
    // unpolled tail would otherwise start after that snapshot.
    let (queued, rejected) = crate::kube::proxy::register_start_batch(configs);

    let mut responses: Vec<CustomResponse> = stream::iter(queued)
        .map(|(config, startup)| async move {
            if startup.cancellation().is_cancelled() {
                return start_failure_response(
                    &config,
                    "Startup cancelled before it began".to_string(),
                );
            }
            match start_config_locked(
                config.clone(),
                protocol,
                mode,
                ssl_override,
                Some(startup.cancellation()),
            )
            .await
            {
                Ok(response) => response,
                Err(error) => start_failure_response(&config, error),
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;
    responses.extend(
        rejected
            .into_iter()
            .map(|(config, error)| start_failure_response(&config, error)),
    );
    Ok(responses)
}

#[cfg(test)]
mod tests {

    use super::*;

    fn setup_test_config() -> Config {
        Config {
            id: Some(1),
            context: Some("test-context".to_string()),
            kubeconfig: None,
            namespace: "test-namespace".to_string(),
            service: Some("test-service".to_string()),
            alias: Some("test-alias".to_string()),
            local_port: Some(0),
            remote_port: Some(8080),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            target: None,
            local_address: None,
            remote_address: None,
            domain_enabled: None,
            auto_loopback_address: false,
            http_logs_enabled: Some(false),
            http_logs_max_file_size: Some(10 * 1024 * 1024),
            http_logs_retention_days: Some(7),
            http_logs_auto_cleanup: Some(true),
            exposure_type: None,
            cert_manager_enabled: None,
            cert_issuer: None,
            cert_issuer_kind: None,
            ingress_class: None,
            ingress_annotations: None,
        }
    }

    fn setup_config_with_domain() -> Config {
        let mut config = setup_test_config();
        config.domain_enabled = Some(true);
        config.local_address = Some("127.0.0.1".to_string());
        config
    }

    fn setup_config_with_invalid_ip() -> Config {
        let mut config = setup_test_config();
        config.domain_enabled = Some(true);
        config.local_address = Some("invalid-ip".to_string());
        config
    }

    #[tokio::test]
    async fn test_start_port_forward_empty_configs() {
        let configs = Vec::new();

        let result = start_port_forward(configs, "tcp").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    /// Inserts a configuration so a start reaches the phase under test instead
    /// of stopping at the missing-row check.
    async fn insert_fixture(config: Config) -> Config {
        let id = kftray_commons::utils::config::insert_config_with_mode(
            config.clone(),
            DatabaseMode::Memory,
        )
        .await
        .unwrap();

        Config {
            id: Some(id),
            ..config
        }
    }

    #[tokio::test]
    async fn test_start_port_forward_invalid_protocol() {
        // Shares the process-wide startup registry with the stop-all tests,
        // which cancel everything they find registered there.
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = insert_fixture(setup_test_config()).await;
        let id = config.id.unwrap();

        let responses =
            start_port_forward_with_mode(vec![config], "invalid", DatabaseMode::Memory, false)
                .await
                .unwrap();

        assert_eq!(responses.len(), 1);
        assert_ne!(responses[0].status, 0);
        assert!(
            responses[0]
                .stderr
                .contains("Unsupported protocol: invalid"),
            "{}",
            responses[0].stderr
        );
        assert!(!CHILD_PROCESSES.contains_key(&id));
    }

    #[tokio::test]
    async fn a_failed_config_does_not_hide_its_siblings_results() {
        // Shares the process-wide startup registry with the stop-all tests,
        // which cancel everything they find registered there.
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let healthy = insert_fixture(setup_config_with_domain()).await;
        let broken = insert_fixture(setup_config_with_invalid_ip()).await;
        let expected = {
            let mut ids = vec![healthy.id.unwrap(), broken.id.unwrap()];
            ids.sort_unstable();
            ids
        };

        let responses =
            start_port_forward_with_mode(vec![healthy, broken], "tcp", DatabaseMode::Memory, false)
                .await
                .unwrap();

        assert_eq!(responses.len(), 2);
        for response in &responses {
            assert_ne!(response.status, 0);
            assert!(!response.stderr.is_empty());
        }
        let mut ids: Vec<_> = responses
            .iter()
            .filter_map(|response| response.id)
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, expected);
    }

    #[tokio::test]
    async fn a_stop_overtakes_a_start_waiting_on_the_lifecycle_lock() {
        // Shares the process-wide startup registry with the stop-all tests,
        // which cancel everything they find registered there.
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_150;
        let config = Config {
            id: Some(id),
            ..setup_config_with_invalid_ip()
        };
        let token = CancellationToken::new();

        // Hold the lifecycle lock the way an in-flight stop does.
        let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
        let held = lock.clone().lock_owned().await;

        let start = tokio::spawn({
            let token = token.clone();
            async move {
                start_config_locked(config, "tcp", DatabaseMode::Memory, false, Some(&token)).await
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !start.is_finished(),
            "the start must be waiting on the lock"
        );

        token.cancel();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), start)
            .await
            .expect("a cancelled start must not wait for the lock")
            .unwrap();

        assert!(
            result.unwrap_err().contains("cancelled"),
            "a start cancelled while queued must not create a listener"
        );
        assert!(!CHILD_PROCESSES.contains_key(&id));
        drop(held);
        drop(lock);
        crate::kube::proxy_recovery::remove_recovery_lock(id);
    }

    #[tokio::test]
    async fn a_batch_start_registers_through_the_shared_pending_registry() {
        // Shares the process-wide startup registry with the stop-all tests,
        // which cancel everything they find registered there.
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_090;
        let (queued, _) = crate::kube::proxy::register_start_batch(vec![Config {
            id: Some(id),
            ..setup_config_with_invalid_ip()
        }]);
        assert_eq!(queued.len(), 1);

        // Holding the registration models the queued tail of a larger batch:
        // stop-all can see and cancel it, and a second start cannot slip past.
        let responses = start_port_forward_with_mode(
            vec![Config {
                id: Some(id),
                ..setup_config_with_invalid_ip()
            }],
            "tcp",
            DatabaseMode::Memory,
            false,
        )
        .await
        .unwrap();

        assert_eq!(responses.len(), 1);
        assert!(
            responses[0].stderr.contains("already in progress"),
            "{}",
            responses[0].stderr
        );
        assert!(!CHILD_PROCESSES.contains_key(&id));

        drop(queued);
        assert!(!crate::kube::proxy::STARTING_PROXIES.contains_key(&id));
    }

    #[tokio::test]
    async fn test_allocate_local_address_for_config_disabled() {
        let mut config = setup_test_config();
        config.auto_loopback_address = false;
        config.local_address = Some("192.168.1.1".to_string());

        let (result, provenance) =
            allocate_local_address_for_config(&mut config, DatabaseMode::Memory)
                .await
                .unwrap();
        assert_eq!(result, "192.168.1.1");
        assert_eq!(provenance, Allocation::Static);
        assert_eq!(config.local_address, Some("192.168.1.1".to_string()));
    }

    #[tokio::test]
    async fn rejected_duplicate_start_keeps_the_existing_listener_alive() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_031;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _listener = listener;
            std::future::pending::<anyhow::Result<()>>().await
        });
        let mut existing = crate::port_forward::PortForwardProcess::new(task, id.to_string());
        existing.set_config(Config {
            id: Some(id),
            ..setup_test_config()
        });
        CHILD_PROCESSES.insert(id, existing);
        let config = Config {
            id: Some(id),
            kubeconfig: Some("/nonexistent/isolated-test-kubeconfig".to_string()),
            ..setup_test_config()
        };
        let responses =
            start_port_forward_with_mode(vec![config], "tcp", DatabaseMode::Memory, false)
                .await
                .unwrap();
        assert_eq!(responses.len(), 1);
        assert_ne!(responses[0].status, 0);
        assert!(responses[0].stderr.contains("already running"));
        assert!(tokio::net::TcpListener::bind(address).await.is_err());
        super::super::stop::stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .unwrap();
        let _listener = tokio::net::TcpListener::bind(address).await.unwrap();
    }
}
