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
            running_snapshot,
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

use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};
use crate::port_forward::CHILD_PROCESSES;
#[cfg(test)]
use crate::port_forward::PortForwardProcess;

/// One tracked cleanup target.
#[derive(Clone, Debug)]
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
    /// The API server the resources were created on, as resolved at the time.
    /// The row names a context and a kubeconfig, and both can come to mean
    /// another server before cleanup runs; cleanup must not delete on that one
    /// and call the obligation settled.
    destination: Option<String>,
}

impl PendingTarget {
    fn is_uncertain(&self, now: Instant) -> bool {
        self.uncertain_until.is_some_and(|until| now < until)
    }

    /// Whether this record may describe the given resources.
    ///
    /// The destination is part of the identity: a configuration names a
    /// context and a kubeconfig, and both can come to mean another server
    /// between two attempts, so resources created on each are different
    /// obligations. A side that never resolved its destination matches any:
    /// it describes the same rows and cannot say which server they reached.
    /// Used to find a record; clearing one goes through [`Self::is_exactly`].
    fn describes(&self, config: &Config, destination: Option<&str>) -> bool {
        same_resources(&self.config, config)
            && match (self.destination.as_deref(), destination) {
                (Some(left), Some(right)) => left == right,
                _ => true,
            }
    }

    /// Whether this record is the one for exactly these resources on exactly
    /// this destination. A record with no destination is only matched by
    /// none: clearing an obligation for "wherever" must not clear the one
    /// for a server that is known.
    fn is_exactly(&self, config: &Config, destination: Option<&str>) -> bool {
        same_resources(&self.config, config) && self.destination.as_deref() == destination
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

lazy_static::lazy_static! {
    /// How many stops have acted on each id in this process. A caller that
    /// stops a forward in order to start it again reads this before and
    /// after, so a stop that lands in between (the user stopping the same
    /// forward) is noticed and the restart is dropped rather than undoing it.
    static ref STOP_GENERATIONS: dashmap::DashMap<i64, u64> = dashmap::DashMap::new();
}

/// The number of stops that have acted on `id` in this process so far.
pub fn stop_generation(id: i64) -> u64 {
    STOP_GENERATIONS.get(&id).map_or(0, |count| *count)
}

/// How long a lifecycle operation waits for another process to finish with
/// the same configuration before giving up. A start elsewhere can hold it
/// through relay readiness, so this is generous without being unbounded.
pub(crate) const SHARED_LOCK_WAIT: Duration = Duration::from_secs(30);

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
        // The alias names the hosts entries cleanup has to verify gone. Two
        // snapshots that differ only there describe different lines, and
        // merging them would let a restart under the new alias discard the
        // record of the old one while it still resolves.
        && left.alias == right.alias
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
/// What a durable cleanup record holds.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedTarget {
    config: Config,
    #[serde(default)]
    destination: Option<String>,
}

async fn persist_uncertain_target(
    id: i64, config: &Config, destination: Option<&str>, mode: DatabaseMode,
) -> Result<(), String> {
    let serialized = serde_json::to_string(&PersistedTarget {
        config: config.clone(),
        destination: destination.map(ToOwned::to_owned),
    })
    .map_err(|error| format!("Failed to describe the cleanup metadata for config {id}: {error}"))?;
    let key = uncertain_create_key(id, config, destination, mode);
    // A fresh attempt starts its confirmation sequence from scratch: a count
    // left by an earlier attempt at the same destination would let this one be
    // forgotten after a single pass.
    if let Err(error) = kftray_commons::utils::settings::delete_setting_with_mode(
        &confirmation_key(id, config, destination, mode),
        mode,
    )
    .await
    {
        log::debug!("Failed to reset the confirmation count for config {id}: {error}");
    }

    kftray_commons::utils::settings::set_setting_with_mode(&key, &serialized, mode)
        .await
        .map_err(|error| format!("Failed to persist an unsettled create for config {id}: {error}"))
}

/// Counts one pass that found nothing, and reports whether the record has now
/// been confirmed often enough to drop.
///
/// A create whose outcome was never answered is not settled by elapsed client
/// time: the server can still admit it. The budget bounds how long this keeps
/// costing a list, without turning one empty result into proof.
async fn confirm_uncertain_target(
    id: i64, config: &Config, destination: Option<&str>, mode: DatabaseMode,
) -> bool {
    const CONFIRMATIONS_REQUIRED: u32 = 2;

    let key = confirmation_key(id, config, destination, mode);
    // Atomic: stop-all (buffer_unordered 16) and reconcile can both reach this
    // target in the same pass, and a separate read-then-write here would let
    // two callers both observe the same count and either never reach
    // CONFIRMATIONS_REQUIRED or confirm after a single pass.
    let seen = match kftray_commons::utils::settings::increment_setting_with_mode(&key, mode).await
    {
        Ok(seen) => seen,
        Err(error) => {
            warn!("Failed to record a cleanup confirmation for config {id}: {error}");
            return false;
        }
    };
    if seen >= CONFIRMATIONS_REQUIRED {
        if let Err(error) =
            kftray_commons::utils::settings::delete_setting_with_mode(&key, mode).await
        {
            log::debug!("Failed to clear the confirmation count for config {id}: {error}");
        }
        return true;
    }

    false
}

/// Forgets a persisted create once its resources are confirmed gone.
///
/// Awaited like the write it undoes: two detached tasks have no ordering, and a
/// late delete would erase a newer attempt's record.
/// A record that could not be deleted is reported: a caller that dropped
/// its in-memory obligation on the strength of a swallowed failure would see
/// the next stop restore the record with a fresh window and a cluster
/// obligation for resources already gone.
async fn forget_uncertain_target(
    id: i64, config: &Config, destination: Option<&str>, mode: DatabaseMode,
) -> Result<(), String> {
    for key in [
        uncertain_create_key(id, config, destination, mode),
        confirmation_key(id, config, destination, mode),
    ] {
        kftray_commons::utils::settings::delete_setting_with_mode(&key, mode)
            .await
            .map_err(|error| {
                format!("Failed to clear the durable cleanup record for config {id}: {error}")
            })?;
    }
    Ok(())
}

/// Key under which a target's confirmation count is kept.
///
/// Deliberately a different prefix: the restore scan reads every key under the
/// create prefix as a target, and a counter is not one.
fn confirmation_key(
    id: i64, config: &Config, destination: Option<&str>, mode: DatabaseMode,
) -> String {
    format!(
        "uncertain_confirmations:{}",
        uncertain_create_key(id, config, destination, mode)
            .strip_prefix(UNCERTAIN_CREATE_PREFIX)
            .unwrap_or_default()
    )
}

/// Identifies one configuration's resources, so two different targets for the
/// same id do not overwrite each other.
fn uncertain_create_key(
    id: i64, config: &Config, destination: Option<&str>, mode: DatabaseMode,
) -> String {
    // The identity matches `PendingTarget::describes` field for field: targets
    // the registry tracks separately must not share a key, or settling one
    // would delete another's restart metadata.
    let digest = stable_digest(&[
        destination,
        Some(config.namespace.as_str()),
        config.context.as_deref(),
        config.kubeconfig.as_deref(),
        config.workload_type.as_deref(),
        Some(config.protocol.as_str()),
        config.service.as_deref(),
        config.local_address.as_deref(),
        config.domain_enabled.map(|on| if on { "1" } else { "0" }),
        config.alias.as_deref(),
        config.exposure_type.as_deref(),
    ]);

    // Scoped by database: an in-memory session's ids mean nothing to the file
    // database, and restoring one there would delete another configuration's
    // resources.
    let scope = match mode {
        DatabaseMode::File => "file",
        DatabaseMode::Memory => "memory",
    };

    format!("{UNCERTAIN_CREATE_PREFIX}{scope}:{id}:{digest:016x}")
}

/// Reloads creates persisted by an earlier run into the cleanup registry.
async fn restore_uncertain_targets(mode: DatabaseMode) {
    let stored = match kftray_commons::utils::settings::get_settings_with_prefix_and_mode(
        UNCERTAIN_CREATE_PREFIX,
        mode,
    )
    .await
    {
        Ok(stored) => stored,
        Err(error) => {
            warn!("Failed to read unsettled creates: {error}");
            return;
        }
    };
    let scope = match mode {
        DatabaseMode::File => "file:",
        DatabaseMode::Memory => "memory:",
    };
    for (key, value) in stored {
        // Only this database's records: an id means something different in the
        // other one, and acting on it would delete an unrelated relay.
        let Some(id) = key
            .strip_prefix(UNCERTAIN_CREATE_PREFIX)
            .and_then(|rest| rest.strip_prefix(scope))
            .and_then(|rest| rest.split(':').next())
            .and_then(|id| id.parse::<i64>().ok())
        else {
            continue;
        };
        // Records from before the destination was kept hold a bare
        // configuration; they are read as one with no destination.
        let (config, destination) = match serde_json::from_str::<PersistedTarget>(&value) {
            Ok(persisted) => (persisted.config, persisted.destination),
            Err(_) => match serde_json::from_str::<Config>(&value) {
                Ok(config) => (config, None),
                Err(_) => continue,
            },
        };
        // Restored with a fresh window: a process can restart seconds after
        // abandoning a create, and elapsed wall time is no more proof here than
        // it was in the run that recorded it. An entry that already tracks
        // this cluster obligation keeps the window it has, so repeated
        // restores cannot push it forever. A record for only the local
        // resources of the same rows is not that: a startup in progress can
        // have made one, and skipping the restore for it would leave the next
        // stop with no cluster obligation for a relay that still exists.
        if PENDING_CLEANUP.get(&id).is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.cluster && entry.is_exactly(&config, destination.as_deref()))
        }) {
            continue;
        }
        record_target(
            id,
            config,
            Some(Instant::now() + UNCERTAIN_CREATE_WINDOW),
            true,
            false,
            destination,
        );
    }
    restore_local_cleanup_targets(mode).await;
}

