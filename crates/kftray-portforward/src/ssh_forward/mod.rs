//! `ssh-port-forward` workload: a local TCP listener whose connections each
//! become a `direct-tcpip` channel of one pooled SSH session.

mod host_key_store;
mod listener;
mod providers;
#[cfg(test)]
mod tests;

use std::sync::{
    Arc,
    LazyLock,
};
use std::time::Duration;

use dashmap::DashMap;
use kftray_commons::models::config_model::Config;
use kftray_commons::models::response::CustomResponse;
use kftray_commons::utils::config_dir::get_config_dir;
use kftray_commons::utils::config_state::set_retry_status_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_ssh::{
    SessionState,
    SshError,
    SshRoute,
    SshRuntime,
    Stage,
    StreamOpener,
};
use kftray_telemetry::{
    Phase,
    PhaseGuard,
};
use serde::Serialize;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{
    error,
    info,
    warn,
};

use self::host_key_store::DbHostKeyStore;
use self::providers::{
    ProviderContexts,
    build_route,
};
use crate::kube::listener::Protocol;
use crate::kube::start::{
    build_tls_acceptor,
    claim_local_resources,
    load_ssl_settings,
    register_started,
    rollback_local_resources,
    write_domain_alias,
};
use crate::port_forward::PortForwardProcess;

/// Bound for the test channel opened at start to check the destination.
const DESTINATION_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

// The only SSH globals: one runtime (key + session pool) per database, and
// the provider contexts (credentials, HTTP clients) shared by all forwards.
static RUNTIMES: LazyLock<DashMap<DatabaseMode, Arc<SshRuntime>>> = LazyLock::new(DashMap::new);
static CONTEXTS: LazyLock<ProviderContexts> = LazyLock::new(ProviderContexts::default);

/// Drops the cached runtimes and provider contexts, so a test gets its own
/// key and endpoints instead of an earlier test's.
#[cfg(test)]
async fn reset_caches() {
    RUNTIMES.clear();
    CONTEXTS.clear().await;
}

fn runtime(mode: DatabaseMode) -> Result<Arc<SshRuntime>, String> {
    if let Some(runtime) = RUNTIMES.get(&mode) {
        return Ok(Arc::clone(&runtime));
    }
    // Built outside the map lock; a concurrent duplicate is harmless because
    // the key file race is settled by `KftrayKey` itself.
    let created = Arc::new(
        SshRuntime::new(&get_config_dir()?, Arc::new(DbHostKeyStore { mode }))
            .map_err(|e| e.to_string())?,
    );
    Ok(Arc::clone(&RUNTIMES.entry(mode).or_insert(created)))
}

/// Destination, provider and protocol checks shared by start and test.
fn validate(
    config: &Config,
) -> Result<
    (
        &kftray_commons::models::ssh_provider::SshProvider,
        &str,
        u16,
    ),
    String,
> {
    if config.protocol != "tcp" {
        return Err("Protocol must be 'tcp' for ssh-port-forward".to_owned());
    }
    let provider = config
        .ssh_provider
        .as_ref()
        .ok_or("ssh_provider is required for ssh-port-forward")?;
    provider.kind()?.validate()?;
    let host = config
        .remote_address
        .as_deref()
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .ok_or("Remote address is required for ssh-port-forward")?;
    let port = config
        .remote_port
        .filter(|port| *port > 0)
        .ok_or("Remote port is required for ssh-port-forward")?;
    Ok((provider, host, port))
}

/// Telemetry span of the stage being established.
struct StageTracker {
    provider: String,
    current: Option<Stage>,
    guard: Option<PhaseGuard>,
}

impl StageTracker {
    fn new(provider: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            current: None,
            guard: None,
        }
    }

    fn enter(&mut self, stage: Stage) {
        self.current = Some(stage);
        self.guard = None;
        let phase = match stage {
            Stage::Transport => Phase::SshTransport,
            Stage::Handshake => Phase::SshHandshake,
            Stage::Auth => Phase::SshAuth,
            Stage::Channel => return,
        };
        let guard = kftray_telemetry::phase(phase);
        guard.set_data("ssh.provider", &self.provider);
        self.guard = Some(guard);
    }

    fn fail(&mut self) {
        if let Some(guard) = self.guard.take() {
            guard.fail();
        }
    }

    fn finish(&mut self) {
        self.guard = None;
    }
}

