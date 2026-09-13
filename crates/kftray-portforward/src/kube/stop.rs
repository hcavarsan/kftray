use std::collections::{
    HashMap,
    HashSet,
};
use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use futures::stream::{
    self,
    StreamExt,
};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kftray_commons::config_model::Config;
use kftray_commons::{
    config::get_config_with_mode,
    models::{
        config_state_model::ConfigState,
        response::CustomResponse,
    },
    utils::{
        config::read_configs_with_mode,
        config_state::{
            get_configs_state_with_mode,
            update_config_state_with_mode,
        },
        db_mode::DatabaseMode,
        timeout_manager::cancel_timeout_for_forward,
    },
};
use kube::Client;
use kube::api::{
    Api,
    DeleteParams,
    ListParams,
};
use tokio::task::spawn_blocking;
use tokio::time::timeout;
use tracing::{
    info,
    warn,
};

use crate::hostsfile::{
    remove_host_entry,
    remove_ssl_host_entry,
};
use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};
use crate::port_forward::CHILD_PROCESSES;
#[cfg(test)]
use crate::port_forward::PortForwardProcess;

/// One tracked cleanup target.
#[derive(Clone)]
struct PendingTarget {
    config: Config,
    /// Set while a create request's outcome is unknown. Until it expires, an
    /// empty resource list is not proof of cleanup: the API server may still be
    /// persisting an object whose request was abandoned.
    uncertain_until: Option<Instant>,
    /// Cluster resources still to delete. Tracked apart from `local` so
    /// confirming one does not discard the other: a proxy whose Deployment is
    /// deleted can still owe a loopback alias, and dropping that record would
    /// leave nothing for stop-all to find.
    cluster: bool,
    /// Loopback address and hosts entries still to release.
    local: bool,
}

impl PendingTarget {
    fn is_uncertain(&self, now: Instant) -> bool {
        self.uncertain_until.is_some_and(|until| now < until)
    }
}

lazy_static::lazy_static! {
    /// Configurations whose cleanup has not been confirmed, kept so a later
    /// stop can retry against the resources that actually exist. The database
    /// row is not a substitute: it can be edited or deleted while a forward
    /// runs. One id can hold several entries, because an edited config that was
    /// restarted describes different resources than the ones an earlier failed
    /// cleanup left behind.
    static ref PENDING_CLEANUP: dashmap::DashMap<i64, Vec<PendingTarget>> = dashmap::DashMap::new();
}

/// How long an abandoned create is assumed to still be in flight.
///
/// Tied to the create deadline: once the client has stopped waiting, the API
/// server either applied the request or dropped it well within another full
/// deadline. Shutdown budgets are derived from this so an abandoned create is
/// always reconciled before the registry disappears with the process.
pub const UNCERTAIN_CREATE_WINDOW: Duration = Duration::from_secs(30);

/// Two cleanup targets are the same when they name the same resources.
///
/// The protocol is part of that identity: `delete_cluster_resources` treats a
/// UDP service config as owning relay resources and the otherwise identical TCP
/// config as a no-op, so collapsing them would let the no-op forget the relay.
/// The local address and domain alias are part of it too, since local cleanup
/// releases exactly those.
fn same_resources(left: &Config, right: &Config) -> bool {
    left.namespace == right.namespace
        && left.context == right.context
        && left.kubeconfig == right.kubeconfig
        && left.workload_type == right.workload_type
        && left.protocol == right.protocol
        && left.service == right.service
        && left.local_address == right.local_address
        && left.domain_enabled == right.domain_enabled
        // A public exposure owns an Ingress that a private one never creates,
        // and cleanup treats a missing ingress permission differently for each.
        && left.exposure_type == right.exposure_type
}

/// Digest that survives compiler and platform changes.
///
/// These keys are persisted, so a hash whose algorithm may change between
/// releases would silently orphan every record written before the change.
/// FNV-1a is fixed here, fields are length-prefixed, and `None` is encoded
/// distinctly from an empty string.
pub(crate) fn stable_digest(fields: &[Option<&str>]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    for field in fields {
        match field {
            Some(value) => {
                feed(b"s");
                feed(&(value.len() as u64).to_le_bytes());
                feed(value.as_bytes());
            }
            None => feed(b"n"),
        }
    }

    hash
}

/// Key prefix under which an ambiguous create is persisted.
const UNCERTAIN_CREATE_PREFIX: &str = "uncertain_create:";

/// Persists an ambiguous create so it outlives this process.
///
/// Client-side elapsed time does not bound server-side completion: a request
/// abandoned on its deadline can still be admitted, and a cleanup pass that
/// listed nothing would forget a relay that appears seconds later. The record
/// survives a restart, which is when nothing else does.
async fn persist_uncertain_target(id: i64, config: &Config) -> Result<(), String> {
    let serialized = serde_json::to_string(config).map_err(|error| {
        format!("Failed to describe the cleanup metadata for config {id}: {error}")
    })?;
    let key = uncertain_create_key(id, config);

    kftray_commons::utils::settings::set_setting(&key, &serialized)
        .await
        .map_err(|error| format!("Failed to persist an unsettled create for config {id}: {error}"))
}

/// Counts one pass that found nothing, and reports whether the record has now
/// been confirmed often enough to drop.
///
/// A create whose outcome was never answered is not settled by elapsed client
/// time: the server can still admit it. The budget bounds how long this keeps
/// costing a list, without turning one empty result into proof.
async fn confirm_uncertain_target(id: i64, config: &Config) -> bool {
    const CONFIRMATIONS_REQUIRED: u32 = 2;

    // Deliberately a different prefix: the restore scan reads every key under
    // the create prefix as a target, and a counter is not one.
    let key = format!(
        "uncertain_confirmations:{}",
        uncertain_create_key(id, config)
            .strip_prefix(UNCERTAIN_CREATE_PREFIX)
            .unwrap_or_default()
    );
    let seen = kftray_commons::utils::settings::get_setting(&key)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
        + 1;
    if seen >= CONFIRMATIONS_REQUIRED {
        if let Err(error) = kftray_commons::utils::settings::delete_setting(&key).await {
            log::debug!("Failed to clear the confirmation count for config {id}: {error}");
        }
        return true;
    }
    if let Err(error) = kftray_commons::utils::settings::set_setting(&key, &seen.to_string()).await
    {
        warn!("Failed to record a cleanup confirmation for config {id}: {error}");
    }

    false
}

/// Forgets a persisted create once its resources are confirmed gone.
///
/// Awaited like the write it undoes: two detached tasks have no ordering, and a
/// late delete would erase a newer attempt's record.
async fn forget_uncertain_target(id: i64, config: &Config) {
    let key = uncertain_create_key(id, config);
    if let Err(error) = kftray_commons::utils::settings::delete_setting(&key).await {
        log::debug!("Failed to clear the unsettled create for config {id}: {error}");
    }
}