/// Key prefix under which a local cleanup obligation (a loopback alias, hosts
/// entries, or both) is persisted, the same way an uncertain cluster create
/// is, so a restart still finds resources this process never got to release.
const LOCAL_CLEANUP_PREFIX: &str = "local_cleanup:";

/// One key per id, not per resource digest like a cluster target: a
/// configuration only ever holds one loopback alias and one set of hosts
/// entries at a time, so the latest local obligation recorded for an id is
/// the only one worth surviving a restart.
fn local_cleanup_key(id: i64, mode: DatabaseMode) -> String {
    let scope = match mode {
        DatabaseMode::File => "file",
        DatabaseMode::Memory => "memory",
    };
    format!("{LOCAL_CLEANUP_PREFIX}{scope}:{id}")
}

/// Reloads local cleanup obligations persisted by an earlier run into the
/// cleanup registry, the same way [`restore_uncertain_targets`] does for
/// cluster ones.
async fn restore_local_cleanup_targets(mode: DatabaseMode) {
    let stored = match kftray_commons::utils::settings::get_settings_with_prefix_and_mode(
        LOCAL_CLEANUP_PREFIX,
        mode,
    )
    .await
    {
        Ok(stored) => stored,
        Err(error) => {
            warn!("Failed to read unsettled local cleanup obligations: {error}");
            return;
        }
    };
    let scope = match mode {
        DatabaseMode::File => "file:",
        DatabaseMode::Memory => "memory:",
    };
    for (key, value) in stored {
        let Some(id) = key
            .strip_prefix(LOCAL_CLEANUP_PREFIX)
            .and_then(|rest| rest.strip_prefix(scope))
            .and_then(|id| id.parse::<i64>().ok())
        else {
            continue;
        };
        let Ok(config) = serde_json::from_str::<Config>(&value) else {
            continue;
        };
        // Already tracked, e.g. by an allocation still in flight in this same
        // process: restoring again would only duplicate the entry.
        if PENDING_CLEANUP.get(&id).is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.local && entry.is_exactly(&config, None))
        }) {
            continue;
        }
        record_target(id, config, None, false, true, None);
    }
}

/// The process id another live kftray process recorded for this row, when
/// it is forwarding it. Only the file database is shared between processes;
/// a row whose recorded process no longer exists is left to be cleaned up.
pub(crate) async fn running_in_another_process(id: i64, mode: DatabaseMode) -> Option<u32> {
    if mode != DatabaseMode::File {
        return None;
    }
    let states = get_configs_state_with_mode(mode).await.ok()?;
    states
        .into_iter()
        .find(|state| state.config_id == id && state.is_running)
        .and_then(|state| state.process_id)
        .filter(|pid| {
            *pid != std::process::id()
                && kftray_commons::utils::config_state::process_is_alive(*pid)
        })
}

/// The configurations still forwarding, as they were when they started: the
/// ones in this process, and, for the file database, the ones another
/// process sharing it reports as running. What they own must not be removed
/// on another's behalf. The in-memory database belongs to one process, so it
/// has nothing to add.
pub(crate) async fn forwarding_configs(mode: DatabaseMode) -> Vec<Config> {
    let mut configs: Vec<Config> = CHILD_PROCESSES
        .iter()
        .filter_map(|entry| entry.value().config().cloned())
        .collect();
    if mode != DatabaseMode::File {
        return configs;
    }
    let this_process = std::process::id();
    let elsewhere =
        match kftray_commons::utils::config_state::get_configs_state_with_mode(mode).await {
            Ok(states) => states,
            Err(error) => {
                warn!("Failed to read which configurations other processes are running: {error}");
                return configs;
            }
        };
    for state in elsewhere.into_iter().filter(|state| {
        state.is_running
            && state.process_id.is_some_and(|pid| {
                pid != this_process && kftray_commons::utils::config_state::process_is_alive(pid)
            })
    }) {
        // The snapshot it started with, not the row: the row can be edited
        // while the forward runs, and the address and aliases it actually
        // holds are the ones recorded when it registered. A row without one
        // was started by a version that kept none, and is the best there is.
        let snapshot =
            kftray_commons::utils::config_state::running_snapshot(state.config_id, mode).await;
        match snapshot {
            Some(config) => configs.push(config),
            None => {
                if let Ok(config) = get_config_with_mode(state.config_id, mode).await {
                    configs.push(config);
                }
            }
        }
    }
    configs
}

/// Records a configuration whose resources exist but whose startup did not
/// finish, so stop-all still reaches them. Dropping a startup future (the
/// terminal's shutdown drain, an aborted task) skips its own rollback.
#[cfg(test)]
pub(crate) fn record_pending_cleanup(id: i64, config: Config) {
    record_target(id, config, None, true, true, None);
}

/// Records local resources a startup holds: a loopback alias or hosts entries
/// that exist before, or without, anything in the cluster. Nothing here says
/// where in the cluster anything is, so the record carries no destination and
/// no cluster obligation. Persisted the same way an uncertain cluster create
/// is, so a restart still finds the address and hosts entries this process
/// never got to release.
pub(crate) async fn record_local_cleanup(id: i64, config: Config, mode: DatabaseMode) {
    record_target(id, config.clone(), None, false, true, None);
    match serde_json::to_string(&config) {
        Ok(serialized) => {
            if let Err(error) = kftray_commons::utils::settings::set_setting_with_mode(
                &local_cleanup_key(id, mode),
                &serialized,
                mode,
            )
            .await
            {
                warn!("Failed to persist a local cleanup obligation for config {id}: {error}");
            }
        }
        Err(error) => {
            warn!("Failed to describe a local cleanup obligation for config {id}: {error}");
        }
    }
}

/// Marks the local resources for these rows as released, on every server
/// they were recorded for. Loopback aliases and hosts entries do not depend on
/// which server the rows reached, so one release settles them all; a record
/// that still owes cluster cleanup keeps that. Forgets the durable record
/// too: nothing is left for a restart to find.
pub(crate) async fn settle_local_cleanup(id: i64, config: &Config, mode: DatabaseMode) {
    if let Some(mut entries) = PENDING_CLEANUP.get_mut(&id) {
        for entry in entries
            .iter_mut()
            .filter(|entry| same_resources(&entry.config, config))
        {
            entry.local = false;
        }
        entries.retain(|entry| entry.cluster || entry.local);
    }
    PENDING_CLEANUP.remove_if(&id, |_, entries| entries.is_empty());
    if let Err(error) = kftray_commons::utils::settings::delete_setting_with_mode(
        &local_cleanup_key(id, mode),
        mode,
    )
    .await
    {
        log::debug!(
            "Failed to clear the persisted local cleanup obligation for config {id}: {error}"
        );
    }
}

fn record_target(
    id: i64, config: Config, uncertain_until: Option<Instant>, cluster: bool, local: bool,
    destination: Option<String>,
) {
    let mut entries = PENDING_CLEANUP.entry(id).or_default();
    if let Some(existing) = entries
        .iter_mut()
        .find(|entry| entry.describes(&config, destination.as_deref()))
    {
        existing.uncertain_until = match (existing.uncertain_until, uncertain_until) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        existing.cluster |= cluster;
        existing.local |= local;
        if existing.destination.is_none() {
            existing.destination = destination;
        }
        return;
    }
    entries.push(PendingTarget {
        config,
        uncertain_until,
        cluster,
        local,
        destination,
    });
}

/// Drops one recorded target, leaving any other resources for this id tracked.
///
/// Exact: a record for a known server is only dropped by naming that server.
pub(crate) fn forget_pending_cleanup(id: i64, config: &Config, destination: Option<&str>) {
    if let Some(mut entries) = PENDING_CLEANUP.get_mut(&id) {
        entries.retain(|entry| !entry.is_exactly(config, destination));
    }
    // Removed only while still empty: allocation tasks record targets outside
    // the lifecycle lock, so one can arrive between the retain above and this
    // call, and an unconditional remove would discard it.
    PENDING_CLEANUP.remove_if(&id, |_, entries| entries.is_empty());
}

/// Forgets the cluster obligation kept for `config_id` at `destination`,
/// once its resources have been removed by hand from the server-resources
/// screen. Local obligations for the same id are left untouched: hand-editing
/// hosts or a loopback alias is not something that screen does.
///
/// Exact, like [`forget_pending_cleanup`]: a record for a known destination
/// is only cleared by naming that destination, so an obligation for a server
/// the resource was not deleted from stays in place.
///
/// Awaits the durable delete before clearing the in-memory record, so a
/// failed durable delete leaves the obligation in place instead of letting
/// `restore_uncertain_targets` resurrect it after the in-memory record was
/// already cleared. The in-memory record is what `delete_configs_if_idle`
/// and stop-all actually consult.
pub async fn settle_cluster_obligation(
    config_id: i64, destination: &str, mode: DatabaseMode,
) -> Result<(), String> {
    let matching: Vec<PendingTarget> = pending_cleanup_targets(config_id)
        .into_iter()
        .filter(|target| target.cluster && target.destination.as_deref() == Some(destination))
        .collect();
    for target in matching {
        forget_uncertain_target(
            config_id,
            &target.config,
            target.destination.as_deref(),
            mode,
        )
        .await
        .map_err(|error| {
            format!("Failed to clear the durable cleanup record for config {config_id}: {error}")
        })?;
        set_target_obligations(config_id, &target, None, false, target.local);
    }
    Ok(())
}