pub(crate) async fn start_single_ssh_forward(
    mut config: Config, mode: DatabaseMode, ssl_override: bool,
    cancellation: Option<&CancellationToken>,
) -> Result<CustomResponse, String> {
    let config_id = config.id.ok_or("Config has no ID")?;
    let cancelled_reason = || format!("Startup cancelled for config {config_id}");
    let (provider, host, remote_port) = validate(&config)?;
    let (provider, host) = (provider.clone(), host.to_owned());
    let runtime = runtime(mode)?;

    let mut local = claim_local_resources(&mut config, mode, cancellation).await?;

    let mut tracker = StageTracker::new(provider.type_name());
    let connect = async {
        let route = build_route(&provider, &CONTEXTS).await?;
        let label = route.label.clone();
        runtime
            .acquire(route, &mut |stage| tracker.enter(stage))
            .await
            .map(|lease| (lease, label))
            .map_err(|e| e.to_string())
    };
    let connected = match cancellation {
        Some(token) => tokio::select! {
            biased;
            _ = token.cancelled() => Err(cancelled_reason()),
            connected = connect => connected,
        },
        None => connect.await,
    };
    let (lease, label) = match connected {
        Ok(connected) => {
            tracker.finish();
            connected
        }
        Err(reason) => {
            tracker.fail();
            return Err(rollback_local_resources(&config, &local.address, reason, mode).await);
        }
    };

    // A wrong destination fails the start instead of the user's first
    // connection. Raced against the startup token: a stop waits on the
    // lifecycle lock this startup holds.
    let opener = Arc::new(lease.opener(host.clone(), remote_port));
    let check = tokio::time::timeout(
        DESTINATION_CHECK_TIMEOUT,
        opener.open(std::net::SocketAddr::from(([127, 0, 0, 1], 0))),
    );
    let check = match cancellation {
        Some(token) => tokio::select! {
            biased;
            _ = token.cancelled() => {
                let reason = cancelled_reason();
                return Err(rollback_local_resources(&config, &local.address, reason, mode).await);
            }
            check = check => check,
        },
        None => check.await,
    };
    match check {
        Ok(Ok(_test_stream)) => {}
        Ok(Err(SshError::TargetUnreachable { .. } | SshError::Timeout { .. })) | Err(_) => {
            let reason = format!("{label} reached, but {host}:{remote_port} did not respond");
            return Err(rollback_local_resources(&config, &local.address, reason, mode).await);
        }
        Ok(Err(error)) => {
            let reason = error.to_string();
            return Err(rollback_local_resources(&config, &local.address, reason, mode).await);
        }
    }

    write_domain_alias(&config, &local, mode).await?;

    let (settings, should_use_ssl) = load_ssl_settings(&config, mode, ssl_override).await;
    let tls = match (&settings, should_use_ssl) {
        (Some(settings), true) => {
            let phase = kftray_telemetry::phase(Phase::Tls);
            match build_tls_acceptor(&config, settings).await {
                Ok(acceptor) => Some(acceptor),
                Err(e) => {
                    phase.fail();
                    warn!("Failed to create TLS acceptor: {}", e);
                    None
                }
            }
        }
        _ => None,
    };

    let local_port = config.local_port.unwrap_or_default();
    let phase = kftray_telemetry::phase(Phase::Listen);
    let bound = match crate::local_port::ensure_free(&local.address, local_port, Protocol::Tcp) {
        Ok(()) => listener::bind(&local.address, local_port).await,
        Err(error) => Err(error),
    };
    drop(phase);
    let (tcp_listener, actual_port) = match bound {
        Ok(bound) => bound,
        Err(error) => {
            let reason = error.to_string();
            return Err(rollback_local_resources(&config, &local.address, reason, mode).await);
        }
    };

    let token = CancellationToken::new();
    let state = lease.state();
    let serve_opener: Arc<dyn StreamOpener> = opener;
    let task_token = token.clone();
    let handle = tokio::spawn(async move {
        // Keeps the session alive for as long as the forward runs.
        let _lease = lease;
        let serve = listener::serve(
            tcp_listener,
            actual_port,
            config_id,
            serve_opener,
            tls,
            mode,
            task_token.clone(),
        );
        // This task's id is what the registered process is recognized by.
        let owner = tokio::task::id();
        run_until_listener_ends(serve, state, config_id, mode, task_token, owner).await
    });
    let mut process = PortForwardProcess::with_token(handle, config_id.to_string(), token);

    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        let reason = match process.cleanup_and_abort().await {
            Ok(()) => cancelled_reason(),
            Err(held) => format!(
                "{}; cleanup incomplete: listener: {held}",
                cancelled_reason()
            ),
        };
        return Err(rollback_local_resources(&config, &local.address, reason, mode).await);
    }

    let ssl = should_use_ssl;
    if let Err(reason) = register_started(&config, process, &mut local, ssl, mode).await {
        return Err(rollback_local_resources(&config, &local.address, reason, mode).await);
    }

    info!("TCP port forwarding is set up on local port {actual_port} via {label}");
    Ok(CustomResponse {
        id: config.id,
        service: String::new(),
        namespace: String::new(),
        local_port: actual_port,
        remote_port,
        context: String::new(),
        protocol: "tcp".to_owned(),
        status: 0,
        stderr: String::new(),
        stdout: format!(
            "{} forwarding from {}:{actual_port} -> {host}:{remote_port} via {label}{}",
            if ssl { "HTTPS" } else { "TCP" },
            local.address,
            if ssl {
                " (HTTP redirects to HTTPS)"
            } else {
                ""
            }
        ),
    })
}