/// Identifies one configuration's resources, so two different targets for the
/// same id do not overwrite each other.
fn uncertain_create_key(id: i64, config: &Config) -> String {
    // The identity matches `same_resources` field for field: targets the
    // registry tracks separately must not share a key, or settling one would
    // delete another's restart metadata.
    let destination = stable_digest(&[
        Some(config.namespace.as_str()),
        config.context.as_deref(),
        config.kubeconfig.as_deref(),
        config.workload_type.as_deref(),
        Some(config.protocol.as_str()),
        config.service.as_deref(),
        config.local_address.as_deref(),
        config.domain_enabled.map(|on| if on { "1" } else { "0" }),
        config.exposure_type.as_deref(),
    ]);

    format!("{UNCERTAIN_CREATE_PREFIX}{id}:{destination:016x}")
}

/// Reloads creates persisted by an earlier run into the cleanup registry.
async fn restore_uncertain_targets() {
    let stored =
        match kftray_commons::utils::settings::get_settings_with_prefix(UNCERTAIN_CREATE_PREFIX)
            .await
        {
            Ok(stored) => stored,
            Err(error) => {
                warn!("Failed to read unsettled creates: {error}");
                return;
            }
        };
    for (key, value) in stored {
        let Some(id) = key
            .strip_prefix(UNCERTAIN_CREATE_PREFIX)
            .and_then(|rest| rest.split(':').next())
            .and_then(|id| id.parse::<i64>().ok())
        else {
            continue;
        };
        let Ok(config) = serde_json::from_str::<Config>(&value) else {
            continue;
        };
        // Restored with a fresh window: a process can restart seconds after
        // abandoning a create, and elapsed wall time is no more proof here than
        // it was in the run that recorded it. An entry already tracked keeps
        // the window it has, so repeated restores cannot push it forever.
        if PENDING_CLEANUP.get(&id).is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| same_resources(&entry.config, &config))
        }) {
            continue;
        }
        record_target(
            id,
            config,
            Some(Instant::now() + UNCERTAIN_CREATE_WINDOW),
            true,
            false,
        );
    }
}

/// Records a configuration whose resources exist but whose startup did not
/// finish, so stop-all still reaches them. Dropping a startup future (the
/// terminal's shutdown drain, an aborted task) skips its own rollback.
pub(crate) fn record_pending_cleanup(id: i64, config: Config) {
    record_target(id, config, None, true, true);
}

fn record_target(
    id: i64, config: Config, uncertain_until: Option<Instant>, cluster: bool, local: bool,
) {
    let mut entries = PENDING_CLEANUP.entry(id).or_default();
    if let Some(existing) = entries
        .iter_mut()
        .find(|entry| same_resources(&entry.config, &config))
    {
        existing.uncertain_until = match (existing.uncertain_until, uncertain_until) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        existing.cluster |= cluster;
        existing.local |= local;
        return;
    }
    entries.push(PendingTarget {
        config,
        uncertain_until,
        cluster,
        local,
    });
}

/// Drops one recorded target, leaving any other resources for this id tracked.
pub(crate) fn forget_pending_cleanup(id: i64, config: &Config) {
    if let Some(mut entries) = PENDING_CLEANUP.get_mut(&id) {
        entries.retain(|entry| !same_resources(&entry.config, config));
    }
    // Removed only while still empty: allocation tasks record targets outside
    // the lifecycle lock, so one can arrive between the retain above and this
    // call, and an unconditional remove would discard it.
    PENDING_CLEANUP.remove_if(&id, |_, entries| entries.is_empty());
}

/// Records what this pass left undone, replacing the entry's obligations
/// rather than adding to them.
///
/// [`record_target`] widens an existing record, which is right for a new
/// obligation but wrong here: cleanup that succeeded would be demanded again on
/// every later stop.
fn set_target_obligations(
    id: i64, config: &Config, uncertain_until: Option<Instant>, cluster: bool, local: bool,
) {
    if !cluster && !local {
        forget_pending_cleanup(id, config);
        return;
    }
    let mut entries = PENDING_CLEANUP.entry(id).or_default();
    if let Some(existing) = entries
        .iter_mut()
        .find(|entry| same_resources(&entry.config, config))
    {
        existing.uncertain_until = uncertain_until;
        existing.cluster = cluster;
        existing.local = local;
        return;
    }
    entries.push(PendingTarget {
        config: config.clone(),
        uncertain_until,
        cluster,
        local,
    });
}

fn pending_cleanup_targets(id: i64) -> Vec<PendingTarget> {
    PENDING_CLEANUP
        .get(&id)
        .map(|entry| entry.value().clone())
        .unwrap_or_default()
}

/// Records created cluster resources so a dropped startup future still leaves a
/// trail for stop-all.
///
/// The record is uncertain until [`confirm`](Self::confirm) observes the
/// create's outcome: a request abandoned in flight can still be persisted, and
/// forgetting it on one empty list would leave it running untracked.
pub(crate) struct ClusterResourceGuard {
    id: i64,
    config: Option<Config>,
    confirmed: bool,
}

impl ClusterResourceGuard {
    /// Awaits the durable record before returning, so the create it guards can
    /// only be issued once something outside this process knows about it.
    ///
    /// A failed write is an error rather than a warning: the caller would
    /// otherwise create a resource that a restart could never find.
    pub(crate) async fn arm(id: i64, config: Config) -> Result<Self, String> {
        record_target(
            id,
            config.clone(),
            Some(Instant::now() + UNCERTAIN_CREATE_WINDOW),
            true,
            false,
        );
        persist_uncertain_target(id, &config).await?;

        Ok(Self {
            id,
            config: Some(config),
            confirmed: false,
        })
    }

    /// The create returned, so the record describes a resource that either
    /// exists or never will. A guard dropped after this keeps the settled
    /// state rather than restarting the uncertainty window.
    pub(crate) fn confirm(&mut self) {
        self.confirmed = true;
        if let Some(config) = &self.config
            && let Some(mut entries) = PENDING_CLEANUP.get_mut(&self.id)
            && let Some(entry) = entries
                .iter_mut()
                .find(|entry| same_resources(&entry.config, config))
        {
            entry.uncertain_until = None;
        }
    }

    /// Nothing was created, so this guard's cluster obligation is settled. Any
    /// local cleanup recorded for the same resources stays tracked.
    pub(crate) async fn disarm(mut self) {
        let Some(config) = self.config.take() else {
            return;
        };
        // Definitively rejected, so nothing was created and the persisted
        // record has nothing left to describe.
        forget_uncertain_target(self.id, &config).await;
        if let Some(mut entries) = PENDING_CLEANUP.get_mut(&self.id) {
            for entry in entries
                .iter_mut()
                .filter(|entry| same_resources(&entry.config, &config))
            {
                entry.cluster = false;
                entry.uncertain_until = None;
            }
            entries.retain(|entry| entry.cluster || entry.local);
        }
        PENDING_CLEANUP.remove_if(&self.id, |_, entries| entries.is_empty());
    }
}

impl Drop for ClusterResourceGuard {
    fn drop(&mut self) {
        if let Some(config) = self.config.take() {
            // An unconfirmed attempt restarts the window here: it can be
            // abandoned long after the guard was armed, and the request it
            // dropped deserves the full reconciliation window from the moment
            // it was abandoned. A confirmed outcome stays settled.
            let uncertain_until =
                (!self.confirmed).then(|| Instant::now() + UNCERTAIN_CREATE_WINDOW);
            record_target(self.id, config, uncertain_until, true, false);
        }
    }
}