/// Records what this pass left undone, replacing the entry's obligations
/// rather than adding to them.
///
/// [`record_target`] widens an existing record, which is right for a new
/// obligation but wrong here: cleanup that succeeded would be demanded again on
/// every later stop.
fn set_target_obligations(
    id: i64, target: &PendingTarget, uncertain_until: Option<Instant>, cluster: bool, local: bool,
) {
    if !cluster && !local {
        forget_pending_cleanup(id, &target.config, target.destination.as_deref());
        return;
    }
    let mut entries = PENDING_CLEANUP.entry(id).or_default();
    if let Some(existing) = entries
        .iter_mut()
        .find(|entry| entry.is_exactly(&target.config, target.destination.as_deref()))
    {
        existing.uncertain_until = uncertain_until;
        existing.cluster = cluster;
        existing.local = local;
        return;
    }
    entries.push(PendingTarget {
        config: target.config.clone(),
        uncertain_until,
        cluster,
        local,
        destination: target.destination.clone(),
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
    /// Database the configuration id belongs to. Ids are only meaningful within
    /// one, so the durable record is scoped by it.
    mode: DatabaseMode,
    destination: Option<String>,
    /// Uncertainty the record already carried when this guard was armed: an
    /// earlier attempt at the same target whose create was never answered.
    /// This attempt's outcome says nothing about that one, so confirming or
    /// disarming this guard leaves it in place.
    inherited_until: Option<Instant>,
}

impl ClusterResourceGuard {
    /// Awaits the durable record before returning, so the create it guards can
    /// only be issued once something outside this process knows about it.
    ///
    /// A failed write is an error rather than a warning: the caller would
    /// otherwise create a resource that a restart could never find.
    pub(crate) async fn arm(
        id: i64, config: Config, destination: Option<String>, mode: DatabaseMode,
    ) -> Result<Self, String> {
        // Expired or not: an expired window is not a settled create, it is
        // one whose confirmation passes have not run yet, and this attempt's
        // outcome does not stand in for them.
        let mut inherited_until = PENDING_CLEANUP.get(&id).and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.is_exactly(&config, destination.as_deref()))
                .and_then(|entry| entry.uncertain_until)
        });
        // The record may exist only on disk: a process that abandoned this
        // create and exited left it there, and this one has not restored it
        // yet. It is inherited the way a restore would take it, with a fresh
        // window, so a rejected retry cannot delete it as its own.
        if inherited_until.is_none()
            && kftray_commons::utils::settings::get_setting_with_mode(
                &uncertain_create_key(id, &config, destination.as_deref(), mode),
                mode,
            )
            .await
            .ok()
            .flatten()
            .is_some()
        {
            inherited_until = Some(Instant::now() + UNCERTAIN_CREATE_WINDOW);
        }
        persist_uncertain_target(id, &config, destination.as_deref(), mode).await?;
        record_target(
            id,
            config.clone(),
            Some(Instant::now() + UNCERTAIN_CREATE_WINDOW),
            true,
            false,
            destination.clone(),
        );

        Ok(Self {
            id,
            config: Some(config),
            confirmed: false,
            mode,
            destination,
            inherited_until,
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
                .find(|entry| entry.is_exactly(config, self.destination.as_deref()))
        {
            entry.uncertain_until = self.inherited_until;
        }
    }

    /// Nothing was created, so this guard's cluster obligation is settled. Any
    /// local cleanup recorded for the same resources stays tracked.
    pub(crate) async fn disarm(mut self) {
        let Some(config) = self.config.take() else {
            return;
        };
        let destination = self.destination.take();
        // An earlier attempt at this target may still be creating something:
        // this one settling with nothing created does not answer for it, so
        // its record, in memory and on disk, stays until its own passes do.
        if let Some(inherited) = self.inherited_until {
            self.confirmed = true;
            if let Some(mut entries) = PENDING_CLEANUP.get_mut(&self.id)
                && let Some(entry) = entries
                    .iter_mut()
                    .find(|entry| entry.is_exactly(&config, destination.as_deref()))
            {
                entry.uncertain_until = Some(inherited);
            }
            return;
        }
        // Definitively rejected, so nothing was created and the persisted
        // record has nothing left to describe.
        if let Err(error) =
            forget_uncertain_target(self.id, &config, destination.as_deref(), self.mode).await
        {
            // The in-memory obligation stays with the record on disk: a stop
            // retries the deletion, and until then a restart would find it.
            warn!("{error}");
            return;
        }
        if let Some(mut entries) = PENDING_CLEANUP.get_mut(&self.id) {
            for entry in entries
                .iter_mut()
                .filter(|entry| entry.is_exactly(&config, destination.as_deref()))
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
            record_target(
                self.id,
                config,
                uncertain_until,
                true,
                false,
                self.destination.take(),
            );
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
            drop(state);
            // `or_default` above may have just created this entry empty; left
            // behind, nothing else ever reclaims it.
            RELEASING_ADDRESSES.remove_if_mut(address, |_, state| {
                state.releases == 0 && state.claims.is_empty()
            });
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
            drop(state);
            // `or_default` above may have just created this entry empty; left
            // behind, nothing else ever reclaims it.
            RELEASING_ADDRESSES.remove_if_mut(address, |_, state| {
                state.releases == 0 && state.claims.is_empty()
            });
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

lazy_static::lazy_static! {
    /// The token of the attempt that currently owns a config id's hosts
    /// entries. Only ever overwritten by a newer claim; never removed by its
    /// own holder except when that holder's own deferred cleanup runs.
    static ref HOST_ENTRY_CLAIMS: dashmap::DashMap<i64, u64> = dashmap::DashMap::new();
}

static HOST_ENTRY_CLAIM_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Claims ownership of a config id's hosts entries for one startup attempt,
/// mirroring [`AddressClaim`] for symmetry.
///
/// A retry for the same id registers in `CHILD_PROCESSES` only at the very
/// end of its startup, so a deferred cleanup task from an earlier, abandoned
/// attempt cannot use that alone to tell a retry's already-written lines from
/// its own: taken before anything is written, a newer claim for the same id
/// silently replaces the older one, and the token returned here lets an
/// abandoned attempt's cleanup task recognize it has been superseded.
pub(crate) fn claim_host_entries(id: i64) -> u64 {
    let token = HOST_ENTRY_CLAIM_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    HOST_ENTRY_CLAIMS.insert(id, token);
    token
}

/// Whether `token` is still the most recent claim for a config id's hosts
/// entries. If so, clears it: the caller's deferred cleanup is about to run,
/// and nothing else needs to know this attempt claimed these entries once it
/// has. If a newer claim has since replaced it, the entry is left in place
/// and this reports `false`, so the caller leaves the newer attempt's lines
/// alone.
pub(crate) fn take_host_entry_claim_if_current(id: i64, token: u64) -> bool {
    let mut was_current = false;
    HOST_ENTRY_CLAIMS.remove_if(&id, |_, current| {
        was_current = *current == token;
        was_current
    });
    was_current
}

/// Whether `token` is still the most recent claim for a config id's hosts
/// entries, without clearing it. Used to decide whether a deferred write is
/// still worth doing before it runs, since taking the claim here would let a
/// newer attempt believe it owns entries nothing has written yet.
pub(crate) fn host_entry_claim_is_current(id: i64, token: u64) -> bool {
    HOST_ENTRY_CLAIMS
        .get(&id)
        .is_some_and(|current| *current == token)
}

/// Whether any attempt currently claims a config id's hosts entries,
/// regardless of which token holds it.
///
/// A deferred write whose own claim was superseded cannot tell from
/// [`host_entry_claim_is_current`] alone whether the newer attempt is still
/// in flight and will manage these lines itself, or has already finished or
/// given up and taken its own claim away. Only the latter leaves the lines
/// this write just landed with no one left to own them.
pub(crate) fn host_entry_has_any_claim(id: i64) -> bool {
    HOST_ENTRY_CLAIMS.contains_key(&id)
}

/// RAII wrapper around [`claim_host_entries`], so a startup that returns
/// early (selector resolution, address validation, a hosts write that never
/// lands) before registration still releases its claim instead of leaving
/// it for the next start of the same id to silently overwrite.
///
/// Left armed, [`Drop`] releases the claim exactly like the explicit
/// release call the non-deferred path used to make on its own. Call
/// [`disarm`](Self::disarm) once responsibility for the claim has been
/// handed off: the process is registered and the claim released right
/// there, or a deferred hosts write is about to run its own cleanup, which
/// manages the same claim from that point on.
pub(crate) struct HostsClaimGuard {
    id: i64,
    token: u64,
    armed: bool,
}

impl HostsClaimGuard {
    pub(crate) fn new(id: i64) -> Self {
        Self {
            id,
            token: claim_host_entries(id),
            armed: true,
        }
    }

    pub(crate) fn token(&self) -> u64 {
        self.token
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for HostsClaimGuard {
    fn drop(&mut self) {
        if self.armed {
            take_host_entry_claim_if_current(self.id, self.token);
        }
    }
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
    /// The loopback alias could not be removed, but a transient helper or
    /// platform failure may still succeed later. The forward itself is gone,
    /// so the stop is not held back, but the target stays recorded so a
    /// later stop retries it.
    deferred: Vec<String>,
    /// The loopback alias could not be removed because that needs
    /// privileges that are not available right now. Retrying automatically
    /// will not help without user action, so the target is not kept
    /// recorded for that reason; the message is surfaced once instead.
    unsatisfiable: Vec<String>,
}

impl LocalCleanup {
    fn settled(&self) -> bool {
        self.failures.is_empty() && self.deferred.is_empty()
    }
}

/// Releases the loopback address and host entries one target describes.
async fn release_local_resources(id: i64, config: &Config, mode: DatabaseMode) -> LocalCleanup {
    let mut cleanup = LocalCleanup::default();
    if let Some(address) = &config.local_address
        && crate::network_utils::is_custom_loopback_address(address)
        && let Err(error) = release_address_with_fallback(address, Some(id), mode).await
    {
        if error.is_unsatisfiable() {
            cleanup.unsatisfiable.push(error.to_string());
        } else {
            cleanup.deferred.push(error.to_string());
        }
    }
    let snapshot = config.clone();
    let in_use = forwarding_configs(mode).await;
    let hosts =
        crate::hostsfile::remove_config_host_entries(id, Some(&snapshot), &in_use, mode).await;
    match hosts {
        Ok(()) => {}
        Err(error) => cleanup.failures.push(error.to_string()),
    }
    cleanup
}

/// Deletes the cluster resources one configuration owns, under one deadline.
///
/// The whole operation is bounded, not just its polling: the client carries no
/// per-request timeout, so a server that accepts a request and never answers
/// would hold the lifecycle lock for as long as an interactive stop waits.
async fn delete_cluster_resources(
    id: i64, config: &Config, destination: Option<&str>, mode: DatabaseMode,
) -> Result<(), String> {
    // Comfortably above expose's own ROLLBACK_DELETION_TIMEOUT (45s): for an
    // expose workload this wraps a wait for earlier resources to disappear,
    // plus the delete and list requests around it.
    const CLUSTER_CLEANUP_TIMEOUT: Duration = Duration::from_secs(60);

    match timeout(
        CLUSTER_CLEANUP_TIMEOUT,
        delete_cluster_resources_inner(id, config, destination, mode),
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

/// Whether `config` describes resources this installation may have created
/// in the cluster: a public expose's Deployment/Service/Ingress, a proxy's
/// relay, or the pod-side half of a UDP tunnel. A plain TCP service/pod
/// forward never creates any, so nothing here ever needs cluster access.
fn config_has_cluster_resources(config: &Config) -> bool {
    matches!(
        config.workload_type.as_deref(),
        Some("expose") | Some("proxy")
    ) || config.protocol == "udp"
}

async fn delete_cluster_resources_inner(
    id: i64, config: &Config, destination: Option<&str>, mode: DatabaseMode,
) -> Result<(), String> {
    if !config_has_cluster_resources(config) {
        return Ok(());
    }
    let key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
    let connection = SHARED_CLIENT_MANAGER
        .get_connection(key)
        .await
        .map_err(|error| error.to_string())?;
    // The context and kubeconfig can resolve to another server than the one
    // the resources were created on: a kubeconfig rewritten in place, or a
    // current-context that moved. Deleting there finds nothing and would
    // settle an obligation whose resources still run elsewhere. Compared
    // through the same normalization the destination was recorded with, so a
    // scheme/host case difference or an explicit default port does not read
    // as a different server.
    let resolved = crate::kube::client::cluster_identity(&connection.cluster_url);
    if let Some(recorded) = destination
        && recorded != resolved
    {
        return Err(format!(
            "The resources for config {id} were created on {recorded}, but the context now \
             resolves to {resolved}; restore that kubeconfig or remove them from the server \
             resources screen"
        ));
    }
    if config.workload_type.as_deref() == Some("expose") {
        crate::expose::kubernetes::delete_expose_resources(
            connection.client.clone(),
            &config.namespace,
            &id.to_string(),
            config.exposure_type.as_deref() == Some("public"),
            &crate::expose::kubernetes::ExposeLocation::resolve(
                &connection.cluster_url,
                &config.namespace,
            ),
            mode,
        )
        .await
    } else {
        delete_proxy_cluster_resources(connection.client.clone(), &config.namespace, id, mode).await
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

/// Whether a helper release error means another live process still owns
/// this address, per kftray-helper's address pool (finding 9). A named
/// predicate rather than an inline string match, so the exact wording the
/// helper commits to is pinned and testable on its own.
fn address_owned_by_another_process(helper_error: &str) -> bool {
    helper_error.contains("owned by another live process")
}

/// Why a loopback address release did not complete.
pub(crate) enum AddressReleaseError {
    /// Retryable: a later stop may succeed where this one did not.
    Failed(String),
    /// Needs privileges that are not available right now (no elevated helper
    /// session, user declined, ...). Retrying automatically will not help
    /// without user action, so a caller must not keep this queued forever.
    PrivilegeUnavailable(String),
}

impl AddressReleaseError {
    pub(crate) fn is_unsatisfiable(&self) -> bool {
        matches!(self, Self::PrivilegeUnavailable(_))
    }
}

impl std::fmt::Display for AddressReleaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(message) | Self::PrivilegeUnavailable(message) => write!(f, "{message}"),
        }
    }
}

/// Release address with timeout. Skips osascript fallback to avoid blocking on
/// user interaction. Address cleanup is not critical - addresses will be freed
/// on system restart.
pub(crate) async fn release_address_with_fallback(
    address: &str, owner: Option<i64>, mode: DatabaseMode,
) -> Result<(), AddressReleaseError> {
    const ADDRESS_RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

    // Another process sharing the file database can hold the same address:
    // the helper hands one address to every configuration of a service,
    // whichever process asks, and a release removes the alias and the pool
    // reservation for all of them. Its persisted state is the only signal
    // this process has, read before the release is marked. A process that
    // starts between this read and the release can still lose the address;
    // closing that window needs the helper to count owners itself.
    if mode == DatabaseMode::File
        && forwarding_configs(mode)
            .await
            .iter()
            .any(|config| config.id != owner && config.local_address.as_deref() == Some(address))
    {
        log::debug!("Skipping the release of {address}: a forward elsewhere is using it");
        return Ok(());
    }

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

    // The mark above closes the same-process race `address_has_other_owner`
    // checks, but not the cross-process one: another process's forward can
    // register its row between the read above and this mark. Re-checked now,
    // immediately before anything is actually removed, so a release that lost
    // that race aborts instead of pulling the address out from under it.
    if mode == DatabaseMode::File
        && forwarding_configs(mode)
            .await
            .iter()
            .any(|config| config.id != owner && config.local_address.as_deref() == Some(address))
    {
        log::debug!("Skipping the release of {address}: a forward elsewhere is using it");
        return Ok(());
    }

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

    // The helper's address pool now tracks which live process allocated
    // each address (kftray-helper finding 9): a release refused because
    // another live process still owns it is not this release's failure,
    // it is proof the alias is still in use and must be left alone. Falling
    // through to the platform release below would remove it out from under
    // that sibling forward.
    if address_owned_by_another_process(&helper_error) {
        log::debug!("Skipping the release of {address}: {helper_error}");
        return Ok(());
    }

    // The helper is optional: a platform that binds the address without an
    // interface alias, or one where cleanup happens on restart, reports success
    // here and the stop can complete. Only a platform release that actually
    // fails keeps the cleanup record for a later retry.
    // Under the same deadline as the helper release: the platform release
    // already shells out its own blocking work, so awaiting it directly here
    // does not hold a runtime worker for the duration of the command.
    let platform = timeout(
        ADDRESS_RELEASE_TIMEOUT,
        crate::network_utils::remove_loopback_address(address),
    )
    .await;
    match platform {
        Ok(Ok(
            crate::network_utils::LoopbackRelease::Released
            | crate::network_utils::LoopbackRelease::AlreadyAbsent,
        )) => {
            warn!(
                "Released address {} without the helper ({}).",
                address, helper_error
            );
            Ok(())
        }
        Ok(Ok(crate::network_utils::LoopbackRelease::PrivilegeUnavailable(platform_error))) => {
            warn!(
                "Cannot release address {}: helper: {}; platform needs elevated privileges that \
                 are not available right now: {}",
                address, helper_error, platform_error
            );
            Err(AddressReleaseError::PrivilegeUnavailable(format!(
                "Releasing address {address} needs elevated privileges that are not available \
                 right now: {platform_error}"
            )))
        }
        Ok(Ok(crate::network_utils::LoopbackRelease::Failed(platform_error))) => {
            warn!(
                "Failed to release address {}: helper: {}; platform: {}",
                address, helper_error, platform_error
            );
            Err(AddressReleaseError::Failed(format!(
                "Failed to release address {address}: {helper_error}; {platform_error}"
            )))
        }
        Ok(Err(platform_error)) => {
            warn!(
                "Failed to release address {}: helper: {}; platform: {}",
                address, helper_error, platform_error
            );
            Err(AddressReleaseError::Failed(format!(
                "Failed to release address {address}: {helper_error}; {platform_error}"
            )))
        }
        Err(_) => Err(AddressReleaseError::Failed(format!(
            "Address release timed out for {address} after {ADDRESS_RELEASE_TIMEOUT:?}"
        ))),
    }
}

pub(crate) async fn delete_proxy_cluster_resources(
    client: Client, namespace: &str, config_id: i64, mode: DatabaseMode,
) -> Result<(), String> {
    let prefix = crate::kube::proxy::proxy_resource_prefix();
    let owned = ListParams::default()
        .labels(&crate::kube::proxy::proxy_owner_selector(&config_id.to_string(), mode).await?);
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

/// Configurations that must not be deleted: their forward is running, starting,
/// or still has resources waiting to be cleaned up.
///
/// Deleting the row does not stop anything, so the tunnel would keep running
/// with no configuration to stop it by.
pub async fn delete_configs_if_idle<F, Fut>(
    ids: &[i64], mode: DatabaseMode, delete: F,
) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    // Held across the delete: `start_config_locked` takes the same lock, so a
    // start cannot register between the check and the row disappearing, and a
    // start already waiting on the lock re-reads the row afterwards.
    //
    // Bounded by SHARED_LOCK_WAIT rather than awaited unboundedly: a start
    // holding this lock through its whole startup would otherwise block a UI
    // delete forever. `delete` itself must not re-enter a recovery lock or
    // `lock_config` for any of these ids; every guard here is already held for
    // the duration of its call.
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
        match timeout(SHARED_LOCK_WAIT, lock.lock_owned()).await {
            Ok(guard) => guards.push(guard),
            Err(_) => {
                return Err(format!(
                    "Configuration {id} is still starting or stopping; stop it before deleting"
                ));
            }
        }
    }
    // Then the cross-process locks, in the same order: a start in another
    // process holds its row's lock from its existence check through its
    // registration, so a delete cannot slip in between.
    let mut shared = Vec::with_capacity(ordered.len());
    for id in &ordered {
        shared.push(
            kftray_commons::utils::config_dir::lock_config(*id, mode, SHARED_LOCK_WAIT).await?,
        );
    }

    // Creates persisted by an earlier run are only in the database until a
    // stop or reconciliation reads them; a relay left by a crash is still a
    // reason not to delete the row that describes it.
    restore_uncertain_targets(mode).await;
    // Another process sharing this database can be forwarding the row: the
    // terminal and the desktop application both use the file database, and
    // neither sees the other's registries. Its persisted state is the only
    // signal, and it stays authoritative until that process clears it.
    let this_process = std::process::id();
    let running_elsewhere: HashSet<i64> = if mode == DatabaseMode::File {
        kftray_commons::utils::config_state::get_configs_state_with_mode(mode)
            .await?
            .into_iter()
            .filter(|state| {
                state.is_running
                    && state.process_id.is_some_and(|pid| {
                        pid != this_process
                            && kftray_commons::utils::config_state::process_is_alive(pid)
                    })
            })
            .map(|state| state.config_id)
            .collect()
    } else {
        HashSet::new()
    };

    // A pending record alone is not evidence the forward is live. Local
    // cleanup that needs the privileged helper can be permanently unsatisfiable
    // on this machine, and its record would then block deleting a row that is
    // already stopped, with no action the user could take to clear it. Only a
    // cluster obligation blocks: those name resources the deleted row is the
    // last description of.
    let active: Vec<i64> = ordered
        .iter()
        .copied()
        .filter(|id| {
            CHILD_PROCESSES.contains_key(id)
                || crate::kube::proxy::STARTING_PROXIES.contains_key(id)
                || crate::kube::proxy_recovery::recovery_in_progress(*id)
                || running_elsewhere.contains(id)
                || PENDING_CLEANUP
                    .get(id)
                    .is_some_and(|entries| entries.iter().any(|entry| entry.cluster))
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

/// Cancels every startup and recovery attempt without waiting for them.
///
/// Shutdown calls this before draining so in-flight startups observe
/// cancellation cooperatively and run their own rollback, instead of being
/// dropped mid-create by an abort deadline.
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

/// Retries every cleanup target still recorded until none remain or the
/// deadline expires, and returns the configurations whose cleanup did not
/// complete.
///
/// A create whose request was abandoned may only appear after the first
/// deletion pass, and the registry lives in memory: leaving it unreconciled at
/// exit leaves the resource running with nothing tracking it. Targets with
/// cluster resources still owed are persisted before returning, so a later
/// run restores and retries them: the registry that tracks them lives only in
/// this process, and the caller is about to exit it.
pub async fn reconcile_pending_cleanup(
    mode: DatabaseMode, deadline: Duration, exclude: &HashSet<i64>,
) -> Vec<i64> {
    const RETRY_DELAY: Duration = Duration::from_secs(2);
    // As `remaining` shrinks toward zero near the deadline, `RETRY_DELAY.
    // min(remaining)` degenerates into a near-zero sleep, spinning on the
    // CPU for the last stretch of the shutdown budget.
    const MIN_RETRY_DELAY: Duration = Duration::from_millis(50);

    // Creates abandoned by an earlier run are picked up here: nothing else in
    // this process knows about them.
    restore_uncertain_targets(mode).await;

    let until = Instant::now() + deadline;
    loop {
        // Registered processes are enumerated too: a stop-all dropped on its
        // deadline leaves the ones it never visited only in `CHILD_PROCESSES`,
        // and they own cluster resources and hosts entries just the same.
        let mut ids: Vec<i64> = PENDING_CLEANUP
            .iter()
            .map(|entry| *entry.key())
            .filter(|id| !exclude.contains(id))
            .collect();
        ids.extend(
            CHILD_PROCESSES
                .iter()
                .map(|entry| *entry.key())
                .filter(|id| !PENDING_CLEANUP.contains_key(id) && !exclude.contains(id)),
        );
        if ids.is_empty() {
            // An allocation still in flight has nothing recorded yet, so an
            // empty registry is only proof once none are outstanding.
            if crate::kube::start::OUTSTANDING_ALLOCATIONS.load(std::sync::atomic::Ordering::SeqCst)
                == 0
            {
                return Vec::new();
            }
            // One clock read for both the deadline test and the sleep: with two
            // reads the deadline can pass in between, and the subtraction that
            // follows would panic and abort the whole reconciliation.
            let Some(remaining) = until.checked_duration_since(Instant::now()) else {
                warn!("Giving up on address allocations that never finished");

                return unresolved_cleanup(mode, exclude).await;
            };
            tokio::time::sleep(RETRY_DELAY.min(remaining).max(MIN_RETRY_DELAY)).await;
            continue;
        }
        let Some(remaining) = until.checked_duration_since(Instant::now()) else {
            warn!(
                "Giving up on cleanup for {} configuration(s) that never settled: {ids:?}",
                ids.len()
            );
            return unresolved_cleanup(mode, exclude).await;
        };
        tokio::time::sleep(RETRY_DELAY.min(remaining).max(MIN_RETRY_DELAY)).await;
        for id in ids {
            // Each attempt carries the remaining budget: a stop can wait on the
            // lifecycle lock and several requests, and the caller's deadline
            // has to hold for the whole pass, not just between passes. A
            // dropped attempt keeps its target recorded.
            let Some(remaining) = until.checked_duration_since(Instant::now()) else {
                return unresolved_cleanup(mode, exclude).await;
            };
            match tokio::time::timeout(remaining, stop_config(id, None, mode)).await {
                Ok(Err(error)) => warn!("Cleanup for config {id} is still incomplete: {error}"),
                Err(_) => {
                    warn!("Cleanup for config {id} did not finish within the shutdown budget");
                    return unresolved_cleanup(mode, exclude).await;
                }
                Ok(Ok(_)) => {}
            }
        }
    }
}

/// What reconciliation is leaving behind, persisted so the next run can pick
/// it up. A registered process counts too: its resources were never visited.
async fn unresolved_cleanup(mode: DatabaseMode, exclude: &HashSet<i64>) -> Vec<i64> {
    let mut ids: Vec<i64> = PENDING_CLEANUP
        .iter()
        .map(|entry| *entry.key())
        .filter(|id| !exclude.contains(id))
        .collect();
    ids.extend(
        CHILD_PROCESSES
            .iter()
            .map(|entry| *entry.key())
            .filter(|id| !PENDING_CLEANUP.contains_key(id) && !exclude.contains(id)),
    );
    for entry in CHILD_PROCESSES.iter() {
        if exclude.contains(entry.key()) {
            continue;
        }
        if let Some(config) = entry.value().config() {
            record_target(
                *entry.key(),
                config.clone(),
                None,
                config_has_cluster_resources(config),
                true,
                entry.value().destination(),
            );
        }
    }
    // Copied out before anything is awaited: an allocation task that finishes
    // now records its address under this map's write lock, and a read guard
    // held across the database write would block it for the whole write.
    let owed: Vec<(i64, PendingTarget)> = PENDING_CLEANUP
        .iter()
        .filter(|entry| !exclude.contains(entry.key()))
        .flat_map(|entry| {
            entry
                .value()
                .iter()
                .filter(|target| target.cluster)
                .map(|target| (*entry.key(), target.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    for (id, target) in owed {
        if let Err(error) =
            persist_uncertain_target(id, &target.config, target.destination.as_deref(), mode).await
        {
            warn!("Failed to persist the outstanding cleanup for config {id}: {error}");
        }
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

pub async fn stop_all_port_forward_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<CustomResponse>, String> {
    stop_all_port_forward_with_mode_excluding(mode, &HashSet::new()).await
}

/// Enumerates the ids a stop-all pass must act on, together with what each
/// one needs to run its stop and interpret the outcome. Shared by every
/// stop-all variant so they enumerate identically and differ only in how
/// they dispatch and await the per-id stops.
async fn collect_stop_all_targets(
    mode: DatabaseMode, exclude: &HashSet<i64>,
) -> (
    HashSet<i64>,
    HashMap<i64, Config>,
    HashSet<i64>,
    Result<Vec<Config>, String>,
    Result<Vec<ConfigState>, String>,
) {
    // Creates abandoned by an earlier run exist only on disk: the desktop
    // application never calls the reconciliation pass, so without this its
    // stop-all cannot find a relay left behind by a crash.
    restore_uncertain_targets(mode).await;

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

    // Ids contributed only by the recovery lock and pending-cleanup registries,
    // not by a live local task: these can legitimately have nothing left, e.g.
    // a recovery lock still held while the work it guarded already settled, or
    // a registry entry that outlived the row it described. Cleared below for
    // any id a persisted row also names, since those still owe a real report.
    let mut registry_only: HashSet<i64> = HashSet::new();
    for entry in crate::kube::proxy_recovery::RECOVERY_LOCKS.iter() {
        if Arc::strong_count(entry.value()) > 1 && !ids.contains(entry.key()) {
            registry_only.insert(*entry.key());
        }
    }
    for entry in PENDING_CLEANUP.iter() {
        if !ids.contains(entry.key()) {
            registry_only.insert(*entry.key());
        }
    }
    ids.extend(registry_only.iter().copied());
    if !exclude.is_empty() {
        ids.retain(|id| !exclude.contains(id));
        registry_only.retain(|id| !exclude.contains(id));
    }

    let configs_result = read_configs_with_mode(mode).await;
    let states_result = get_configs_state_with_mode(mode).await;
    if let Ok(states) = &states_result {
        let this_process = std::process::id();
        for state in states {
            if !state.is_running {
                continue;
            }
            let owned_elsewhere = mode == DatabaseMode::File
                && !CHILD_PROCESSES.contains_key(&state.config_id)
                && state.process_id.is_some_and(|pid| {
                    pid != this_process
                        && kftray_commons::utils::config_state::process_is_alive(pid)
                });
            if owned_elsewhere {
                // That process's row to stop and to report on: a forward it
                // still owns is not this stop-all's failure to surface.
                log::debug!(
                    "Stop-all: skipping config {} forwarded by another live kftray process",
                    state.config_id
                );
                registry_only.remove(&state.config_id);
                ids.remove(&state.config_id);
                continue;
            }
            registry_only.remove(&state.config_id);
            ids.insert(state.config_id);
        }
    }
    let configs: HashMap<i64, Config> = configs_result
        .as_ref()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|config| config.id.map(|id| (id, config.clone())))
        .collect();
    (ids, configs, registry_only, configs_result, states_result)
}

/// Same as [`stop_all_port_forward_with_mode`], but skips every id in
/// `exclude`. Lets a caller that has already detached its own stop tasks for
/// those ids keep this pass from contending with them for the per-config
/// recovery lock those tasks still hold.
pub async fn stop_all_port_forward_with_mode_excluding(
    mode: DatabaseMode, exclude: &HashSet<i64>,
) -> Result<Vec<CustomResponse>, String> {
    let (ids, configs, registry_only, configs_result, states_result) =
        collect_stop_all_targets(mode, exclude).await;

    let mut responses: Vec<CustomResponse> = stream::iter(ids)
        .map(|id| {
            let config = configs.get(&id).cloned();
            let suppress_missing = registry_only.contains(&id);
            async move {
                match stop_config(id, config.as_ref(), mode).await {
                    Ok(response) => Some(response),
                    Err(error) => {
                        if suppress_missing && error == nothing_forwarding_error(id) {
                            log::debug!(
                                "Stop-all: config {id} was only tracked by the recovery/cleanup \
                                 registry and nothing remained to clean"
                            );
                            None
                        } else {
                            Some(stop_response(id, config.as_ref(), Some(error)))
                        }
                    }
                }
            }
        })
        .buffer_unordered(16)
        .collect::<Vec<Option<CustomResponse>>>()
        .await
        .into_iter()
        .flatten()
        .collect();

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

/// Same target enumeration as [`stop_all_port_forward_with_mode_excluding`],
/// but each stop runs as its own spawned task and the whole pass is bounded
/// by `deadline` instead of waiting for every stop to finish. A stop still
/// running when the deadline elapses is left detached, not aborted: dropping
/// its `JoinHandle` here does not cancel the task, so it keeps running and
/// cleaning up in the background. Its config id comes back in the second
/// element instead of a response, for the caller to fold into whatever it
/// does next (excluding it from a reconciliation pass, for instance) instead
/// of contending with it for the same per-config recovery lock.
pub async fn stop_all_port_forward_with_deadline(
    mode: DatabaseMode, exclude: &HashSet<i64>, deadline: Duration,
) -> Result<(Vec<CustomResponse>, Vec<i64>), String> {
    let (ids, configs, registry_only, configs_result, states_result) =
        collect_stop_all_targets(mode, exclude).await;

    let handles: Vec<(i64, tokio::task::JoinHandle<Option<CustomResponse>>)> = ids
        .iter()
        .map(|&id| {
            let config = configs.get(&id).cloned();
            let suppress_missing = registry_only.contains(&id);
            let handle = tokio::spawn(async move {
                match stop_config(id, config.as_ref(), mode).await {
                    Ok(response) => Some(response),
                    Err(error) => {
                        if suppress_missing && error == nothing_forwarding_error(id) {
                            log::debug!(
                                "Stop-all: config {id} was only tracked by the recovery/cleanup \
                                 registry and nothing remained to clean"
                            );
                            None
                        } else {
                            Some(stop_response(id, config.as_ref(), Some(error)))
                        }
                    }
                }
            });
            (id, handle)
        })
        .collect();

    let mut responses = Vec::new();
    let mut completed: HashSet<i64> = HashSet::new();
    let deadline_at = Instant::now() + deadline;
    for (id, handle) in handles {
        let remaining = deadline_at.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, handle).await {
            Ok(Ok(Some(response))) => {
                completed.insert(id);
                responses.push(response);
            }
            Ok(Ok(None)) => {
                completed.insert(id);
            }
            Ok(Err(join_error)) => {
                completed.insert(id);
                warn!("Stop-all: stopping config {id} panicked: {join_error}");
                responses.push(stop_response(
                    id,
                    configs.get(&id),
                    Some(format!("Stopping config {id} panicked: {join_error}")),
                ));
            }
            Err(_) => {
                // Left detached: not counted as completed, and nothing here
                // aborts it.
            }
        }
    }
    let unfinished: Vec<i64> = ids
        .into_iter()
        .filter(|id| !completed.contains(id))
        .collect();

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
    Ok((responses, unfinished))
}

/// The error `stop_config` reports when it found no local process, database
/// row or pending-cleanup record to act on for an id. Shared so `stop-all`
/// can recognize it exactly, rather than duplicating the wording.
fn nothing_forwarding_error(id: i64) -> String {
    format!("No port forwarding process found for config_id '{id}'")
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
    // The durable records are loaded before the cleanup snapshot is chosen.
    // After a crash the in-memory registry is empty, and a configuration
    // edited to another namespace since would otherwise have only its new
    // row to go by, leaving the exposure the old row described running.
    restore_uncertain_targets(mode).await;
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
    let _shared =
        match kftray_commons::utils::config_dir::lock_config(id, mode, SHARED_LOCK_WAIT).await {
            Ok(shared) => shared,
            Err(error) => {
                drop(guard);
                drop(lock);
                crate::kube::proxy_recovery::remove_recovery_lock(id);
                return Err(error);
            }
        };
    if let Some(startup) = crate::kube::proxy::STARTING_PROXIES.get(&id) {
        startup.cancel();
    }
    if let Some((_, manager)) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.remove(&id) {
        manager.cancel();
    }
    // A row another live process is forwarding is that process's to stop:
    // its listener cannot be reached from here, and deleting its relay and
    // releasing its address would break a forward that keeps running with
    // nothing in the database saying so.
    // A pending record is not ownership either: a durable record of a
    // running exposure is restored into this process's registry by a stop
    // here, while the tunnel it describes runs in the other process.
    if !CHILD_PROCESSES.contains_key(&id)
        && let Some(owner) = running_in_another_process(id, mode).await
    {
        drop(guard);
        drop(lock);
        crate::kube::proxy_recovery::remove_recovery_lock(id);
        return Err(format!(
            "Config {id} is being forwarded by another kftray process ({owner}); stop it there"
        ));
    }
    *STOP_GENERATIONS.entry(id).or_insert(0) += 1;
    let process = CHILD_PROCESSES.remove(&id);
    let existed = process.is_some();
    let mut retained = None;
    let mut retained_destination = None;
    if let Some((_, mut process)) = process {
        retained = process.config().cloned();
        retained_destination = process.destination();
        // Recorded before the first await: the process is already out of
        // CHILD_PROCESSES, so aborting this stop, as the terminal's shutdown
        // drain does, would otherwise drop the only snapshot of its resources.
        // The destination goes with it: the row's context can come to mean
        // another server while the forward runs, and cleanup on that one must
        // not count as settling this.
        if let Some(config) = &retained {
            record_target(
                id,
                config.clone(),
                None,
                config_has_cluster_resources(config),
                true,
                retained_destination.clone(),
            );
        }
        process.cleanup_and_abort().await;
    }
    // The snapshot taken at startup describes the resources that actually
    // exist. The database record can have been edited since without stopping
    // the forward, which would point cleanup at the new destination.
    let pending = pending_cleanup_targets(id);
    let refreshed = if retained.is_none() && pending.is_empty() && was_starting {
        running_snapshot(id, mode).await
    } else {
        None
    };
    // Only a registered process, a pending target or a persisted running
    // snapshot is proof resources may exist; a config found only through
    // the plain row must not infer a cluster obligation from its workload
    // type alone, or an already-stopped proxy would need cluster access
    // just to be stopped again.
    let evidenced = existed || !pending.is_empty() || refreshed.is_some();
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
            .find(|target| target.describes(config, retained_destination.as_deref()));
        let mut targets = vec![PendingTarget {
            config: config.clone(),
            uncertain_until: recorded.and_then(|target| target.uncertain_until),
            // A record for these resources describes what is left: an earlier
            // stop that deleted the Deployment and failed to release the
            // address must not need cluster access again. Without one, the
            // obligation is whatever this workload type can actually create:
            // a plain TCP forward owes no cluster cleanup at all.
            cluster: recorded.map_or_else(
                || evidenced && config_has_cluster_resources(config),
                |target| target.cluster,
            ),
            local: recorded.is_none_or(|target| target.local),
            destination: recorded
                .and_then(|target| target.destination.clone())
                .or(retained_destination),
        }];
        // Every other record for this id is a target of its own, including
        // one for the same rows on another server: a configuration started
        // against one server, left with a failed cleanup, and redirected to
        // another owes both.
        targets.extend(
            pending
                .iter()
                .filter(|target| !recorded.is_some_and(|primary| std::ptr::eq(primary, *target)))
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
                target.destination.clone(),
            );
        }

        let mut errors: Vec<String> = Vec::new();
        let mut settled: Vec<PendingTarget> = Vec::new();
        // Targets whose create was never answered, so an empty list is not
        // proof that nothing was created.
        let unanswered: Vec<PendingTarget> = targets
            .iter()
            .filter(|target| target.uncertain_until.is_some())
            .cloned()
            .collect();
        let is_unanswered = |target: &PendingTarget| {
            unanswered
                .iter()
                .any(|entry| entry.describes(&target.config, target.destination.as_deref()))
        };
        let now = Instant::now();
        for target in &targets {
            let cluster = if target.cluster {
                delete_cluster_resources(id, &target.config, target.destination.as_deref(), mode)
                    .await
            } else {
                Ok(())
            };
            // Local resources are released per target too: an edited row can
            // name a different loopback address than the one still bound.
            let local = if target.local {
                release_local_resources(id, &target.config, mode).await
            } else {
                LocalCleanup::default()
            };
            let uncertain = target.is_uncertain(now);
            if cluster.is_ok() && local.settled() && !uncertain {
                settled.push(target.clone());
                continue;
            }

            // The only remaining record of where these resources live: the
            // database row can be edited or deleted while a forward runs.
            // An unanswered create keeps its cluster obligation until its
            // confirmations complete, whatever the local cleanup did: the list
            // that came back empty is not proof, and dropping the obligation
            // here would stop any later pass from looking again.
            // Charged only by a pass that actually completed a listing after
            // the window expired: one whose delete failed, or that ran while
            // the outcome could still change, observed nothing and must not
            // count towards the evidence that nothing was created.
            let unconfirmed = is_unanswered(target)
                && (cluster.is_err()
                    || uncertain
                    || !confirm_uncertain_target(
                        id,
                        &target.config,
                        target.destination.as_deref(),
                        mode,
                    )
                    .await);
            let cluster_owed = target.cluster && (cluster.is_err() || uncertain || unconfirmed);
            // A cluster obligation that is confirmed complete clears its
            // durable record now, whatever the local cleanup did. Left in
            // place, a restart would resurrect it with a fresh uncertainty
            // window, need cluster access again for resources already proven
            // gone, and block deleting a configuration that is only waiting on
            // a loopback alias.
            // The uncertainty marker goes with the obligation it qualified:
            // kept on a target that only owes local cleanup, it would make
            // the pass that finally releases the alias treat the target as
            // unanswered again, restart a counter that no longer exists and
            // recreate a cluster obligation already proven settled.
            // Every armed create was persisted, not only the unanswered ones,
            // so a settled cluster obligation clears its durable record
            // whatever the local cleanup did: left behind, a restart would
            // resurrect a cluster obligation for a relay already removed.
            let mut cluster_owed = cluster_owed;
            let mut cluster_settled = target.cluster && !cluster_owed;
            if cluster_settled
                && let Err(error) =
                    forget_uncertain_target(id, &target.config, target.destination.as_deref(), mode)
                        .await
            {
                // Not settled until the record is gone: kept, and reported,
                // so a later stop tries the deletion again.
                cluster_owed = true;
                cluster_settled = false;
                errors.push(error);
            }
            set_target_obligations(
                id,
                target,
                if cluster_settled {
                    None
                } else {
                    target.uncertain_until
                },
                cluster_owed,
                target.local && !local.settled(),
            );
            if let Err(error) = cluster {
                SHARED_CLIENT_MANAGER.invalidate_client(&ServiceClientKey::new(
                    target.config.context.clone(),
                    target.config.kubeconfig.clone(),
                ));
                errors.push(error);
            } else if uncertain || unconfirmed {
                // An abandoned create may still be persisting, so one empty
                // list is not proof, and the confirmation that follows the
                // window is not complete until every pass it needs has run.
                // Reported so the configuration is not marked stopped while a
                // cleanup obligation remains: the desktop application has no
                // reconciliation pass of its own, and a stop that reports
                // success here leaves a row that cannot be deleted.
                errors.push(format!(
                    "A create request for config {id} was never answered, so its cluster resources \
                     are still being reconciled; stop it again to complete the check"
                ));
            }
            errors.extend(local.failures);
            // Reported once, like a failure, but not kept recorded above: a
            // privilege that is not available right now will not become
            // available by retrying automatically.
            errors.extend(local.unsatisfiable);
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
            let destination = target.destination.as_deref();
            let answered = !is_unanswered(&target);
            if answered || confirm_uncertain_target(id, &target.config, destination, mode).await {
                match forget_uncertain_target(id, &target.config, destination, mode).await {
                    Ok(()) => forget_pending_cleanup(id, &target.config, destination),
                    Err(error) => {
                        set_target_obligations(id, &target, None, true, false);
                        errors.push(error);
                    }
                }
                continue;
            }
            // Still unconfirmed: the obligation stays so the next pass looks
            // again, rather than leaving the durable record with nothing in
            // this process to act on it, and the stop reports it for the same
            // reason as above.
            set_target_obligations(id, &target, None, true, false);
            errors.push(format!(
                "A create request for config {id} was never answered, so its cluster resources are \
                 still being reconciled; stop it again to complete the check"
            ));
        }
        // is_running tracks the local process, not the durable cleanup
        // obligation: the process is already out of CHILD_PROCESSES, cancelled
        // and aborted by this point, however the cluster/local cleanup above
        // went. Clearing it here regardless of `errors` keeps the UI from
        // showing a torn-down forward as running forever; the remaining
        // obligation stays in PENDING_CLEANUP and is still reported below.
        let state = ConfigState::new(id, false);
        if let Err(error) = update_config_state_with_mode(&state, mode).await {
            errors.push(error);
        } else {
            kftray_commons::utils::config_state::clear_running_snapshot(id, mode).await;
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
        Err(nothing_forwarding_error(id))
    };
    release_unused_clients();
    drop(guard);
    drop(lock);
    crate::kube::proxy_recovery::remove_recovery_lock(id);
    result
}

/// Drops the cached API clients no registered forward uses any more, so
/// the keep-alive connections they pooled close with the forward instead of
/// staying open for the cache TTL.
fn release_unused_clients() {
    let in_use: HashSet<ServiceClientKey> = CHILD_PROCESSES
        .iter()
        .filter_map(|entry| {
            let config = entry.value().config()?;
            Some(ServiceClientKey::new(
                config.context.clone(),
                config.kubeconfig.clone(),
            ))
        })
        .collect();
    SHARED_CLIENT_MANAGER.release_unused(&in_use);
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
    async fn every_stop_that_acts_on_an_id_advances_its_generation() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_012;
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_config(local_config(id));
        CHILD_PROCESSES.insert(id, process);

        let before = stop_generation(id);
        stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(
            stop_generation(id),
            before + 1,
            "a stop that found the forward counts once"
        );

        // A second stop finds nothing to stop, but still counts: a restart
        // that reads the generation around its own stop must see that
        // someone else acted on the id in between, whatever they found.
        let _ = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory).await;
        assert_eq!(stop_generation(id), before + 2);
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
    async fn failed_cluster_cleanup_clears_running_but_keeps_pending_cleanup() {
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
                .any(|state| state.config_id == id && !state.is_running),
            "the local process is already torn down, so is_running must clear even though \
             cluster cleanup failed"
        );
        assert!(
            !pending_cleanup_targets(id).is_empty(),
            "the unmet cluster obligation must stay in the durable registry so a later stop-all \
             retries it"
        );

        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn unresolved_cleanup_does_not_record_a_cluster_obligation_for_a_plain_forward() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_071;
        PENDING_CLEANUP.remove(&id);
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_config(local_config(id));
        CHILD_PROCESSES.insert(id, process);

        unresolved_cleanup(DatabaseMode::Memory, &HashSet::new()).await;

        let targets = pending_cleanup_targets(id);
        assert!(
            targets.iter().any(|target| !target.cluster),
            "a plain TCP forward never creates cluster resources, so it must not need cluster \
             access to be considered idle: {targets:?}"
        );

        if let Some((_, mut process)) = CHILD_PROCESSES.remove(&id) {
            process.cleanup_and_abort().await;
        }
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn stopping_an_already_stopped_proxy_by_row_alone_does_not_need_cluster_access() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = Config {
            namespace: "default".to_string(),
            service: Some("relay".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };
        let id =
            kftray_commons::utils::config::insert_config_with_mode(config, DatabaseMode::Memory)
                .await
                .unwrap();
        PENDING_CLEANUP.remove(&id);

        // Nothing is running, nothing is pending, and no snapshot was ever
        // recorded: only the database row exists. Its workload type alone
        // must not be read as proof cluster resources exist, or stopping an
        // already-stopped proxy would fail every time cluster access is
        // unavailable.
        let response = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .expect(
                "stopping a config with nothing running and no evidence of cluster resources \
                 must not need cluster access",
            );
        assert_eq!(response.status, 0);

        PENDING_CLEANUP.remove(&id);
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
        ClusterResourceGuard::arm(id, config.clone(), None, DatabaseMode::Memory)
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

    #[tokio::test]
    async fn a_local_cleanup_obligation_survives_a_simulated_restart() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = -9_313;
        PENDING_CLEANUP.remove(&id);
        let mode = DatabaseMode::Memory;
        let key = local_cleanup_key(id, mode);
        kftray_commons::utils::settings::delete_setting_with_mode(&key, mode)
            .await
            .unwrap();
        let config = Config {
            id: Some(id),
            namespace: "prod".to_string(),
            service: Some("relay".to_string()),
            local_address: Some("127.0.44.2".to_string()),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };

        record_local_cleanup(id, config.clone(), mode).await;
        assert!(
            kftray_commons::utils::settings::get_setting_with_mode(&key, mode)
                .await
                .unwrap()
                .is_some(),
            "the obligation must be durable, not only in this process's memory"
        );

        // A restart starts with an empty registry; only the durable record
        // survives it.
        PENDING_CLEANUP.remove(&id);
        assert!(pending_cleanup_targets(id).is_empty());

        restore_uncertain_targets(mode).await;

        let targets = pending_cleanup_targets(id);
        assert_eq!(
            targets.len(),
            1,
            "the persisted local obligation must be reloaded into the registry"
        );
        assert!(targets[0].local);
        assert!(!targets[0].cluster);
        assert_eq!(targets[0].config.local_address, config.local_address);

        settle_local_cleanup(id, &config, mode).await;
        assert!(
            kftray_commons::utils::settings::get_setting_with_mode(&key, mode)
                .await
                .unwrap()
                .is_none(),
            "settling must forget the durable record too"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[test]
    fn owned_by_another_process_is_recognized_and_settled_not_failed() {
        assert!(address_owned_by_another_process(
            "Address 127.0.44.1 is owned by another live process (pid 4242)"
        ));
        assert!(
            !address_owned_by_another_process("Helper service is not available"),
            "an unrelated helper failure must still be reported, not silently settled"
        );
        assert!(
            !address_owned_by_another_process("timed out after 3s"),
            "a timeout must still be reported, not silently settled"
        );
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
    async fn a_retry_settling_does_not_answer_for_an_earlier_unanswered_create() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = Config {
            id: Some(730_412),
            namespace: "shared".to_string(),
            service: Some("relay".to_string()),
            workload_type: Some("expose".to_string()),
            ..Config::default()
        };
        let id = config.id.unwrap();
        PENDING_CLEANUP.remove(&id);
        let destination = Some("https://a".to_string());

        // An earlier attempt whose create was never answered.
        record_target(
            id,
            config.clone(),
            Some(Instant::now() + UNCERTAIN_CREATE_WINDOW),
            true,
            false,
            destination.clone(),
        );

        // The retry is definitively rejected before creating anything.
        ClusterResourceGuard::arm(
            id,
            config.clone(),
            destination.clone(),
            DatabaseMode::Memory,
        )
        .await
        .unwrap()
        .disarm()
        .await;
        let targets = pending_cleanup_targets(id);
        assert!(
            targets
                .iter()
                .any(|target| target.cluster && target.is_uncertain(Instant::now())),
            "the earlier create is still unanswered: {targets:?}"
        );

        // A retry that is answered does not settle it either.
        let mut guard =
            ClusterResourceGuard::arm(id, config.clone(), destination, DatabaseMode::Memory)
                .await
                .unwrap();
        guard.confirm();
        drop(guard);
        let targets = pending_cleanup_targets(id);
        assert!(
            targets
                .iter()
                .any(|target| target.cluster && target.is_uncertain(Instant::now())),
            "confirming the retry must not clear inherited uncertainty: {targets:?}"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn a_rejected_retry_after_a_restart_keeps_the_persisted_unanswered_create() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = Config {
            id: Some(730_413),
            namespace: "shared".to_string(),
            service: Some("relay".to_string()),
            workload_type: Some("expose".to_string()),
            ..Config::default()
        };
        let id = config.id.unwrap();
        let destination = Some("https://a".to_string());
        PENDING_CLEANUP.remove(&id);

        // An earlier run abandoned the create and exited: only the durable
        // record remains.
        persist_uncertain_target(id, &config, destination.as_deref(), DatabaseMode::Memory)
            .await
            .unwrap();
        let key = uncertain_create_key(id, &config, destination.as_deref(), DatabaseMode::Memory);

        ClusterResourceGuard::arm(
            id,
            config.clone(),
            destination.clone(),
            DatabaseMode::Memory,
        )
        .await
        .unwrap()
        .disarm()
        .await;

        let stored =
            kftray_commons::utils::settings::get_setting_with_mode(&key, DatabaseMode::Memory)
                .await
                .unwrap();
        assert!(
            stored.is_some(),
            "the retry's rejection must not erase the earlier run's unanswered create"
        );
        assert!(
            pending_cleanup_targets(id)
                .iter()
                .any(|target| target.cluster && target.uncertain_until.is_some()),
            "the obligation is tracked in memory as unanswered"
        );
        forget_uncertain_target(id, &config, destination.as_deref(), DatabaseMode::Memory)
            .await
            .unwrap();
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn settle_clears_the_durable_record_under_the_callers_own_mode() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = Config {
            id: Some(730_414),
            namespace: "shared".to_string(),
            service: Some("relay".to_string()),
            workload_type: Some("expose".to_string()),
            ..Config::default()
        };
        let id = config.id.unwrap();
        let destination = Some("https://a".to_string());
        PENDING_CLEANUP.remove(&id);

        // The durable key is mode-scoped: persisted under `Memory` here, the
        // same as a memory-mode session's own record would be.
        persist_uncertain_target(id, &config, destination.as_deref(), DatabaseMode::Memory)
            .await
            .unwrap();
        let key = uncertain_create_key(id, &config, destination.as_deref(), DatabaseMode::Memory);
        record_target(id, config.clone(), None, true, false, destination.clone());

        settle_cluster_obligation(id, "https://a", DatabaseMode::Memory)
            .await
            .unwrap();

        let stored =
            kftray_commons::utils::settings::get_setting_with_mode(&key, DatabaseMode::Memory)
                .await
                .unwrap();
        assert!(
            stored.is_none(),
            "settle_cluster_obligation must clear the durable record under the caller's own \
             mode, not always DatabaseMode::File: a memory-mode session's obligation would \
             otherwise survive and be resurrected by the next `restore_uncertain_targets`"
        );
        assert!(
            pending_cleanup_targets(id).is_empty(),
            "the in-memory cluster obligation must be settled too"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn settle_forgets_the_record_only_for_a_matching_destination() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config_dir = tempfile::tempdir().unwrap();
        let original_config_dir = std::env::var("KFTRAY_CONFIG").ok();
        unsafe { std::env::set_var("KFTRAY_CONFIG", config_dir.path()) };
        // The durable delete goes through the file-mode database, so the
        // isolated config dir needs its schema before it can succeed.
        kftray_commons::utils::db::init().await.unwrap();

        let config = Config {
            id: Some(910_501),
            namespace: "settle-test".to_string(),
            service: Some("relay".to_string()),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };
        let id = config.id.unwrap();
        PENDING_CLEANUP.remove(&id);

        // Two servers this configuration's resources have lived on: only the
        // one named in `settle_cluster_obligation` is settled.
        record_target(
            id,
            config.clone(),
            None,
            true,
            false,
            Some("https://a".to_string()),
        );
        record_target(
            id,
            config.clone(),
            None,
            true,
            false,
            Some("https://b".to_string()),
        );

        settle_cluster_obligation(id, "https://a", DatabaseMode::File)
            .await
            .unwrap();

        if let Some(original) = original_config_dir {
            unsafe { std::env::set_var("KFTRAY_CONFIG", original) };
        } else {
            unsafe { std::env::remove_var("KFTRAY_CONFIG") };
        }

        let targets = pending_cleanup_targets(id);
        assert!(
            !targets
                .iter()
                .any(|target| target.destination.as_deref() == Some("https://a")),
            "the matching destination's cluster obligation must be forgotten: {targets:?}"
        );
        assert!(
            targets
                .iter()
                .any(|target| target.destination.as_deref() == Some("https://b") && target.cluster),
            "an obligation for another destination must stay: {targets:?}"
        );
        PENDING_CLEANUP.remove(&id);
    }

    #[tokio::test]
    async fn the_same_rows_on_two_servers_are_two_cleanup_targets() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config = Config {
            id: Some(730_411),
            namespace: "shared".to_string(),
            service: Some("relay".to_string()),
            context: Some("missing-context".to_string()),
            kubeconfig: Some("/nonexistent/kubeconfig".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("proxy".to_string()),
            ..Config::default()
        };
        let id = config.id.unwrap();
        PENDING_CLEANUP.remove(&id);

        // The same configuration created resources on server A, whose cleanup
        // failed, and then, after its context was redirected, on server B.
        record_target(
            id,
            config.clone(),
            None,
            true,
            false,
            Some("https://a".into()),
        );
        record_target(
            id,
            config.clone(),
            None,
            true,
            false,
            Some("https://b".into()),
        );
        assert_eq!(pending_cleanup_targets(id).len(), 2);

        // A stop whose running process was built against B must still visit A.
        let task = tokio::spawn(std::future::pending::<anyhow::Result<()>>());
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_config(config.clone());
        CHILD_PROCESSES.insert(id, process);
        let error = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .expect_err("an unreachable cluster must fail the stop");
        assert_eq!(
            error.matches("Failed to create Kubernetes client").count(),
            2,
            "one pass must attempt both servers: {error}"
        );

        let mut destinations: Vec<Option<String>> = pending_cleanup_targets(id)
            .into_iter()
            .map(|target| target.destination)
            .collect();
        destinations.sort();
        assert_eq!(
            destinations,
            vec![Some("https://a".to_string()), Some("https://b".to_string())],
            "both servers stay owed after a failed pass"
        );
        PENDING_CLEANUP.remove(&id);
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
    async fn an_unremovable_alias_does_not_block_deleting_a_stopped_config() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_131;
        let config = Config {
            id: Some(id),
            namespace: "default".to_string(),
            local_address: Some("127.0.0.99".to_string()),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            ..Config::default()
        };
        // What a stop leaves behind when releasing the loopback alias needs a
        // privileged helper this machine does not have: the forward is gone,
        // only the local cleanup is still owed.
        record_target(id, config, None, false, true, None);

        let deleted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let marker = std::sync::Arc::clone(&deleted);
        delete_configs_if_idle(&[id], DatabaseMode::Memory, || async move {
            marker.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })
        .await
        .expect("a stopped configuration must be deletable");
        assert!(
            deleted.load(std::sync::atomic::Ordering::Relaxed),
            "deletion must run: nothing about this configuration is still forwarding"
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
        let guard = ClusterResourceGuard::arm(id, config.clone(), None, DatabaseMode::Memory)
            .await
            .unwrap();
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

        // A later attempt at the same target does not answer for the abandoned
        // one; its confirmation leaves the record uncertain.
        let mut guard = ClusterResourceGuard::arm(id, config.clone(), None, DatabaseMode::Memory)
            .await
            .unwrap();
        guard.confirm();
        std::mem::forget(guard);
        let _ = stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory).await;
        assert!(
            !pending_cleanup_targets(id).is_empty(),
            "a retry's confirmation does not settle the abandoned create"
        );

        // Once nothing unanswered is left, a create whose outcome was observed
        // is settled by the same pass.
        PENDING_CLEANUP.remove(&id);
        forget_uncertain_target(id, &config, None, DatabaseMode::Memory)
            .await
            .unwrap();
        let mut guard = ClusterResourceGuard::arm(id, config.clone(), None, DatabaseMode::Memory)
            .await
            .unwrap();
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