/// Runs the listener with the state watcher beside it. When the listener
/// ends, even with an error, the watcher is stopped too, so the task ends
/// and drops its lease instead of waiting on a session that stays alive.
/// A listener that fails, rather than being stopped, stops its forward, so
/// the config is not left registered and shown as running.
async fn run_until_listener_ends(
    serve: impl Future<Output = anyhow::Result<()>>, state: watch::Receiver<SessionState>,
    config_id: i64, mode: DatabaseMode, token: CancellationToken, owner: tokio::task::Id,
) -> anyhow::Result<()> {
    let served = async {
        let served = serve.await;
        token.cancel();
        served
    };
    let (served, ()) = tokio::join!(served, watch_state(state, config_id, mode, token.clone()));
    if let Err(listener_error) = &served {
        error!("SSH listener of config {config_id} failed, stopping the forward: {listener_error}");
        // Detached: the stop aborts `owner`, the task this runs in.
        tokio::spawn(stop_after_listener_failure(config_id, owner, mode));
    }
    served
}

/// Stops the forward whose listener task `owner` failed, unless it has
/// been stopped or replaced meanwhile.
async fn stop_after_listener_failure(config_id: i64, owner: tokio::task::Id, mode: DatabaseMode) {
    if let Err(error) = crate::kube::stop::stop_failed_forward(config_id, owner, mode).await {
        error!("Failed to clean up config {config_id} after its listener failed: {error}");
    }
}