lazy_static::lazy_static! {
    /// Addresses whose release may still be running. They must not be handed to
    /// a new forward until it finishes.
    /// Addresses being released, and how many releases are still running for
    /// each. Counted rather than flagged: a timed-out release keeps running,
    /// and a later attempt for the same address must not clear the mark when it
    /// finishes first.
    static ref RELEASING_ADDRESSES: dashmap::DashMap<String, AddressState> = dashmap::DashMap::new();
}

/// How many holders an address has, in each direction.
///
/// Releases and startups are counted rather than flagged, and both go through
/// the same map entry: a check against a separate registry could pass just
/// before the other side registered, which is exactly the window where a
/// release strips the alias from underneath a starting forward.
#[derive(Default)]
struct AddressState {
    /// Startups holding this address, by the configuration that holds it. A
    /// startup's own claim must not block the rollback that releases what that
    /// same startup allocated.
    claims: Vec<Option<i64>>,
    /// Releases still running.
    releases: usize,
}

/// Clears the in-flight mark once every holder is done.
struct ReleaseInFlight(String);

impl ReleaseInFlight {
    /// Marks a release, unless a startup or a registered forward holds the
    /// address.
    ///
    /// Both checks happen under the same entry lock, so a startup cannot
    /// register between them: the helper hands the same address to two
    /// configurations of one service without reference counting, and releasing
    /// it for either would take the alias from under the other.
    fn mark(
        address: &str, owner: Option<i64>, keep_for: impl Fn(&str) -> bool,
    ) -> Option<Arc<Self>> {
        let mut state = RELEASING_ADDRESSES.entry(address.to_owned()).or_default();
        // A claim held by the configuration being released is this startup's
        // own: its rollback is exactly what should take the address back.
        if state.claims.iter().any(|claim| *claim != owner) || keep_for(address) {
            return None;
        }
        state.releases += 1;
        drop(state);

        Some(Arc::new(Self(address.to_owned())))
    }
}

/// Whether a registered forward other than `exclude` is listening on `address`.
pub(crate) fn address_has_other_owner(address: &str, exclude: Option<i64>) -> bool {
    CHILD_PROCESSES.iter().any(|entry| {
        Some(*entry.key()) != exclude
            && entry
                .value()
                .config()
                .and_then(|config| config.local_address.clone())
                .is_some_and(|local| local == address)
    })
}

impl Drop for ReleaseInFlight {
    fn drop(&mut self) {
        // Removed under the entry lock so a concurrent `mark` cannot observe a
        // zeroed count and then have this remove the entry it just created.
        RELEASING_ADDRESSES.remove_if_mut(&self.0, |_, state| {
            state.releases = state.releases.saturating_sub(1);
            state.releases == 0 && state.claims.is_empty()
        });
    }
}

/// Holds an address for a startup, so no release can take it away meanwhile.
pub(crate) struct AddressClaim(String, Option<i64>);

impl AddressClaim {
    /// Claims an address for a configuration, unless a release is running.
    pub(crate) fn take(address: &str, owner: Option<i64>) -> Option<Self> {
        let mut state = RELEASING_ADDRESSES.entry(address.to_owned()).or_default();
        if state.releases > 0 {
            return None;
        }
        state.claims.push(owner);
        drop(state);

        Some(Self(address.to_owned(), owner))
    }
}

impl Drop for AddressClaim {
    fn drop(&mut self) {
        RELEASING_ADDRESSES.remove_if_mut(&self.0, |_, state| {
            if let Some(index) = state.claims.iter().position(|claim| *claim == self.1) {
                state.claims.swap_remove(index);
            }
            state.releases == 0 && state.claims.is_empty()
        });
    }
}

/// Marks an address as being released until the returned guard is dropped.
///
/// Returns `None` when a startup holds the address, which is the same check a
/// caller would otherwise make separately and lose the race on.
pub(crate) fn mark_address_release(
    address: &str, exclude: Option<i64>,
) -> Option<Arc<impl Send + Sync + use<>>> {
    ReleaseInFlight::mark(address, exclude, |address| {
        address_has_other_owner(address, exclude)
    })
}

/// Whether an address is still being released and cannot be reused yet.
pub(crate) fn address_release_in_flight(address: &str) -> bool {
    RELEASING_ADDRESSES
        .get(address)
        .is_some_and(|state| state.releases > 0)
}

/// Outcome of releasing the local resources one target describes.
#[derive(Default)]
struct LocalCleanup {
    /// Cleanup that failed and can be retried by a later stop.
    failures: Vec<String>,
    /// The loopback alias could not be removed, which on some platforms needs
    /// the privileged helper. The forward itself is gone, so the stop is not
    /// held back, but the target stays recorded so a later stop retries it.
    deferred: Vec<String>,
}

impl LocalCleanup {
    fn settled(&self) -> bool {
        self.failures.is_empty() && self.deferred.is_empty()
    }
}

/// Releases the loopback address and host entries one target describes.
async fn release_local_resources(id: i64, config: &Config) -> LocalCleanup {
    let mut cleanup = LocalCleanup::default();
    if let Some(address) = &config.local_address
        && crate::network_utils::is_custom_loopback_address(address)
        && let Err(error) = release_address_with_fallback(address, Some(id)).await
    {
        cleanup.deferred.push(error);
    }
    // Hosts-file work is synchronous and serialized behind one lock, so it runs
    // on a blocking thread: several stops at once would otherwise queue up on
    // runtime workers and stall unrelated forwards.
    let domain_enabled = config.domain_enabled.unwrap_or_default();
    let hosts = spawn_blocking(move || {
        let mut errors = Vec::new();
        if domain_enabled && let Err(error) = remove_host_entry(&id.to_string()) {
            errors.push(error.to_string());
        }
        if let Err(error) = remove_ssl_host_entry(&id.to_string()) {
            errors.push(error.to_string());
        }
        errors
    })
    .await;
    match hosts {
        Ok(errors) => cleanup.failures.extend(errors),
        Err(error) => cleanup
            .failures
            .push(format!("Hosts cleanup task failed: {error}")),
    }
    cleanup
}

/// Deletes the cluster resources one configuration owns, under one deadline.
///
/// The whole operation is bounded, not just its polling: the client carries no
/// per-request timeout, so a server that accepts a request and never answers
/// would hold the lifecycle lock for as long as an interactive stop waits.
async fn delete_cluster_resources(id: i64, config: &Config) -> Result<(), String> {
    const CLUSTER_CLEANUP_TIMEOUT: Duration = Duration::from_secs(45);

    match timeout(
        CLUSTER_CLEANUP_TIMEOUT,
        delete_cluster_resources_inner(id, config),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(format!(
            "Timed out after {CLUSTER_CLEANUP_TIMEOUT:?} deleting the cluster resources for config \
             {id}"
        )),
    }
}

async fn delete_cluster_resources_inner(id: i64, config: &Config) -> Result<(), String> {
    if config.workload_type.as_deref() != Some("expose")
        && config.workload_type.as_deref() != Some("proxy")
        && config.protocol != "udp"
    {
        return Ok(());
    }
    let key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
    let connection = SHARED_CLIENT_MANAGER
        .get_connection(key)
        .await
        .map_err(|error| error.to_string())?;
    if config.workload_type.as_deref() == Some("expose") {
        crate::expose::kubernetes::delete_expose_resources(
            connection.client.clone(),
            &config.namespace,
            &id.to_string(),
            config.exposure_type.as_deref() == Some("public"),
            &crate::expose::kubernetes::ExposeLocation::of(config),
        )
        .await
    } else {
        delete_proxy_cluster_resources(connection.client.clone(), &config.namespace, id).await
    }
}

/// Synchronous helper function to release address via helper service.
/// Must be called from spawn_blocking to avoid blocking the tokio runtime.
fn try_release_address_sync(address: &str) -> Result<(), String> {
    let app_id = "com.kftray.app".to_string();

    let socket_path =
        kftray_helper::communication::get_default_socket_path().map_err(|e| e.to_string())?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err("Helper service is not available".to_string());
    }

    let command = kftray_helper::messages::RequestCommand::Address(
        kftray_helper::messages::AddressCommand::Release {
            address: address.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(response) => match response.result {
            kftray_helper::messages::RequestResult::Success => Ok(()),
            kftray_helper::messages::RequestResult::Error(error) => Err(error),
            _ => Err("Unexpected response format".to_string()),
        },
        Err(e) => Err(e.to_string()),
    }
}

/// Release address with timeout. Skips osascript fallback to avoid blocking on
/// user interaction. Address cleanup is not critical - addresses will be freed
/// on system restart.
pub(crate) async fn release_address_with_fallback(
    address: &str, owner: Option<i64>,
) -> Result<(), String> {
    const ADDRESS_RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

    // Marked for as long as a release may still be executing, and owned by the
    // work itself rather than by this future: timing out the wait does not stop
    // the helper request or the platform command, and a restart that reused the
    // address could have it removed underneath it.
    let Some(releasing) = ReleaseInFlight::mark(address, owner, |address| {
        address_has_other_owner(address, owner)
    }) else {
        // Another forward holds this address: the helper hands the same one to
        // two configurations of the same service, so releasing it here would
        // break a forward that is up or coming up.
        log::debug!("Skipping the release of {address}: another forward is using it");
        return Ok(());
    };

    let address_owned = address.to_string();

    // Wrap blocking helper service call in spawn_blocking with timeout
    let result = timeout(ADDRESS_RELEASE_TIMEOUT, {
        let addr = address_owned.clone();
        let releasing = Arc::clone(&releasing);
        spawn_blocking(move || {
            let _releasing = releasing;
            try_release_address_sync(&addr)
        })
    })
    .await;

    // Every failure is reported so the caller keeps the cleanup record and a
    // later stop retries, rather than marking the config stopped while its
    // loopback address is still bound.
    let helper_error = match result {
        Ok(Ok(Ok(_))) => {
            info!("Successfully released address via helper: {}", address);
            return Ok(());
        }
        Ok(Ok(Err(e))) => e.to_string(),
        // spawn_blocking panicked
        Ok(Err(e)) => e.to_string(),
        // Timeout elapsed
        Err(_) => format!("timed out after {ADDRESS_RELEASE_TIMEOUT:?}"),
    };

    // The helper is optional: a platform that binds the address without an
    // interface alias, or one where cleanup happens on restart, reports success
    // here and the stop can complete. Only a platform release that actually
    // fails keeps the cleanup record for a later retry.
    // Run on a blocking thread under the same deadline: the platform release
    // shells out, and doing that inline would hold a runtime worker, and the
    // lifecycle lock with it, for as long as the command takes.
    let address_owned = address.to_string();
    let platform = timeout(ADDRESS_RELEASE_TIMEOUT, {
        let releasing = Arc::clone(&releasing);
        spawn_blocking(move || {
            let _releasing = releasing;
            tokio::runtime::Handle::current().block_on(
                crate::network_utils::remove_loopback_address(&address_owned),
            )
        })
    })
    .await;
    match platform {
        Ok(Ok(Ok(()))) => {
            warn!(
                "Released address {} without the helper ({}).",
                address, helper_error
            );
            Ok(())
        }
        Ok(Ok(Err(platform_error))) => {
            warn!(
                "Failed to release address {}: helper: {}; platform: {}",
                address, helper_error, platform_error
            );
            Err(format!(
                "Failed to release address {address}: {helper_error}; {platform_error}"
            ))
        }
        Ok(Err(join_error)) => Err(format!(
            "Address release task failed for {address}: {helper_error}; {join_error}"
        )),
        Err(_) => Err(format!(
            "Address release timed out for {address} after {ADDRESS_RELEASE_TIMEOUT:?}"
        )),
    }
}