/// Mirrors the SSH session state into the forward's retry columns, so the
/// UI shows a reconnecting or failed session.
async fn watch_state(
    mut state: watch::Receiver<SessionState>, config_id: i64, mode: DatabaseMode,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            changed = state.changed() => if changed.is_err() { return },
        }
        let current = state.borrow_and_update().clone();
        let written = match &current {
            SessionState::Reconnecting { attempt } => {
                let attempt = i32::try_from(*attempt).unwrap_or(i32::MAX);
                set_retry_status_with_mode(config_id, Some(attempt), None, mode).await
            }
            SessionState::Ready => set_retry_status_with_mode(config_id, None, None, mode).await,
            SessionState::Failed(reason) => {
                set_retry_status_with_mode(config_id, None, Some(reason), mode).await
            }
            SessionState::Connecting => continue,
        };
        if let Err(error) = written {
            warn!("Failed to record the SSH session state of config {config_id}: {error}");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SshTestStepKind {
    Route,
    Transport,
    Handshake,
    Auth,
    Channel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SshTestStatus {
    Ok,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostKeyChange {
    pub known: String,
    pub presented: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SshTestStep {
    pub step: SshTestStepKind,
    pub status: SshTestStatus,
    pub message: Option<String>,
    pub host_key: Option<HostKeyChange>,
}

const TEST_STEPS: [SshTestStepKind; 5] = [
    SshTestStepKind::Route,
    SshTestStepKind::Transport,
    SshTestStepKind::Handshake,
    SshTestStepKind::Auth,
    SshTestStepKind::Channel,
];

fn step_of(stage: Stage) -> SshTestStepKind {
    match stage {
        Stage::Transport => SshTestStepKind::Transport,
        Stage::Handshake => SshTestStepKind::Handshake,
        Stage::Auth => SshTestStepKind::Auth,
        Stage::Channel => SshTestStepKind::Channel,
    }
}

/// Steps before `failed` are ok, `failed` carries the error, later ones are
/// skipped; with no failure every step is ok.
fn test_report(
    failed: Option<(SshTestStepKind, String, Option<HostKeyChange>)>,
) -> Vec<SshTestStep> {
    let failed_at = failed
        .as_ref()
        .and_then(|(kind, ..)| TEST_STEPS.iter().position(|step| step == kind));
    let mut failure = failed;
    TEST_STEPS
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let (status, message, host_key) = match failed_at {
                Some(at) if index == at => match failure.take() {
                    Some((_, message, host_key)) => {
                        (SshTestStatus::Failed, Some(message), host_key)
                    }
                    None => (SshTestStatus::Failed, None, None),
                },
                Some(at) if index > at => (SshTestStatus::Skipped, None, None),
                _ => (SshTestStatus::Ok, None, None),
            };
            SshTestStep {
                step: *step,
                status,
                message,
                host_key,
            }
        })
        .collect()
}

async fn route_for(config: &Config) -> Result<(SshRoute, String, u16), String> {
    let (provider, host, port) = validate(config)?;
    let route = build_route(provider, &CONTEXTS).await?;
    Ok((route, host.to_owned(), port))
}

/// Runs every stage of a forward on a fresh session, without starting it.
pub async fn test_connection(config: &Config, mode: DatabaseMode) -> Vec<SshTestStep> {
    let (route, host, port) = match route_for(config).await {
        Ok(route) => route,
        Err(message) => return test_report(Some((SshTestStepKind::Route, message, None))),
    };
    let runtime = match runtime(mode) {
        Ok(runtime) => runtime,
        Err(message) => return test_report(Some((SshTestStepKind::Route, message, None))),
    };
    let mut current = Stage::Transport;
    let probed = runtime
        .probe(route, &host, port, &mut |stage| current = stage)
        .await;
    match probed {
        Ok(()) => test_report(None),
        Err(error) => {
            let host_key = match &error {
                SshError::HostKeyMismatch {
                    known, presented, ..
                } => Some(HostKeyChange {
                    known: known.clone(),
                    presented: presented.clone(),
                }),
                _ => None,
            };
            test_report(Some((step_of(current), error.to_string(), host_key)))
        }
    }
}

/// Drops the first-use host key of the forward's bastion, so the next
/// connection trusts whatever key it presents.
pub async fn forget_host_key(config: &Config, mode: DatabaseMode) -> Result<(), String> {
    let (route, ..) = route_for(config).await?;
    runtime(mode)?
        .forget_host_key(&route.host_keys.alias())
        .await
        .map_err(|e| e.to_string())
}