pub(crate) async fn delete_proxy_cluster_resources(
    client: Client, namespace: &str, config_id: i64,
) -> Result<(), String> {
    let prefix = crate::kube::proxy::proxy_resource_prefix();
    let owned = ListParams::default()
        .labels(&crate::kube::proxy::proxy_owner_selector(&config_id.to_string()).await?);
    let dp = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::default()
    };
    // Foreground propagation for the Deployment: with the default background
    // policy its disappearance says nothing about the ReplicaSet, which can
    // create a replacement pod after the pod pass has already seen an empty
    // list. Waiting for the foreground deletion to finish means the pods are
    // gone with it.
    let deployment_dp = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::foreground()
    };
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let deployments: Api<Deployment> = Api::namespaced(client, namespace);
    let selectors = [&owned];
    let mut errors: Vec<String> =
        match delete_prefixed(&deployments, &selectors, &prefix, &deployment_dp).await {
            Ok(()) => Vec::new(),
            Err(error) => vec![error],
        };
    // Pods are deleted after the Deployment is gone: doing both at once lets a
    // surviving ReplicaSet replace a pod this pass already deleted.
    if let Err(error) = delete_prefixed(&pods, &selectors, &prefix, &dp).await {
        errors.push(error);
    }

    // Relays created before the installation label existed carry only
    // app/config_id. Config ids are local, so another machine under the same
    // username produces the same labels and name prefix: the prefix is not
    // proof of ownership and these are never deleted automatically. They are
    // reported instead, so an empty owned list is not mistaken for confirmed
    // cleanup and the user can remove them from the server resources screen.
    let unlabelled = format!(
        "config_id={config_id},!{}",
        crate::kube::proxy::INSTALLATION_LABEL
    );
    let leftovers = list_prefixed_names(&pods, &unlabelled, &prefix).await?
        + &list_prefixed_names(&deployments, &unlabelled, &prefix).await?;
    if !leftovers.is_empty() {
        errors.push(format!(
            "Relay resources from an earlier version are still running and cannot be attributed to \
             this installation: {}. Remove them from the server resources screen.",
            leftovers.trim_end_matches(", ")
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

async fn list_prefixed_names<K>(
    api: &Api<K>, selector: &str, prefix: &str,
) -> Result<String, String>
where
    K: Clone + serde::de::DeserializeOwned + std::fmt::Debug + kube::Resource,
{
    let lp = ListParams::default().labels(selector);
    let list = api.list(&lp).await.map_err(|error| error.to_string())?;

    Ok(list
        .items
        .iter()
        .filter_map(|item| item.meta().name.clone())
        .filter(|name| name.starts_with(prefix))
        .map(|name| format!("{name}, "))
        .collect())
}

async fn delete_prefixed<K>(
    api: &Api<K>, selectors: &[&ListParams], prefix: &str, dp: &DeleteParams,
) -> Result<(), String>
where
    K: Clone + serde::de::DeserializeOwned + std::fmt::Debug + kube::Resource,
{
    const DELETION_TIMEOUT: Duration = Duration::from_secs(20);
    const POLL: Duration = Duration::from_millis(250);

    let mut errors = Vec::new();
    for lp in selectors {
        let list = api.list(lp).await.map_err(|error| error.to_string())?;
        for item in list.items {
            let Some(name) = item.meta().name.clone() else {
                continue;
            };
            if !name.starts_with(prefix) {
                continue;
            }
            if let Err(error) = api.delete(&name, dp).await
                && !matches!(&error, kube::Error::Api(response) if response.code == 404)
            {
                errors.push(error.to_string());
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    // An accepted DELETE only starts deletion: a finalizer can keep the object,
    // and its containers, running. Cleanup is only complete once it is gone, so
    // anything still present keeps the configuration tracked for a later retry.
    let deadline = Instant::now() + DELETION_TIMEOUT;
    loop {
        let mut remaining = Vec::new();
        for lp in selectors {
            let list = api.list(lp).await.map_err(|error| error.to_string())?;
            remaining.extend(
                list.items
                    .iter()
                    .filter_map(|item| item.meta().name.clone())
                    .filter(|name| name.starts_with(prefix)),
            );
        }
        if remaining.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Resources are still terminating after {DELETION_TIMEOUT:?}: {}",
                remaining.join(", ")
            ));
        }
        tokio::time::sleep(POLL).await;
    }
}

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    stop_all_port_forward_with_mode(DatabaseMode::File).await
}

/// Cancels every startup and recovery attempt without waiting for them.
///
/// Shutdown calls this before draining so in-flight startups observe
/// cancellation cooperatively and run their own rollback, instead of being
/// dropped mid-create by an abort deadline.
/// Configurations that must not be deleted: their forward is running, starting,
/// or still has resources waiting to be cleaned up.
///
/// Deleting the row does not stop anything, so the tunnel would keep running
/// with no configuration to stop it by.
pub async fn delete_configs_if_idle<F, Fut>(ids: &[i64], delete: F) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    // Held across the delete: `start_config_locked` takes the same lock, so a
    // start cannot register between the check and the row disappearing, and a
    // start already waiting on the lock re-reads the row afterwards.
    //
    // Sorted and deduplicated first: these mutexes are not reentrant, so a
    // repeated id would wait on a lock this call already holds, and two batches
    // naming the same ids in different orders would deadlock each other.
    let mut ordered: Vec<i64> = ids.to_vec();
    ordered.sort_unstable();
    ordered.dedup();
    let mut guards = Vec::with_capacity(ordered.len());
    for id in &ordered {
        let lock = crate::kube::proxy_recovery::acquire_recovery_lock(*id).await;
        guards.push(lock.lock_owned().await);
    }

    let active: Vec<i64> = ordered
        .iter()
        .copied()
        .filter(|id| {
            CHILD_PROCESSES.contains_key(id)
                || crate::kube::proxy::STARTING_PROXIES.contains_key(id)
                || PENDING_CLEANUP.contains_key(id)
        })
        .collect();
    if !active.is_empty() {
        return Err(format!(
            "Stop these configurations before deleting them: {}",
            active
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    delete().await
}

pub fn cancel_all_startups() {
    for entry in crate::kube::proxy::STARTING_PROXIES.iter() {
        entry.value().cancel();
    }
    for entry in crate::kube::proxy_recovery::RECOVERY_MANAGERS.iter() {
        entry.value().cancel();
    }
    for entry in CHILD_PROCESSES.iter() {
        entry.value().cancel();
    }
}

/// Retries every cleanup target still recorded, until none remain or the
/// deadline expires.
///
/// A create whose request was abandoned may only appear after the first
/// deletion pass, and the registry lives in memory: leaving it unreconciled at
/// exit leaves the resource running with nothing tracking it.
pub async fn reconcile_pending_cleanup(mode: DatabaseMode, deadline: Duration) {
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    // Creates abandoned by an earlier run are picked up here: nothing else in
    // this process knows about them.
    restore_uncertain_targets().await;

    let until = Instant::now() + deadline;
    loop {
        // Registered processes are enumerated too: a stop-all dropped on its
        // deadline leaves the ones it never visited only in `CHILD_PROCESSES`,
        // and they own cluster resources and hosts entries just the same.
        let mut ids: Vec<i64> = PENDING_CLEANUP.iter().map(|entry| *entry.key()).collect();
        ids.extend(
            CHILD_PROCESSES
                .iter()
                .map(|entry| *entry.key())
                .filter(|id| !PENDING_CLEANUP.contains_key(id)),
        );
        if ids.is_empty() {
            // An allocation still in flight has nothing recorded yet, so an
            // empty registry is only proof once none are outstanding.
            if crate::kube::start::OUTSTANDING_ALLOCATIONS.load(std::sync::atomic::Ordering::SeqCst)
                == 0
            {
                return;
            }
            if Instant::now() >= until {
                warn!("Giving up on address allocations that never finished");

                return;
            }
            tokio::time::sleep(RETRY_DELAY.min(until - Instant::now())).await;
            continue;
        }
        let Some(remaining) = until.checked_duration_since(Instant::now()) else {
            warn!(
                "Giving up on cleanup for {} configuration(s) that never settled: {ids:?}",
                ids.len()
            );
            return;
        };
        tokio::time::sleep(RETRY_DELAY.min(remaining)).await;
        for id in ids {
            // Each attempt carries the remaining budget: a stop can wait on the
            // lifecycle lock and several requests, and the caller's deadline
            // has to hold for the whole pass, not just between passes. A
            // dropped attempt keeps its target recorded.
            let Some(remaining) = until.checked_duration_since(Instant::now()) else {
                return;
            };
            match tokio::time::timeout(remaining, stop_config(id, None, mode)).await {
                Ok(Err(error)) => warn!("Cleanup for config {id} is still incomplete: {error}"),
                Err(_) => {
                    warn!("Cleanup for config {id} did not finish within the shutdown budget");
                    return;
                }
                Ok(Ok(_)) => {}
            }
        }
    }
}

pub async fn stop_all_port_forward_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<CustomResponse>, String> {
    // Creates abandoned by an earlier run exist only on disk: the desktop
    // application never calls the reconciliation pass, so without this its
    // stop-all cannot find a relay left behind by a crash.
    restore_uncertain_targets().await;

    let mut ids: HashSet<i64> = CHILD_PROCESSES.iter().map(|entry| *entry.key()).collect();
    for entry in crate::kube::proxy::STARTING_PROXIES.iter() {
        entry.value().cancel();
        ids.insert(*entry.key());
    }
    for entry in crate::kube::proxy_recovery::RECOVERY_MANAGERS.iter() {
        entry.value().cancel();
        ids.insert(*entry.key());
    }
    for entry in CHILD_PROCESSES.iter() {
        entry.value().cancel();
    }
    for entry in crate::kube::proxy_recovery::RECOVERY_LOCKS.iter() {
        if Arc::strong_count(entry.value()) > 1 {
            ids.insert(*entry.key());
        }
    }
    ids.extend(PENDING_CLEANUP.iter().map(|entry| *entry.key()));

    let configs_result = read_configs_with_mode(mode).await;
    let states_result = get_configs_state_with_mode(mode).await;
    if let Ok(states) = &states_result {
        ids.extend(
            states
                .iter()
                .filter(|state| state.is_running)
                .map(|state| state.config_id),
        );
    }
    let configs: HashMap<_, _> = configs_result
        .as_ref()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|config| config.id.map(|id| (id, config)))
        .collect();
    let mut responses: Vec<CustomResponse> = stream::iter(ids)
        .map(|id| {
            let config = configs.get(&id).copied();
            async move {
                match stop_config(id, config, mode).await {
                    Ok(response) => response,
                    Err(error) => stop_response(id, config, Some(error)),
                }
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;

    if let Err(error) = configs_result {
        warn!("Could not read configs while stopping every forward: {error}");
        responses.push(enumeration_failure(format!(
            "Could not read configs, so some forwards may not have been cleaned up: {error}"
        )));
    }
    if let Err(error) = states_result {
        warn!("Could not read config states while stopping every forward: {error}");
        responses.push(enumeration_failure(format!(
            "Could not read config states, so persisted forwards may have been missed: {error}"
        )));
    }
    Ok(responses)
}

/// Failure response that is not tied to a single config, used when stop-all
/// cannot enumerate everything it was supposed to stop.
fn enumeration_failure(error: String) -> CustomResponse {
    CustomResponse {
        id: None,
        service: String::new(),
        namespace: String::new(),
        local_port: 0,
        remote_port: 0,
        context: String::new(),
        protocol: String::new(),
        stdout: String::new(),
        status: 1,
        stderr: error,
    }
}

pub async fn stop_port_forward(config_id: String) -> Result<CustomResponse, String> {
    stop_port_forward_with_mode(config_id, DatabaseMode::File).await
}

pub async fn stop_port_forward_with_mode(
    config_id: String, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
    let id = config_id
        .parse::<i64>()
        .map_err(|_| "Invalid config ID".to_string())?;
    if let Some(startup) = crate::kube::proxy::STARTING_PROXIES.get(&id) {
        startup.cancel();
    }
    if let Some(manager) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&id) {
        manager.cancel();
    }
    if let Some(process) = CHILD_PROCESSES.get(&id) {
        process.cancel();
    }
    let config = get_config_with_mode(id, mode).await;
    let response = stop_config(id, config.as_ref().ok(), mode).await;
    match (config, response) {
        // The cleanup error describes what is still running; the lookup error
        // only says the row is gone, which is not the actionable part.
        (Err(lookup), Err(cleanup)) => Err(format!("{cleanup}; {lookup}")),
        (_, response) => response,
    }
}

async fn stop_config(
    id: i64, config: Option<&Config>, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
    if let Some(startup) = crate::kube::proxy::STARTING_PROXIES.get(&id) {
        startup.cancel();
    }
    if let Some(manager) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&id) {
        manager.cancel();
    }
    let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
    let (guard, was_starting) = match lock.try_lock() {
        Ok(guard) => (guard, false),
        Err(_) => (lock.lock().await, true),
    };
    if let Some(startup) = crate::kube::proxy::STARTING_PROXIES.get(&id) {
        startup.cancel();
    }
    if let Some((_, manager)) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.remove(&id) {
        manager.cancel();
    }
    let process = CHILD_PROCESSES.remove(&id);
    let existed = process.is_some();
    let mut retained = None;
    if let Some((_, mut process)) = process {
        retained = process.config().cloned();
        // Recorded before the first await: the process is already out of
        // CHILD_PROCESSES, so aborting this stop, as the terminal's shutdown
        // drain does, would otherwise drop the only snapshot of its resources.
        if let Some(config) = &retained {
            record_pending_cleanup(id, config.clone());
        }
        process.cleanup_and_abort().await;
    }
    // The snapshot taken at startup describes the resources that actually
    // exist. The database record can have been edited since without stopping
    // the forward, which would point cleanup at the new destination.
    let pending = pending_cleanup_targets(id);
    let refreshed = if retained.is_none() && pending.is_empty() && was_starting {
        get_config_with_mode(id, mode).await.ok()
    } else {
        None
    };
    // A recorded target describes resources that exist. The database row can
    // have been edited since the forward started, so it is only consulted when
    // nothing else describes what to clean.
    let config = retained
        .as_ref()
        .or(pending.first().map(|target| &target.config))
        .or(refreshed.as_ref())
        .or(config);
    cancel_timeout_for_forward(id).await;

    let result = if let Some(config) = config {
        // Every distinct set of resources this id ever created, not just the
        // current one: an edited config that was restarted does not describe
        // the resources an earlier failed cleanup left behind.
        let recorded = pending
            .iter()
            .find(|target| same_resources(&target.config, config));
        let mut targets = vec![PendingTarget {
            config: config.clone(),
            uncertain_until: recorded.and_then(|target| target.uncertain_until),
            // A record for these resources describes what is left: an earlier
            // stop that deleted the Deployment and failed to release the
            // address must not need cluster access again. Without one the
            // configuration is still forwarding and owes both.
            cluster: recorded.is_none_or(|target| target.cluster),
            local: recorded.is_none_or(|target| target.local),
        }];
        targets.extend(
            pending
                .iter()
                .filter(|target| !same_resources(&target.config, config))
                .cloned(),
        );

        // Recorded before any cleanup runs, not only after one fails: a
        // configuration known solely from the database (a persisted running
        // proxy after a restart) has nothing tracking it, so a stop cancelled
        // partway through would leave its relay with no record at all.
        for target in &targets {
            record_target(
                id,
                target.config.clone(),
                target.uncertain_until,
                target.cluster,
                target.local,
            );
        }

        let mut errors: Vec<String> = Vec::new();
        let mut settled: Vec<Config> = Vec::new();
        // Targets whose create was never answered, so an empty list is not
        // proof that nothing was created.
        let unanswered: Vec<Config> = targets
            .iter()
            .filter(|target| target.uncertain_until.is_some())
            .map(|target| target.config.clone())
            .collect();
        let now = Instant::now();
        for target in &targets {
            let cluster = if target.cluster {
                delete_cluster_resources(id, &target.config).await
            } else {
                Ok(())
            };
            // Local resources are released per target too: an edited row can
            // name a different loopback address than the one still bound.
            let local = if target.local {
                release_local_resources(id, &target.config).await
            } else {
                LocalCleanup::default()
            };
            let uncertain = target.is_uncertain(now);
            if cluster.is_ok() && local.settled() && !uncertain {
                settled.push(target.config.clone());
                continue;
            }

            // The only remaining record of where these resources live: the
            // database row can be edited or deleted while a forward runs.
            // An uncertain create keeps its cluster obligation: the list that
            // came back empty is not proof, so the next pass must look again.
            set_target_obligations(
                id,
                &target.config,
                target.uncertain_until,
                target.cluster && (cluster.is_err() || uncertain),
                target.local && !local.settled(),
            );
            if let Err(error) = cluster {
                SHARED_CLIENT_MANAGER.invalidate_client(&ServiceClientKey::new(
                    target.config.context.clone(),
                    target.config.kubeconfig.clone(),
                ));
                errors.push(error);
            } else if uncertain {
                // An abandoned create may still be persisting, so one empty
                // list is not proof.
                errors.push(format!(
                    "A create request for config {id} was never answered, so its cluster resources \
                     are still being reconciled"
                ));
            }
            errors.extend(local.failures);
            // A loopback alias that needs the privileged helper is not a reason
            // to keep reporting the forward as running: it is already gone. The
            // record above keeps the alias retryable.
            for deferred in local.deferred {
                warn!("Config {id} stopped with cleanup still pending: {deferred}");
            }
        }
        for target in settled {
            // A create whose outcome was observed is settled by this pass. One
            // that was never answered is not: the server can still admit it, so
            // its record outlives a single empty list and goes only once a
            // later pass has confirmed it again.
            let answered = !unanswered.contains(&target);
            if answered || confirm_uncertain_target(id, &target).await {
                forget_uncertain_target(id, &target).await;
                forget_pending_cleanup(id, &target);
                continue;
            }
            // Still unconfirmed: the obligation stays so the next pass looks
            // again, rather than leaving the durable record with nothing in
            // this process to act on it.
            set_target_obligations(id, &target, None, true, false);
        }
        if errors.is_empty() {
            let state = ConfigState::new(id, false);
            if let Err(error) = update_config_state_with_mode(&state, mode).await {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(stop_response(id, Some(config), None))
        } else {
            Err(errors.join("; "))
        }
    } else if existed {
        Err(format!(
            "Stopped the local process for config {id} but could not load its configuration, so \
             cluster resources, loopback addresses and host entries were left in place"
        ))
    } else {
        Err(format!(
            "No port forwarding process found for config_id '{id}'"
        ))
    };
    drop(guard);
    drop(lock);
    crate::kube::proxy_recovery::remove_recovery_lock(id);
    result
}

fn stop_response(id: i64, config: Option<&Config>, error: Option<String>) -> CustomResponse {
    CustomResponse {
        id: Some(id),
        service: config
            .and_then(|config| config.service.clone())
            .unwrap_or_default(),
        namespace: config
            .map(|config| config.namespace.clone())
            .unwrap_or_default(),
        local_port: config
            .and_then(|config| config.local_port)
            .unwrap_or_default(),
        remote_port: config
            .and_then(|config| config.remote_port)
            .unwrap_or_default(),
        context: config
            .and_then(|config| config.context.clone())
            .unwrap_or_default(),
        protocol: config
            .map(|config| config.protocol.clone())
            .unwrap_or_default(),
        stdout: if error.is_none() {
            "Port forwarding has been stopped".to_string()
        } else {
            String::new()
        },
        status: i32::from(error.is_some()),
        stderr: error.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::{
        TcpListener,
        UdpSocket,
    };

    use super::*;

    /// Mirrors the snapshot `start_config` stores: a plain local forward with
    /// no cluster resources, loopback address or host entry to release.
    fn local_config(id: i64) -> Config {
        Config {
            id: Some(id),
            namespace: "default".to_string(),
            service: Some("test-service".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            ..Config::default()
        }
    }

    #[tokio::test]
    async fn stop_releases_listener_and_websocket_owner_before_returning() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_011;
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp_address = tcp.local_addr().unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_address = udp.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _tcp = tcp;
            std::future::pending::<anyhow::Result<()>>().await
        });
        let websocket = tokio::spawn(async move {
            let _udp = udp;
            std::future::pending::<()>().await;
        });
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_ws_client_handle(websocket);
        process.set_config(local_config(id));
        CHILD_PROCESSES.insert(id, process);

        stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .unwrap();

        let _tcp = TcpListener::bind(tcp_address).await.unwrap();
        let _udp = UdpSocket::bind(udp_address).await.unwrap();
    }

    #[tokio::test]
    async fn stop_uses_the_retained_config_when_the_database_lookup_fails() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_051;
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_config(Config {
            id: Some(id),
            workload_type: Some("proxy".to_string()),
            protocol: "tcp".to_string(),
            namespace: "default".to_string(),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/isolated-test-kubeconfig".to_string()),
            ..Config::default()
        });
        CHILD_PROCESSES.insert(id, process);

        let error = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .expect_err("cluster cleanup must be attempted from the retained config");

        assert!(!CHILD_PROCESSES.contains_key(&id));
        assert!(
            !error.contains("could not load its configuration"),
            "the retained snapshot should have supplied the cleanup metadata: {error}"
        );
    }

    #[tokio::test]
    async fn stop_all_releases_every_transport_in_memory_mode() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mut addresses = Vec::new();
        for id in [410_021, 410_022] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            addresses.push(listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                let _listener = listener;
                std::future::pending::<anyhow::Result<()>>().await
            });
            let mut process = PortForwardProcess::new(task, id.to_string());
            process.set_config(local_config(id));
            CHILD_PROCESSES.insert(id, process);
        }
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_address = udp.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _udp = udp;
            std::future::pending::<anyhow::Result<()>>().await
        });
        let mut process = PortForwardProcess::new(task, "410023".to_string());
        process.set_config(local_config(410_023));
        CHILD_PROCESSES.insert(410_023, process);

        let responses = stop_all_port_forward_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        for id in [410_021, 410_022, 410_023] {
            assert!(
                responses
                    .iter()
                    .any(|response| response.id == Some(id) && response.status == 0)
            );
        }
        for address in addresses {
            let _listener = TcpListener::bind(address).await.unwrap();
        }
        let _udp = UdpSocket::bind(udp_address).await.unwrap();
    }

    #[tokio::test]
    async fn failed_cluster_cleanup_keeps_the_config_marked_running() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = Config {
            namespace: "default".to_string(),
            service: Some("expose-target".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("expose".to_string()),
            ..Config::default()
        };
        let id =
            kftray_commons::utils::config::insert_config_with_mode(config, DatabaseMode::Memory)
                .await
                .unwrap();
        update_config_state_with_mode(&ConfigState::new(id, true), DatabaseMode::Memory)
            .await
            .unwrap();
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        CHILD_PROCESSES.insert(id, PortForwardProcess::new(task, id.to_string()));

        let error = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .expect_err("an unreachable cluster must fail the stop");
        assert!(!error.is_empty());

        let states = get_configs_state_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert!(
            states
                .iter()
                .any(|state| state.config_id == id && state.is_running),
            "orphaned cluster resources must keep the config retryable"
        );
    }

    #[tokio::test]
    async fn a_settled_create_keeps_its_local_cleanup_recorded() {
        let id = -9_312;
        PENDING_CLEANUP.remove(&id);
        let config = Config {
            id: Some(id),
            namespace: "prod".to_string(),
            service: Some("relay".to_string()),
            local_address: Some("127.0.44.1".to_string()),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };

        // A failed startup owes a loopback release.
        record_pending_cleanup(id, config.clone());
        // The same resources are then armed and deleted as a cluster target.
        ClusterResourceGuard::arm(id, config.clone())
            .await
            .unwrap()
            .disarm()
            .await;

        let targets = pending_cleanup_targets(id);
        assert_eq!(targets.len(), 1);
        assert!(
            targets[0].local,
            "deleting the deployment must not discard the address still bound"
        );
        assert!(!targets[0].cluster);
        PENDING_CLEANUP.remove(&id);
    }

    #[test]
    fn a_startup_can_release_the_address_it_claimed() {
        let address = "127.0.57.1";
        let claim = AddressClaim::take(address, Some(11)).expect("a free address can be claimed");

        // Its own rollback: the claim exists precisely because this startup
        // allocated the address, so it must not block taking it back.
        let releasing = ReleaseInFlight::mark(address, Some(11), |_| false);
        assert!(
            releasing.is_some(),
            "a startup must be able to release what it allocated"
        );
        // Another configuration's claim still blocks it.
        assert!(
            ReleaseInFlight::mark(address, Some(12), |_| false).is_none(),
            "a release for another configuration must not take a claimed address"
        );
        drop(releasing);
        drop(claim);
    }

    #[test]
    fn a_claimed_address_cannot_be_released_underneath_its_startup() {
        let address = "127.0.56.1";
        let claim = AddressClaim::take(address, Some(7)).expect("a free address can be claimed");

        assert!(
            ReleaseInFlight::mark(address, None, |_| false).is_none(),
            "an abandoned allocation must not remove an address a startup is using"
        );

        drop(claim);
        let releasing =
            ReleaseInFlight::mark(address, None, |_| false).expect("the address is free again");
        assert!(
            AddressClaim::take(address, Some(8)).is_none(),
            "a startup must not adopt an address mid-removal"
        );
        drop(releasing);
        assert!(AddressClaim::take(address, Some(8)).is_some());
    }

    #[test]
    fn an_address_stays_marked_until_every_release_finishes() {
        let address = "127.0.55.1";
        let first = ReleaseInFlight::mark(address, None, |_| false);
        // A stop that timed out leaves its release running; the next attempt
        // marks the same address again.
        let second = ReleaseInFlight::mark(address, None, |_| false);

        drop(second);
        assert!(
            address_release_in_flight(address),
            "the first release is still executing and can remove the alias"
        );

        drop(first);
        assert!(!address_release_in_flight(address));
    }

    #[tokio::test]
    async fn a_failed_cleanup_keeps_its_snapshot_after_the_config_is_edited() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = Config {
            namespace: "original-namespace".to_string(),
            service: Some("proxy-target".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };
        let id = kftray_commons::utils::config::insert_config_with_mode(
            config.clone(),
            DatabaseMode::Memory,
        )
        .await
        .unwrap();
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_config(Config {
            id: Some(id),
            ..config
        });
        CHILD_PROCESSES.insert(id, process);

        stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .expect_err("an unreachable cluster must fail the stop");

        // The row is gone and the process is no longer registered, so the
        // retained snapshot is the only thing that still knows the namespace.
        kftray_commons::utils::config::delete_config_with_mode(id, DatabaseMode::Memory)
            .await
            .unwrap();
        let pending = pending_cleanup_targets(id);
        assert_eq!(
            pending.len(),
            1,
            "a failed cleanup must keep its snapshot for the next stop"
        );
        assert_eq!(pending[0].config.namespace, "original-namespace");

        let responses = stop_all_port_forward_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert!(
            responses.iter().any(|response| response.id == Some(id)),
            "stop-all must retry a configuration whose cleanup never finished"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn a_dropped_startup_leaves_its_resources_for_stop_all() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_130;
        let config = Config {
            id: Some(id),
            namespace: "default".to_string(),
            service: Some("kftray-forward-abc".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };

        // Models a startup future dropped after it created cluster resources:
        // its own rollback never runs, so the trail is all stop-all has.
        record_pending_cleanup(id, config);

        let responses = stop_all_port_forward_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert!(
            responses.iter().any(|response| response.id == Some(id)),
            "stop-all must reach resources left behind by a dropped startup"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn an_uncertain_create_survives_an_empty_cleanup_pass() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_160;
        let config = Config {
            id: Some(id),
            namespace: "default".to_string(),
            service: Some("kftray-forward-pending".to_string()),
            protocol: "tcp".to_string(),
            // A plain service forward: cluster cleanup is a successful no-op,
            // which is exactly the "empty list" the create could outlive.
            workload_type: Some("service".to_string()),
            ..Config::default()
        };
        let guard = ClusterResourceGuard::arm(id, config.clone()).await.unwrap();
        // Dropped without `confirm`: the create request was abandoned, so the
        // resource may still be persisting.
        drop(guard);

        let _ = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory).await;

        assert!(
            pending_cleanup_targets(id)
                .iter()
                .any(|target| target.config.service.as_deref() == Some("kftray-forward-pending")),
            "an unconfirmed create must not be forgotten on one empty pass"
        );

        // Once the create's outcome is observed, the same pass settles it.
        let mut guard = ClusterResourceGuard::arm(id, config.clone()).await.unwrap();
        guard.confirm();
        std::mem::forget(guard);
        let _ = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory).await;
        assert!(
            pending_cleanup_targets(id).is_empty(),
            "a confirmed target is forgotten once cleanup succeeds"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn a_restart_after_failed_cleanup_keeps_both_resource_sets() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_140;
        let orphaned = Config {
            id: Some(id),
            namespace: "old-namespace".to_string(),
            service: Some("kftray-forward-old".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };
        record_pending_cleanup(id, orphaned.clone());

        // The config was edited and restarted as a plain TCP forward, whose own
        // cleanup is a no-op and must not discard the orphaned proxy.
        let restarted = Config {
            id: Some(id),
            namespace: "new-namespace".to_string(),
            service: Some("plain-service".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            ..Config::default()
        };
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_config(restarted);
        CHILD_PROCESSES.insert(id, process);

        let stopped = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory).await;

        assert!(
            stopped.is_err(),
            "the orphaned proxy must still be attempted, not silently skipped"
        );
        let still_pending = pending_cleanup_targets(id);
        assert!(
            still_pending
                .iter()
                .any(|target| target.config.namespace == "old-namespace"),
            "cleaning the restarted config must not forget the earlier resources"
        );
        PENDING_CLEANUP.remove(&id);
    }
}
